use super::Runtime;
use crate::model::{PeopleSearchData, PeopleSearchInput, Person};
use crate::sanitize::{limit, plain_text};
use crate::{ApiResponse, AppError, ErrorCode, Result};

impl Runtime {
    /// Searches one server directory, returning only names and addresses.
    pub async fn people_search(&self, input: PeopleSearchInput) -> ApiResponse<PeopleSearchData> {
        Self::response(self.people_search_result(input).await)
    }

    async fn people_search_result(
        &self,
        input: PeopleSearchInput,
    ) -> Result<(PeopleSearchData, Vec<crate::Warning>)> {
        let limit = limit(input.limit, 20, 50)?;
        let query = input.query.trim();
        if query.is_empty() || query.chars().count() > 256 || query.chars().any(char::is_control) {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "directory query must contain 1-256 printable characters",
            ));
        }
        let backend = if let Some(id) = input.account_id {
            self.backend(&id)?
        } else {
            let mut backends = self.selected(None)?;
            if backends.len() != 1 {
                return Err(AppError::new(
                    ErrorCode::AccountSelectionRequired,
                    "select one account for directory search",
                ));
            }
            backends
                .pop()
                .ok_or_else(|| AppError::new(ErrorCode::ConfigInvalid, "no enabled accounts"))?
        };
        let account_id = backend.account().account_id;
        let page = self.account_result(&account_id, backend.search_people(query, limit).await)?;
        let total = page.total.max(page.items.len());
        let items = page
            .items
            .into_iter()
            .take(limit)
            .map(|person| Person {
                name: plain_text(&person.name).chars().take(1024).collect(),
                email: person.email,
            })
            .collect::<Vec<_>>();
        Ok((
            PeopleSearchData {
                account_id,
                results_truncated: total > items.len(),
                items,
                untrusted_external_content: true,
            },
            Vec::new(),
        ))
    }
}
