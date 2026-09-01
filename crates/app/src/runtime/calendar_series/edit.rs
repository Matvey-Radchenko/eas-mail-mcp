use chrono::{DateTime, Utc};
use eas_mail_protocol::{CalendarApplication, CalendarAttendee};
use serde::Serialize;

use super::{invalid, original_time, selected, validate_member};
use crate::Result;
use crate::backend::BackendEvent;
use crate::model::{CalendarCancelInput, CalendarDeleteInput, CalendarScope, CalendarUpdateInput};
use crate::runtime::calendar_mime::CalendarMessageMethod;
use crate::runtime::calendar_prepare::{self, EventOwnership, PreparedEvent};
use crate::runtime::calendar_write_preview::event_preview;
use crate::runtime::calendar_write_result::{STEP_ITEM, STEP_NOTIFY_CURRENT, STEP_NOTIFY_REMOVED};
use crate::runtime::write_preview::WritePreview;

#[derive(Serialize)]
#[serde(untagged)]
pub(in crate::runtime) enum EditInput {
    Update(Box<CalendarUpdateInput>),
    Delete(CalendarDeleteInput),
    Cancel(CalendarCancelInput),
}

impl EditInput {
    pub(in crate::runtime) fn name(&self) -> &'static str {
        match self {
            Self::Update(_) => "calendar_update",
            Self::Delete(_) => "calendar_delete",
            Self::Cancel(_) => "calendar_cancel",
        }
    }
    pub(in crate::runtime) fn reference(&self) -> &str {
        match self {
            Self::Update(v) => &v.event_ref,
            Self::Delete(v) => &v.event_ref,
            Self::Cancel(v) => &v.event_ref,
        }
    }
    pub(in crate::runtime) fn key(&self) -> &str {
        match self {
            Self::Update(v) => &v.idempotency_key,
            Self::Delete(v) => &v.idempotency_key,
            Self::Cancel(v) => &v.idempotency_key,
        }
    }
    pub(in crate::runtime) fn scope(&self) -> Option<CalendarScope> {
        match self {
            Self::Update(v) => v.scope,
            Self::Delete(v) => v.scope,
            Self::Cancel(v) => v.scope,
        }
    }
    pub(in crate::runtime) fn comment(&self) -> &str {
        match self {
            Self::Cancel(v) => &v.comment,
            _ => "",
        }
    }
}

pub(in crate::runtime) enum ItemAction {
    Create(Box<PreparedEvent>),
    Update(Box<PreparedEvent>),
    Delete,
}

pub(in crate::runtime) struct ItemStep {
    pub(in crate::runtime) bit: u32,
    pub(in crate::runtime) action: ItemAction,
}

pub(in crate::runtime) struct Notice {
    pub(in crate::runtime) bit: u32,
    pub(in crate::runtime) event: PreparedEvent,
    pub(in crate::runtime) recipients: Vec<CalendarAttendee>,
    pub(in crate::runtime) method: CalendarMessageMethod,
}

pub(in crate::runtime) struct EditPlan {
    pub(in crate::runtime) steps: Vec<ItemStep>,
    pub(in crate::runtime) notices: Vec<Notice>,
    pub(in crate::runtime) preview: WritePreview,
    pub(in crate::runtime) occurrence_start: Option<DateTime<Utc>>,
    pub(in crate::runtime) meeting: bool,
}

pub(in crate::runtime) fn plan(
    input: &EditInput,
    source: &BackendEvent,
    now: DateTime<Utc>,
    email: &str,
) -> Result<EditPlan> {
    calendar_prepare::validate_comment(input.comment())?;
    let old = calendar_prepare::existing(source, now)?;
    let master = &old.mutation.application;
    let scope = resolve_scope(input.scope(), source, master)?;
    let ownership = calendar_prepare::ownership(source, email);
    match input {
        EditInput::Update(_) if ownership == EventOwnership::Attendee => {
            return Err(invalid("only the organizer can update this meeting"));
        }
        EditInput::Delete(_) if ownership != EventOwnership::Personal => {
            return Err(invalid("calendar_delete only accepts personal events"));
        }
        EditInput::Cancel(_) if ownership != EventOwnership::Organizer => {
            return Err(invalid("calendar_cancel requires an organizer meeting"));
        }
        _ => {}
    }
    let preview = event_preview(input.name(), &source.account_id, &old)
        .field("Scope", format!("{scope:?}"))
        .field(
            "Selected original start",
            source.occurrence_start.map(|v| v.to_rfc3339()).unwrap_or_default(),
        )
        .field("Comment", input.comment());
    let mut plan = EditPlan {
        steps: Vec::new(),
        notices: Vec::new(),
        preview,
        occurrence_start: None,
        meeting: !item_attendees(&old).is_empty(),
    };
    match scope {
        CalendarScope::Series => whole(input, source, &old, now, email, &mut plan)?,
        CalendarScope::Occurrence => {
            super::exceptions::edit(input, source, &old, now, email, &mut plan)?
        }
        CalendarScope::Following => super::split::edit(input, source, &old, now, email, &mut plan)?,
    }
    plan.meeting |= plan.steps.iter().any(|step| match &step.action {
        ItemAction::Create(event) | ItemAction::Update(event) => !item_attendees(event).is_empty(),
        ItemAction::Delete => false,
    });
    // The full pre-image participates in the CLI fingerprint, not in durable storage.
    plan.preview = plan.preview.field("Current revision", super::revision(source)?);
    Ok(plan)
}

