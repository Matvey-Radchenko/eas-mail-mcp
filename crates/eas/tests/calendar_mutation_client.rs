use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{Command, EasError};

#[expect(dead_code, reason = "shared mutation boundary also supports mail-specific fixtures")]
#[path = "mail_mutation_client/support.rs"]
mod boundary;
#[path = "calendar_mutation_client/support.rs"]
mod calendar;

use boundary::{Boundary, collection, http, sync};
use calendar::*;

#[tokio::test]
async fn calendar_accepts_explicit_and_documented_implicit_acknowledgements() -> anyhow::Result<()>
{
    for operation in CALENDAR {
        let response = accepted(Some(vec![item(operation.name(), "1", Some(operation.id()))]))?;
        let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
        assert_eq!(operation.run(&client).await?, 1);
        boundary.request(Command::Sync)?;
    }
    for operation in [Operation::Change, Operation::Delete] {
        for response in [
            accepted(None)?,
            accepted(Some(Vec::new()))?,
            accepted(Some(vec![item(operation.name(), "1", None)]))?,
        ] {
            let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
            assert_eq!(operation.run(&client).await?, 1);
            boundary.request(Command::Sync)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn calendar_rejections_are_definite_and_never_reissued() -> anyhow::Result<()> {
    for operation in CALENDAR {
        for status in ["3", "4", "5", "6", "7", "8", "9", "12", "13", "15", "16"] {
            let response =
                accepted(Some(vec![item(operation.name(), status, Some(operation.id()))]))?;
            let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
            let result = operation.run(&client).await;
            if status == "3" {
                assert!(matches!(result, Err(EasError::InvalidSyncKey)));
            } else {
                assert_eq!(result?.to_string(), status);
            }
            boundary.request(Command::Sync)?;
        }
        let mut root = Element::new("AirSync", "Sync");
        root.push(Element::text("AirSync", "Status", "3"));
        let (client, boundary) = Boundary::client(Ok(http(200, encode(&root)?, None)));
        assert!(matches!(operation.run(&client).await, Err(EasError::InvalidSyncKey)));
        boundary.request(Command::Sync)?;
    }
    Ok(())
}

#[tokio::test]
async fn unsupported_or_malformed_calendar_status_is_unknown_at_every_level() -> anyhow::Result<()>
{
    for operation in CALENDAR {
        for status in ["0", "2", "10", "11", "14", "17", "104", "999", "bad", ""] {
            for level in ["root", "collection", "item"] {
                let response = status_at(operation, status, level)?;
                let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
                assert!(
                    matches!(operation.run(&client).await, Err(EasError::OutcomeUnknown)),
                    "confirmed unsupported {level} status {status}"
                );
                boundary.request(Command::Sync)?;
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn calendar_does_not_confirm_missing_duplicate_or_wrong_acknowledgements()
-> anyhow::Result<()> {
    for operation in CALENDAR {
        let mut cases = malformed_sync(operation)?;
        if matches!(operation, Operation::Add) {
            cases.push(accepted(None)?);
            cases.push(accepted(Some(vec![item("Add", "1", None)]))?);
            let mut add = Element::new("AirSync", "Add");
            add.push(Element::text("AirSync", "ClientId", "client"));
            add.push(Element::text("AirSync", "Status", "1"));
            cases.push(accepted(Some(vec![add]))?);
        }
        for response in cases {
            let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
            assert!(matches!(operation.run(&client).await, Err(EasError::OutcomeUnknown)));
            boundary.request(Command::Sync)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn meeting_accepts_one_result_and_optional_echoes_but_never_retries() -> anyhow::Result<()> {
    for operation in MEETINGS {
        for status in ["1", "2", "3", "4"] {
            for echo in [false, true] {
                let response = meeting(operation, status, echo)?;
                let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
                assert_eq!(operation.run(&client).await?.to_string(), status);
                boundary.request(Command::MeetingResponse)?;
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn meeting_rejects_unrecognized_status_and_mismatched_results() -> anyhow::Result<()> {
    for operation in MEETINGS {
        let mut cases = vec![Vec::new(), vec![0xff], encode(&Element::new("AirSync", "Sync"))?];
        for status in ["0", "5", "14", "104", "146", "999", "bad", ""] {
            cases.push(meeting(operation, status, true)?);
        }
        cases.extend(malformed_meeting(operation)?);
        for response in cases {
            let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
            assert!(matches!(operation.run(&client).await, Err(EasError::OutcomeUnknown)));
            boundary.request(Command::MeetingResponse)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn lost_or_ambiguous_http_calendar_and_meeting_results_stop_after_one_request()
-> anyhow::Result<()> {
    for operation in CALENDAR.into_iter().chain(MEETINGS) {
        for response in [
            Err(EasError::OutcomeUnknown),
            Ok(http(500, Vec::new(), None)),
            Ok(http(503, Vec::new(), Some("60"))),
            Ok(http(204, Vec::new(), None)),
        ] {
            let (client, boundary) = Boundary::client(response);
            assert!(matches!(operation.run(&client).await, Err(EasError::OutcomeUnknown)));
            boundary.request(operation.command())?;
        }
        let (client, boundary) = Boundary::client(Ok(http(429, Vec::new(), Some("37"))));
        assert!(matches!(
            operation.run(&client).await,
            Err(EasError::Throttled { retry_after_seconds: Some(37) })
        ));
        boundary.request(operation.command())?;
    }
    Ok(())
}

fn malformed_sync(operation: Operation) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut truncated = accepted(None)?;
    truncated.pop();
    let mut cases = vec![
        Vec::new(),
        vec![0xff],
        truncated,
        encode(&Element::new("Search", "Search"))?,
        sync(None)?,
    ];
    for fields in [
        vec![("CollectionId", "calendar"), ("SyncKey", "next")],
        vec![("CollectionId", "calendar"), ("Status", "1")],
        vec![("CollectionId", "calendar"), ("SyncKey", "0"), ("Status", "1")],
        vec![("CollectionId", "other"), ("SyncKey", "next"), ("Status", "1")],
        vec![("SyncKey", "next"), ("Status", "1")],
        vec![("CollectionId", "calendar"), ("SyncKey", "next"), ("Status", "1"), ("Status", "1")],
    ] {
        cases.push(sync(Some(collection(&fields, None)))?);
    }
    let mut missing = item(operation.name(), "1", Some(operation.id()));
    missing.content.retain(|node| !matches!(node, eas_mail_protocol::wbxml::Node::Element(child) if child.name == "Status"));
    cases.push(accepted(Some(vec![missing]))?);
    let good = item(operation.name(), "1", Some(operation.id()));
    cases.push(accepted(Some(vec![good.clone(), good.clone()]))?);
    cases.push(accepted(Some(vec![item(operation.name(), "1", Some("different"))]))?);
    cases.push(accepted(Some(vec![item("Fetch", "1", Some(operation.id()))]))?);
    let mut duplicate = good.clone();
    duplicate.push(Element::text("AirSync", "Status", "1"));
    cases.push(accepted(Some(vec![duplicate]))?);
    let mut root = Element::new("AirSync", "Sync");
    let mut collections = Element::new("AirSync", "Collections");
    let col =
        collection(&[("CollectionId", "calendar"), ("SyncKey", "next"), ("Status", "1")], None);
    collections.push(col.clone());
    collections.push(col);
    root.push(collections);
    cases.push(encode(&root)?);
    Ok(cases)
}
