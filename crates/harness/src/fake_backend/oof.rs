use eas_mail_mcp::{AppError, ErrorCode, Result};
use eas_mail_protocol::{OofAudience, OofSettings, OofState};

use super::FakeBackend;

#[derive(Debug)]
pub(super) struct OofFixture {
    settings: OofSettings,
    block_external: bool,
    verification_error: Option<ErrorCode>,
    set_error: Option<ErrorCode>,
    attempts: usize,
}

impl Default for OofFixture {
    fn default() -> Self {
        Self {
            settings: OofSettings {
                state: OofState::Disabled,
                starts_at: None,
                ends_at: None,
                messages: Vec::new(),
            },
            block_external: false,
            verification_error: None,
            set_error: None,
            attempts: 0,
        }
    }
}

impl FakeBackend {
    /// Scripts administrator-blocked external replies, a failed verification read, or a Set error.
    pub fn configure_auto_reply(
        &self,
        block_external: bool,
        verification_error: Option<ErrorCode>,
        set_error: Option<ErrorCode>,
    ) -> Result<()> {
        let mut fixture = self.oof.lock().map_err(|_| storage())?;
        fixture.block_external = block_external;
        fixture.verification_error = verification_error;
        fixture.set_error = set_error;
        Ok(())
    }

    /// Returns attempted Settings/Oof updates, including uncertain outcomes.
    pub fn auto_reply_attempts(&self) -> Result<usize> {
        self.oof.lock().map(|fixture| fixture.attempts).map_err(|_| storage())
    }

    pub(super) async fn read_auto_reply_fixture(&self) -> Result<OofSettings> {
        self.check().await?;
        let fixture = self.oof.lock().map_err(|_| storage())?;
        if fixture.attempts > 0
            && let Some(code) = fixture.verification_error
        {
            return Err(AppError::new(code, "scripted automatic-reply verification error"));
        }
        Ok(fixture.settings.clone())
    }

    pub(super) async fn write_auto_reply_fixture(&self, settings: &OofSettings) -> Result<()> {
        self.check_operation("set_auto_reply").await?;
        let mut fixture = self.oof.lock().map_err(|_| storage())?;
        fixture.attempts += 1;
        if let Some(code) = fixture.set_error {
            return Err(AppError::new(code, "scripted automatic-reply update error"));
        }
        self.record("set_auto_reply")?;
        fixture.settings.state = settings.state;
        fixture.settings.starts_at = settings.starts_at;
        fixture.settings.ends_at = settings.ends_at;
        for incoming in &settings.messages {
            let mut message = incoming.clone();
            if fixture.block_external && message.audience != OofAudience::Internal {
                message.enabled = false;
            }
            if let Some(existing) =
                fixture.settings.messages.iter_mut().find(|item| item.audience == message.audience)
            {
                if message.message.is_none() {
                    message.message.clone_from(&existing.message);
                }
                *existing = message;
            } else {
                fixture.settings.messages.push(message);
            }
        }
        Ok(())
    }
}

fn storage() -> AppError {
    AppError::new(ErrorCode::StorageError, "fake automatic-reply state unavailable")
}
