//! Opt-in mail acceptance checks restricted to two freshly created self-test messages.

mod reads;
#[cfg(test)]
mod tests;
mod writes;

use chrono::{DateTime, Utc};
use eas_mail_mcp::{ApiResponse, MailDetail, MailGetInput, Runtime};
use serde::Serialize;

/// One message created by the caller in the current acceptance run.
///
/// References must come from explicit Inbox listing after a confirmed self-send.
pub struct SyntheticMail {
    /// Portable Item reference returned by `mail_list`, never an arbitrary search hit.
    pub mail_ref: String,
    /// Exact subject: `EAS Mail MCP self-test ` followed by a new UUID.
    pub subject: String,
}

/// Provenance required before any live mail mutation can begin.
pub struct SyntheticMailFixture {
    /// Local account used to send both messages to itself.
    pub account_id: String,
    /// Account's system Inbox identifier, obtained by `folders_list`.
    pub inbox_id: String,
    /// Account's system Trash identifier, obtained by `folders_list`.
    pub trash_id: String,
    /// Time immediately before the caller sent these messages.
    pub started_at: DateTime<Utc>,
    /// Two distinct self-test messages created in this run.
    pub messages: [SyntheticMail; 2],
}

/// Content-free coverage returned after every mutation has been restored.
#[derive(Debug, Serialize)]
pub struct MailLifecycleCoverage {
    /// Whether the server exhausted the exact test search within its bounded budget.
    pub search_candidates_complete: bool,
    /// Whether the server returned verified conversation members; false is explicit unavailability.
    pub native_thread_available: bool,
    /// Both bounded reads returned the intended fresh messages.
    pub get_many_verified: bool,
    /// Active, complete and cleared flag states were fetched after confirmed mutations.
    pub flag_verified: bool,
    /// Set and cleared categories were fetched after confirmed mutations.
    pub categories_verified: bool,
    /// Trash and return to Inbox used the newly returned portable references.
    pub move_and_trash_verified: bool,
    /// Both read-state batch changes and restoration were fetched and verified.
    pub batch_verified: bool,
}

/// Checks and restores only caller-created messages; never creates mail or changes OOF.
///
/// The caller must explicitly authorize live mutations. This function first validates both
/// references against exact subjects, account, Inbox and creation time. Any ambiguous write
/// stops immediately without retry or automatic cleanup. Errors never include mail content.
pub async fn check_synthetic_mail(
    runtime: &Runtime,
    fixture: &SyntheticMailFixture,
) -> anyhow::Result<MailLifecycleCoverage> {
    let [first, second] = &fixture.messages;
    anyhow::ensure!(
        first.mail_ref != second.mail_ref && first.subject != second.subject,
        "synthetic mail fixtures must be distinct"
    );
    let first_mail = preflight(runtime, fixture, first).await?;
    let second_mail = preflight(runtime, fixture, second).await?;
    reads::many(runtime, &[first_mail.clone(), second_mail.clone()]).await?;
    let search_candidates_complete = reads::search(runtime, fixture, &first_mail).await?;
    let native_thread_available = reads::thread(runtime, first).await?;
    writes::properties(runtime, &first_mail).await?;
    writes::batch(runtime, &[first_mail.clone(), second_mail]).await?;
    writes::trash_and_restore(runtime, fixture, &first_mail).await?;
    Ok(MailLifecycleCoverage {
        search_candidates_complete,
        native_thread_available,
        get_many_verified: true,
        flag_verified: true,
        categories_verified: true,
        move_and_trash_verified: true,
        batch_verified: true,
    })
}

async fn preflight(
    runtime: &Runtime,
    fixture: &SyntheticMailFixture,
    expected: &SyntheticMail,
) -> anyhow::Result<MailDetail> {
    let token = expected.subject.strip_prefix("EAS Mail MCP self-test ");
    anyhow::ensure!(
        token.is_some_and(|value| uuid::Uuid::parse_str(value).is_ok()),
        "synthetic mail requires a fresh UUID self-test subject"
    );
    let mail = get(runtime, &expected.mail_ref).await?;
    anyhow::ensure!(
        mail.summary.account_id == fixture.account_id
            && mail.summary.folder_id == fixture.inbox_id
            && mail.summary.subject == expected.subject
            && mail.summary.received_at.is_some_and(|time| time >= fixture.started_at),
        "synthetic mail provenance did not match; no writes were started"
    );
    anyhow::ensure!(
        mail.summary.flag.is_none_or(|flag| flag == eas_mail_mcp::MailFlagState::None)
            && mail.summary.categories.as_ref().is_none_or(Vec::is_empty),
        "synthetic mail already has user properties; no writes were started"
    );
    Ok(mail)
}

pub(super) async fn get(runtime: &Runtime, mail_ref: &str) -> anyhow::Result<MailDetail> {
    required(
        runtime.mail_get(MailGetInput { mail_ref: mail_ref.into(), body_limit: Some(1) }).await,
        "mail_get fixture",
    )
}

pub(super) fn required<T>(response: ApiResponse<T>, operation: &str) -> anyhow::Result<T> {
    if let Some(error) = response.error {
        anyhow::bail!("{operation} failed: {:?}; no automatic write retry", error.code);
    }
    anyhow::ensure!(response.warnings.is_empty(), "{operation} returned partial warnings");
    response.data.ok_or_else(|| anyhow::anyhow!("{operation} returned no data"))
}

pub(super) fn operation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
