use eas_mail_mcp::backend::{BackendMail, MailSource};
use eas_mail_protocol::{Attachment, MailFields, MeetingRequest, Patch};

pub(super) fn mail(account_id: &str, source: MailSource) -> BackendMail {
    let meeting =
        matches!(&source, MailSource::LongId(value) if value.starts_with("meeting-request"));
    BackendMail {
        account_id: account_id.into(),
        folder_id: match &source {
            MailSource::Item { folder_id, .. } => folder_id.clone(),
            MailSource::LongId(_) => String::new(),
        },
        source,
        fields: MailFields {
            subject: Patch::Value("Quarterly update".into()),
            sender: Patch::Value("Sender <sender@example.invalid>".into()),
            recipients: Patch::Value(format!("{account_id}@example.invalid")),
            cc: Patch::Value(String::new()),
            received_at: Patch::Value(chrono::DateTime::from_timestamp(1_700_000_000, 0)),
            body: Patch::Value("<p>Safe <strong>plain</strong> body</p>".into()),
            body_truncated: Patch::Value(false),
            is_read: Patch::Value(false),
            importance: Patch::Value(1),
            attachments: Patch::Value(vec![Attachment {
                display_name: "report.txt".into(),
                file_reference: "attachment-1".into(),
                size: 18,
                content_type: "text/plain".into(),
                is_inline: false,
                content_id: String::new(),
            }]),
            message_class: Patch::Value(if meeting {
                "IPM.Schedule.Meeting.Request".into()
            } else {
                "IPM.Note".into()
            }),
            meeting_request: if meeting { Patch::Value(meeting_request()) } else { Patch::Missing },
            conversation_id: Patch::Value(vec![1; 16]),
            conversation_index: Patch::Value(vec![1; 22]),
            flag: Patch::Value(eas_mail_protocol::wbxml::Element::new("Email", "Flag")),
            categories: Patch::Value(Vec::new()),
        },
    }
}

fn meeting_request() -> MeetingRequest {
    MeetingRequest {
        all_day: false,
        dt_stamp: chrono::DateTime::from_timestamp(1_700_000_000, 0),
        starts_at: chrono::DateTime::from_timestamp(1_800_000_000, 0),
        ends_at: chrono::DateTime::from_timestamp(1_800_003_600, 0),
        instance_type: 0,
        location: "Room 1".into(),
        organizer: "Organizer <organizer@example.invalid>".into(),
        reminder_minutes: Some(15),
        response_requested: true,
        busy_status: 2,
        time_zone: "AAAA".into(),
        global_object_id: "BAAAAIIA4AB0xbcQGoLgCAAAAAAAAAAAAAAAAAAAAAAAAAAAMwAAAHZDYWwtVWlkAQAAAHs4MTQxMkQzQy0yQTI0LTRFOUQtQjIwRS0xMUY3QkJFOTI3OTl9AA==".into(),
        uid: String::new(),
        message_type: 1,
    }
}
