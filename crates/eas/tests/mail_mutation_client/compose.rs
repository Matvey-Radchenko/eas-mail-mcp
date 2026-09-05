use super::{Boundary, http};
use eas_mail_protocol::protocol::ComposeSource;
use eas_mail_protocol::wbxml::{Element, Node, encode};
use eas_mail_protocol::{Command, EasClient, EasError, MutationResult};

const COMMANDS: [Command; 3] = [Command::SendMail, Command::SmartReply, Command::SmartForward];

#[tokio::test]
async fn compose_empty_http200_and_documented_rejections_send_once() -> anyhow::Result<()> {
    for command in COMMANDS {
        let (client, boundary) = Boundary::client(Ok(http(200, Vec::new(), None)));
        assert_eq!(execute(&client, command).await?.status, 1);
        boundary.request(command)?;
        for status in [101, 107, 115, 116, 117, 119, 120, 121, 122, 130, 150, 166, 167] {
            let body = acknowledgement(command, &status.to_string())?;
            let (client, boundary) = Boundary::client(Ok(http(200, body, None)));
            assert_eq!(execute(&client, command).await?.status, status);
            boundary.request(command)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn compose_unknown_ambiguous_or_later_version_status_never_confirms_rejection()
-> anyhow::Result<()> {
    for command in COMMANDS {
        for status in [
            "0", "1", "7", "100", "110", "111", "118", "132", "153", "154", "157", "169", "178",
            "183", "999", "65536", "invalid",
        ] {
            let (client, boundary) =
                Boundary::client(Ok(http(200, acknowledgement(command, status)?, None)));
            assert!(
                matches!(execute(&client, command).await, Err(EasError::OutcomeUnknown)),
                "unexpectedly confirmed {} status {status}",
                command.name()
            );
            boundary.request(command)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn compose_missing_duplicate_nested_or_wrong_root_ack_is_unknown() -> anyhow::Result<()> {
    for command in COMMANDS {
        let mut truncated = acknowledgement(command, "122")?;
        truncated.pop();
        let mut bodies = vec![
            vec![0xff],
            vec![3, 1, 0x6a, 0],
            truncated,
            encode(&Element::new("ComposeMail", command.name()))?,
        ];
        for other in COMMANDS.into_iter().filter(|other| *other != command) {
            bodies.push(acknowledgement(other, "122")?);
        }
        let mut root = Element::new("AirSync", "Sync");
        root.push(Element::text("ComposeMail", "Status", "122"));
        bodies.push(encode(&root)?);
        let status = Element::text("ComposeMail", "Status", "122");
        let mut root = Element::new("ComposeMail", command.name());
        root.push(status.clone());
        root.push(status.clone());
        bodies.push(encode(&root)?);
        let mut wrapper = Element::new("ComposeMail", "Source");
        wrapper.push(status.clone());
        let mut root = Element::new("ComposeMail", command.name());
        root.push(wrapper);
        bodies.push(encode(&root)?);
        for node in [Node::Element(status), Node::Opaque(b"122".to_vec())] {
            let mut status = Element::text("ComposeMail", "Status", "122");
            status.content.push(node);
            let mut root = Element::new("ComposeMail", command.name());
            root.push(status);
            bodies.push(encode(&root)?);
        }
        for body in bodies {
            let (client, boundary) = Boundary::client(Ok(http(200, body, None)));
            assert!(matches!(execute(&client, command).await, Err(EasError::OutcomeUnknown)));
            boundary.request(command)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn compose_unconfirmed_http_or_lost_response_is_unknown_without_resending()
-> anyhow::Result<()> {
    for command in COMMANDS {
        for response in [
            Ok(http(201, Vec::new(), None)),
            Ok(http(204, Vec::new(), None)),
            Ok(http(500, Vec::new(), None)),
            Ok(http(503, Vec::new(), Some("10"))),
            Err(EasError::OutcomeUnknown),
        ] {
            let (client, boundary) = Boundary::client(response);
            assert!(matches!(execute(&client, command).await, Err(EasError::OutcomeUnknown)));
            boundary.request(command)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn compose_remote_wipe_and_definite_http_errors_remain_distinct() -> anyhow::Result<()> {
    for command in COMMANDS {
        let body = acknowledgement(command, "140")?;
        let (client, boundary) = Boundary::client(Ok(http(200, body, None)));
        assert!(matches!(execute(&client, command).await, Err(EasError::AccountRemoteWipe)));
        boundary.request(command)?;
        for status in [401, 403, 429, 449] {
            let (client, boundary) = Boundary::client(Ok(http(status, Vec::new(), Some("7"))));
            assert!(matches!(
                (status, execute(&client, command).await),
                (401, Err(EasError::Authentication))
                    | (403, Err(EasError::AccessDenied))
                    | (429, Err(EasError::Throttled { retry_after_seconds: Some(7) }))
                    | (449, Err(EasError::PolicyRefreshRequired))
            ));
            boundary.request(command)?;
        }
    }
    Ok(())
}

fn acknowledgement(command: Command, status: &str) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("ComposeMail", command.name());
    root.push(Element::text("ComposeMail", "Status", status));
    Ok(encode(&root)?)
}

async fn execute(
    client: &EasClient,
    command: Command,
) -> eas_mail_protocol::Result<MutationResult> {
    if command == Command::SendMail {
        client.send(7, "fixture-client-id", b"fixture MIME".to_vec()).await
    } else {
        client
            .smart_compose(
                7,
                command == Command::SmartForward,
                "fixture-client-id",
                ComposeSource::LongId("fixture-source"),
                b"fixture MIME".to_vec(),
            )
            .await
    }
}
