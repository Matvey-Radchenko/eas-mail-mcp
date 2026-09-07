use eas_mail_protocol::{Command, EasError, MeetingResponseChoice};

use super::session::EasMailbox;
use crate::backend::MailSource;
use crate::{AppError, ErrorCode, Result};

impl EasMailbox {
    pub(super) async fn respond_request(
        &self,
        source: &MailSource,
        response: MeetingResponseChoice,
    ) -> Result<Option<String>> {
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_calendar_capability(&state, Command::MeetingResponse, "MeetingResponse")?;
        let result = match source {
            MailSource::Item { folder_id, server_id } => {
                self.client.meeting_response(state.policy_key, folder_id, server_id, response).await
            }
            MailSource::LongId(long_id) => {
                self.client.meeting_response_long_id(state.policy_key, long_id, response).await
            }
        };
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            match source {
                MailSource::Item { folder_id, server_id } => {
                    self.client
                        .meeting_response(state.policy_key, folder_id, server_id, response)
                        .await
                }
                MailSource::LongId(long_id) => {
                    self.client.meeting_response_long_id(state.policy_key, long_id, response).await
                }
            }
        } else {
            result
        }
        .map_err(self.scoped_error())?;
        require_status(result.status)?;
        Ok(result.calendar_id)
    }
}

pub(super) fn require_status(status: u16) -> Result<()> {
    if status == 1 {
        return Ok(());
    }
    let message = match status {
        2 => {
            "Exchange rejected MeetingResponse: invalid meeting request or calendar item (status 2)"
        }
        3 => "Exchange could not update the mailbox for MeetingResponse (status 3)",
        4 => "Exchange could not complete MeetingResponse (status 4)",
        _ => "Exchange returned an unsupported MeetingResponse status",
    };
    Err(AppError::new(ErrorCode::ProtocolError, message).remediation(
        "Refresh the invitation or calendar reference and inspect its response status. If Exchange still rejects it, respond in Outlook or ask the original organizer for a direct invitation. Do not blindly retry with a new UUID",
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejections_explain_the_recovery_without_claiming_safe_retry() -> anyhow::Result<()> {
        assert!(super::require_status(1).is_ok());
        for status in [2, 3, 4] {
            let error = super::require_status(status)
                .err()
                .ok_or_else(|| anyhow::anyhow!("rejection expected"))?;
            assert_eq!(error.envelope.code, crate::ErrorCode::ProtocolError);
            assert!(!error.envelope.retryable);
            assert!(error.envelope.remediation.is_some());
        }
        Ok(())
    }
}
