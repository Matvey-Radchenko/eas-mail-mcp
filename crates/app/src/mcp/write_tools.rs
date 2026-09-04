use rmcp::handler::server::wrapper::Parameters;

use super::response::MutationToolResponse as Json;
use rmcp::{tool, tool_router};

use super::MailMcpServer;
use crate::model::{
    CalendarCancelInput, CalendarCreateInput, CalendarDeleteInput, CalendarOperationResult,
    CalendarRespondInput, CalendarUpdateInput, MailForwardInput, MailReplyInput, MailSendInput,
    MarkReadInput, OperationResult,
};

#[tool_router(router = write_tools, vis = "pub(crate)")]
impl MailMcpServer {
    /// Changes a message's read state for a write-enabled account after explicit mail_list for its folder in this session. Missing synchronization state returns FEATURE_UNAVAILABLE; no hidden folder download.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<OperationResult>>(),
        name = "mail_mark_read",
        annotations(
            title = "Change mail read state",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_mark_read(
        &self,
        Parameters(input): Parameters<MarkReadInput>,
    ) -> Json<OperationResult> {
        Json(self.runtime.mail_mark_read(input).await)
    }

    /// Immediately sends a plain-text message for a write-enabled account.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<OperationResult>>(),
        name = "mail_send",
        annotations(
            title = "Send work mail",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_send(
        &self,
        Parameters(input): Parameters<MailSendInput>,
    ) -> Json<OperationResult> {
        Json(self.runtime.mail_send(input).await)
    }

    /// Immediately replies to a referenced message for a write-enabled account.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<OperationResult>>(),
        name = "mail_reply",
        annotations(
            title = "Reply to work mail",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_reply(
        &self,
        Parameters(input): Parameters<MailReplyInput>,
    ) -> Json<OperationResult> {
        Json(self.runtime.mail_reply(input).await)
    }

    /// Immediately forwards a referenced message for a write-enabled account.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<OperationResult>>(),
        name = "mail_forward",
        annotations(
            title = "Forward work mail",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_forward(
        &self,
        Parameters(input): Parameters<MailForwardInput>,
    ) -> Json<OperationResult> {
        Json(self.runtime.mail_forward(input).await)
    }

    /// Immediately creates a personal event or meeting, optionally with a recurrence rule.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarOperationResult>>(),
        name = "calendar_create",
        annotations(
            title = "Create calendar event",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn calendar_create(
        &self,
        Parameters(input): Parameters<CalendarCreateInput>,
    ) -> Json<CalendarOperationResult> {
        Json(self.runtime.calendar_create(input).await)
    }

    /// Immediately updates an event or organizer meeting; recurring events require an explicit scope.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarOperationResult>>(),
        name = "calendar_update",
        annotations(
            title = "Update calendar event",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn calendar_update(
        &self,
        Parameters(input): Parameters<CalendarUpdateInput>,
    ) -> Json<CalendarOperationResult> {
        Json(self.runtime.calendar_update(input).await)
    }

    /// Immediately deletes a personal event; recurring events require an explicit scope.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarOperationResult>>(),
        name = "calendar_delete",
        annotations(
            title = "Delete personal calendar event",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn calendar_delete(
        &self,
        Parameters(input): Parameters<CalendarDeleteInput>,
    ) -> Json<CalendarOperationResult> {
        Json(self.runtime.calendar_delete(input).await)
    }

    /// Immediately cancels an organizer meeting; recurring events require an explicit scope.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarOperationResult>>(),
        name = "calendar_cancel",
        annotations(
            title = "Cancel calendar meeting",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn calendar_cancel(
        &self,
        Parameters(input): Parameters<CalendarCancelInput>,
    ) -> Json<CalendarOperationResult> {
        Json(self.runtime.calendar_cancel(input).await)
    }

    /// Immediately responds to an event, a series, one occurrence, or actionable invitation mail.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarOperationResult>>(),
        name = "calendar_respond",
        annotations(
            title = "Respond to calendar meeting",
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn calendar_respond(
        &self,
        Parameters(input): Parameters<CalendarRespondInput>,
    ) -> Json<CalendarOperationResult> {
        Json(self.runtime.calendar_respond(input).await)
    }
}
