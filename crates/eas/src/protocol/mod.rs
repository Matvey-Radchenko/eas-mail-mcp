//! EAS command builders and strict response parsers.

mod availability;
mod calendar_mutation;
mod calendar_mutation_response;
pub(crate) use calendar_mutation_response::parse_for as parse_calendar_mutation_for;
mod calendar_properties;
mod calendar_properties_write;
mod calendar_recurrence;
mod calendar_validation;
mod mutation_response;
pub use calendar_properties::exception_fields;
mod compose;
mod compose_response;
pub(crate) use compose_response::parse_for as parse_compose_for;
mod mail_mime;
pub use mail_mime::{
    MAX_ATTACHMENT_BYTES, MAX_MIME_BYTES, MAX_OUTGOING_ATTACHMENTS, MimeAttachment, MimeMessage,
    build_mime_with_attachments,
};
mod folders;
mod global_object_id;
mod items;
mod mail_mutation;
mod mail_search;
pub use mail_mutation::{MailPatch, build_mail_change, build_move, parse_mail_change, parse_move};
pub use mail_search::{build_mail_search, parse_mail_search};
mod meeting_response;
pub use meeting_response::build_meeting_response_instance;
pub(crate) use meeting_response::parse_for as parse_meeting_response_for;
mod oof;
mod people;
pub use oof::{build_oof_get, build_oof_set, parse_oof_get, parse_oof_set};
mod policy;
mod provision;
mod sync;
mod tree;

pub use availability::{build_availability, parse_availability};
pub use calendar_mutation::{
    build_calendar_add, build_calendar_change, build_calendar_delete, parse_calendar_mutation_sync,
};
pub use compose::{ComposeSource, build_mime_message, build_send, build_smart, parse_compose};
pub use folders::{build_folder_sync, parse_folder_sync};
pub use global_object_id::global_object_id_uid;
pub use items::{
    build_attachment_fetch, build_calendar_search, build_item_fetch, build_search,
    parse_attachment_fetch, parse_calendar_item_fetch, parse_calendar_search, parse_item_fetch,
    parse_search,
};
pub use meeting_response::{
    build_meeting_response, build_meeting_response_long_id, parse_meeting_response,
};
pub use people::{DirectoryPage, DirectoryPerson, build_people_search, parse_people_search};
pub use policy::{PolicyDecision, evaluate_policy};
pub use provision::{
    ProvisionResult, build_initial_provision, build_policy_ack, build_wipe_ack, parse_provision,
};
pub use sync::{build_mark_read, build_sync, parse_mutation_sync, parse_sync};
