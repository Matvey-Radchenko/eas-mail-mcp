#[expect(dead_code, reason = "shared integration-test support is compiled once per test binary")]
mod support;

use std::sync::Arc;

use eas_mail_mcp::backend::{AccountBackend, MailSource};
use eas_mail_mcp::{MailSearchInput, Runtime};
use eas_mail_mcp_harness::{FixedClock, MemoryJournal, SequenceIds};
use eas_mail_protocol::protocol::{build_item_fetch, build_mail_search};
use eas_mail_protocol::wbxml::{Element, Node, encode};
use eas_mail_protocol::{Command, MailSearchQuery, Patch};

#[tokio::test]
async fn search_then_mutable_locator_resolution_uses_only_point_requests() -> anyhow::Result<()> {
    let query = MailSearchQuery { text: "planning".into(), ..Default::default() };
    let calls = vec![
        support::options(),
        support::read(
            Command::Search,
            build_mail_search(&query, 0, 100, 500)?,
            search_response(true)?,
        ),
        support::read(
            Command::ItemOperations,
            build_item_fetch(Some("long-1"), None, None, 1)?,
            item_response()?,
        ),
    ];
    let (mailbox, transport) = support::mailbox(calls, support::default_policy())?;
    let page = mailbox.search_mail_page(&query, 0, 100).await?;
    let seed = page.items.first().ok_or_else(|| anyhow::anyhow!("missing candidate"))?;
    assert!(matches!(seed.source, MailSource::LongId(_)));
    let resolved = mailbox.resolve_mail_source(&seed.source).await?;
    assert_eq!(
        resolved.source,
        MailSource::Item { folder_id: "inbox".into(), server_id: "message-1".into() }
    );
    assert_eq!(resolved.fields.conversation_id, Patch::Value(vec![0x80; 16]));
    transport.verify_complete()?;
    Ok(())
}

#[tokio::test]
async fn missing_search_total_cannot_claim_complete_coverage() -> anyhow::Result<()> {
    let query = MailSearchQuery { text: "planning".into(), ..Default::default() };
    let (mailbox, transport) = support::mailbox(
        vec![
            support::options(),
            support::read(
                Command::Search,
                build_mail_search(&query, 0, 100, 500)?,
                search_response(false)?,
            ),
        ],
        support::default_policy(),
    )?;
    let directory = tempfile::tempdir()?;
    let runtime = Runtime::with_dependencies(
        vec![Arc::new(mailbox)],
        Arc::new(MemoryJournal::default()),
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    let response =
        runtime.mail_search(MailSearchInput { query: query.text, ..Default::default() }).await;
    let data = response
        .data
        .ok_or_else(|| anyhow::anyhow!("missing search data: {:?}", response.error))?;
    assert_eq!(data.items.len(), 1);
    assert!(data.results_truncated);
    assert_eq!(
        data.coverage
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing fixture result"))?
            .estimated_total,
        None
    );
    assert!(
        !data
            .coverage
            .first()
            .ok_or_else(|| anyhow::anyhow!("missing fixture result"))?
            .candidates_complete
    );
    transport.verify_complete()?;
    Ok(())
}

fn search_response(with_total: bool) -> anyhow::Result<Vec<u8>> {
    search_response_status(with_total, "1")
}

fn search_response_status(with_total: bool, status: &str) -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("Search", "Search");
    root.push(Element::text("Search", "Status", "1"));
    let mut response = Element::new("Search", "Response");
    let mut store = Element::new("Search", "Store");
    store.push(Element::text("Search", "Status", status));
    store.push(Element::text("Search", "Range", "0-0"));
    if with_total {
        store.push(Element::text("Search", "Total", "1"));
    }
    let mut result = Element::new("Search", "Result");
    result.push(Element::text("Search", "LongId", "long-1"));
    result.push(properties("Search"));
    store.push(result);
    response.push(store);
    root.push(response);
    Ok(encode(&root)?)
}

#[tokio::test]
async fn store_range_warning_preserves_items_and_incompleteness_even_when_total_matches()
-> anyhow::Result<()> {
    let query = MailSearchQuery { text: "planning".into(), ..Default::default() };
    let (mailbox, transport) = support::mailbox(
        vec![
            support::options(),
            support::read(
                Command::Search,
                build_mail_search(&query, 0, 100, 500)?,
                search_response_status(true, "12")?,
            ),
        ],
        support::default_policy(),
    )?;
    let directory = tempfile::tempdir()?;
    let runtime = Runtime::with_dependencies(
        vec![Arc::new(mailbox)],
        Arc::new(MemoryJournal::default()),
        Arc::new(FixedClock::new(chrono::DateTime::UNIX_EPOCH)),
        Arc::new(SequenceIds::default()),
        vec![7; 32],
        directory.path().join("attachments"),
    )?;
    let response =
        runtime.mail_search(MailSearchInput { query: query.text, ..Default::default() }).await;
    let data = response.data.ok_or_else(|| anyhow::anyhow!("missing partial data"))?;
    assert_eq!(data.items.len(), 1);
    assert!(data.results_truncated);
    let coverage = data.coverage.first().ok_or_else(|| anyhow::anyhow!("missing coverage"))?;
    assert_eq!(coverage.estimated_total, Some(1));
    assert!(!coverage.candidates_complete);
    assert_eq!(coverage.search_calls, 1);
    transport.verify_complete()?;
    Ok(())
}

fn item_response() -> anyhow::Result<Vec<u8>> {
    let mut root = Element::new("ItemOperations", "ItemOperations");
    root.push(Element::text("ItemOperations", "Status", "1"));
    let mut response = Element::new("ItemOperations", "Response");
    let mut fetch = Element::new("ItemOperations", "Fetch");
    fetch.push(Element::text("ItemOperations", "Status", "1"));
    fetch.push(Element::text("AirSync", "CollectionId", "inbox"));
    fetch.push(Element::text("AirSync", "ServerId", "message-1"));
    fetch.push(properties("ItemOperations"));
    response.push(fetch);
    root.push(response);
    Ok(encode(&root)?)
}

fn properties(namespace: &str) -> Element {
    let mut properties = Element::new(namespace, "Properties");
    properties.push(Element::text("Email", "Subject", "Planning"));
    let mut conversation = Element::new("Email2", "ConversationId");
    conversation.content.push(Node::Opaque(vec![0x80; 16]));
    properties.push(conversation);
    properties
}
