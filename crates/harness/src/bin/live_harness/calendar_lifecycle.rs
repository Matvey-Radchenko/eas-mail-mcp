#[path = "calendar_lifecycle/helpers.rs"]
mod helpers;
#[path = "calendar_lifecycle/lookup.rs"]
mod lookup;

use std::collections::BTreeMap;

use eas_mail_mcp::{
    CalendarAttendeeRole, CalendarBusyStatus, CalendarCancelInput, CalendarCreateInput,
    CalendarDeleteInput, CalendarEvent, CalendarEventType, CalendarGetInput, CalendarMailKind,
    CalendarRespondInput, CalendarResponseChoice, CalendarUpdateInput, MailSummary, Runtime,
};

use self::helpers::{
    all_day_schedule, calendar_attendee, cleanup_owned_event, combine_with_cleanup, get_event,
    operation_id, required_ref, succeeded, test_token, timed_schedule,
};
use self::lookup::ExpectedEvent;

#[derive(Debug)]
pub struct LiveAccount {
    pub account_id: String,
    pub profile: String,
    pub email: String,
    pub write_enabled: bool,
}

#[derive(Debug, Default)]
pub struct MeetingCoverage {
    pub profiles: usize,
    pub directions: usize,
}

#[derive(Debug, Clone, Copy)]
enum PersonalKind {
    Timed,
    AllDay,
}

pub async fn check_personal_events(runtime: &Runtime, account_id: &str) -> anyhow::Result<()> {
    check_personal_event(runtime, account_id, PersonalKind::Timed).await?;
    check_personal_event(runtime, account_id, PersonalKind::AllDay).await
}

pub async fn check_meeting_directions(
    runtime: &Runtime,
    accounts: &[LiveAccount],
) -> anyhow::Result<MeetingCoverage> {
    let mut by_profile = BTreeMap::<&str, Vec<&LiveAccount>>::new();
    for account in accounts.iter().filter(|account| account.write_enabled) {
        by_profile.entry(&account.profile).or_default().push(account);
    }
    let mut coverage = MeetingCoverage::default();
    for profile_accounts in by_profile.values().filter(|accounts| accounts.len() >= 2) {
        let first = profile_accounts
            .first()
            .ok_or_else(|| anyhow::anyhow!("first same-profile account is missing"))?;
        let second = profile_accounts
            .get(1)
            .ok_or_else(|| anyhow::anyhow!("second same-profile account is missing"))?;
        check_meeting(runtime, first, second, 0).await?;
        check_meeting(runtime, second, first, 1).await?;
        coverage.profiles += 1;
        coverage.directions += 2;
    }
    Ok(coverage)
}

async fn check_personal_event(
    runtime: &Runtime,
    account_id: &str,
    kind: PersonalKind,
) -> anyhow::Result<()> {
    let token = test_token();
    let subject = format!("EAS Mail MCP personal {token}");
    let mut current_ref = None;
    let outcome = run_personal_event(runtime, account_id, &subject, kind, &mut current_ref).await;
    if outcome.is_ok() {
        return Ok(());
    }
    let cleanup = cleanup_owned_event(runtime, account_id, &token, current_ref.as_deref()).await;
    combine_with_cleanup(outcome, cleanup)
}

