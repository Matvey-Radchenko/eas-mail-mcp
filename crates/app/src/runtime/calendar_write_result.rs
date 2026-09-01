use crate::model::{CalendarOperationResult, CalendarOperationState};
use crate::{JournalRecord, OperationStatus};

pub(super) const STEP_ITEM: u32 = 1 << 0;
pub(super) const STEP_NOTIFY_CURRENT: u32 = 1 << 1;
pub(super) const STEP_NOTIFY_REMOVED: u32 = 1 << 2;
pub(super) const STEP_RESPONSE: u32 = 1 << 3;
pub(super) const STEP_REPLY: u32 = 1 << 4;
pub(super) const STEP_NEW_SERIES: u32 = 1 << 5;
pub(super) const STEP_TRUNCATE_SERIES: u32 = 1 << 6;
pub(super) const STEP_NOTIFY_OLD_SERIES: u32 = 1 << 7;

pub(super) fn existing(record: JournalRecord) -> CalendarOperationResult {
    let (status, message) = match record.status {
        OperationStatus::Succeeded => {
            (CalendarOperationState::Succeeded, "the prior operation was confirmed")
        }
        OperationStatus::Failed => {
            (CalendarOperationState::Failed, "the prior operation failed safely")
        }
        OperationStatus::Partial => (
            CalendarOperationState::Partial,
            "the prior operation completed only some Calendar steps",
        ),
        OperationStatus::Pending | OperationStatus::Unknown => {
            (CalendarOperationState::Unknown, "the prior operation outcome is unknown")
        }
    };
    result(&record.operation_id, status, record.completed_steps, message, None)
}

pub(super) fn result(
    operation_id: &str,
    status: CalendarOperationState,
    steps: u32,
    message: &str,
    event_ref: Option<String>,
) -> CalendarOperationResult {
    CalendarOperationResult {
        operation_id: operation_id.to_owned(),
        status,
        completed_steps: step_names(steps),
        message: message.to_owned(),
        event_ref,
    }
}

fn step_names(steps: u32) -> Vec<String> {
    [
        (STEP_ITEM, "calendar_item"),
        (STEP_NOTIFY_CURRENT, "notify_current_attendees"),
        (STEP_NOTIFY_REMOVED, "notify_removed_attendees"),
        (STEP_RESPONSE, "meeting_response"),
        (STEP_REPLY, "reply_notification"),
        (STEP_NEW_SERIES, "new_series"),
        (STEP_TRUNCATE_SERIES, "truncate_old_series"),
        (STEP_NOTIFY_OLD_SERIES, "notify_old_series"),
    ]
    .into_iter()
    .filter(|(bit, _)| steps & bit != 0)
    .map(|(_, name)| name.to_owned())
    .collect()
}
