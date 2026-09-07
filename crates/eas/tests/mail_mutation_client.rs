use chrono::{Duration, Utc};
use eas_mail_protocol::protocol::MailPatch;
use eas_mail_protocol::wbxml::{Element, encode};
use eas_mail_protocol::{Command, EasClient, EasError, MutationResult};

#[path = "mail_mutation_client/support.rs"]
mod boundary;
#[path = "mail_mutation_client/compose.rs"]
mod compose;
use boundary::*;

#[tokio::test]
async fn property_change_sends_one_minimal_mutation_and_preserves_the_new_key() -> anyhow::Result<()>
{
    for (patch, property) in [
        (MailPatch::Read(true), "Read"),
        (MailPatch::Categories(vec!["Project".into(), "Shared".into()]), "Categories"),
        (MailPatch::Categories(Vec::new()), "Categories"),
        (MailPatch::Flag { status: 2, previous: None, updated_at: Utc::now() }, "Flag"),
    ] {
        let (client, boundary) = Boundary::client(Ok(http(200, accepted(None)?, None)));
        let result = client.mail_change(7, "inbox", "message", "current", &patch).await?;
        assert_eq!(result.status, 1);
        assert_eq!(result.sync_key.as_deref(), Some("next"));
        let tree = boundary.request(Command::Sync)?;
        assert_eq!(text(&tree, "AirSync", "CollectionId").as_deref(), Some("inbox"));
        assert_eq!(text(&tree, "AirSync", "ServerId").as_deref(), Some("message"));
        assert_eq!(text(&tree, "AirSync", "SyncKey").as_deref(), Some("current"));
        assert_eq!(text(&tree, "AirSync", "GetChanges").as_deref(), Some("0"));
        let data = tree
            .descendant("AirSync", "ApplicationData")
            .ok_or_else(|| anyhow::anyhow!("missing property data"))?;
        assert_eq!(data.children().count(), 1);
        assert_eq!(data.children().next().map(|value| value.name.as_str()), Some(property));
    }
    Ok(())
}

#[tokio::test]
async fn single_change_accepts_optional_server_id_but_rejects_nested_status() -> anyhow::Result<()>
{
    // MS-ASCMD 2.2.3.166.8 makes ServerId optional under Responses/Change.
    for status in ["1", "6"] {
        let response = accepted(Some(vec![change(&[("Status", status)])]))?;
        let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
        assert_eq!(change_read(&client).await?.status.to_string(), status);
        boundary.request(Command::Sync)?;
    }
    let mut item = Element::new("AirSync", "Change");
    let mut status = Element::new("AirSync", "Status");
    status.push(Element::text("AirSync", "Status", "1"));
    item.push(status);
    let response = accepted(Some(vec![item]))?;
    let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
    assert!(matches!(change_read(&client).await, Err(EasError::OutcomeUnknown)));
    boundary.request(Command::Sync)?;
    Ok(())
}

#[tokio::test]
async fn explicit_change_and_move_rejections_remain_definite_without_resending()
-> anyhow::Result<()> {
    for status in ["1", "3", "4", "5", "6", "7", "8", "9", "12", "13", "15", "16"] {
        let response =
            accepted(Some(vec![change(&[("ServerId", "message"), ("Status", status)])]))?;
        let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
        let result = change_read(&client).await?;
        assert_eq!(result.status.to_string(), status);
        assert_eq!(result.sync_key.as_deref(), Some("next"));
        boundary.request(Command::Sync)?;
    }
    for status in ["1", "2", "3", "4", "5", "7"] {
        let mut fields = vec![("SrcMsgId", "message"), ("Status", status)];
        if status == "3" {
            fields.push(("DstMsgId", "moved-message"));
        }
        let (client, boundary) = Boundary::client(Ok(http(200, moved(&fields)?, None)));
        let result = client.move_mail(7, "inbox", "message", "trash").await?;
        assert_eq!(result.status.to_string(), status);
        assert_eq!(result.server_id.as_deref(), (status == "3").then_some("moved-message"));
        let tree = boundary.request(Command::MoveItems)?;
        assert_eq!(text(&tree, "Move", "SrcMsgId").as_deref(), Some("message"));
        assert_eq!(text(&tree, "Move", "SrcFldId").as_deref(), Some("inbox"));
        assert_eq!(text(&tree, "Move", "DstFldId").as_deref(), Some("trash"));
    }
    Ok(())
}

