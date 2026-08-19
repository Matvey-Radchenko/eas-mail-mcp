# Threat model and release controls

The authoritative trust-boundary summary is in [`SECURITY.md`](../SECURITY.md).
This document records engineering controls used by release builds.

## Local profile controls

- Public binaries and npm tarballs contain no deployment profiles.
- `profile.example.toml` is public and non-routable.
- `profile validate` and `cargo xtask profile verify` validate schema, host and domain syntax,
  duplicate IDs, realm syntax, Device ID length, trust mode, traversal, symlink,
  PEM shape, and certificate fingerprint.
- Profile writes are atomic and use private local permissions.
- Import replacement is explicit and rejects account-invalidating changes.
- Inline PEM is public trust material, must contain one certificate and no
  private key, and is checked against its declared SHA-256 fingerprint.

## Publication controls

`cargo xtask public-audit` rejects tracked private-directory files, local user
paths, proprietary-license residue, and any operator denylist terms in the
tracked tree or Git history. Gitleaks scans the public tree and full history.
When present, `.private/` receives a separate credential/private-key scan.

Release builds remap workspace and Cargo source paths. `cargo xtask npm pack`
checks binary size, architecture, code signature, local path leakage, package
contents, version parity, and the absence of lifecycle scripts. Source releases
contain no binaries or ignored files.

## Runtime controls

- Redirects and changed response origins fail closed.
- HTTP and arbitrary runtime endpoints are not representable.
- EAS policy is acknowledged only when its requirements are supported.
- Remote wipe purges account credentials, process references, attachments, and
  journal rows.
- Ambiguous mutations are not retried and return `OUTCOME_UNKNOWN`.
- Write tools require account opt-in and an idempotency UUID. Client identity and
  version are diagnostic only.
- An explicit write-tool call executes immediately after validation. The server
  does not add an elicitation or preview step.
- Agents must not call a write tool when the user requested only a draft or
  review. Client-level approval remains optional user-experience policy.
