# Security policy

## Supported versions

Security fixes are applied to the latest release. Public binaries are
distributed through platform-restricted npm packages for macOS and Windows.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting for this repository. Do not place
credentials, mailbox content, private endpoint metadata, or internal trust
material in a public issue. Include the affected commit, reproduction steps,
and the expected security boundary.

## Trust boundary

The MCP server and every other process running as the same operating-system
user are in one trusted local boundary. Credentials are stored in that user's
macOS Keychain or Windows Credential Manager, and runtime files are restricted
to that user where the platform supports Unix permission modes. The application
does not attempt to protect data from another process in the same user session.

MCP client name and version are self-reported diagnostic fields, not
authentication or authorization. The only server-side mutation opt-in is the
account's `write_enabled` flag, and every mutation requires an idempotency UUID.
An explicit MCP write-tool call executes immediately after validation; draft
review is an agent-client workflow and must happen before the tool call. The
operational CLI separately shows a complete escaped preview and asks on a
controlling terminal unless `--yes` is present. This prompt is a user-experience
guard inside the same-user boundary, not authentication or authorization.
Calendar operations can have several external steps. Confirmed step bits are
stored without event content; a later safe failure returns `partial`, while an
ambiguous step returns `unknown`. Neither outcome is blindly retried.

## Network boundary

Profiles are local, user-owned configuration and are validated at every process
start. Runtime account config stores only a profile key resolved from that
registry. The transport fixes HTTPS, the
`/Microsoft-Server-ActiveSync` path, Basic authentication over TLS, EAS 14.1,
disabled redirects, and response-origin validation. Profiles cannot contain
ports, IP addresses, wildcard hosts, alternate paths, or TLS bypasses.

Trust mode is either the operating system trust store or one exclusive inline PEM with a
validated SHA-256 fingerprint. The fingerprint detects accidental mismatch
inside the profile; it is not authentication against another same-user process
that can replace both values. TLS verification cannot be disabled by
configuration or environment variables.

## Secrets and content

Passwords, Device IDs, policy state, and the HMAC key are not `Debug` values and
are stored in macOS Keychain or Windows Credential Manager. SQLite contains only idempotency metadata and
keyed payload hashes. Mail and calendar data stays in process memory, except for
explicitly downloaded attachments in a private 24-hour cache.

On Windows, these non-secret runtime files are rooted at
`%LOCALAPPDATA%\EAS Mail MCP`. Reparse points are rejected for application-owned
directories and files.

Mailbox content is untrusted external input. HTML is converted to plain text,
external images are not fetched, file names are sanitized, and MCP responses
mark external content explicitly.

Portable `ref1` object references are untrusted input. Their version, kind,
decoded size, fields, and locator lengths are validated before use. They contain
only account and EAS locator metadata, not bodies, subjects, recipients, or
credentials. They are neither signed nor time-limited because another process
with the same UID can already access the same local account boundary. Immutable
mail-page cursors remain random process-local values with a 15-minute TTL.