#[tokio::test]
async fn unsupported_numeric_acknowledgements_are_unknown_without_resending() -> anyhow::Result<()>
{
    for patch in [
        MailPatch::Read(true),
        MailPatch::Categories(vec!["Project".into()]),
        MailPatch::Flag { status: 2, previous: None, updated_at: Utc::now() },
    ] {
        for status in ["0", "2", "10", "11", "14", "17", "100", "101", "110", "153", "154", "999"] {
            let response =
                accepted(Some(vec![change(&[("ServerId", "message"), ("Status", status)])]))?;
            let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
            assert!(
                matches!(
                    client.mail_change(7, "inbox", "message", "current", &patch).await,
                    Err(EasError::OutcomeUnknown)
                ),
                "unexpectedly confirmed Sync status {status}"
            );
            boundary.request(Command::Sync)?;
        }
    }
    for status in ["0", "6", "8", "100", "101", "110", "153", "154", "999"] {
        let response = moved(&[("SrcMsgId", "message"), ("Status", status), ("DstMsgId", "new")])?;
        let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
        assert!(
            matches!(
                client.move_mail(7, "inbox", "message", "trash").await,
                Err(EasError::OutcomeUnknown)
            ),
            "unexpectedly confirmed MoveItems status {status}"
        );
        boundary.request(Command::MoveItems)?;
    }
    Ok(())
}

#[tokio::test]
async fn missing_truncated_or_wrong_sync_acknowledgement_is_unknown() -> anyhow::Result<()> {
    let mut truncated = accepted(None)?;
    truncated.pop();
    let mut malformed = vec![
        Vec::new(),
        vec![0xff],
        truncated,
        encode(&Element::new("Search", "Search"))?,
        encode(&Element::new("AirSync", "Sync"))?,
        sync(None)?,
    ];
    for fields in [
        vec![("CollectionId", "inbox"), ("SyncKey", "next")],
        vec![("CollectionId", "inbox"), ("Status", "1")],
        vec![("CollectionId", "inbox"), ("Status", "1"), ("SyncKey", "0")],
        vec![("CollectionId", "other"), ("Status", "1"), ("SyncKey", "next")],
        vec![("CollectionId", "inbox"), ("Status", "1"), ("Status", "6"), ("SyncKey", "next")],
    ] {
        malformed.push(sync(Some(collection(&fields, None)))?);
    }
    for fields in [
        vec![("ServerId", "message")],
        vec![("ServerId", "message"), ("Status", "invalid")],
        vec![("ServerId", "other"), ("Status", "1")],
    ] {
        malformed.push(accepted(Some(vec![change(&fields)]))?);
    }
    for response in malformed {
        let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
        assert!(matches!(change_read(&client).await, Err(EasError::OutcomeUnknown)));
        boundary.request(Command::Sync)?;
    }
    let stale = collection(&[("CollectionId", "inbox"), ("Status", "3")], None);
    let (client, boundary) = Boundary::client(Ok(http(200, sync(Some(stale))?, None)));
    assert!(matches!(change_read(&client).await, Err(EasError::InvalidSyncKey)));
    boundary.request(Command::Sync)?;
    Ok(())
}

