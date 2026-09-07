use serde_json::json;

use super::*;

#[test]
fn support_report_allowlist_drops_identifiers_and_unexpected_fields() -> anyhow::Result<()> {
    let diagnostics = json!({
        "config": "ok",
        "profile_store": {"sha256": "private-profile", "host": "private-host"},
        "secret": "private-secret",
        "accounts": [{
            "account_id": "private-account", "status": "ok",
            "username": "private-user", "path": "/private-directory/file",
            "capabilities": {
                "calendar_availability": "available", "mail_writes": true,
                "personal_calendar_writes": false, "meeting_lifecycle": false,
                "future": "private-data",
            },
        }, {
            "account_id": "private-other", "status": "private-status",
            "code": "NETWORK_UNREACHABLE", "remediation": "private-address@example.invalid",
        }],
    });
    let report = SupportReport::from_diagnostics(&diagnostics);
    assert!(!report.healthy);
    let serialized = serde_json::to_string(&report)?;
    assert!(!serialized.contains("private-"));
    assert!(!serialized.contains("example.invalid"));
    assert!(serialized.contains("NETWORK_UNREACHABLE"));
    assert!(serialized.contains("calendar_availability"));
    Ok(())
}

#[test]
fn check_health_requires_a_ready_account_and_all_enabled_accounts_to_succeed() {
    for accounts in [json!([]), json!([{"status":"disabled"}]), json!([{"status":"failed"}])] {
        assert!(
            !SupportReport::from_diagnostics(&json!({
                "config":"ok", "profile_store":{}, "accounts":accounts,
            }))
            .healthy
        );
    }
    assert!(
        SupportReport::from_diagnostics(&json!({
            "config":"ok", "profile_store":{},
            "accounts":[{"status":"ok"}, {"status":"disabled"}],
        }))
        .healthy
    );
    assert!(
        !SupportReport::from_diagnostics(&json!({
            "config":"ok", "profile_store":"missing", "accounts":[{"status":"ok"}],
        }))
        .healthy
    );
}

#[test]
fn saved_report_is_private_and_does_not_change_parent_permissions() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("report.json");
    let report = SupportReport::failure(ErrorCode::AuthRequired);
    #[cfg(unix)]
    let before = {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755))?;
        std::fs::metadata(directory.path())?.permissions().mode()
    };
    report.write(&path)?;
    let contents: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(contents.get("error_code").and_then(Value::as_str), Some("AUTH_REQUIRED"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(std::fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(directory.path())?.permissions().mode(), before);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn saved_report_rejects_symlinks_without_touching_the_target() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("target");
    let link = directory.path().join("link");
    std::fs::write(&target, b"preserve")?;
    std::os::unix::fs::symlink(&target, &link)?;
    assert!(SupportReport::failure(ErrorCode::AuthRequired).write(&link).is_err());
    assert_eq!(std::fs::read(target)?, b"preserve");
    Ok(())
}
