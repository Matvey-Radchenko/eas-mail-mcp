use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};

use super::MailMcpServer;
use crate::ApiResponse;
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
        name = "people_search",
        annotations(
            title = "Find directory people",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn people_search(
        &self,
        Parameters(input): Parameters<PeopleSearchInput>,
    ) -> Json<ApiResponse<PeopleSearchData>> {
        Json(self.runtime.people_search(input).await)
    }

    /// Lists configured managed accounts without returning credentials.
    #[tool(
        name = "accounts_list",
        annotations(
            title = "List work mail accounts",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn accounts_list(&self) -> Json<ApiResponse<AccountsData>> {
        Json(self.runtime.accounts_list())
    }

    /// Refreshes and lists Exchange mail and calendar folders.
    #[tool(
        name = "folders_list",
        annotations(
            title = "List Exchange folders",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn folders_list(
        &self,
        Parameters(input): Parameters<AccountSelection>,
    ) -> Json<ApiResponse<FoldersData>> {
        Json(self.runtime.folders_list(input).await)
    }

    /// Returns synchronization state held by this MCP process.
    #[tool(
        name = "sync_status",
        annotations(
            title = "Get mail synchronization status",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn sync_status(
        &self,
        Parameters(input): Parameters<AccountSelection>,
    ) -> Json<ApiResponse<SyncData>> {
        Json(self.runtime.sync_status(input))
    }

    /// Refreshes selected mail collections immediately.
    #[tool(
        name = "sync_now",
        annotations(
            title = "Synchronize work mail now",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn sync_now(
        &self,
        Parameters(input): Parameters<AccountSelection>,
    ) -> Json<ApiResponse<SyncData>> {
        Json(self.runtime.sync_now(input).await)
    }

    /// Lists at most 100 messages from a fresh Inbox and Sent snapshot by default.
    #[tool(
        name = "mail_list",
        annotations(title = "List work mail", read_only_hint = true, open_world_hint = true)
    )]
    async fn mail_list(
        &self,
        Parameters(input): Parameters<MailListInput>,
    ) -> Json<ApiResponse<MailPage>> {
        Json(self.runtime.mail_list(input).await)
    }

    /// Searches Exchange directly and returns a 15-minute snapshot cursor.
    #[tool(
        name = "mail_search",
        annotations(title = "Search work mail", read_only_hint = true, open_world_hint = true)
    )]
    async fn mail_search(
        &self,
        Parameters(input): Parameters<MailSearchInput>,
    ) -> Json<ApiResponse<MailPage>> {
        Json(self.runtime.mail_search(input).await)
    }

    /// Fetches a sanitized plain-text message body only on demand.
    #[tool(
        name = "mail_get",
        annotations(
            title = "Read work mail message",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn mail_get(
        &self,
        Parameters(input): Parameters<MailGetInput>,
    ) -> Json<ApiResponse<MailDetail>> {
        Json(self.runtime.mail_get(input).await)
    }

    /// Lists message attachment metadata without downloading payloads.
    #[tool(
        name = "mail_list_attachments",
        annotations(
            title = "List mail attachments",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn mail_list_attachments(
        &self,
        Parameters(input): Parameters<MailAttachmentsInput>,
    ) -> Json<ApiResponse<AttachmentsData>> {
        Json(self.runtime.mail_list_attachments(input).await)
    }

    /// Downloads one attachment into a private managed 24-hour cache.
    #[tool(
        name = "mail_download_attachment",
        annotations(
            title = "Download mail attachment",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn mail_download_attachment(
        &self,
        Parameters(input): Parameters<AttachmentDownloadInput>,
    ) -> Json<ApiResponse<AttachmentDownload>> {
        Json(self.runtime.mail_download_attachment(input).await)
    }

    /// Resolves directory recipients and returns compact 30-minute free/busy intervals.
    #[tool(
        name = "calendar_availability",
        annotations(
            title = "Check calendar availability",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn calendar_availability(
        &self,
        Parameters(input): Parameters<CalendarAvailabilityInput>,
    ) -> Json<ApiResponse<CalendarAvailabilityData>> {
        Json(self.runtime.calendar_availability(input).await)
    }

    /// Finds common free windows without returning other people's meeting content.
    #[tool(
        name = "calendar_find_slots",
        annotations(
            title = "Find common calendar slots",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn calendar_find_slots(
        &self,
        Parameters(input): Parameters<CalendarFindSlotsInput>,
    ) -> Json<ApiResponse<CalendarSlotsData>> {
        Json(self.runtime.calendar_find_slots(input).await)
    }

    /// Searches own-calendar text or returns a compact date-range agenda.
    #[tool(
        name = "calendar_search",
        annotations(
            title = "Search or list work calendar",
            read_only_hint = true,
            open_world_hint = true
        )
    )]
    async fn calendar_search(
        &self,
        Parameters(input): Parameters<CalendarSearchInput>,
    ) -> Json<ApiResponse<CalendarSearchData>> {
        Json(self.runtime.calendar_search(input).await)
    }

    /// Fetches one own-calendar event without a write side effect.
    #[tool(
        name = "calendar_get",
        annotations(title = "Read calendar event", read_only_hint = true, open_world_hint = true)
    )]
    async fn calendar_get(
        &self,
        Parameters(input): Parameters<CalendarGetInput>,
    ) -> Json<ApiResponse<CalendarEvent>> {
        Json(self.runtime.calendar_get(input).await)
    }
}
