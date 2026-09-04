use std::collections::BTreeSet;

use futures::future::join_all;

use super::Runtime;
use crate::{
    AccountHealth, AccountHealthStatus, AccountSelection, AccountsStatusData, ApiResponse,
    AppError, ErrorCode, Result,
};

impl Runtime {
    /// Probes selected accounts independently without fetching mail or calendar content.
    pub async fn accounts_status(
        &self,
        input: AccountSelection,
    ) -> ApiResponse<AccountsStatusData> {
        let account_ids = match self.health_selection(input) {
            Ok(ids) => ids,
            Err(error) => return ApiResponse::failure(error.envelope),
        };
        let accounts = join_all(account_ids.iter().map(|id| self.account_health(id))).await;
        ApiResponse::success(AccountsStatusData { accounts }, Vec::new())
    }

    fn health_selection(&self, input: AccountSelection) -> Result<Vec<String>> {
        let Some(requested) = input.account_ids else {
            return Ok(self.backends.keys().cloned().collect());
        };
        let ids = requested.into_iter().collect::<BTreeSet<_>>();
        if ids.is_empty() || ids.iter().any(|id| !self.backends.contains_key(id)) {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "account selection must contain configured account identifiers",
            ));
        }
        Ok(ids.into_iter().collect())
    }

    async fn account_health(&self, account_id: &str) -> AccountHealth {
        let account = self.backends.get(account_id).map(|backend| backend.account());
        let mut health = AccountHealth {
            account_id: account_id.into(),
            enabled: account.as_ref().is_some_and(|value| value.enabled),
            write_enabled: account.as_ref().is_some_and(|value| value.write_enabled),
            server_write_permission: None,
            status: AccountHealthStatus::Disabled,
            error_code: None,
            capabilities: None,
        };
        if !health.enabled {
            return health;
        }
        let result = async {
            let backend = self.backend(account_id)?;
            let capabilities = backend.capabilities().await?;
            let _ = backend.folders().await?;
            Ok(capabilities)
        }
        .await;
        match self.account_result(account_id, result) {
            Ok(capabilities) => {
                health.status = AccountHealthStatus::Ready;
                health.capabilities = Some(capabilities.into());
            }
            Err(error) => {
                health.status = AccountHealthStatus::Failed;
                health.error_code = Some(error.envelope.code);
            }
        }
        health
    }
}
