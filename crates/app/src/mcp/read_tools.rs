use rmcp::handler::server::wrapper::Parameters;

use super::response::ToolResponse as Json;
use rmcp::{tool, tool_router};

use super::MailMcpServer;
use crate::model::{
    AccountSelection, AccountsData, AttachmentDownload, AttachmentDownloadInput, AttachmentsData,
    CalendarAvailabilityData, CalendarAvailabilityInput, CalendarEvent, CalendarFindSlotsInput,
    CalendarGetInput, CalendarSearchData, CalendarSearchInput, CalendarSlotsData, FoldersData,
    MailAttachmentsInput, MailDetail, MailGetInput, MailListInput, MailPage, MailSearchInput,
    PeopleSearchData, PeopleSearchInput, SyncData,
};

#[tool_router(router = read_tools, vis = "pub(crate)")]
impl MailMcpServer {
    /// Searches one account's directory for names and email addresses, without calendar data.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<PeopleSearchData>>(),
        name = "people_search",
        annotations(
            title = "Find directory people",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn people_search(
        &self,
        Parameters(input): Parameters<PeopleSearchInput>,
    ) -> Json<PeopleSearchData> {
        Json(self.runtime.people_search(input).await)
    }

    /// Lists configured managed accounts without returning credentials.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<AccountsData>>(),
        name = "accounts_list",
        annotations(
            title = "List work mail accounts",
            read_only_hint = true,
            open_world_hint = false
        ))]
    async fn accounts_list(&self) -> Json<AccountsData> {
        Json(self.runtime.accounts_list())
    }

    /// Refreshes and lists Exchange mail and calendar folders.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<FoldersData>>(),
        name = "folders_list",
        annotations(
            title = "List Exchange folders",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn folders_list(
        &self,
        Parameters(input): Parameters<AccountSelection>,
    ) -> Json<FoldersData> {
        Json(self.runtime.folders_list(input).await)
    }

    /// Returns synchronization state held by this MCP process.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<SyncData>>(),
        name = "sync_status",
        annotations(
            title = "Get mail synchronization status",
            read_only_hint = true,
            open_world_hint = false
        ))]
    async fn sync_status(&self, Parameters(input): Parameters<AccountSelection>) -> Json<SyncData> {
        Json(self.runtime.sync_status(input))
    }

    /// Refreshes selected mail collections immediately.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<SyncData>>(),
        name = "sync_now",
        annotations(
            title = "Synchronize work mail now",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn sync_now(&self, Parameters(input): Parameters<AccountSelection>) -> Json<SyncData> {
        Json(self.runtime.sync_now(input).await)
    }

    /// Lists at most 100 messages from a fresh Inbox and Sent snapshot by default.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailPage>>(),
        name = "mail_list",
        annotations(title = "List work mail", read_only_hint = true, open_world_hint = true))]
    async fn mail_list(&self, Parameters(input): Parameters<MailListInput>) -> Json<MailPage> {
        Json(self.runtime.mail_list(input).await)
    }

    /// Searches Exchange directly and returns a 15-minute snapshot cursor.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailPage>>(),
        name = "mail_search",
        annotations(title = "Search work mail", read_only_hint = true, open_world_hint = true))]
    async fn mail_search(&self, Parameters(input): Parameters<MailSearchInput>) -> Json<MailPage> {
        Json(self.runtime.mail_search(input).await)
    }

    /// Fetches a sanitized plain-text message body only on demand.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<MailDetail>>(),
        name = "mail_get",
        annotations(
            title = "Read work mail message",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn mail_get(&self, Parameters(input): Parameters<MailGetInput>) -> Json<MailDetail> {
        Json(self.runtime.mail_get(input).await)
    }

    /// Lists message attachment metadata without downloading payloads.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<AttachmentsData>>(),
        name = "mail_list_attachments",
        annotations(
            title = "List mail attachments",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn mail_list_attachments(
        &self,
        Parameters(input): Parameters<MailAttachmentsInput>,
    ) -> Json<AttachmentsData> {
        Json(self.runtime.mail_list_attachments(input).await)
    }

    /// Downloads one attachment into a private managed 24-hour cache.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<AttachmentDownload>>(),
        name = "mail_download_attachment",
        annotations(
            title = "Download mail attachment",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn mail_download_attachment(
        &self,
        Parameters(input): Parameters<AttachmentDownloadInput>,
    ) -> Json<AttachmentDownload> {
        Json(self.runtime.mail_download_attachment(input).await)
    }

    /// Resolves directory recipients and returns compact 30-minute free/busy intervals.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarAvailabilityData>>(),
        name = "calendar_availability",
        annotations(
            title = "Check calendar availability",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn calendar_availability(
        &self,
        Parameters(input): Parameters<CalendarAvailabilityInput>,
    ) -> Json<CalendarAvailabilityData> {
        Json(self.runtime.calendar_availability(input).await)
    }

    /// Finds common free windows without returning other people's meeting content.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarSlotsData>>(),
        name = "calendar_find_slots",
        annotations(
            title = "Find common calendar slots",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn calendar_find_slots(
        &self,
        Parameters(input): Parameters<CalendarFindSlotsInput>,
    ) -> Json<CalendarSlotsData> {
        Json(self.runtime.calendar_find_slots(input).await)
    }

    /// Searches own-calendar text or returns a compact date-range agenda.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarSearchData>>(),
        name = "calendar_search",
        annotations(
            title = "Search or list work calendar",
            read_only_hint = true,
            open_world_hint = true
        ))]
    async fn calendar_search(
        &self,
        Parameters(input): Parameters<CalendarSearchInput>,
    ) -> Json<CalendarSearchData> {
        Json(self.runtime.calendar_search(input).await)
    }

    /// Fetches one own-calendar event without a write side effect.
    #[tool(
        output_schema = rmcp::handler::server::tool::schema_for_output::<crate::ApiResponse<CalendarEvent>>(),
        name = "calendar_get",
        annotations(title = "Read calendar event", read_only_hint = true, open_world_hint = true))]
    async fn calendar_get(
        &self,
        Parameters(input): Parameters<CalendarGetInput>,
    ) -> Json<CalendarEvent> {
        Json(self.runtime.calendar_get(input).await)
    }
}
