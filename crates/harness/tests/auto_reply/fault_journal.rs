use eas_mail_mcp::{
    AppError, ErrorCode, JournalBegin, JournalEntry, JournalFilter, JournalRecord,
    MailResultLocator, OperationJournal, OperationStatus, Result,
};
use eas_mail_mcp_harness::MemoryJournal;

pub(super) struct FaultJournal {
    inner: MemoryJournal,
    fail_status: Option<OperationStatus>,
    fail_purge: bool,
}

impl FaultJournal {
    pub(super) fn new(fail_status: Option<OperationStatus>, fail_purge: bool) -> Self {
        Self { inner: MemoryJournal::default(), fail_status, fail_purge }
    }
}

impl OperationJournal for FaultJournal {
    fn lookup(&self, operation_id: &str) -> Result<Option<JournalRecord>> {
        self.inner.lookup(operation_id)
    }
    fn inspect(&self, operation_id: &str) -> Result<Option<JournalEntry>> {
        self.inner.inspect(operation_id)
    }
    fn list(&self, filter: &JournalFilter) -> Result<Vec<JournalEntry>> {
        self.inner.list(filter)
    }
    fn pending_accounts(&self) -> Result<Vec<String>> {
        self.inner.pending_accounts()
    }
    fn recover_account(&self, account_id: &str) -> Result<usize> {
        self.inner.recover_account(account_id)
    }
    fn begin(&self, record: &JournalRecord) -> Result<JournalBegin> {
        self.inner.begin(record)
    }
    fn checkpoint(&self, operation_id: &str, completed_steps: u32) -> Result<()> {
        self.inner.checkpoint(operation_id, completed_steps)
    }
    fn finish_with_locator(
        &self,
        operation_id: &str,
        status: OperationStatus,
        completed_steps: u32,
        locator: Option<&MailResultLocator>,
    ) -> Result<()> {
        if self.fail_status == Some(status) {
            return Err(AppError::new(ErrorCode::StorageError, "scripted finish failure"));
        }
        self.inner.finish_with_locator(operation_id, status, completed_steps, locator)
    }
    fn prune(&self) -> Result<usize> {
        self.inner.prune()
    }
    fn purge_account(&self, account_id: &str) -> Result<usize> {
        if self.fail_purge {
            return Err(AppError::new(ErrorCode::StorageError, "scripted purge failure"));
        }
        self.inner.purge_account(account_id)
    }
}
