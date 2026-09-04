use std::fs::OpenOptions;
use std::future::{Future as _, poll_fn};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use eas_mail_mcp::{
    AutoReplyGetInput, ErrorCode, ErrorEnvelope, MailListInput, OperationJournal as _, RandomIds,
    Runtime, SystemClock,
};
use eas_mail_mcp_harness::{FakeBackend, MemoryJournal};
use serde_json::{Value, json};

const KINDS: [&str; 11] = [
    "send",
    "reply",
    "forward",
    "read",
    "oof",
    "create",
    "update",
    "delete",
    "cancel",
    "respond_event",
    "respond_mail",
];

#[tokio::test]
async fn queued_mutations_recheck_remote_wipe_after_acquiring_account_lock() -> anyhow::Result<()> {
    for kind in KINDS {
        let directory = tempfile::tempdir()?;
        let backend = Arc::new(FakeBackend::new("work"));
        let journal = Arc::new(MemoryJournal::default());
        let runtime = Runtime::with_dependencies(
            vec![backend.clone()],
            journal.clone(),
            Arc::new(SystemClock),
            Arc::new(RandomIds),
            vec![7; 32],
            directory.path().join("attachments"),
        )?;
        let input = request(&runtime, kind).await?;
        let owner = OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.path().join("write-locks/work.lock"))?;
        owner.lock()?;
        let mut waiting = Box::pin(run(&runtime, kind, input));
        // Fake reads finish immediately: this poll must reach the held account lock.
        let pending =
            poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context).is_pending())).await;
        assert!(pending, "{kind} must wait for the account owner");
        backend.set_failure(Some(ErrorCode::RemoteWipe))?;
        let wipe =
            runtime.mail_get_auto_reply(AutoReplyGetInput { account_id: "work".into() }).await;
        assert_eq!(wipe.error.map(|error| error.code), Some(ErrorCode::RemoteWipe));
        backend.set_failure(None)?;
        drop(owner);
        let error = tokio::time::timeout(Duration::from_secs(1), waiting)
            .await??
            .ok_or_else(|| anyhow::anyhow!("{kind} was not stopped"))?;
        assert_eq!(error.code, ErrorCode::RemoteWipe, "{kind}");
        assert_eq!(error.account_id.as_deref(), Some("work"));
        assert!(backend.operations()?.is_empty(), "{kind} dispatched a mutation after wipe");
        assert_eq!(backend.auto_reply_attempts()?, 0);
        assert!(journal.pending_accounts()?.is_empty());
    }
    Ok(())
}

async fn request(runtime: &Runtime, kind: &str) -> anyhow::Result<Value> {
    let mut value = match kind {
        "send" => {
            json!({"account_id":"work", "to":["self@example.invalid"], "subject":"Fixture", "body":"Body"})
        }
        "oof" => json!({"account_id":"work", "state":"enabled", "internal_message":"Fixture"}),
        "create" => {
            json!({"account_id":"work", "subject":"Fixture", "schedule":{"kind":"timed", "start":"2026-09-15T10:00:00Z", "end":"2026-09-15T11:00:00Z", "time_zone":"UTC"}})
        }
        "reply" | "forward" | "read" => {
            let page = runtime
                .mail_list(MailListInput::default())
                .await
                .data
                .ok_or_else(|| anyhow::anyhow!("mail list"))?;
            let reference =
                page.items.first().ok_or_else(|| anyhow::anyhow!("mail"))?.mail_ref.clone();
            match kind {
                "read" => json!({"mail_ref":reference, "is_read":true}),
                "forward" => {
                    json!({"mail_ref":reference, "to":["self@example.invalid"], "body":"Body"})
                }
                _ => json!({"mail_ref":reference, "body":"Body"}),
            }
        }
        "respond_mail" => {
            let query = serde_json::from_value(
                json!({"query":"meeting-request", "account_ids":["work"], "limit":1}),
            )?;
            let page = runtime
                .mail_search(query)
                .await
                .data
                .ok_or_else(|| anyhow::anyhow!("mail search"))?;
            let reference =
                page.items.first().ok_or_else(|| anyhow::anyhow!("meeting mail"))?.mail_ref.clone();
            json!({"event_ref":reference, "response":"accept"})
        }
        _ => {
            let query = match kind {
                "delete" => "personal",
                "respond_event" => "received",
                _ => "planning",
            };
            let input =
                serde_json::from_value(json!({"query":query, "account_ids":["work"], "limit":1}))?;
            let page = runtime
                .calendar_search(input)
                .await
                .data
                .ok_or_else(|| anyhow::anyhow!("calendar search"))?;
            let reference =
                page.items.first().ok_or_else(|| anyhow::anyhow!("event"))?.event_ref.clone();
            match kind {
                "update" => json!({"event_ref":reference, "subject":"Changed"}),
                "respond_event" => json!({"event_ref":reference, "response":"accept"}),
                _ => json!({"event_ref":reference}),
            }
        }
    };
    value
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("fixture object"))?
        .insert("idempotency_key".into(), json!("00000000-0000-4000-8000-000000000041"));
    Ok(value)
}

async fn run(runtime: &Runtime, kind: &str, input: Value) -> anyhow::Result<Option<ErrorEnvelope>> {
    Ok(match kind {
        "send" => runtime.mail_send(serde_json::from_value(input)?).await.error,
        "reply" => runtime.mail_reply(serde_json::from_value(input)?).await.error,
        "forward" => runtime.mail_forward(serde_json::from_value(input)?).await.error,
        "read" => runtime.mail_mark_read(serde_json::from_value(input)?).await.error,
        "oof" => runtime.mail_set_auto_reply(serde_json::from_value(input)?).await.error,
        "create" => runtime.calendar_create(serde_json::from_value(input)?).await.error,
        "update" => runtime.calendar_update(serde_json::from_value(input)?).await.error,
        "delete" => runtime.calendar_delete(serde_json::from_value(input)?).await.error,
        "cancel" => runtime.calendar_cancel(serde_json::from_value(input)?).await.error,
        "respond_event" | "respond_mail" => {
            runtime.calendar_respond(serde_json::from_value(input)?).await.error
        }
        _ => anyhow::bail!("unknown fixture kind"),
    })
}
