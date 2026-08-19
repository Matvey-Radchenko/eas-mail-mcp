# ADR 0001: Local security boundary

- Status: accepted
- Date: 2026-08-14

## Context

The server handles sensitive mail and calendar data on a user's Mac. Runtime
endpoint configuration, plaintext secret files, TLS bypasses, or an
unauthenticated shared daemon would expand the required trust boundary.

## Decision

Run one direct stdio process per MCP client. Compile a reviewed endpoint
registry into the binary, require TLS, disable redirects, and validate response
origin. Store credentials and EAS device state in Keychain. Keep mailbox data in
RAM and use SQLite only for content-free mutation idempotency.

Use the honest public identity `EasMailMCP`. Device ID length is an explicit
validated profile field because EAS front doors can impose different limits.
Unsupported policy blocks the account. Remote wipe removes all application data
scoped to that account.

Processes running as the same user are trusted. MCP client identity and version
are diagnostics, not authentication or authorization. Client approval policy is
an optional user-experience control and does not replace account opt-in.

## Consequences

Processes do not share warm mailbox state and may repeat network work. In
return, this version avoids an IPC authentication surface and persistent mailbox
cache. Endpoint allowlisting and organizational data-handling approval remain
deployment-specific release gates.
