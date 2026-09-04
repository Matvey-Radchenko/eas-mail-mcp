use std::path::Path;
use std::time::{Duration, Instant};

use eas_mail_mcp::{
    AccountSelection, ApiResponse, AttachmentDownloadInput, CalendarAvailabilityInput,
    CalendarFindSlotsInput, CalendarGetInput, CalendarSearchInput, MailAttachmentsInput,
    MailForwardInput, MailGetInput, MailListInput, MailPage, MailReplyInput, MailSearchInput,
    MailSendInput, MailSummary, MarkReadInput, OperationResult, OperationState, Runtime,
    ScheduleWeekday, WorkingHoursInput,
};

use super::support::AccountReport;
use super::write_outcome::{check_warnings, incomplete};

const COLD_MAIL_TARGET: Duration = Duration::from_secs(15);
const WARM_MAIL_TARGET: Duration = Duration::from_secs(3);
const LIVE_CALL_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn check_account(
    runtime: &Runtime,
    account_id: &str,
    email: &str,
    self_write: bool,
) -> anyhow::Result<AccountReport> {
    let selection = Some(vec![account_id.to_owned()]);
    let list_input = MailListInput {
        account_ids: selection.clone(),
        limit: Some(100),
        ..MailListInput::default()
    };
    let (mail, cold_mail_list_ms) =
        timed_mail_list(runtime, list_input.clone(), "cold mail_list").await?;
    anyhow::ensure!(
        cold_mail_list_ms <= COLD_MAIL_TARGET.as_millis(),
        "cold mail_list exceeded the 15 second target: {cold_mail_list_ms} ms"
    );
    let (_, warm_mail_list_ms) = timed_mail_list(runtime, list_input, "warm mail_list").await?;
    anyhow::ensure!(
        warm_mail_list_ms <= WARM_MAIL_TARGET.as_millis(),
        "warm mail_list exceeded the 3 second target: {warm_mail_list_ms} ms"
    );
    required(
        runtime.sync_now(AccountSelection { account_ids: selection.clone() }).await,
        "sync_now",
    )?;
    let folders = required(
        runtime.folders_list(AccountSelection { account_ids: selection.clone() }).await,
        "folders_list",
    )?;
    let calendar_count = check_calendar(runtime, account_id, email, selection.clone()).await?;
    let first = mail
        .items
        .first()
        .ok_or_else(|| anyhow::anyhow!("mail_list returned no messages for {account_id}"))?;
    let search_query = search_term(first, email);
    let search = required(
        runtime
            .mail_search(MailSearchInput {
                filters: Default::default(),
                query: search_query,
                account_ids: selection,
                cursor: None,
                limit: Some(100),
            })
            .await,
        "mail_search",
    )?;
    anyhow::ensure!(!search.items.is_empty(), "mail_search did not find a known message");
    required(
        runtime
            .mail_get(MailGetInput { mail_ref: first.mail_ref.clone(), body_limit: Some(12_000) })
            .await,
        "mail_get",
    )?;
    let attachment_checked = check_attachment(runtime, &mail.items).await?;
    if self_write {
        let inbox_id = folders
            .folders
            .iter()
            .find(|folder| folder.role == "inbox")
            .map(|folder| folder.folder_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("folders_list returned no EAS Inbox"))?;
        check_writes(runtime, account_id, email, inbox_id).await?;
    }
    Ok(AccountReport {
        account_id: account_id.to_owned(),
        folders: folders.folders.len(),
        mail: mail.items.len(),
        calendar: calendar_count,
        search: search.items.len(),
        attachment_checked,
        writes_checked: self_write,
        calendar_writes_checked: false,
        cold_mail_list_ms,
        warm_mail_list_ms,
    })
}

