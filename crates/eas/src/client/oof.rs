use super::{EasClient, mutation_parse};
use crate::{Command, EasError, OofSettings, Result, protocol};

impl EasClient {
    /// Reads out-of-office state and messages without altering settings.
    pub async fn get_oof(&self, key: u32) -> Result<OofSettings> {
        let body = protocol::build_oof_get()?;
        let response = self.read_command(Command::Settings, &body, key).await?;
        protocol::parse_oof_get(&response.body)
    }

    /// Sends one out-of-office update without an automatic retry after an ambiguous result.
    pub async fn set_oof(&self, key: u32, settings: &OofSettings) -> Result<()> {
        let body = protocol::build_oof_set(settings)?;
        let response = self.mutation_command(Command::Settings, &body, key).await?;
        let status = mutation_parse(protocol::parse_oof_set(&response.body))?;
        if matches!(status, 110 | 111) {
            // A server processing failure does not establish whether the update was applied.
            Err(EasError::OutcomeUnknown)
        } else if status == 1 {
            Ok(())
        } else {
            Err(EasError::Protocol(format!(
                "Exchange rejected out-of-office settings with status {status}"
            )))
        }
    }
}
