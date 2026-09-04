use super::{BackendMail, ErrorCode, FakeBackend, MailSource, Result, failure, mail};
use eas_mail_protocol::{Patch, wbxml::Element};

impl FakeBackend {
    pub(super) fn stored_mail(&self, source: &MailSource) -> Result<BackendMail> {
        if self.removed_mail.lock().map_err(|_| failure(ErrorCode::StorageError))?.contains(source)
        {
            return Err(failure(ErrorCode::NotFound));
        }
        Ok(self
            .mail_items
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .get(source)
            .cloned()
            .unwrap_or_else(|| mail(&self.account.account_id, source.clone())))
    }
    fn store_mail(&self, mail: BackendMail) -> Result<()> {
        self.mail_items
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .insert(mail.source.clone(), mail);
        Ok(())
    }
    /// Replaces the fake body for bounded bulk-read tests.
    pub fn set_mail_body(&self, source: &MailSource, body: String) -> Result<()> {
        let mut mail = self.stored_mail(source)?;
        mail.fields.body = Patch::Value(body);
        self.store_mail(mail)
    }
    pub(super) async fn fake_move(
        &self,
        source: &MailSource,
        destination: &str,
    ) -> Result<MailSource> {
        self.check_operation("mail_move").await?;
        let mut mail = self.stored_mail(source)?;
        let MailSource::Item { folder_id, server_id } = source else {
            return Err(failure(ErrorCode::FeatureUnavailable));
        };
        if folder_id == destination {
            return Ok(source.clone());
        }
        let target = MailSource::Item {
            folder_id: destination.into(),
            server_id: format!("moved-{server_id}"),
        };
        mail.source = target.clone();
        mail.folder_id = destination.into();
        self.store_mail(mail)?;
        self.removed_mail
            .lock()
            .map_err(|_| failure(ErrorCode::StorageError))?
            .insert(source.clone());
        self.record("mail_move")?;
        Ok(target)
    }
    pub(super) async fn fake_flag(&self, source: &MailSource, status: u8) -> Result<()> {
        self.check_operation("mail_set_flag").await?;
        let mut mail = self.stored_mail(source)?;
        let mut flag = Element::new("Email", "Flag");
        if status != 0 {
            flag.push(Element::text("Email", "Status", status.to_string()));
        }
        mail.fields.flag = Patch::Value(flag);
        self.store_mail(mail)?;
        self.record("mail_set_flag")
    }
    pub(super) async fn fake_categories(
        &self,
        source: &MailSource,
        categories: &[String],
    ) -> Result<()> {
        self.check_operation("mail_set_categories").await?;
        let mut mail = self.stored_mail(source)?;
        mail.fields.categories = Patch::Value(categories.to_vec());
        self.store_mail(mail)?;
        self.record("mail_set_categories")
    }
    pub(super) async fn fake_read(&self, source: &MailSource, is_read: bool) -> Result<()> {
        self.check_operation("mail_mark_read").await?;
        let mut mail = self.stored_mail(source)?;
        mail.fields.is_read = Patch::Value(is_read);
        self.store_mail(mail)?;
        self.record("mail_mark_read")
    }
}