async fn check_calendar(
    runtime: &Runtime,
    account_id: &str,
    email: &str,
    selection: Option<Vec<String>>,
) -> anyhow::Result<usize> {
    let (date, agenda_to, working_hours) = current_schedule();
    let calendar = required(
        runtime
            .calendar_availability(CalendarAvailabilityInput {
                account_id: Some(account_id.to_owned()),
                participants: vec![email.to_owned()],
                date_from: date.clone(),
                date_to: date.clone(),
                time_zone: "UTC".into(),
                working_hours: working_hours.clone(),
            })
            .await,
        "calendar_availability",
    )?;
    anyhow::ensure!(calendar.resolution_complete, "self calendar recipient did not resolve");
    required(
        runtime
            .calendar_find_slots(CalendarFindSlotsInput {
                account_id: Some(account_id.to_owned()),
                participants: vec![email.to_owned()],
                date_from: date.clone(),
                date_to: date.clone(),
                time_zone: "UTC".into(),
                working_hours,
                duration_minutes: 30,
                allow_tentative: false,
                buffer_minutes: 0,
                participant_options: Vec::new(),
                limit: Some(20),
            })
            .await,
        "calendar_find_slots",
    )?;
    let agenda = required(
        runtime
            .calendar_search(CalendarSearchInput {
                query: None,
                date_from: Some(date.clone()),
                date_to: Some(agenda_to),
                time_zone: Some("UTC".into()),
                account_ids: selection.clone(),
                limit: Some(100),
            })
            .await,
        "calendar_search agenda",
    )?;
    let search = required(
        runtime
            .calendar_search(CalendarSearchInput {
                query: Some(email.to_owned()),
                date_from: None,
                date_to: None,
                time_zone: None,
                account_ids: selection,
                limit: Some(1),
            })
            .await,
        "calendar_search",
    )?;
    if let Some(event) = search.items.first() {
        required(
            runtime
                .calendar_get(CalendarGetInput {
                    event_ref: event.event_ref.clone(),
                    body_limit: Some(12_000),
                })
                .await,
            "calendar_get",
        )?;
    }
    Ok(agenda.items.len())
}

fn current_schedule() -> (String, String, Vec<WorkingHoursInput>) {
    use chrono::Datelike as _;
    let date = chrono::Utc::now().date_naive();
    let agenda_to = date.checked_add_days(chrono::Days::new(6)).unwrap_or(date);
    let weekday = match date.weekday() {
        chrono::Weekday::Mon => ScheduleWeekday::Mon,
        chrono::Weekday::Tue => ScheduleWeekday::Tue,
        chrono::Weekday::Wed => ScheduleWeekday::Wed,
        chrono::Weekday::Thu => ScheduleWeekday::Thu,
        chrono::Weekday::Fri => ScheduleWeekday::Fri,
        chrono::Weekday::Sat => ScheduleWeekday::Sat,
        chrono::Weekday::Sun => ScheduleWeekday::Sun,
    };
    (
        date.to_string(),
        agenda_to.to_string(),
        vec![WorkingHoursInput {
            weekdays: vec![weekday],
            start: "00:00".into(),
            end: "23:30".into(),
        }],
    )
}

async fn timed_mail_list(
    runtime: &Runtime,
    input: MailListInput,
    operation: &str,
) -> anyhow::Result<(MailPage, u128)> {
    let started = Instant::now();
    let response = tokio::time::timeout(LIVE_CALL_TIMEOUT, runtime.mail_list(input))
        .await
        .map_err(|_| anyhow::anyhow!("{operation} timed out"))?;
    let elapsed = started.elapsed().as_millis();
    Ok((required(response, operation)?, elapsed))
}

async fn check_attachment(runtime: &Runtime, mail: &[MailSummary]) -> anyhow::Result<bool> {
    let Some(message) = mail.iter().find(|message| message.has_attachments) else {
        return Ok(false);
    };
    let attachments = required(
        runtime
            .mail_list_attachments(MailAttachmentsInput { mail_ref: message.mail_ref.clone() })
            .await,
        "mail_list_attachments",
    )?;
    let attachment = attachments
        .attachments
        .first()
        .ok_or_else(|| anyhow::anyhow!("mail flagged an attachment but returned no metadata"))?;
    let download = required(
        runtime
            .mail_download_attachment(AttachmentDownloadInput {
                attachment_ref: attachment.attachment_ref.clone(),
            })
            .await,
        "mail_download_attachment",
    )?;
    let path = Path::new(&download.path);
    anyhow::ensure!(path.is_file(), "downloaded attachment is unavailable");
    std::fs::remove_file(path)?;
    Ok(true)
}

