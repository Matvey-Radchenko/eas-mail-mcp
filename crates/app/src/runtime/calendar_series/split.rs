use chrono::{DateTime, Utc};
use eas_mail_protocol::{CalendarApplication, RecurrenceEnd};

use super::edit::{
    EditInput, EditPlan, ItemAction, ItemStep, add_result_preview, item_attendees, notice,
    projected,
};
use super::{invalid, original_time, prepared, validate_member};
use crate::Result;
use crate::backend::BackendEvent;
use crate::runtime::calendar_mime::CalendarMessageMethod;
use crate::runtime::calendar_prepare::{self, PreparedEvent};
use crate::runtime::calendar_write_result::{
    STEP_NEW_SERIES, STEP_NOTIFY_CURRENT, STEP_NOTIFY_OLD_SERIES, STEP_NOTIFY_REMOVED,
    STEP_TRUNCATE_SERIES,
};
use crate::runtime::calendar_write_support::{operation_uid, step_client_id};

pub(super) fn edit(
    input: &EditInput,
    source: &BackendEvent,
    old: &PreparedEvent,
    now: DateTime<Utc>,
    email: &str,
    plan: &mut EditPlan,
) -> Result<()> {
    let original = original_time(source)?;
    let master = &old.mutation.application;
    let ordinal = validate_member(master, original)?;
    let shortened = truncate(master, original)?;
    if let EditInput::Update(update_input) = input {
        let mut tail = master.clone();
        tail.properties.instance_start = None;
        tail.properties.instance_all_day = None;
        tail.properties.recurrence = master.properties.recurrence.clone();
        tail.properties.exceptions = master
            .properties
            .exceptions
            .iter()
            .filter(|value| value.original_start >= original)
            .cloned()
            .collect();
        if let Some(rule) = &mut tail.properties.recurrence
            && let RecurrenceEnd::Count(total) = rule.end
        {
            rule.end = RecurrenceEnd::Count(
                u16::try_from(u32::from(total) - ordinal + 1)
                    .map_err(|_| invalid("series count overflow"))?,
            );
        }
        // Anchor the new master to the original slot; preserve a moved selected exception as an override.
        let anchor = super::occurrence::base_occurrence(master, original)?;
        tail.starts_at = anchor.starts_at;
        tail.ends_at = anchor.ends_at;
        let projected = projected(source, &tail);
        let mut update = calendar_prepare::update(update_input, &projected, now, email)?;
        let item = &mut update.event.mutation.application;
        if let Some(rule) = &update_input.recurrence {
            item.properties.recurrence = Some(super::rule::prepare(rule, item)?);
        }
        super::exceptions::preserve(&tail, item)?;
        super::exceptions::validate(item)?;
        item.uid = operation_uid(&step_client_id(input.key(), "new-series-uid")?)?;
        for attendee in &mut item.attendees {
            attendee.attendee_status = 0;
        }
        for exception in &mut item.properties.exceptions {
            exception.fields.response_type = eas_mail_protocol::Patch::Missing;
            if let eas_mail_protocol::Patch::Value(attendees) = &mut exception.fields.attendees {
                for attendee in attendees {
                    attendee.attendee_status = 0;
                }
            }
        }
        add_result_preview(plan, &update.event);
        notice(
            plan,
            STEP_NOTIFY_CURRENT,
            &update.event,
            item_attendees(&update.event),
            CalendarMessageMethod::Request,
        );
        plan.steps.push(ItemStep {
            bit: STEP_NEW_SERIES,
            action: ItemAction::Create(Box::new(update.event)),
        });
    }
    let shortened = prepared(shortened)?;
    let remaining = item_attendees(&shortened);
    let removed = item_attendees(old)
        .into_iter()
        .filter(|attendee| {
            !remaining.iter().any(|current| current.email.eq_ignore_ascii_case(&attendee.email))
        })
        .collect();
    // Updating the master's RRULE removes the tail without unsupported RANGE=THISANDFUTURE.
    notice(plan, STEP_NOTIFY_OLD_SERIES, &shortened, remaining, CalendarMessageMethod::Request);
    notice(plan, STEP_NOTIFY_REMOVED, old, removed, CalendarMessageMethod::Cancel);
    plan.preview = plan.preview.clone().field("Old series ends before", original.to_rfc3339());
    plan.steps.push(ItemStep {
        bit: STEP_TRUNCATE_SERIES,
        action: ItemAction::Update(Box::new(shortened)),
    });
    Ok(())
}

fn truncate(master: &CalendarApplication, original: DateTime<Utc>) -> Result<CalendarApplication> {
    let count = validate_member(master, original)?
        .checked_sub(1)
        .and_then(|count| u16::try_from(count).ok())
        .filter(|count| *count > 0)
        .ok_or_else(|| invalid("series prefix exceeds the EAS occurrence count limit"))?;
    let mut value = master.clone();
    let rule = value.properties.recurrence.as_mut().ok_or_else(super::stale)?;
    rule.end = RecurrenceEnd::Count(count);
    value.properties.exceptions.retain(|exception| exception.original_start < original);
    calendar_prepare::refresh_organizer_status(&mut value);
    Ok(value)
}
