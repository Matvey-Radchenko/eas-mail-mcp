use eas_mail_mcp::{
    ErrorCode, MailDetail, MailGetManyInput, MailGetThreadInput, MailSearchFilters,
    MailSearchInput, Runtime,
};

use super::{SyntheticMail, SyntheticMailFixture, required};

pub(super) async fn many(runtime: &Runtime, expected: &[MailDetail]) -> anyhow::Result<()> {
    let result = required(
        runtime
            .mail_get_many(MailGetManyInput {
                mail_refs: expected.iter().map(|mail| mail.summary.mail_ref.clone()).collect(),
                body_limit: Some(10),
                total_body_limit: Some(10),
            })
            .await,
        "mail_get_many fixtures",
    )?;
    anyhow::ensure!(result.items.len() == expected.len(), "bulk read lost a fixture result");
    let mut total = 0;
    for (item, expected) in result.items.iter().zip(expected) {
        anyhow::ensure!(item.error.is_none(), "bulk read fixture failed");
        let mail = item.mail.as_ref().ok_or_else(|| anyhow::anyhow!("bulk read omitted mail"))?;
        anyhow::ensure!(
            mail.summary.account_id == expected.summary.account_id
                && mail.summary.subject == expected.summary.subject,
            "bulk read returned a different message"
        );
        total += mail.body.chars().count();
    }
    anyhow::ensure!(total <= 10, "bulk read exceeded its total body budget");
    Ok(())
}

pub(super) async fn search(
    runtime: &Runtime,
    fixture: &SyntheticMailFixture,
    expected: &MailDetail,
) -> anyhow::Result<bool> {
    let result = required(
        runtime
            .mail_search(MailSearchInput {
                query: expected.summary.subject.clone(),
                account_ids: Some(vec![fixture.account_id.clone()]),
                limit: Some(100),
                filters: MailSearchFilters {
                    received_after: Some(fixture.started_at - chrono::Duration::seconds(1)),
                    received_before: Some(chrono::Utc::now() + chrono::Duration::minutes(1)),
                    is_read: Some(expected.summary.is_read),
                    has_attachments: Some(expected.summary.has_attachments),
                    folder_ids: vec![fixture.inbox_id.clone()],
                    ..MailSearchFilters::default()
                },
                ..MailSearchInput::default()
            })
            .await,
        "mail_search fixture",
    )?;
    anyhow::ensure!(
        result.items.iter().any(|mail| mail.subject == expected.summary.subject),
        "new synthetic message was not indexed by Search yet"
    );
    let coverage = result.coverage.first().ok_or_else(|| anyhow::anyhow!("missing coverage"))?;
    anyhow::ensure!(
        result.coverage.len() == 1
            && coverage.search_calls <= 10
            && coverage.candidates_examined <= 1000
            && (coverage.candidates_complete || result.results_truncated),
        "Search did not retain its truthful bounded coverage"
    );
    Ok(coverage.candidates_complete)
}

pub(super) async fn thread(runtime: &Runtime, expected: &SyntheticMail) -> anyhow::Result<bool> {
    let response = runtime
        .mail_get_thread(MailGetThreadInput {
            mail_ref: expected.mail_ref.clone(),
            limit: Some(3),
            body_limit: Some(10),
            total_body_limit: Some(20),
        })
        .await;
    if response.error.as_ref().is_some_and(|error| error.code == ErrorCode::FeatureUnavailable) {
        return Ok(false);
    }
    let result = required(response, "mail_get_thread fixture")?;
    anyhow::ensure!(
        !result.items.is_empty()
            && result.items.len() <= 3
            && result.items.iter().map(|mail| mail.body.chars().count()).sum::<usize>() <= 20
            && result.coverage.search_calls <= 10
            && result.coverage.candidates_examined <= 1000,
        "native conversation did not honor its bounded contract"
    );
    Ok(true)
}