async fn check_writes(
    runtime: &Runtime,
    account_id: &str,
    email: &str,
    inbox_id: &str,
) -> anyhow::Result<()> {
    let subject = format!("EAS Mail MCP self-test {}", uuid::Uuid::new_v4());
    mail_succeeded(
        runtime
            .mail_send(MailSendInput {
                attachments: Vec::new(),
                account_id: account_id.to_owned(),
                to: vec![email.to_owned()],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: subject.clone(),
                body: "Automated self-test. No reply is required.".into(),
                idempotency_key: operation_id(),
            })
            .await,
        "mail_send",
    )?;
    let sent_ref = wait_for_mail(runtime, account_id, inbox_id, &subject).await?;
    mail_succeeded(
        runtime
            .mail_reply(MailReplyInput {
                attachments: Vec::new(),
                mail_ref: sent_ref.clone(),
                body: "Automated self-reply test.".into(),
                reply_all: false,
                idempotency_key: operation_id(),
            })
            .await,
        "mail_reply",
    )?;
    mail_succeeded(
        runtime
            .mail_forward(MailForwardInput {
                attachments: Vec::new(),
                mail_ref: sent_ref.clone(),
                to: vec![email.to_owned()],
                cc: Vec::new(),
                bcc: Vec::new(),
                body: "Automated self-forward test.".into(),
                idempotency_key: operation_id(),
            })
            .await,
        "mail_forward",
    )?;
    mail_succeeded(
        runtime
            .mail_mark_read(MarkReadInput {
                mail_ref: sent_ref.clone(),
                is_read: false,
                idempotency_key: operation_id(),
            })
            .await,
        "mail_mark_read toggle",
    )?;
    mail_succeeded(
        runtime
            .mail_mark_read(MarkReadInput {
                mail_ref: sent_ref,
                is_read: true,
                idempotency_key: operation_id(),
            })
            .await,
        "mail_mark_read restore",
    )?;
    Ok(())
}

async fn wait_for_mail(
    runtime: &Runtime,
    account_id: &str,
    inbox_id: &str,
    subject: &str,
) -> anyhow::Result<String> {
    for _ in 0..15 {
        let result = required(
            runtime
                .mail_list(MailListInput {
                    account_ids: Some(vec![account_id.to_owned()]),
                    folder_ids: Some(vec![inbox_id.to_owned()]),
                    cursor: None,
                    limit: Some(100),
                })
                .await,
            "mail_list after send",
        )?;
        if let Some(mail) = result.items.iter().find(|mail| mail.subject == subject) {
            return Ok(mail.mail_ref.clone());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    anyhow::bail!("self-sent message did not arrive in Inbox within 30 seconds")
}

pub(super) fn required<T>(response: ApiResponse<T>, operation: &str) -> anyhow::Result<T> {
    check_warnings(&response, operation)?;
    if let Some(error) = response.error {
        if error.code == eas_mail_mcp::ErrorCode::OutcomeUnknown {
            return Err(incomplete(operation, "Unknown", error.operation_id.as_deref()));
        }
        anyhow::bail!(
            "{operation} failed with {:?}; operation_id={:?}: {}",
            error.code,
            error.operation_id,
            error.message
        );
    }
    response.data.ok_or_else(|| anyhow::anyhow!("{operation} returned no data"))
}

pub(super) fn mail_succeeded(
    response: ApiResponse<OperationResult>,
    operation: &str,
) -> anyhow::Result<OperationResult> {
    let result = required(response, operation)?;
    if result.status == OperationState::Unknown {
        return Err(incomplete(operation, "Unknown", Some(&result.operation_id)));
    }
    anyhow::ensure!(
        result.status == OperationState::Succeeded,
        "{operation} returned {:?}; operation_id={}",
        result.status,
        result.operation_id
    );
    Ok(result)
}

fn operation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn search_term(message: &MailSummary, fallback: &str) -> String {
    message
        .subject
        .split_whitespace()
        .map(|word| word.trim_matches(|character: char| !character.is_alphanumeric()))
        .find(|word| word.chars().count() >= 4)
        .map(str::to_owned)
        .unwrap_or_else(|| fallback.to_owned())
}