pub(in crate::runtime) fn resolve_scope(
    scope: Option<CalendarScope>,
    source: &BackendEvent,
    master: &CalendarApplication,
) -> Result<CalendarScope> {
    if master.properties.recurrence.is_none() {
        if source.occurrence_start.is_some() {
            return Err(super::stale());
        }
        return match scope {
            None | Some(CalendarScope::Series) => Ok(CalendarScope::Series),
            _ => Err(invalid("occurrence and following require a recurring event")),
        };
    }
    let scope = scope.ok_or_else(|| {
        invalid("recurring events require explicit scope: series, occurrence, or following")
    })?;
    if let Some(original) = source.occurrence_start {
        selected(master, original)?;
    }
    if scope == CalendarScope::Series {
        return Ok(scope);
    }
    let original = original_time(source)?;
    let ordinal = validate_member(master, original)?;
    Ok(if scope == CalendarScope::Following && ordinal == 1 {
        CalendarScope::Series
    } else {
        scope
    })
}

fn whole(
    input: &EditInput,
    source: &BackendEvent,
    old: &PreparedEvent,
    now: DateTime<Utc>,
    email: &str,
    plan: &mut EditPlan,
) -> Result<()> {
    let old_item = &old.mutation.application;
    if let EditInput::Update(input) = input {
        let mut update = calendar_prepare::update(input, source, now, email)?;
        let item = &mut update.event.mutation.application;
        if let Some(rule) = &input.recurrence {
            item.properties.recurrence = Some(super::rule::prepare(rule, item)?);
        }
        super::exceptions::preserve(old_item, item)?;
        super::exceptions::validate(item)?;
        let removed: Vec<_> = item_attendees(old)
            .into_iter()
            .filter(|attendee| {
                !item_attendees(&update.event)
                    .iter()
                    .any(|current| current.email.eq_ignore_ascii_case(&attendee.email))
            })
            .collect();
        plan.preview = plan.preview.clone().field(
            "Removed attendees",
            crate::runtime::calendar_write_preview::attendee_list(&removed),
        );
        add_result_preview(plan, &update.event);
        notice(
            plan,
            STEP_NOTIFY_CURRENT,
            &update.event,
            item_attendees(&update.event),
            CalendarMessageMethod::Request,
        );
        notice(plan, STEP_NOTIFY_REMOVED, old, removed, CalendarMessageMethod::Cancel);
        plan.steps
            .push(ItemStep { bit: STEP_ITEM, action: ItemAction::Update(Box::new(update.event)) });
    } else {
        if matches!(input, EditInput::Cancel(_)) {
            notice(
                plan,
                STEP_NOTIFY_CURRENT,
                old,
                item_attendees(old),
                CalendarMessageMethod::Cancel,
            );
        }
        plan.steps.push(ItemStep { bit: STEP_ITEM, action: ItemAction::Delete });
    }
    Ok(())
}

pub(super) fn notice(
    plan: &mut EditPlan,
    bit: u32,
    event: &PreparedEvent,
    recipients: Vec<CalendarAttendee>,
    method: CalendarMessageMethod,
) {
    if !recipients.is_empty() {
        plan.notices.push(Notice { bit, event: event.clone(), recipients, method });
    }
}

pub(super) fn add_result_preview(plan: &mut EditPlan, event: &PreparedEvent) {
    let result = event_preview("calendar_update", "", event);
    plan.preview = plan.preview.clone().field("Change", "Resulting event").append(result);
}

pub(super) fn item_attendees(event: &PreparedEvent) -> Vec<CalendarAttendee> {
    let item = &event.mutation.application;
    let mut attendees = std::collections::BTreeMap::new();
    for attendee in &item.attendees {
        attendees.insert(attendee.email.to_ascii_lowercase(), attendee.clone());
    }
    for exception in item.properties.exceptions.iter().filter(|value| !value.deleted) {
        if let eas_mail_protocol::Patch::Value(values) = &exception.fields.attendees {
            for attendee in values {
                attendees.insert(attendee.email.to_ascii_lowercase(), attendee.clone());
            }
        }
    }
    attendees.into_values().collect()
}

pub(super) fn projected(source: &BackendEvent, item: &CalendarApplication) -> BackendEvent {
    let mut value = source.clone();
    value.fields = eas_mail_protocol::CalendarFields::from(item);
    value.fields.organizer_email = source.fields.organizer_email.clone();
    value.fields.organizer = source.fields.organizer.clone();
    value
}
