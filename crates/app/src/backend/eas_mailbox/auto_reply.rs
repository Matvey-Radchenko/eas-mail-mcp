use eas_mail_protocol::{Command, EasError, OofSettings};

use super::session::{EasMailbox, SessionState};
use crate::{AppError, ErrorCode, Result};

impl EasMailbox {
    pub(super) async fn read_auto_reply(&self) -> Result<OofSettings> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        require_oof(&state)?;
        let result = self.client.get_oof(state.policy_key).await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.get_oof(state.policy_key).await
        } else {
            result
        };
        result.map_err(self.scoped_error())
    }

    pub(super) async fn write_auto_reply(&self, settings: &OofSettings) -> Result<()> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        require_oof(&state)?;
        let result = self.client.set_oof(state.policy_key, settings).await;
        // HTTP 449 rejects the request before execution; only that case may retry.
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.set_oof(state.policy_key, settings).await
        } else {
            result
        };
        result.map_err(self.scoped_error())
    }
}

fn require_oof(state: &SessionState) -> Result<()> {
    if state.capabilities.as_ref().is_some_and(|value| value.supports(Command::Settings)) {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::FeatureUnavailable,
            "Exchange does not advertise automatic-reply settings",
        ))
    }
}
