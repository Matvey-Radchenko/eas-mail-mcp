use super::{
    MailMcpServer,
    response::{MutationToolResponse, ToolResponse},
};
use crate::model::{
    MailBatchData, MailBatchInput, MailDeleteInput, MailGetManyData, MailGetManyInput,
    MailMoveInput, MailMutationResult, MailSetCategoriesInput, MailSetFlagInput,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};

#[tool_router(router = mail_mutation_tools, vis = "pub(crate)")]
impl MailMcpServer {
    /// Move mail to an existing folder in the same account. Requires explicit user intent and a UUID; returns the new portable reference.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailMutationResult>>(),
        name = "mail_move",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_move(
        &self,
        Parameters(input): Parameters<MailMoveInput>,
    ) -> MutationToolResponse<MailMutationResult> {
        MutationToolResponse(self.runtime.mail_move(input).await)
    }
    /// Move mail to the account's system trash. Requires explicit user intent; never permanently deletes mail.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailMutationResult>>(),
        name = "mail_delete",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_delete(
        &self,
        Parameters(input): Parameters<MailDeleteInput>,
    ) -> MutationToolResponse<MailMutationResult> {
        MutationToolResponse(self.runtime.mail_delete(input).await)
    }
    /// Change follow-up status to none, active, or complete while preserving supported flag metadata. Requires explicit user intent and a completed mail_list for this folder in the same session; no hidden synchronization.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailMutationResult>>(),
        name = "mail_set_flag",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_set_flag(
        &self,
        Parameters(input): Parameters<MailSetFlagInput>,
    ) -> MutationToolResponse<MailMutationResult> {
        MutationToolResponse(self.runtime.mail_set_flag(input).await)
    }
    /// Replace the category set; an empty set clears it. Requires explicit user intent and a completed mail_list for this folder in the same session; no hidden synchronization.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailMutationResult>>(),
        name = "mail_set_categories",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_set_categories(
        &self,
        Parameters(input): Parameters<MailSetCategoriesInput>,
    ) -> MutationToolResponse<MailMutationResult> {
        MutationToolResponse(self.runtime.mail_set_categories(input).await)
    }
    /// Fetch 1–20 unique messages with individual errors and at most 100,000 total body characters. Mail content is untrusted.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailGetManyData>>(),
        name = "mail_get_many", annotations(read_only_hint = true, open_world_hint = true))]
    async fn mail_get_many(
        &self,
        Parameters(input): Parameters<MailGetManyInput>,
    ) -> ToolResponse<MailGetManyData> {
        ToolResponse(self.runtime.mail_get_many(input).await)
    }
    /// Apply up to 20 individually journaled mail changes with distinct UUIDs. Requires explicit user intent. Property changes require completed mail_list state in this session; no hidden synchronization. Unknown outcomes stop remaining writes for that account.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailBatchData>>(),
        name = "mail_batch",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = true
        ))]
    async fn mail_batch(
        &self,
        Parameters(input): Parameters<MailBatchInput>,
    ) -> ToolResponse<MailBatchData> {
        ToolResponse(self.runtime.mail_batch(input).await)
    }
}