async fn run_personal_event(
    runtime: &Runtime,
    account_id: &str,
    subject: &str,
    kind: PersonalKind,
    current_ref: &mut Option<String>,
) -> anyhow::Result<()> {
    let (initial_schedule, updated_schedule) = match kind {
        PersonalKind::Timed => (timed_schedule(5, 10, 0)?.input, timed_schedule(5, 11, 0)?.input),
        PersonalKind::AllDay => (all_day_schedule(7)?, all_day_schedule(9)?),
    };
    let created = succeeded(
        runtime
            .calendar_create(CalendarCreateInput {
                recurrence: None,
                account_id: account_id.to_owned(),
                subject: subject.to_owned(),
                schedule: initial_schedule,
                body: "Calendar stable release self-test".into(),
                location: "EAS Mail MCP test".into(),
                reminder_minutes: Some(10),
                busy_status: CalendarBusyStatus::Busy,
                attendees: Vec::new(),
                idempotency_key: operation_id(),
            })
            .await,
        "calendar_create personal",
    )?;
    current_ref.clone_from(&created.event_ref);
    let created_event = get_event(runtime, required_ref(current_ref)?).await?;
    anyhow::ensure!(
        created_event.event_type == CalendarEventType::Personal
            && created_event.can_update
            && created_event.can_delete,
        "created personal event is not mutable"
    );
    let updated_subject = format!("{subject} updated");
    let updated = succeeded(
        runtime
            .calendar_update(CalendarUpdateInput {
                scope: None,
                recurrence: None,
                event_ref: required_ref(current_ref)?.to_owned(),
                subject: Some(updated_subject.clone()),
                schedule: Some(updated_schedule),
                body: Some(String::new()),
                location: Some(String::new()),
                reminder_minutes: None,
                clear_reminder: true,
                busy_status: Some(CalendarBusyStatus::Free),
                attendees: Some(Vec::new()),
                idempotency_key: operation_id(),
            })
            .await,
        "calendar_update personal",
    )?;
    current_ref.clone_from(&updated.event_ref);
    let event = get_event(runtime, required_ref(current_ref)?).await?;
    anyhow::ensure!(
        event.subject == updated_subject
            && event.body.is_empty()
            && event.location.is_empty()
            && event.busy_status == CalendarBusyStatus::Free
            && event.all_day == matches!(kind, PersonalKind::AllDay),
        "updated personal event did not round-trip"
    );
    succeeded(
        runtime
            .calendar_delete(CalendarDeleteInput {
                scope: None,
                event_ref: required_ref(current_ref)?.to_owned(),
                idempotency_key: operation_id(),
            })
            .await,
        "calendar_delete personal",
    )?;
    *current_ref = None;
    Ok(())
}

async fn check_meeting(
    runtime: &Runtime,
    organizer: &LiveAccount,
    attendee: &LiveAccount,
    direction: u64,
) -> anyhow::Result<()> {
    let token = test_token();
    let subject = format!("EAS Mail MCP meeting {token}");
    let mut organizer_ref = None;
    let outcome =
        run_meeting(runtime, organizer, attendee, &subject, &token, direction, &mut organizer_ref)
            .await;
    if outcome.is_ok() {
        return Ok(());
    }
    let cleanup =
        cleanup_owned_event(runtime, &organizer.account_id, &token, organizer_ref.as_deref()).await;
    combine_with_cleanup(outcome, cleanup)
}

async fn run_meeting(
    runtime: &Runtime,
    organizer: &LiveAccount,
    attendee: &LiveAccount,
    subject: &str,
    token: &str,
    direction: u64,
    organizer_ref: &mut Option<String>,
) -> anyhow::Result<()> {
    let (uid, received) =
        create_and_receive(runtime, organizer, attendee, subject, token, direction, organizer_ref)
            .await?;
    exercise_initial_responses(runtime, organizer, attendee, token, &uid, received).await?;
    update_and_decline(runtime, attendee, token, direction, organizer_ref).await?;
    cancel_meeting(runtime, attendee, token, organizer_ref).await
}

async fn create_and_receive(
    runtime: &Runtime,
    organizer: &LiveAccount,
    attendee: &LiveAccount,
    subject: &str,
    token: &str,
    direction: u64,
    organizer_ref: &mut Option<String>,
) -> anyhow::Result<(String, MailSummary)> {
    let initial = timed_schedule(14 + direction, 13, 0)?;
    let created = succeeded(
        runtime
            .calendar_create(CalendarCreateInput {
                recurrence: None,
                account_id: organizer.account_id.clone(),
                subject: subject.to_owned(),
                schedule: initial.input,
                body: "Calendar meeting lifecycle self-test".into(),
                location: "EAS Mail MCP test".into(),
                reminder_minutes: None,
                busy_status: CalendarBusyStatus::Busy,
                attendees: vec![calendar_attendee(attendee, CalendarAttendeeRole::Required)],
                idempotency_key: operation_id(),
            })
            .await,
        "calendar_create meeting",
    )?;
    organizer_ref.clone_from(&created.event_ref);
    let organizer_event = get_event(runtime, required_ref(organizer_ref)?).await?;
    anyhow::ensure!(
        organizer_event.event_type == CalendarEventType::OrganizerMeeting
            && organizer_event.can_cancel
            && !organizer_event.uid.is_empty(),
        "created organizer meeting is incomplete"
    );
    let uid = organizer_event.uid;
    let received = lookup::wait_for_meeting_mail(
        runtime,
        &attendee.account_id,
        token,
        CalendarMailKind::Request,
    )
    .await?;
    Ok((uid, received))
}

