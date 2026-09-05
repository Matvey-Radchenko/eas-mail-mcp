use super::{EasClient, calendar_mutation_result, mutation_parse};
use crate::{
    CalendarApplication, Command, MeetingResponseChoice, MeetingResponseResult, MutationResult,
    Result, protocol,
};
use chrono::{DateTime, Utc};

impl EasClient {
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
        calendar_mutation_result(mutation_parse(protocol::parse_calendar_mutation_for(
            &response.body,
            collection_id,
            "Add",
            client_id,
        ))?)
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
        calendar_mutation_result(mutation_parse(protocol::parse_calendar_mutation_for(
            &response.body,
            collection_id,
            "Change",
            server_id,
        ))?)
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
        calendar_mutation_result(mutation_parse(protocol::parse_calendar_mutation_for(
            &response.body,
            collection_id,
            "Delete",
            server_id,
        ))?)
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
        let result = self.mutation_command(Command::MeetingResponse, &body, key).await?;
        mutation_parse(protocol::parse_meeting_response_for(
            &result.body,
            "MeetingResponse",
            request_id,
        ))
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
        let result = self.mutation_command(Command::MeetingResponse, &body, key).await?;
        mutation_parse(protocol::parse_meeting_response_for(
            &result.body,
            "MeetingResponse",
            request_id,
        ))
    }

    /// Responds to one meeting request returned by Search LongId with no network retry.
    pub async fn meeting_response_long_id(
        &self,
        key: u32,
        long_id: &str,
        response: MeetingResponseChoice,
    ) -> Result<MeetingResponseResult> {
        let body = protocol::build_meeting_response_long_id(long_id, response)?;
        let result = self.mutation_command(Command::MeetingResponse, &body, key).await?;
        mutation_parse(protocol::parse_meeting_response_for(&result.body, "Search", long_id))
    }
}
