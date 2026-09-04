# Changelog

## 1.0.0 — unreleased

### Added

- Attach local files to sent, replied, and forwarded mail with bounded MIME
  construction, content-sensitive idempotency, and CLI file previews.
- Structured mail filters and explicit per-account search coverage, with no
  hidden full-mailbox synchronization; read a bounded Exchange conversation.
- Move mail, move it to trash, change follow-up flags and categories, and process
  bounded batches with separate outcomes for each item. Property changes reuse
  explicit listing state; standalone CLI commands accept `--sync-folder`.
- Read and set automatic replies, including schedules and explicit external
  audiences, with effective-setting verification after every acknowledged Set.
- Rank meeting starts using required and optional participants, individual
  working hours and time zones, buffers, and weekly recurring patterns.
- Probe individual accounts with `accounts_status`; inspect stored write
  outcomes through MCP and CLI; preserve unavailable account metadata while
  other accounts continue working.
- `doctor --check`, a support report that omits private identifiers, and local
  cache usage and clearing with interprocess coordination.
- Upgrade, recovery, uninstall, compatibility, and support documentation.

### Reliability and compatibility

- Versioned transactional journal migration preserves existing operation keys
  and protects uncertain outcomes from accidental replay.
- Runtime and historical failed/unknown write outcomes are represented as MCP
  tool errors; partial outcomes retain structured results and warnings.
- Existing 0.5.x input forms remain accepted; new optional features have explicit
  bounds. Server availability and real platform evidence are recorded in the
  [1.0 acceptance record](docs/releases/1.0.0-acceptance.md).

## 0.5.1

- Fixed stale calendar references across EAS sessions, empty server-managed
  calendar fields, explicit reminder clearing, and Calendar Delete semantics.
- Details: [0.5.1 acceptance](docs/releases/0.5.1-acceptance.md).

## 0.5.0

- Added directory search and recurring calendar write scopes.
- Details: [0.5.0 acceptance](docs/releases/0.5.0-acceptance.md).
