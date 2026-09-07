mod filters;

use std::collections::BTreeSet;

use futures::future::join_all;

use crate::backend::{AccountBackend, BackendMail};
use crate::model::{MailPage, MailSearchCoverage, MailSearchFilters, MailSearchInput, Warning};
use crate::sanitize::limit;
use crate::{AppError, ErrorCode, Result, Runtime};

pub(super) struct SearchCandidates {
    pub(super) items: Vec<BackendMail>,
    pub(super) coverage: MailSearchCoverage,
}

impl Runtime {
    pub(super) async fn mail_search_result(
        &self,
        input: MailSearchInput,
    ) -> Result<(MailPage, Vec<Warning>)> {
        let page_limit = limit(input.limit.map(u32::from), 50, 100)?;
        if let Some(cursor) = input.cursor {
            return self.references.next_search_page(&cursor, page_limit);
        }
        let query = filters::query(&input)?;
        let selected = self.selected(input.account_ids.as_deref())?;
        let results = join_all(selected.into_iter().map(|backend| {
            let query = query.clone();
            let filters = input.filters.clone();
            async move {
                let account_id = backend.account().account_id;
                (account_id, search_candidates(backend.as_ref(), &query, &filters).await)
            }
        }))
        .await;
        let (groups, warnings) = self.collect_partial(results)?;
        let mut summaries = Vec::new();
        let mut coverage = Vec::new();
        for group in groups {
            coverage.push(group.coverage);
            for mail in group.items {
                summaries.push(self.mail_summary(mail)?);
            }
        }
        summaries.sort_by(|left, right| {
            right
                .received_at
                .cmp(&left.received_at)
                .then_with(|| left.account_id.cmp(&right.account_id))
                .then_with(|| left.mail_ref.cmp(&right.mail_ref))
        });
        self.references.first_search_page(summaries, coverage, warnings, page_limit)
    }
}

pub(super) async fn search_candidates(
    backend: &dyn AccountBackend,
    query: &eas_mail_protocol::MailSearchQuery,
    filters: &MailSearchFilters,
) -> Result<SearchCandidates> {
    let mut coverage = MailSearchCoverage {
        account_id: backend.account().account_id,
        ..MailSearchCoverage::default()
    };
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    let mut start = 0;
    for _ in 0..10 {
        let page = backend.search_mail_page(query, start, 100).await?;
        coverage.search_calls += 1;
        coverage.estimated_total = page.total.or(coverage.estimated_total);
        if page.items.len() > 100 || page.range.is_some_and(|range| range.start != start) {
            return Err(protocol("Exchange returned an inconsistent mail search page"));
        }
        let count = page.items.len();
        let next = start + count;
        let more_reported = page.total.is_some_and(|total| total > next);
        for mail in page.items {
            let key = format!("{:?}", mail.source);
            if !seen.insert(key) {
                return Err(protocol("Exchange repeated a mail search candidate"));
            }
            coverage.candidates_examined += 1;
            match filters::matches(&mail.fields, filters) {
                Some(true) => items.push(mail),
                Some(false) => {}
                None => coverage.metadata_unknown += 1,
            }
        }
        if page.server_truncated {
            break;
        }
        if count < 100 && !more_reported {
            coverage.candidates_complete = page.total.is_some();
            break;
        }
        if count == 0 {
            break;
        }
        start = next;
    }
    Ok(SearchCandidates { items, coverage })
}

fn protocol(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ProtocolError, message)
}
