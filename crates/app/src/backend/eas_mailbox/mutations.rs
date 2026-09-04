use eas_mail_protocol::protocol::{ComposeSource, MimeMessage, build_mime_with_attachments};
use eas_mail_protocol::{Command, EasError};

use super::super::{MailSource, OutgoingMail};
use super::session::EasMailbox;
use crate::{AppError, ErrorCode, Result};

impl EasMailbox {
    pub(super) async fn change_read(&self, source: &MailSource, is_read: bool) -> Result<()> {
        self.change_mail_property(source, &eas_mail_protocol::protocol::MailPatch::Read(is_read))
            .await
    }

    pub(super) async fn send_message(&self, client_id: &str, message: &OutgoingMail) -> Result<()> {
        let mime = self.mime(message)?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_capability(&state, Command::SendMail)?;
        let result = self.client.send(state.policy_key, client_id, mime.clone()).await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.send(state.policy_key, client_id, mime).await
        } else {
            result
        }
        .map_err(self.mutation_error())?;
        require_success(result.status)
    }

    pub(super) async fn compose(
        &self,
        forward: bool,
        client_id: &str,
        source: &MailSource,
        message: &OutgoingMail,
    ) -> Result<()> {
        let mime = self.mime(message)?;
        let mut state = self.state.lock().await;
        self.ensure_ready(&mut state).await?;
        self.require_capability(
            &state,
            if forward { Command::SmartForward } else { Command::SmartReply },
        )?;
        let source = compose_source(source);
        let result = self
            .client
            .smart_compose(state.policy_key, forward, client_id, source.clone(), mime.clone())
            .await;
        let result = if matches!(result, Err(EasError::PolicyRefreshRequired)) {
            self.refresh_policy(&mut state).await?;
            self.client.smart_compose(state.policy_key, forward, client_id, source, mime).await
        } else {
            result
        }
        .map_err(self.mutation_error())?;
        require_success(result.status)
    }

    fn mime(&self, message: &OutgoingMail) -> Result<Vec<u8>> {
        build_mime_with_attachments(
            MimeMessage {
                sender: &self.account.email,
                to: &message.to,
                cc: &message.cc,
                bcc: &message.bcc,
                subject: &message.subject,
                body: &message.body,
            },
            &message.attachments,
        )
        .map_err(self.scoped_error())
    }

    fn mutation_error(&self) -> impl FnOnce(EasError) -> AppError + '_ {
        |error| AppError::from(error).account(&self.account.account_id)
    }
}

fn compose_source(source: &MailSource) -> ComposeSource<'_> {
    match source {
        MailSource::Item { folder_id, server_id } => {
            ComposeSource::Item { folder_id, item_id: server_id }
        }
        MailSource::LongId(long_id) => ComposeSource::LongId(long_id),
    }
}

fn require_success(status: u16) -> Result<()> {
    if status == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            ErrorCode::ProtocolError,
            format!("Exchange rejected the mail mutation with status {status}"),
        ))
    }
}
