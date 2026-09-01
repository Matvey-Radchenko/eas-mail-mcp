use std::collections::BTreeSet;
use std::sync::Arc;

use crate::protocol::{self, ComposeSource, PolicyDecision};
use chrono::{DateTime, Utc};

use crate::{
    CalendarApplication, CalendarItemResult, CollectionKind, Command, EasError, FolderPage,
    ItemResult, MeetingResponseChoice, MeetingResponseResult, MutationResult,
    RecipientAvailability, RequestSafety, Result, SearchCalendarPage, SearchMail, SyncPage,
    Transport,
};

/// Successfully acknowledged Exchange policy and its final key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NegotiatedPolicy {
    /// Final policy key to persist in Keychain.
    pub key: u32,
    /// Enforceable policy limits.
    pub decision: PolicyDecision,
}

/// EAS 14.1 commands advertised by one endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCapabilities {
    commands: BTreeSet<Command>,
}

impl ServerCapabilities {
    /// Reports whether the endpoint advertises one command.
    #[must_use]
    pub fn supports(&self, command: Command) -> bool {
        self.commands.contains(&command)
    }

    /// Reports whether all externally visible compose operations are available.
    #[must_use]
    pub fn supports_writes(&self) -> bool {
        [Command::SendMail, Command::SmartReply, Command::SmartForward]
            .into_iter()
            .all(|command| self.supports(command))
    }

    /// Reports whether personal Calendar Add, Change, and Delete can use Sync.
    #[must_use]
    pub fn supports_personal_calendar_writes(&self) -> bool {
        self.supports(Command::Sync)
    }

    /// Reports whether the full meeting response lifecycle is advertised.
    #[must_use]
    pub fn supports_meeting_lifecycle(&self) -> bool {
        self.supports_personal_calendar_writes()
            && self.supports(Command::SendMail)
            && self.supports(Command::MeetingResponse)
    }
}

/// Stateless EAS command client over an injected transport.
pub struct EasClient {
    transport: Arc<dyn Transport>,
}

