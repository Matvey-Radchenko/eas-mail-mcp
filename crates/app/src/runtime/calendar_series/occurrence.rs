use chrono::{DateTime, Duration, Utc};
use eas_mail_protocol::{CalendarApplication, CalendarException, Patch};

use super::{invalid, stale, zone};
use crate::Result;
use crate::runtime::calendar_agenda::recurrence::pattern::Pattern;

#[derive(Clone, Copy)]
enum DurationPolicy {
    Positive,
    NonNegative,
}

impl DurationPolicy {
    fn allows(self, duration: Duration) -> bool {
        match self {
            Self::Positive => duration > Duration::zero(),
            Self::NonNegative => duration >= Duration::zero(),
        }
    }
}

pub(in crate::runtime) fn validate_member(
    master: &CalendarApplication,
    original: DateTime<Utc>,
) -> Result<u32> {
    let rule = master.properties.recurrence.as_ref().ok_or_else(stale)?;
    let zone = zone(master)?;
    let local_master = zone.to_local(master.starts_at)?;
    let local = zone.to_local(original)?;
    let pattern = Pattern::parse(&rule.to_fields())?;
    let ordinal = pattern.ordinal(local.date(), local_master.date())?.ok_or_else(stale)?;
    if local.time() != local_master.time()
        || zone.to_utc(local)? != original
        || !pattern.allows(ordinal)
        || pattern.until.is_some_and(|until| original > until)
    {
        return Err(stale());
    }
    Ok(ordinal)
}

pub(in crate::runtime) fn selected(
    master: &CalendarApplication,
    original: DateTime<Utc>,
) -> Result<CalendarApplication> {
    select(master, original, DurationPolicy::Positive)
}

pub(super) fn selected_for_read(
    master: &CalendarApplication,
    original: DateTime<Utc>,
) -> Result<CalendarApplication> {
    select(master, original, DurationPolicy::NonNegative)
}

fn select(
    master: &CalendarApplication,
    original: DateTime<Utc>,
    policy: DurationPolicy,
) -> Result<CalendarApplication> {
    validate_member(master, original)?;
    let mut event = project_occurrence(master, original, policy)?;
    if let Some(exception) =
        master.properties.exceptions.iter().find(|value| value.original_start == original)
    {
        if exception.deleted {
            return Err(stale());
        }
        apply_exception(&mut event, exception, policy)?;
    }
    Ok(event)
}

pub(in crate::runtime) fn base_occurrence(
    master: &CalendarApplication,
    original: DateTime<Utc>,
) -> Result<CalendarApplication> {
    project_occurrence(master, original, DurationPolicy::Positive)
}

fn project_occurrence(
    master: &CalendarApplication,
    original: DateTime<Utc>,
    policy: DurationPolicy,
) -> Result<CalendarApplication> {
    let zone = zone(master)?;
    let duration = zone.to_local(master.ends_at)? - zone.to_local(master.starts_at)?;
    if !policy.allows(duration) || duration > Duration::days(366) {
        return Err(invalid("recurring duration is invalid"));
    }
    let mut event = master.clone();
    event.starts_at = original;
    event.ends_at =
        zone.to_utc(zone.to_local(original)?.checked_add_signed(duration).ok_or_else(stale)?)?;
    event.properties.recurrence = None;
    event.properties.exceptions.clear();
    event.properties.instance_start = Some(original);
    event.properties.instance_all_day = Some(master.all_day);
    Ok(event)
}

fn apply_exception(
    event: &mut CalendarApplication,
    exception: &CalendarException,
    policy: DurationPolicy,
) -> Result<()> {
    let fields = &exception.fields;
    replace(&mut event.subject, &fields.subject);
    replace(&mut event.body, &fields.body);
    replace(&mut event.location, &fields.location);
    replace(&mut event.all_day, &fields.all_day);
    replace(&mut event.attendees, &fields.attendees);
    replace(&mut event.busy_status, &fields.busy_status);
    replace(&mut event.response_requested, &fields.response_requested);
    replace(&mut event.meeting_status, &fields.meeting_status);
    if let Patch::Value(Some(value)) = fields.starts_at {
        event.starts_at = value;
    }
    if let Patch::Value(Some(value)) = fields.ends_at {
        event.ends_at = value;
    }
    if let Patch::Value(value) = fields.reminder_minutes {
        event.reminder_minutes = value;
    }
    if let Some(properties) = &fields.properties {
        if properties.sensitivity.is_some() {
            event.properties.sensitivity = properties.sensitivity;
        }
        if properties.categories.is_some() {
            event.properties.categories.clone_from(&properties.categories);
        }
    }
    if !policy.allows(event.ends_at - event.starts_at) {
        return Err(invalid("recurrence exception has invalid duration"));
    }
    Ok(())
}

fn replace<T: Clone>(target: &mut T, patch: &Patch<T>) {
    if let Patch::Value(value) = patch {
        target.clone_from(value);
    }
}
