# Security policy

## Supported versions

Security fixes are applied to the latest release. Public beta binaries are
distributed through platform-restricted npm packages for macOS only.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting for this repository. Do not place
credentials, mailbox content, private endpoint metadata, or internal trust
material in a public issue. Include the affected commit, reproduction steps,
and the expected security boundary.

## Trust boundary

The MCP server and every other process running as the same macOS user are in
one trusted local boundary. Credentials are stored in that user's Keychain and
runtime files use user-only permissions. The application does not attempt to
protect data from another process with the same UID.

MCP client name and version are self-reported diagnostic fields, not
authentication or authorization. The only server-side mutation opt-in is the
account's `write_enabled` flag, and every mutation requires an idempotency UUID.
An explicit write-tool call executes immediately after validation. The server
does not provide a human-confirmation boundary; draft review is an agent-client
workflow and must happen before the tool call. Any client approval policy is
user-experience configuration rather than authentication or authorization.

## Network boundary

Profiles are local, user-owned configuration and are validated at every process
start. Runtime account config stores only a profile key resolved from that
registry. The transport fixes HTTPS, the
`/Microsoft-Server-ActiveSync` path, Basic authentication over TLS, EAS 14.1,
disabled redirects, and response-origin validation. Profiles cannot contain
ports, IP addresses, wildcard hosts, alternate paths, or TLS bypasses.

Trust mode is either the macOS system store or one exclusive inline PEM with a
validated SHA-256 fingerprint. The fingerprint detects accidental mismatch
inside the profile; it is not authentication against another same-user process
that can replace both values. TLS verification cannot be disabled by
configuration or environment variables.

## Secrets and content

Passwords, Device IDs, policy state, and the HMAC key are not `Debug` values and
are stored in macOS Keychain. SQLite contains only idempotency metadata and
keyed payload hashes. Mail and calendar data stays in process memory, except for
explicitly downloaded attachments in a private 24-hour cache.

Mailbox content is untrusted external input. HTML is converted to plain text,
external images are not fetched, file names are sanitized, and MCP responses
mark external content explicitly.
