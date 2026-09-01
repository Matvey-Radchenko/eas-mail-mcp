use chrono::{DateTime, Duration, Utc};
use eas_mail_protocol::{CalendarApplication, CalendarException, CalendarFields};

use super::edit::{
    EditInput, EditPlan, ItemAction, ItemStep, add_result_preview, item_attendees, notice,
    projected,
};
use super::{invalid, original_time, prepared, selected, validate_member, zone};
use crate::Result;
use crate::backend::BackendEvent;
use crate::runtime::calendar_mime::CalendarMessageMethod;
use crate::runtime::calendar_prepare::{self, PreparedEvent};
use crate::runtime::calendar_write_result::{STEP_ITEM, STEP_NOTIFY_CURRENT, STEP_NOTIFY_REMOVED};

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
    let prior = prepared(selected(master, original)?)?;
    let mut changed = master.clone();
    changed.properties.exceptions.retain(|value| value.original_start != original);
    let exception = if let EditInput::Update(input) = input {
        if input.recurrence.is_some() {
            return Err(invalid("one occurrence cannot have its own recurrence rule"));
        }
        let projected = projected(source, &prior.mutation.application);
        let update = calendar_prepare::update(input, &projected, now, email)?;
        if update.event.mutation.application.time_zone != master.time_zone {
            return Err(invalid("an occurrence inherits its series timezone"));
        }
        if input.clear_reminder {
            return Err(invalid(
                "clearing an occurrence reminder is not representable by EAS 14.1; update the series instead",
            ));
        }
        validate_move(master, original, &update.event.mutation.application, true)?;
        add_result_preview(plan, &update.event);
        plan.preview = plan.preview.clone().field(
            "Removed attendees",
            crate::runtime::calendar_write_preview::attendee_list(&update.removed_attendees),
        );
        notice(
            plan,
            STEP_NOTIFY_CURRENT,
            &update.event,
            item_attendees(&update.event),
            CalendarMessageMethod::Request,
        );
        notice(
            plan,
            STEP_NOTIFY_REMOVED,
            &prior,
            update.removed_attendees,
            CalendarMessageMethod::Cancel,
        );
        let fields = super::exception_patch::merge(
            master,
            original,
            input,
            &update.event.mutation.application,
        );
        CalendarException { original_start: original, deleted: false, fields }
    } else {
        if matches!(input, EditInput::Cancel(_)) {
            notice(
                plan,
                STEP_NOTIFY_CURRENT,
                &prior,
                item_attendees(&prior),
                CalendarMessageMethod::Cancel,
            );
        }
        CalendarException {
            original_start: original,
            deleted: true,
            fields: CalendarFields::default(),
        }
    };
    changed.properties.exceptions.push(exception);
    changed.properties.exceptions.sort_by_key(|value| value.original_start);
    calendar_prepare::refresh_organizer_status(&mut changed);
    validate(&changed)?;
    plan.occurrence_start = matches!(input, EditInput::Update(_)).then_some(original);
    plan.steps.push(ItemStep {
        bit: STEP_ITEM,
        action: ItemAction::Update(Box::new(prepared(changed)?)),
    });
    Ok(())
}

pub(super) fn preserve(old: &CalendarApplication, new: &CalendarApplication) -> Result<()> {
    if old.properties.exceptions.is_empty() {
        return Ok(());
    }
    if old.time_zone != new.time_zone || old.all_day != new.all_day {
        return Err(invalid(
            "timezone or all-day changes cannot preserve existing exceptions unambiguously",
        ));
    }
    for exception in &old.properties.exceptions {
        let old_ordinal = validate_member(old, exception.original_start)?;
        let new_ordinal = validate_member(new, exception.original_start)
            .map_err(|_| invalid("the changed schedule would orphan an existing exception"))?;
        if old_ordinal != new_ordinal {
            return Err(invalid("the changed schedule would remap an existing exception"));
        }
    }
    Ok(())
}

pub(in crate::runtime) fn validate(item: &CalendarApplication) -> Result<()> {
    if !item.properties.can_write() {
        return Err(invalid("Calendar exceptions exceed supported bounds"));
    }
    if item.properties.recurrence.is_none() {
        return if item.properties.exceptions.is_empty() {
            Ok(())
        } else {
            Err(invalid("exceptions require a recurring master"))
        };
    }
    validate_member(item, item.starts_at)?;
    validate_move(
        item,
        item.starts_at,
        &super::occurrence::base_occurrence(item, item.starts_at)?,
        false,
    )?;
    for exception in &item.properties.exceptions {
        validate_member(item, exception.original_start)?;
        if !exception.deleted {
            validate_move(
                item,
                exception.original_start,
                &selected(item, exception.original_start)?,
                true,
            )?;
        }
    }
    Ok(())
}

fn validate_move(
    master: &CalendarApplication,
    original: DateTime<Utc>,
    event: &CalendarApplication,
    include_overrides: bool,
) -> Result<()> {
    if event.ends_at <= event.starts_at || event.ends_at - event.starts_at > Duration::days(366) {
        return Err(invalid("recurring duration is outside supported bounds"));
    }
    let zone = zone(master)?;
    let original_local = zone.to_local(original)?;
    let master_local = zone.to_local(master.starts_at)?;
    let rule = master.properties.recurrence.as_ref().ok_or_else(super::stale)?;
    let pattern =
        crate::runtime::calendar_agenda::recurrence::pattern::Pattern::parse(&rule.to_fields())?;
    let ordinal = validate_member(master, original)?;
    // Inspect neighboring dates with the same rule, never fetch or cache other events.
    for direction in [-1, 1] {
        if direction > 0
            && matches!(rule.end, eas_mail_protocol::RecurrenceEnd::Count(total) if ordinal >= u32::from(total))
        {
            continue;
        }
        for days in 1..=366_000 {
            let local = original_local
                .checked_add_signed(Duration::days(direction * days))
                .ok_or_else(super::stale)?;
            if direction < 0 && local < zone.to_local(master.starts_at)? {
                break;
            }
            let Some(next_ordinal) = pattern.ordinal(local.date(), master_local.date())? else {
                continue;
            };
            if !pattern.allows(next_ordinal) {
                break;
            }
            let candidate = zone.to_utc(local)?;
            if validate_member(master, candidate).is_ok() {
                if include_overrides
                    && master
                        .properties
                        .exceptions
                        .iter()
                        .any(|e| e.original_start == candidate && e.deleted)
                {
                    continue;
                }
                let neighbor = if include_overrides {
                    selected(master, candidate)?
                } else {
                    super::occurrence::base_occurrence(master, candidate)?
                };
                if (direction < 0 && event.starts_at < neighbor.ends_at)
                    || (direction > 0 && event.ends_at > neighbor.starts_at)
                {
                    return Err(invalid(
                        "an occurrence cannot overlap or cross its neighboring occurrences",
                    ));
                }
                break;
            }
            if direction > 0 && beyond_end(master, candidate) {
                break;
            }
        }
    }
    Ok(())
}

fn beyond_end(master: &CalendarApplication, candidate: DateTime<Utc>) -> bool {
    matches!(master.properties.recurrence.as_ref().map(|rule| &rule.end), Some(eas_mail_protocol::RecurrenceEnd::Until(until)) if candidate > *until)
}