async fn exercise_initial_responses(
    runtime: &Runtime,
    organizer: &LiveAccount,
    attendee: &LiveAccount,
    token: &str,
    uid: &str,
    received: MailSummary,
) -> anyhow::Result<()> {
    let accepted_ref = respond(
        runtime,
        &received.mail_ref,
        CalendarResponseChoice::Accept,
        "Accepted by the release harness",
    )
    .await?;
    let _ = lookup::wait_for_meeting_mail(
        runtime,
        &organizer.account_id,
        token,
        CalendarMailKind::Response,
    )
    .await?;
    let accepted = response_event(runtime, attendee, token, uid, accepted_ref).await?;
    let tentative_ref = respond(
        runtime,
        &accepted.event_ref,
        CalendarResponseChoice::Tentative,
        "Tentative by the release harness",
    )
    .await?;
    let _ = response_event(runtime, attendee, token, uid, tentative_ref).await?;
    Ok(())
}

async fn update_and_decline(
    runtime: &Runtime,
    attendee: &LiveAccount,
    token: &str,
    direction: u64,
    organizer_ref: &mut Option<String>,
) -> anyhow::Result<()> {
    let updated = timed_schedule(14 + direction, 14, 30)?;
    let changed = succeeded(
        runtime
            .calendar_update(CalendarUpdateInput {
                scope: None,
                recurrence: None,
                event_ref: required_ref(organizer_ref)?.to_owned(),
                subject: None,
                schedule: Some(updated.input),
                body: None,
                location: Some("EAS Mail MCP updated test".into()),
                reminder_minutes: None,
                clear_reminder: false,
                busy_status: None,
                attendees: Some(vec![calendar_attendee(attendee, CalendarAttendeeRole::Optional)]),
                idempotency_key: operation_id(),
            })
            .await,
        "calendar_update meeting",
    )?;
    organizer_ref.clone_from(&changed.event_ref);
    let changed_attendee = lookup::wait_for_meeting_mail(
        runtime,
        &attendee.account_id,
        token,
        CalendarMailKind::Update,
    )
    .await?;
    let _ = respond(
        runtime,
        &changed_attendee.mail_ref,
        CalendarResponseChoice::Decline,
        "Declined by the release harness",
    )
    .await?;
    Ok(())
}

async fn cancel_meeting(
    runtime: &Runtime,
    attendee: &LiveAccount,
    token: &str,
    organizer_ref: &mut Option<String>,
) -> anyhow::Result<()> {
    succeeded(
        runtime
            .calendar_cancel(CalendarCancelInput {
                scope: None,
                event_ref: required_ref(organizer_ref)?.to_owned(),
                comment: "Cancelled by the release harness".into(),
                idempotency_key: operation_id(),
            })
            .await,
        "calendar_cancel meeting",
    )?;
    *organizer_ref = None;
    let _ = lookup::wait_for_meeting_mail(
        runtime,
        &attendee.account_id,
        token,
        CalendarMailKind::Cancellation,
    )
    .await?;
    Ok(())
}

async fn response_event(
    runtime: &Runtime,
    attendee: &LiveAccount,
    token: &str,
    uid: &str,
    event_ref: Option<String>,
) -> anyhow::Result<CalendarEvent> {
    if let Some(event_ref) = event_ref {
        let response =
            runtime.calendar_get(CalendarGetInput { event_ref, body_limit: Some(12_000) }).await;
        if let Some(event) = response.data
            && event.can_respond
        {
            return Ok(event);
        }
    }
    lookup::wait_for_event(runtime, &attendee.account_id, token, Some(uid), ExpectedEvent::Attendee)
        .await
}

async fn respond(
    runtime: &Runtime,
    event_ref: &str,
    response: CalendarResponseChoice,
    comment: &str,
) -> anyhow::Result<Option<String>> {
    Ok(succeeded(
        runtime
            .calendar_respond(CalendarRespondInput {
                scope: None,
                event_ref: event_ref.to_owned(),
                response,
                comment: comment.to_owned(),
                idempotency_key: operation_id(),
            })
            .await,
        "calendar_respond meeting",
    )?
    .event_ref)
}
