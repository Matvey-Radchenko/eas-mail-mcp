//! EAS command builders and strict response parsers.

mod availability;
mod calendar_mutation;
mod calendar_properties;
mod calendar_properties_write;
mod calendar_recurrence;
mod calendar_validation;
pub use calendar_properties::exception_fields;
mod compose;
mod folders;
mod global_object_id;
mod items;
mod meeting_response;
pub use meeting_response::build_meeting_response_instance;
mod people;
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
