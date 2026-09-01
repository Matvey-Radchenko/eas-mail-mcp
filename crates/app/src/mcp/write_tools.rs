use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};

use super::MailMcpServer;
use crate::ApiResponse;
use crate::model::{
    CalendarCancelInput, CalendarCreateInput, CalendarDeleteInput, CalendarOperationResult,
    CalendarRespondInput, CalendarUpdateInput, MailForwardInput, MailReplyInput, MailSendInput,
    MarkReadInput, OperationResult,
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

    /// Immediately creates a personal event or meeting, optionally with a recurrence rule.
    #[tool(
        name = "calendar_create",
        annotations(
            title = "Create calendar event",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn calendar_create(
        &self,
        Parameters(input): Parameters<CalendarCreateInput>,
    ) -> Json<ApiResponse<CalendarOperationResult>> {
        Json(self.runtime.calendar_create(input).await)
    }

    /// Immediately updates an event or organizer meeting; recurring events require an explicit scope.
    #[tool(
        name = "calendar_update",
        annotations(
            title = "Update calendar event",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn calendar_update(
        &self,
        Parameters(input): Parameters<CalendarUpdateInput>,
    ) -> Json<ApiResponse<CalendarOperationResult>> {
        Json(self.runtime.calendar_update(input).await)
    }

    /// Immediately deletes a personal event; recurring events require an explicit scope.
    #[tool(
        name = "calendar_delete",
        annotations(
            title = "Delete personal calendar event",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn calendar_delete(
        &self,
        Parameters(input): Parameters<CalendarDeleteInput>,
    ) -> Json<ApiResponse<CalendarOperationResult>> {
        Json(self.runtime.calendar_delete(input).await)
    }

    /// Immediately cancels an organizer meeting; recurring events require an explicit scope.
    #[tool(
        name = "calendar_cancel",
        annotations(
            title = "Cancel calendar meeting",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn calendar_cancel(
        &self,
        Parameters(input): Parameters<CalendarCancelInput>,
    ) -> Json<ApiResponse<CalendarOperationResult>> {
        Json(self.runtime.calendar_cancel(input).await)
    }

    /// Immediately responds to an event, a series, one occurrence, or actionable invitation mail.
    #[tool(
        name = "calendar_respond",
        annotations(
            title = "Respond to calendar meeting",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn calendar_respond(
        &self,
        Parameters(input): Parameters<CalendarRespondInput>,
    ) -> Json<ApiResponse<CalendarOperationResult>> {
        Json(self.runtime.calendar_respond(input).await)
    }
}