impl EasClient {
    /// Creates a client over a strict production or scripted test transport.
    #[must_use]
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        Self { transport }
    }

    /// Verifies EAS 14.1 and returns the endpoint's advertised capabilities.
    pub async fn options(&self) -> Result<ServerCapabilities> {
        let response = self.transport.options().await?;
        require_http_success(response.status)?;
        let versions =
            response.headers.get("ms-asprotocolversions").map(String::as_str).unwrap_or_default();
        if !versions.split(',').any(|value| value.trim() == "14.1") {
            return Err(EasError::Protocol("Exchange does not advertise EAS 14.1".into()));
        }
        let advertised =
            response.headers.get("ms-asprotocolcommands").map(String::as_str).unwrap_or_default();
        let commands = [
            Command::Provision,
            Command::FolderSync,
            Command::Sync,
            Command::Search,
            Command::ItemOperations,
            Command::SendMail,
            Command::SmartReply,
            Command::SmartForward,
            Command::MeetingResponse,
            Command::ResolveRecipients,
        ]
        .into_iter()
        .filter(|command| advertised.split(',').any(|value| value.trim() == command.name()))
        .collect::<BTreeSet<_>>();
        for required in [
            Command::Provision,
            Command::FolderSync,
            Command::Sync,
            Command::Search,
            Command::ItemOperations,
        ] {
            if !commands.contains(&required) {
                return Err(EasError::Protocol(format!(
                    "Exchange does not advertise required command {}",
                    required.name()
                )));
            }
        }
        Ok(ServerCapabilities { commands })
    }

    /// Negotiates and acknowledges only policy requirements the client can enforce.
    pub async fn provision(&self) -> Result<NegotiatedPolicy> {
        let body = protocol::build_initial_provision()?;
        let response = self
            .transport
            .command(Command::Provision, &body, None, RequestSafety::RetrySafe)
            .await?;
        require_http_success(response.status)?;
        let initial = protocol::parse_provision(&response.body)?;
        if initial.remote_wipe || initial.account_only_remote_wipe {
            let acknowledgement = protocol::build_wipe_ack(initial.account_only_remote_wipe)?;
            let _ = self
                .transport
                .command(
                    Command::Provision,
                    &acknowledgement,
                    initial.policy_key,
                    RequestSafety::Mutation,
                )
                .await;
            self.transport.purge_secrets().await;
            return Err(EasError::AccountRemoteWipe);
        }
        if initial.status != 1 {
            return Err(EasError::Protocol(format!("Provision status is {}", initial.status)));
        }
        let temporary_key = initial
            .policy_key
            .ok_or_else(|| EasError::Protocol("Provision returned no policy key".into()))?;
        let decision = protocol::evaluate_policy(&initial.policy);
        let acknowledgement = protocol::build_policy_ack(temporary_key, decision.supported)?;
        let response = self
            .transport
            .command(Command::Provision, &acknowledgement, Some(0), RequestSafety::RetrySafe)
            .await?;
        require_http_success(response.status)?;
        let acknowledged = protocol::parse_provision(&response.body)?;
        if !decision.supported {
            return Err(EasError::UnsupportedDevicePolicy(decision.reasons.join("; ")));
        }
        if acknowledged.status != 1 || acknowledged.policy_status.is_some_and(|status| status != 1)
        {
            return Err(EasError::Protocol("Exchange rejected policy acknowledgement".into()));
        }
        let key = acknowledged
            .policy_key
            .ok_or_else(|| EasError::Protocol("Provision returned no final policy key".into()))?;
        Ok(NegotiatedPolicy { key, decision })
    }

    /// Synchronizes folder hierarchy once.
    pub async fn folder_sync(&self, key: u32, sync_key: &str) -> Result<FolderPage> {
        let body = protocol::build_folder_sync(sync_key)?;
        let response = self.read_command(Command::FolderSync, &body, key).await?;
        let page = protocol::parse_folder_sync(&response.body)?;
        match page.status {
            1 => Ok(page),
            9 => Err(EasError::InvalidFolderSyncKey),
            status => Err(EasError::Protocol(format!("FolderSync status is {status}"))),
        }
    }

    /// Synchronizes one collection page.
    pub async fn sync(
        &self,
        key: u32,
        collection_id: &str,
        sync_key: &str,
        kind: CollectionKind,
        filter_type: u8,
        preview_size: usize,
    ) -> Result<SyncPage> {
        let body = protocol::build_sync(collection_id, sync_key, kind, filter_type, preview_size)?;
        let response = self.read_command(Command::Sync, &body, key).await?;
        if response.body.is_empty() && sync_key != "0" {
            return Ok(SyncPage {
                account_status: 1,
                collection_status: 1,
                sync_key: sync_key.to_owned(),
                more_available: false,
                changes: Vec::new(),
            });
        }
        let page = protocol::parse_sync(&response.body, kind)?;
        match page.collection_status {
            1 => Ok(page),
            3 => Err(EasError::InvalidSyncKey),
            status => Err(EasError::Protocol(format!("Sync status is {status}"))),
        }
    }

    /// Searches mail on Exchange instead of a local cache.
    pub async fn search(
        &self,
        key: u32,
        query: &str,
        start: usize,
        limit: usize,
        preview_size: usize,
    ) -> Result<Vec<SearchMail>> {
        let body = protocol::build_search(query, start, limit, preview_size)?;
        let response = self.read_command(Command::Search, &body, key).await?;
        protocol::parse_search(&response.body)
    }

    /// Searches Calendar items on Exchange instead of synchronizing all future events.
    pub async fn search_calendar(
        &self,
        key: u32,
        query: &str,
        start: usize,
        limit: usize,
    ) -> Result<SearchCalendarPage> {
        let body = protocol::build_calendar_search(query, start, limit)?;
        let response = self.read_command(Command::Search, &body, key).await?;
        protocol::parse_calendar_search(&response.body)
    }

    /// Searches the global address list without downloading unrelated directory properties.
    pub async fn search_people(
        &self,
        key: u32,
        query: &str,
        limit: usize,
    ) -> Result<protocol::DirectoryPage> {
        let body = protocol::build_people_search(query, limit)?;
        let response = self.read_command(Command::Search, &body, key).await?;
        protocol::parse_people_search(&response.body, limit)
    }

    /// Resolves recipients and retrieves one bounded free/busy range.
    pub async fn availability(
        &self,
        key: u32,
        participants: &[String],
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
    ) -> Result<Vec<RecipientAvailability>> {
        let body = protocol::build_availability(participants, starts_at, ends_at)?;
        let duration = ends_at.signed_duration_since(starts_at).num_milliseconds();
        let slots = duration
            .checked_add(1_799_999)
            .and_then(|value| usize::try_from(value / 1_800_000).ok())
            .ok_or_else(|| {
                EasError::InvalidConfiguration("availability range is invalid".into())
            })?;
        let response = self.read_command(Command::ResolveRecipients, &body, key).await?;
        protocol::parse_availability(&response.body, slots)
    }

    /// Fetches a full mail item on demand.
    pub async fn fetch_item(
        &self,
        key: u32,
        long_id: Option<&str>,
        collection_id: Option<&str>,
        server_id: Option<&str>,
        body_limit: usize,
    ) -> Result<ItemResult> {
        let body =
            protocol::build_item_fetch(long_id, collection_id, server_id, body_limit.min(50_000))?;
        let response = self.read_command(Command::ItemOperations, &body, key).await?;
        protocol::parse_item_fetch(&response.body)
    }

    /// Fetches one full Calendar item by Search LongId.
    pub async fn fetch_calendar_item(
        &self,
        key: u32,
        long_id: &str,
        body_limit: usize,
    ) -> Result<CalendarItemResult> {
        let body = protocol::build_item_fetch(Some(long_id), None, None, body_limit.min(50_000))?;
        let response = self.read_command(Command::ItemOperations, &body, key).await?;
        protocol::parse_calendar_item_fetch(&response.body)
    }

    /// Fetches a Calendar item by LongId or collection/server identifiers.
    pub async fn fetch_calendar_source(
        &self,
        key: u32,
        long_id: Option<&str>,
        collection_id: Option<&str>,
        server_id: Option<&str>,
        body_limit: usize,
    ) -> Result<CalendarItemResult> {
        let body =
            protocol::build_item_fetch(long_id, collection_id, server_id, body_limit.min(50_000))?;
        let response = self.read_command(Command::ItemOperations, &body, key).await?;
        protocol::parse_calendar_item_fetch(&response.body)
    }

    /// Downloads one attachment on demand.
    pub async fn fetch_attachment(&self, key: u32, reference: &str) -> Result<Vec<u8>> {
        let body = protocol::build_attachment_fetch(reference)?;
        let response = self.read_command(Command::ItemOperations, &body, key).await?;
        protocol::parse_attachment_fetch(&response.body)
    }

    /// Changes read state with no automatic network retry.
    pub async fn mark_read(
        &self,
        key: u32,
        collection_id: &str,
        server_id: &str,
        sync_key: &str,
        is_read: bool,
    ) -> Result<MutationResult> {
        let body = protocol::build_mark_read(collection_id, server_id, sync_key, is_read)?;
        let response = self.mutation_command(Command::Sync, &body, key).await?;
        protocol::parse_mutation_sync(&response.body)
    }

    /// Adds one non-recurring Calendar item with no automatic network retry.
    pub async fn calendar_add(
        &self,
        key: u32,
        collection_id: &str,
        sync_key: &str,
        client_id: &str,
        item: &CalendarApplication,
    ) -> Result<MutationResult> {
        let body = protocol::build_calendar_add(collection_id, sync_key, client_id, item)?;
        let response = self.mutation_command(Command::Sync, &body, key).await?;
        calendar_mutation_result(protocol::parse_calendar_mutation_sync(&response.body)?)
    }

    /// Replaces one non-recurring Calendar item with no automatic network retry.
    pub async fn calendar_change(
        &self,
        key: u32,
        collection_id: &str,
        server_id: &str,
        sync_key: &str,
        item: &CalendarApplication,
    ) -> Result<MutationResult> {
        let body = protocol::build_calendar_change(collection_id, sync_key, server_id, item)?;
        let response = self.mutation_command(Command::Sync, &body, key).await?;
        calendar_mutation_result(protocol::parse_calendar_mutation_sync(&response.body)?)
    }

    /// Deletes one Calendar item with no automatic network retry.
    pub async fn calendar_delete(
        &self,
        key: u32,
        collection_id: &str,
        server_id: &str,
        sync_key: &str,
    ) -> Result<MutationResult> {
        let body = protocol::build_calendar_delete(collection_id, sync_key, server_id)?;
        let response = self.mutation_command(Command::Sync, &body, key).await?;
        calendar_mutation_result(protocol::parse_calendar_mutation_sync(&response.body)?)
    }

    /// Responds to one meeting request with no automatic network retry.
    pub async fn meeting_response(
        &self,
        key: u32,
        collection_id: &str,
        request_id: &str,
        response: MeetingResponseChoice,
    ) -> Result<MeetingResponseResult> {
        let body = protocol::build_meeting_response(collection_id, request_id, response)?;
        let response = self.mutation_command(Command::MeetingResponse, &body, key).await?;
        protocol::parse_meeting_response(&response.body)
    }

    /// Responds to one meeting request returned by Search LongId with no network retry.
    pub async fn meeting_response_instance(
        &self,
        key: u32,
        collection_id: &str,
        request_id: &str,
        response: MeetingResponseChoice,
        original: Option<DateTime<Utc>>,
    ) -> Result<MeetingResponseResult> {
        let body = protocol::build_meeting_response_instance(
            collection_id,
            request_id,
            response,
            original,
        )?;
        let response = self.mutation_command(Command::MeetingResponse, &body, key).await?;
        protocol::parse_meeting_response(&response.body)
    }

    /// Responds to one meeting request returned by Search LongId with no network retry.
    pub async fn meeting_response_long_id(
        &self,
        key: u32,
        long_id: &str,
        response: MeetingResponseChoice,
    ) -> Result<MeetingResponseResult> {
        let body = protocol::build_meeting_response_long_id(long_id, response)?;
        let response = self.mutation_command(Command::MeetingResponse, &body, key).await?;
        protocol::parse_meeting_response(&response.body)
    }

    /// Sends a new MIME message with an EAS ClientId.
    pub async fn send(&self, key: u32, client_id: &str, mime: Vec<u8>) -> Result<MutationResult> {
        let body = protocol::build_send(client_id, mime)?;
        let response = self.mutation_command(Command::SendMail, &body, key).await?;
        protocol::parse_compose(&response.body)
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
        protocol::parse_compose(&response.body)
    }

    async fn read_command(
        &self,
        command: Command,
        body: &[u8],
        key: u32,
    ) -> Result<crate::TransportResponse> {
        let response =
            self.transport.command(command, body, Some(key), RequestSafety::RetrySafe).await?;
        normalize_command_response(response)
    }

    async fn mutation_command(
        &self,
        command: Command,
        body: &[u8],
        key: u32,
    ) -> Result<crate::TransportResponse> {
        let response =
            self.transport.command(command, body, Some(key), RequestSafety::Mutation).await?;
        normalize_command_response(response)
    }
}

fn calendar_mutation_result(result: MutationResult) -> Result<MutationResult> {
    if result.status == 3 { Err(EasError::InvalidSyncKey) } else { Ok(result) }
}

fn normalize_command_response(
    response: crate::TransportResponse,
) -> Result<crate::TransportResponse> {
    if response.status == 449 {
        return Err(EasError::PolicyRefreshRequired);
    }
    require_http_success(response.status)?;
    Ok(response)
}

fn require_http_success(status: u16) -> Result<()> {
    match status {
        200 | 201 | 204 => Ok(()),
        401 => Err(EasError::Authentication),
        403 => Err(EasError::AccessDenied),
        status => Err(EasError::Protocol(format!("Exchange returned HTTP {status}"))),
    }
}
