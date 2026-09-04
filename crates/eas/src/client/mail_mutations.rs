use super::{EasClient, mutation_parse};
use crate::{Command, MutationResult, Result, protocol};

impl EasClient {
    /// Applies one minimal property change with no automatic network retry.
    pub async fn mail_change(
        &self,
        key: u32,
        folder: &str,
        server_id: &str,
        sync_key: &str,
        patch: &protocol::MailPatch,
    ) -> Result<MutationResult> {
        let body = protocol::build_mail_change(folder, server_id, sync_key, patch)?;
        let response = self.mutation_command(Command::Sync, &body, key).await?;
        mutation_parse(protocol::parse_mail_change(&response.body, folder, server_id))
    }

    /// Moves one message and returns its new server identifier without an automatic retry.
    pub async fn move_mail(
        &self,
        key: u32,
        folder: &str,
        server_id: &str,
        destination: &str,
    ) -> Result<MutationResult> {
        let body = protocol::build_move(folder, server_id, destination)?;
        let response = self.mutation_command(Command::MoveItems, &body, key).await?;
        mutation_parse(protocol::parse_move(&response.body, server_id))
    }
}
