# Threat model and release controls

The authoritative trust-boundary summary is in [`SECURITY.md`](../SECURITY.md).
This document records engineering controls used by release builds.

## Local profile controls

- Public binaries and npm tarballs contain no deployment profiles.
- `profile.example.toml` is public and non-routable.
- `profile validate` and `cargo xtask profile verify` validate schema, host and domain syntax,
  duplicate IDs, identity mode, realm and hint syntax, Device ID length, trust
  mode, traversal, symlink, PEM shape, and certificate fingerprint.
- Profile writes are atomic and use private local permissions.
- Import replacement is explicit and rejects account-invalidating changes.
- Inline PEM is public trust material, must contain one certificate and no
  private key, and is checked against its declared SHA-256 fingerprint.

## Publication controls

`cargo xtask public-audit` rejects tracked private-directory files, local user
paths, proprietary-license residue, and any operator denylist terms in the
tracked tree or Git history. Gitleaks scans the public tree and full history.
When present, `.private/` receives a separate credential/private-key scan.
An explicitly approved historical identity exception can match only one exact
commit and the SHA-256 of its raw author/committer metadata; all content scans
remain mandatory. The optional local exception file is inactive when absent.
See [historical metadata audit controls](public-audit.md).

Release builds remap workspace and Cargo source paths. `cargo xtask npm pack`
checks binary size, architecture, code signature, local path leakage, package
contents, version parity, and the absence of lifecycle scripts. Release binary
strings and every unpacked npm tarball are also scanned against the local
operator denylist. Source releases contain no binaries or ignored files.

## Runtime controls

- Redirects and changed response origins fail closed.
- HTTP and arbitrary runtime endpoints are not representable.
- EAS policy is acknowledged only when its requirements are supported.
- Remote wipe purges account credentials, process references, attachments, and
  journal rows.
- Ambiguous mutations are not retried and return `OUTCOME_UNKNOWN`.
- Partial Calendar lifecycles and auto-reply verification persist a content-free
  completed-step mask and are not retried with a new UUID automatically. The
  journal also retains minimal result locators for recovery after mail moves.
- Independent stdio processes serialize mail, auto-reply, and Calendar writes
  per account. Attachment cleanup and writes use a separate cross-process lock.
- Single-write MCP failures and unknown replays set `isError`; acknowledged
  partial results remain structured data with a recovery warning. Batches
  preserve each item's status and never claim transaction semantics.
- Outgoing file bytes enter the operation fingerprint and are checked again
  before execution. The journal stores neither file paths nor attachment bytes.
- Public doctor reports are generated from an explicit allowlist and exclude
  account identifiers, server names, local paths, and mailbox content.
- Write tools require account opt-in and an idempotency UUID. Client identity and
  version are diagnostic only.
- An explicit MCP write-tool call executes immediately after validation. The
  server does not add an elicitation or preview step.
- CLI writes prepare the final operation, render an escaped preview to stderr,
  and require a controlling-terminal confirmation unless `--yes` is explicit.
  No journal or external mutation is created before confirmation.
- Agents must not call a write tool when the user requested only a draft or
  review. Client-level approval remains optional user-experience policy.
- Portable object references contain only bounded account and EAS locator
  metadata. They are strictly decoded but unsigned and have no TTL inside the
  documented same-user trust boundary. RAM snapshot cursors retain a 15-minute
  TTL and never cross processes.
