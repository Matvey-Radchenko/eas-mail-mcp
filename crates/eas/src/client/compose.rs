use super::{EasClient, mutation_parse};
use crate::protocol::{self, ComposeSource};
use crate::{Command, EasError, MutationResult, Result, TransportResponse};

impl EasClient {
    /// Sends a new MIME message with an EAS ClientId.
    pub async fn send(&self, key: u32, client_id: &str, mime: Vec<u8>) -> Result<MutationResult> {
        let body = protocol::build_send(client_id, mime)?;
        let response = self.mutation_command(Command::SendMail, &body, key).await?;
        parse_response(response, Command::SendMail)
    }

    /// Replies to or forwards a referenced message.
    pub async fn smart_compose(
        &self,
        key: u32,
        forward: bool,
        client_id: &str,
        source: ComposeSource<'_>,
        mime: Vec<u8>,
    ) -> Result<MutationResult> {
        let body = protocol::build_smart(forward, client_id, source, mime)?;
        let command = if forward { Command::SmartForward } else { Command::SmartReply };
        let response = self.mutation_command(command, &body, key).await?;
        parse_response(response, command)
    }
}

fn parse_response(response: TransportResponse, command: Command) -> Result<MutationResult> {
    // An empty body confirms composing only with the EAS HTTP 200 acknowledgement.
    if response.status != 200 {
        return Err(EasError::OutcomeUnknown);
    }
    mutation_parse(protocol::parse_compose_for(&response.body, command))
}
