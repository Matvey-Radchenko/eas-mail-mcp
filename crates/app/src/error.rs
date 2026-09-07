use eas_mail_protocol::EasError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Application result type.
pub type Result<T> = std::result::Result<T, AppError>;

/// Stable machine-readable MCP error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Account credentials are absent or rejected.
    AuthRequired,
    /// Exchange denied EAS access after reaching the managed endpoint.
    AccessDenied,
    /// Current network cannot reach the managed endpoint.
    NetworkUnreachable,
    /// Exchange rejected a request due to throttling.
    Throttled,
    /// Exchange is temporarily unavailable for a safe read.
    ServiceUnavailable,
    /// The bounded local request queue is full.
    ResourceBusy,
    /// Configuration is invalid.
    ConfigInvalid,
    /// Exchange policy cannot be enforced by this app.
    PolicyBlocked,
    /// Requested object is no longer available.
    NotFound,
    /// A process-local snapshot cursor has expired.
    ReferenceExpired,
    /// Input violates an application limit.
    ValidationFailed,
    /// The selected Exchange server does not support the requested feature.
    FeatureUnavailable,
    /// More than one configured account could serve this request.
    AccountSelectionRequired,
    /// A complete response would exceed the public output limit.
    ResultTooLarge,
    /// A command requires an interactive terminal or complete explicit arguments.
    InteractiveRequired,
    /// Exchange returned an invalid response.
    ProtocolError,
    /// State changed and the caller should obtain a fresh reference.
    SyncStale,
    /// A mutation may have reached Exchange.
    OutcomeUnknown,
    /// Remote wipe removed or blocked account state.
    RemoteWipe,
    /// Another operation already uses this idempotency key with different input.
    IdempotencyConflict,
    /// Local secure storage failed.
    StorageError,
}

impl ErrorCode {
    /// Returns the stable serialized error code used by CLI and MCP responses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::AccessDenied => "ACCESS_DENIED",
            Self::NetworkUnreachable => "NETWORK_UNREACHABLE",
            Self::Throttled => "THROTTLED",
            Self::ServiceUnavailable => "SERVICE_UNAVAILABLE",
            Self::ResourceBusy => "RESOURCE_BUSY",
            Self::ConfigInvalid => "CONFIG_INVALID",
            Self::PolicyBlocked => "POLICY_BLOCKED",
            Self::NotFound => "NOT_FOUND",
            Self::ReferenceExpired => "REFERENCE_EXPIRED",
            Self::ValidationFailed => "VALIDATION_FAILED",
            Self::FeatureUnavailable => "FEATURE_UNAVAILABLE",
            Self::AccountSelectionRequired => "ACCOUNT_SELECTION_REQUIRED",
            Self::ResultTooLarge => "RESULT_TOO_LARGE",
            Self::InteractiveRequired => "INTERACTIVE_REQUIRED",
            Self::ProtocolError => "PROTOCOL_ERROR",
            Self::SyncStale => "SYNC_STALE",
            Self::OutcomeUnknown => "OUTCOME_UNKNOWN",
            Self::RemoteWipe => "REMOTE_WIPE",
            Self::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
            Self::StorageError => "STORAGE_ERROR",
        }
    }
}

/// Stable structured error returned by every tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorEnvelope {
    /// Stable error code.
    pub code: ErrorCode,
    /// Safe user-facing message without mailbox content.
    pub message: String,
    /// Whether a later retry can be useful.
    pub retryable: bool,
    /// Account affected by the failure.
    pub account_id: Option<String>,
    /// Operation journal identifier for writes.
    pub operation_id: Option<String>,
    /// Concrete remediation hint.
    pub remediation: Option<String>,
    /// Optional extended context is boxed to keep ordinary Result errors small.
    #[serde(flatten)]
    pub context: Box<ErrorContext>,
}

/// Extended optional error metadata serialized at the envelope's top level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ErrorContext {
    /// Server-advertised delay before a safe retry, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_seconds: Option<u64>,
    /// Every scoped failure when a multi-account request failed completely.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub account_errors: Vec<crate::Warning>,
}

impl std::ops::Deref for ErrorEnvelope {
    type Target = ErrorContext;
    fn deref(&self) -> &Self::Target {
        &self.context
    }
}
impl std::ops::DerefMut for ErrorEnvelope {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.context
    }
}

