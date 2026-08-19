use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};

use super::MailMcpServer;
use crate::ApiResponse;
use crate::model::{
    MailForwardInput, MailReplyInput, MailSendInput, MarkReadInput, OperationResult,
};

#[tool_router(router = write_tools, vis = "pub(crate)")]
impl MailMcpServer {
    /// Immediately changes a message's read state for a write-enabled account.
    #[tool(
        name = "mail_mark_read",
        annotations(
            title = "Change mail read state",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn mail_mark_read(
        &self,
        Parameters(input): Parameters<MarkReadInput>,
    ) -> Json<ApiResponse<OperationResult>> {
        Json(self.runtime.mail_mark_read(input).await)
    }

    /// Immediately sends a plain-text message for a write-enabled account.
    #[tool(
        name = "mail_send",
        annotations(
            title = "Send work mail",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn mail_send(
        &self,
        Parameters(input): Parameters<MailSendInput>,
    ) -> Json<ApiResponse<OperationResult>> {
        Json(self.runtime.mail_send(input).await)
    }

    /// Immediately replies to a referenced message for a write-enabled account.
    #[tool(
        name = "mail_reply",
        annotations(
            title = "Reply to work mail",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn mail_reply(
        &self,
        Parameters(input): Parameters<MailReplyInput>,
    ) -> Json<ApiResponse<OperationResult>> {
        Json(self.runtime.mail_reply(input).await)
    }

    /// Immediately forwards a referenced message for a write-enabled account.
    #[tool(
        name = "mail_forward",
        annotations(
            title = "Forward work mail",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn mail_forward(
        &self,
        Parameters(input): Parameters<MailForwardInput>,
    ) -> Json<ApiResponse<OperationResult>> {
        Json(self.runtime.mail_forward(input).await)
    }
}