#[tokio::test]
async fn missing_or_mismatched_move_acknowledgement_is_unknown() -> anyhow::Result<()> {
    let mut truncated = moved(&[("SrcMsgId", "message"), ("Status", "3"), ("DstMsgId", "new")])?;
    truncated.pop();
    let mut malformed = vec![
        Vec::new(),
        vec![0xff],
        truncated,
        encode(&Element::new("AirSync", "Sync"))?,
        encode(&Element::new("Move", "MoveItems"))?,
    ];
    for fields in [
        vec![("SrcMsgId", "message")],
        vec![("SrcMsgId", "message"), ("Status", "invalid")],
        vec![("SrcMsgId", "other"), ("Status", "3"), ("DstMsgId", "new")],
        vec![("SrcMsgId", "message"), ("Status", "3")],
        vec![("SrcMsgId", "message"), ("Status", "3"), ("DstMsgId", "")],
    ] {
        malformed.push(moved(&fields)?);
    }
    for response in malformed {
        let (client, boundary) = Boundary::client(Ok(http(200, response, None)));
        assert!(matches!(
            client.move_mail(7, "inbox", "message", "trash").await,
            Err(EasError::OutcomeUnknown)
        ));
        boundary.request(Command::MoveItems)?;
    }
    Ok(())
}

#[tokio::test]
async fn lost_http_response_and_ambiguous_server_errors_are_never_retried() -> anyhow::Result<()> {
    for move_item in [false, true] {
        for response in [
            Err(EasError::OutcomeUnknown),
            Ok(http(500, Vec::new(), None)),
            Ok(http(503, Vec::new(), Some("120"))),
            Ok(http(204, Vec::new(), None)),
        ] {
            let (client, boundary) = Boundary::client(response);
            assert!(matches!(operation(&client, move_item).await, Err(EasError::OutcomeUnknown)));
            boundary.request(if move_item { Command::MoveItems } else { Command::Sync })?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn http_rejections_and_retry_after_survive_both_mutation_clients() -> anyhow::Result<()> {
    for move_item in [false, true] {
        for status in [401, 403, 449] {
            let (client, boundary) = Boundary::client(Ok(http(status, Vec::new(), None)));
            let error = operation(&client, move_item)
                .await
                .err()
                .ok_or_else(|| anyhow::anyhow!("expected rejection"))?;
            assert!(matches!(
                (status, error),
                (401, EasError::Authentication)
                    | (403, EasError::AccessDenied)
                    | (449, EasError::PolicyRefreshRequired)
            ));
            boundary.request(if move_item { Command::MoveItems } else { Command::Sync })?;
        }
        for delay in [Some("37"), Some("invalid"), None] {
            let (client, boundary) = Boundary::client(Ok(http(429, Vec::new(), delay)));
            let expected = delay.and_then(|value| value.parse().ok());
            assert!(matches!(operation(&client, move_item).await,
                Err(EasError::Throttled { retry_after_seconds }) if retry_after_seconds == expected));
            boundary.request(if move_item { Command::MoveItems } else { Command::Sync })?;
        }
        let future = (Utc::now() + Duration::seconds(120)).to_rfc2822();
        let (client, boundary) = Boundary::client(Ok(http(429, Vec::new(), Some(&future))));
        assert!(matches!(operation(&client, move_item).await,
            Err(EasError::Throttled { retry_after_seconds: Some(remaining) }) if (115..=120).contains(&remaining)));
        boundary.request(if move_item { Command::MoveItems } else { Command::Sync })?;
    }
    Ok(())
}

#[tokio::test]
async fn invalid_property_is_rejected_before_the_transport_boundary() -> anyhow::Result<()> {
    let (client, boundary) = Boundary::client(Err(EasError::OutcomeUnknown));
    let patch = MailPatch::Flag { status: 3, previous: None, updated_at: Utc::now() };
    assert!(matches!(
        client.mail_change(7, "inbox", "message", "current", &patch).await,
        Err(EasError::InvalidConfiguration(_))
    ));
    boundary.assert_no_calls()?;
    Ok(())
}

async fn change_read(client: &EasClient) -> eas_mail_protocol::Result<MutationResult> {
    client.mail_change(7, "inbox", "message", "current", &MailPatch::Read(false)).await
}

async fn operation(
    client: &EasClient,
    move_item: bool,
) -> eas_mail_protocol::Result<MutationResult> {
    if move_item {
        client.move_mail(7, "inbox", "message", "trash").await
    } else {
        change_read(client).await
    }
}