/// Internal application error carrying the public envelope.
#[derive(Debug, Error)]
#[error("{envelope:?}")]
pub struct AppError {
    /// Public structured failure.
    pub envelope: ErrorEnvelope,
}

impl AppError {
    /// Constructs a safe application error.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            envelope: ErrorEnvelope {
                code,
                message: message.into(),
                retryable: false,
                account_id: None,
                operation_id: None,
                remediation: None,
                context: Box::default(),
            },
        }
    }

    /// Associates an account with this failure.
    #[must_use]
    pub fn account(mut self, account_id: impl Into<String>) -> Self {
        self.envelope.account_id = Some(account_id.into());
        self
    }

    /// Marks this failure as retryable.
    #[must_use]
    pub const fn retryable(mut self) -> Self {
        self.envelope.retryable = true;
        self
    }

    /// Adds a safe remediation instruction.
    #[must_use]
    pub fn remediation(mut self, value: impl Into<String>) -> Self {
        self.envelope.remediation = Some(value.into());
        self
    }

    /// Associates an idempotent operation with this failure.
    #[must_use]
    pub fn operation(mut self, value: impl Into<String>) -> Self {
        self.envelope.operation_id = Some(value.into());
        self
    }

    /// Adds the server's optional delay without changing retry safety.
    #[must_use]
    pub fn retry_after(mut self, seconds: Option<u64>) -> Self {
        self.envelope.retry_after_seconds = seconds;
        self
    }
}

impl From<EasError> for AppError {
    fn from(error: EasError) -> Self {
        match error {
            EasError::ResourceBusy => {
                Self::new(ErrorCode::ResourceBusy, "local Exchange request queue is busy")
                    .retryable()
            }
            EasError::FeatureUnavailable(message) => {
                Self::new(ErrorCode::FeatureUnavailable, message)
            }
            EasError::Authentication => {
                Self::new(ErrorCode::AuthRequired, "Exchange rejected the account credentials")
            }
            EasError::AccessDenied => Self::new(
                ErrorCode::AccessDenied,
                "Exchange denied ActiveSync access for this account",
            )
            .remediation(
                "Ask the Exchange administrator to verify EAS access and client allowlisting",
            ),
            EasError::Network(_) => Self::new(
                ErrorCode::NetworkUnreachable,
                "Cannot reach the managed Exchange endpoint",
            )
            .retryable(),
            EasError::Throttled { retry_after_seconds } => {
                Self::new(ErrorCode::Throttled, "Exchange throttled the request")
                    .retryable()
                    .retry_after(retry_after_seconds)
            }
            EasError::HttpUnavailable { retry_after_seconds } => {
                Self::new(ErrorCode::ServiceUnavailable, "Exchange is temporarily unavailable")
                    .retryable()
                    .retry_after(retry_after_seconds)
            }
            EasError::ResponseTooLarge => Self::new(
                ErrorCode::ResultTooLarge,
                "Exchange response exceeds the command byte limit",
            ),
            EasError::ServiceUnavailable => Self::new(
                ErrorCode::ProtocolError,
                "Exchange availability service is temporarily unavailable",
            )
            .retryable(),
            EasError::OutcomeUnknown => {
                Self::new(ErrorCode::OutcomeUnknown, "The mutation outcome is unknown")
            }
            EasError::InvalidConfiguration(_) => {
                Self::new(ErrorCode::ConfigInvalid, "The managed account configuration is invalid")
            }
            EasError::InvalidSyncKey
            | EasError::InvalidFolderSyncKey
            | EasError::PolicyRefreshRequired => {
                Self::new(ErrorCode::SyncStale, "Exchange state changed; refresh and retry")
                    .retryable()
            }
            EasError::AccountRemoteWipe => {
                Self::new(ErrorCode::RemoteWipe, "Exchange removed local data for this account")
            }
            EasError::UnsupportedDevicePolicy(_) => Self::new(
                ErrorCode::PolicyBlocked,
                "Exchange requires an unsupported device policy",
            ),
            EasError::Protocol(_) => Self::new(
                ErrorCode::ProtocolError,
                "Exchange returned an invalid protocol response",
            ),
        }
    }
}

#[cfg(test)]
mod tests;
