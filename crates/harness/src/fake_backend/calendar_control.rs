use super::{FakeBackend, failure};
use eas_mail_mcp::backend::BackendEvent;
use eas_mail_mcp::{ErrorCode, Result};

impl FakeBackend {
    /// Includes a fixed recurring meeting in agenda responses for cross-process CLI tests.
    #[must_use]
    pub const fn with_series_fixture(mut self) -> Self {
        self.include_series = true;
        self
    }

    /// Fails a named operation after this many successful calls of that operation.
    pub fn fail_calendar_step_after(
        &self,
        name: &str,
        successes: usize,
        code: ErrorCode,
    ) -> Result<()> {
        *self.operation_failure.lock().map_err(|_| failure(ErrorCode::StorageError))? =
            Some((name.to_owned(), successes, code));
        Ok(())
    }

    /// Installs a deterministic server-side Calendar pre-image without a client mutation.
    pub fn put_calendar_fixture(&self, event: BackendEvent) -> Result<()> {
        self.store_calendar_item(event)
    }

    /// Returns notifications emitted by the runtime, without contacting any server.
    pub fn calendar_messages(&self) -> Result<Vec<Vec<u8>>> {
        self.calendar_messages
            .lock()
            .map(|value| value.clone())
            .map_err(|_| failure(ErrorCode::StorageError))
    }

    /// Returns original occurrence identifiers supplied to MeetingResponse.
    pub fn calendar_responses(&self) -> Result<Vec<Option<chrono::DateTime<chrono::Utc>>>> {
        self.calendar_responses
            .lock()
            .map(|value| value.clone())
            .map_err(|_| failure(ErrorCode::StorageError))
    }
}
