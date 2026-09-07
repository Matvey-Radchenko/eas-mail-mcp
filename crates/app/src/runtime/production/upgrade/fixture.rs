// Frozen synthetic 0.5.1 formats, checked against commit 93968a0:
// config.rs, keychain.rs, references/object.rs, model/input.rs, and journal.rs.
// Keep these bytes independent of current serializers and migration builders.

pub(super) const CONFIG: &str = r#"version = 1

[accounts.work]
profile = "example"
email = "user@example.invalid"
username = "example_user"
enabled = true
write_enabled = true
"#;

pub(super) const PROFILES: &str = r#"schema_version = 2
bundle_version = "example-1"

[[profiles]]
id = "example"
display_name = "Example EAS"
host = "mail.example.invalid"
email_domains = ["example.invalid"]
device_id_length = 16

[profiles.identity]
mode = "username"
username_hint = "Your Exchange login"

[profiles.trust]
mode = "system"
"#;

pub(super) const SECRETS: &str = r#"{
  "version": 1,
  "hmac_key": [7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7],
  "accounts": {
    "work": {
      "password": "fixture-value",
      "device_id": "0011223344556677",
      "policy_key": 123,
      "policy": {
        "max_attachment_bytes": 1048576,
        "attachments_enabled": true,
        "body_limit": 10240,
        "mail_filter_type": 2,
        "calendar_filter_type": 5
      }
    }
  }
}"#;

pub(super) const SCHEMA: &str = "PRAGMA journal_mode=WAL;
 PRAGMA synchronous=FULL;
 CREATE TABLE operations (
 operation_id TEXT PRIMARY KEY,
 account_id TEXT NOT NULL,
 kind TEXT NOT NULL,
 payload_hmac TEXT NOT NULL,
 client_id TEXT NOT NULL,
 status TEXT NOT NULL,
 completed_steps INTEGER NOT NULL DEFAULT 0,
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL
 );";

pub(super) const UUID: &str = "11111111-2222-4333-8444-555555555555";
pub(super) const CANONICAL_SEND: &str = r#"{"account_id":"work","to":["self@example.invalid"],"cc":[],"bcc":[],"subject":"Upgrade fixture","body":"Synthetic body","idempotency_key":"11111111-2222-4333-8444-555555555555"}"#;
pub(super) const HMAC: &str = "872199be1062090d2691adda2ed37166fb690a63b66a8ca9e36e27f9e8e420a6";
pub(super) const MAIL_ITEM: &str = "ref1.mail.eyJhY2NvdW50X2lkIjoid29yayIsInNvdXJjZSI6eyJraW5kIjoiaXRlbSIsImZvbGRlcl9pZCI6ImluYm94Iiwic2VydmVyX2lkIjoibWVzc2FnZS0xIn19";
pub(super) const MAIL_SEARCH: &str = "ref1.mail.eyJhY2NvdW50X2lkIjoid29yayIsInNvdXJjZSI6eyJraW5kIjoibG9uZ19pZCIsImxvbmdfaWQiOiJsZWdhY3ktc2VhcmNoLTEifX0";
pub(super) const EVENT: &str = "ref1.event.eyJhY2NvdW50X2lkIjoid29yayIsInVpZCI6ImV2ZW50LXVpZEBleGFtcGxlLmludmFsaWQiLCJsb25nX2lkIjoiZXZlbnQtMSIsImNvbGxlY3Rpb25faWQiOiJjYWxlbmRhciIsInNlcnZlcl9pZCI6ImV2ZW50LTEifQ";
pub(super) const OCCURRENCE: &str = "ref1.event.eyJvY2N1cnJlbmNlX3N0YXJ0IjoiMjAyNi0wOS0xNVQxMDowMDowMFoiLCJhY2NvdW50X2lkIjoid29yayIsInVpZCI6ImV2ZW50LXVpZEBleGFtcGxlLmludmFsaWQiLCJsb25nX2lkIjoiZXZlbnQtMSIsImNvbGxlY3Rpb25faWQiOiJjYWxlbmRhciIsInNlcnZlcl9pZCI6ImV2ZW50LTEifQ";
