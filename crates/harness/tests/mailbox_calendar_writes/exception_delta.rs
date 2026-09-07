use anyhow::Context as _;
use chrono::Duration;
use eas_mail_protocol::protocol::parse_calendar_item_fetch;
use eas_mail_protocol::wbxml::decode;
use eas_mail_protocol::{
    CalendarException, CalendarProperties, CalendarRecurrence, RecurrenceEnd, RecurrencePattern,
};

use super::*;

fn original() -> anyhow::Result<(CalendarApplication, Vec<u8>)> {
    let mut original = application()?;
    original.properties.recurrence = Some(CalendarRecurrence {
        pattern: RecurrencePattern::Weekly { days: 2 },
        interval: 1,
        first_day_of_week: 1,
        end: RecurrenceEnd::Count(3),
    });
    original.properties.exceptions = vec![CalendarException {
        original_start: original.starts_at + Duration::weeks(1),
        deleted: false,
        fields: CalendarFields {
            location: Patch::Value("Changed once".into()),
            starts_at: Patch::Value(Some(original.starts_at + Duration::weeks(1))),
            ends_at: Patch::Value(Some(original.ends_at + Duration::weeks(1))),
            body: Patch::Value(String::new()),
            properties: Some(CalendarProperties {
                categories: Some(Vec::new()),
                ..Default::default()
            }),
            ..Default::default()
        },
    }];
    let response = item_response(&original)?;
    // The pre-image matches what the server actually returns, including default-expanded properties.
    let fields = parse_calendar_item_fetch(&response)?.fields;
    original.properties = fields.properties.context("missing fixture properties")?;
    Ok((original, response))
}

#[tokio::test]
async fn deleting_another_occurrence_does_not_replay_server_expanded_sibling_overrides()
-> anyhow::Result<()> {
    let (original, response) = original()?;
    let deleted = CalendarException {
        original_start: original.starts_at + Duration::weeks(2),
        deleted: true,
        fields: CalendarFields::default(),
    };
    let mut changed = original.clone();
    changed.properties.exceptions.push(deleted.clone());
    let mut delta = changed.clone();
    delta.properties.exceptions = vec![deleted];
    verify_change(response, changed, delta).await
}

#[tokio::test]
async fn editing_an_existing_exception_deletes_empty_categories_without_replaying_an_empty_container()
-> anyhow::Result<()> {
    for categories in [Vec::new(), vec!["Preserved category".into()]] {
        let (original, response) = original()?;
        let mut changed = original.clone();
        let fields =
            &mut changed.properties.exceptions.first_mut().context("missing exception")?.fields;
        fields.location = Patch::Value("Changed twice".into());
        fields.properties.as_mut().context("missing exception properties")?.categories =
            Some(categories.clone());
        let mut delta = changed.clone();
        if categories.is_empty() {
            delta
                .properties
                .exceptions
                .first_mut()
                .context("missing exception")?
                .fields
                .properties
                .as_mut()
                .context("missing exception properties")?
                .categories = None;
        }
        verify_change(response, changed, delta).await?;
    }
    Ok(())
}

async fn verify_change(
    response: Vec<u8>,
    changed: CalendarApplication,
    delta: CalendarApplication,
) -> anyhow::Result<()> {
    let calls = vec![
        options_with_calendar_writes(),
        read(
            Command::Sync,
            build_sync("calendar", "0", CollectionKind::Calendar, 6, 0)?,
            sync_response("key-1", 1, false, Vec::new())?,
        ),
        read(
            Command::Sync,
            build_sync("calendar", "key-1", CollectionKind::Calendar, 6, 0)?,
            sync_response("key-2", 1, false, vec![calendar_uid_change("event", "uid-1")])?,
        ),
        read(
            Command::ItemOperations,
            build_item_fetch(None, Some("calendar"), Some("event"), 50_000)?,
            response,
        ),
        mutation(
            Command::Sync,
            build_calendar_change("calendar", "key-2", "event", &delta)?,
            mutation_response("Change", "key-3", 1, Some("event"))?,
        ),
    ];
    let (mailbox, transport) = mailbox(calls, default_policy())?;
    let result = mailbox
        .update_calendar_item(
            &source("event", "uid-1"),
            &BackendCalendarMutation { target_collection: None, application: changed.clone() },
        )
        .await?;
    assert_eq!(result.fields.properties, Some(changed.properties));
    transport.verify_complete()?;
    Ok(())
}

fn item_response(item: &CalendarApplication) -> anyhow::Result<Vec<u8>> {
    let add = decode(&build_calendar_add("calendar", "key", "client", item)?)?
        .context("missing synthetic add")?;
    let mut properties =
        add.descendant("AirSync", "ApplicationData").context("missing fixture data")?.clone();
    properties.namespace = "ItemOperations".into();
    properties.name = "Properties".into();
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    fetch.push(Element::text("AirSync", "CollectionId", "calendar"));
    fetch.push(Element::text("AirSync", "ServerId", "event"));
    fetch.push(properties);
    let mut root = Element::new("ItemOperations", "ItemOperations");
    root.push(fetch);
    Ok(encode(&root)?)
}
