use crate::backend::{BackendMail, OutgoingMail};
use crate::model::{MAX_OUTGOING_BODY_CHARS, MailReplyInput};
use crate::sanitize::mailbox;
use crate::{AppError, ErrorCode, Result};

use super::convert::string;

pub(super) fn reply_message(
    mail: &BackendMail,
    own_email: &str,
    input: &MailReplyInput,
) -> Result<OutgoingMail> {
    let sender = mailbox(string(&mail.fields.sender));
    if sender.is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "the source message has no reply recipient",
        ));
    }
    let mut to = vec![sender];
    let mut cc = Vec::new();
    if input.reply_all {
        let mut additional = addresses(string(&mail.fields.recipients));
        remove_own_and_duplicates(&mut additional, own_email);
        to.extend(additional);
        cc.extend(addresses(string(&mail.fields.cc)));
    }
    deduplicate(&mut to);
    remove_own_and_duplicates(&mut cc, own_email);
    Ok(OutgoingMail {
        to,
        cc,
        bcc: Vec::new(),
        subject: prefix_subject("Re:", string(&mail.fields.subject)),
        body: input.body.clone(),
    })
}

pub(super) fn forward_message(mail: &BackendMail, input_body: &str) -> OutgoingMail {
    OutgoingMail {
        to: Vec::new(),
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: prefix_subject("Fwd:", string(&mail.fields.subject)),
        body: input_body.into(),
    }
}

pub(super) fn validate_message(message: &OutgoingMail) -> Result<()> {
    if message.to.is_empty() && message.cc.is_empty() && message.bcc.is_empty() {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "at least one recipient is required",
        ));
    }
    if message.subject.chars().count() > 998 {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "subject exceeds the supported limit",
        ));
    }
    if message.body.chars().count() > MAX_OUTGOING_BODY_CHARS {
        return Err(AppError::new(
            ErrorCode::ValidationFailed,
            "body exceeds the 50,000 character limit",
        ));
    }
    if message.subject.contains(['\r', '\n']) {
        return Err(AppError::new(ErrorCode::ValidationFailed, "subject contains a newline"));
    }
    for address in message.to.iter().chain(&message.cc).chain(&message.bcc) {
        if !address.contains('@') || address.contains(['\r', '\n']) {
            return Err(AppError::new(ErrorCode::ValidationFailed, "recipient address is invalid"));
        }
    }
    Ok(())
}

fn deduplicate(values: &mut Vec<String>) {
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| seen.insert(value.to_ascii_lowercase()));
}

fn addresses(value: &str) -> Vec<String> {
    value.split([',', ';']).map(mailbox).filter(|value| !value.is_empty()).collect()
}

fn remove_own_and_duplicates(values: &mut Vec<String>, own_email: &str) {
    let own_email = own_email.to_ascii_lowercase();
    let mut seen = std::collections::BTreeSet::new();
    values.retain(|value| {
        let normalized = value.to_ascii_lowercase();
        normalized != own_email && seen.insert(normalized)
    });
}

fn prefix_subject(prefix: &str, subject: &str) -> String {
    if subject.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()) {
        subject.to_owned()
    } else {
        format!("{prefix} {subject}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(body: String) -> OutgoingMail {
        OutgoingMail {
            to: vec!["recipient@example.invalid".into()],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Subject".into(),
            body,
        }
    }

    #[test]
    fn outgoing_body_limit_counts_unicode_scalars() {
        assert!(validate_message(&message("я".repeat(MAX_OUTGOING_BODY_CHARS))).is_ok());
        let result = validate_message(&message("я".repeat(MAX_OUTGOING_BODY_CHARS + 1)));
        assert!(result.is_err_and(|error| error.envelope.code == ErrorCode::ValidationFailed));
    }

    #[test]
    fn subject_newlines_are_rejected_before_write() {
        let mut value = message("body".into());
        value.subject = "safe\r\nBcc: injected@example.invalid".into();
        assert!(validate_message(&value).is_err());
    }
}
