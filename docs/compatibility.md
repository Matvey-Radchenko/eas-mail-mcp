# Compatibility and limits

## Platform coverage

| Platform | Package | Validation route and boundary |
| --- | --- | --- |
| macOS 14+, Apple Silicon | `eas-mail-mcp-darwin-arm64` | Native build, automated tests, exact-package installation, and live operator acceptance |
| macOS 14+, Intel | `eas-mail-mcp-darwin-x64` | Cross-built and inspected on macOS; a physical Intel Mac is not part of the 1.0 acceptance route |
| Windows 11 x64 | `eas-mail-mcp-windows-x64` | Native Windows Server 2022 CI plus Wine/Whisky package and process checks; physical Windows 11 Credential Manager UI and live-network acceptance is not claimed |
| Windows ARM64, Linux | None | Unsupported |

This describes the validation route, not completed evidence for a candidate.
Each [release acceptance record](releases/1.0.0-acceptance.md) records which
checks actually passed on the exact release artifacts. Native Windows CI and
Wine/Whisky do not establish physical Windows 11 Credential Manager or corporate
network behavior. Windows link tests may omit link creation when privileges are
unavailable.

macOS binaries use ad-hoc code signatures; they are not Developer ID notarized.
The Windows executable is not Authenticode-signed. The four npm packages are
staged by the repository's trusted GitHub Actions publisher. Before approval,
release operators inspect package integrity and provenance for the exact staged
tarballs. npm provenance identifies the build origin; it does not replace an
operating-system publisher signature or certify behavior on a specific server.

## MCP client evidence

Development acceptance on macOS passed actual read-and-draft sessions in Codex
CLI 0.153.1 and OpenCode 1.14.23 against the synthetic `harness-server`. Each
client discovered 36 tools, performed three read calls, returned a text-only
reply draft, and attempted no writes. Server process cleanup was verified;
full-client shutdown did not use the separate EOF-success marker.

Codex's final run is bound to a frozen harness hash. OpenCode's successful run
has no exact loaded-binary hash because a shared rebuild overlapped it; frozen
rechecks reached the model provider's rate limit before tool calls. Claude Code
real-client acceptance was waived by the operator, not marked as passed.
See the [release evidence](releases/1.0.0-acceptance.md#actual-mcp-client-acceptance)
for hashes, protocol versions, and cleanup details. These results do not imply
staged-package, real-mailbox, or every-client-version compatibility.

## Server and authentication

The supported protocol is Exchange ActiveSync 14.1 with Basic authentication
over validated HTTPS at `/Microsoft-Server-ActiveSync`. Required base commands
are Provision, FolderSync, Sync, Search, and ItemOperations. Optional commands
are checked per feature. OAuth, Graph, IMAP, custom paths or ports, redirects,
TLS bypasses, and pretending to be another client are not supported.

The endpoint must allow the configured identity, EAS device, and policy. VPN,
corporate CA deployment, and administrator allowlists remain the operator's
responsibility. `accounts_status` separates local write opt-in from advertised
commands; effective server-side write permission stays unknown until a requested
operation is attempted.

## Feature boundaries

| Feature | Bound or limitation |
| --- | --- |
| Send, reply, forward attachments | Up to 20 files, 25 MiB combined source bytes, 35 MiB encoded MIME; Exchange can enforce a smaller limit |
| Structured mail search | Up to 1,000 candidates and ten logical Search pages per account; precise filters and coverage are documented in [mail search](mail-search-and-threads.md) |
| Conversation reads | Exchange ConversationId is required; no subject heuristic or full mailbox fallback; at most 100 messages and 100,000 total body characters |
| Mail move and delete | Existing folders in the same account; delete moves to system trash and never performs permanent deletion; retain the new reference returned by a move |
| Mutable mail references | Some providers return readable Search LongIds without CollectionId/ServerId, so writes need an Item locator obtained from `mail_list`; no implicit full-mailbox fallback |
| Flags, categories, read state | Completed explicit `mail_list` context in the same MCP session, or CLI `--sync-folder`; no implicit synchronization. Supported flag shapes only; categories replace the set. See [mail properties](mail-properties.md) |
| Batch reads and changes | Up to 20 items; individual outcomes and idempotency keys; no cross-item transaction or automatic rollback |
| Automatic replies | Settings command required; external policy can silently restrict updates, so [read-back verification](auto-reply.md) is mandatory |
| Availability and scheduling | Server precision remains 30 minutes; 15-minute candidate grids do not imply finer free/busy knowledge; see [ranked and recurring scheduling](calendar-scheduling.md) |
| Recurring calendar writes | Explicit series/occurrence/following scope; unsupported shapes fail safely; a split is not an atomic server operation |
| Local attachment retention | Eligible after 24 hours; cleanup runs at startup and before another download, with explicit [cache clear](diagnostics.md) available |
| Offline use | No offline mailbox index; content remains in memory except explicitly downloaded files |

Search coverage, conversation identifiers, EAS flags, categories, and Settings
behavior can differ by provider. Synthetic tests establish request and error
handling, not support on every Exchange deployment. See the release record for
actual provider tests and remaining gaps.

## Windows credential capacity

All accounts share one Windows Credential Manager entry. Its 2,560-byte
UTF-16 capacity includes passwords, device and policy state, identifiers, and
the HMAC key. There is no fixed supported account count. An oversized update
fails safely with `STORAGE_ERROR` and retains the previous entry. Removing unused
accounts can free capacity; shortening passwords is not a suitable workaround.
This limit does not apply to macOS Keychain.
