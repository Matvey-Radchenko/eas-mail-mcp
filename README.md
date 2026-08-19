# EAS Mail MCP

![EAS Mail MCP connects mail and calendar data to local AI tools](docs/assets/readme-hero.png)

`eas-mail-mcp` is a local Rust MCP server for Exchange ActiveSync 14.1. It gives
supported AI clients structured mail tools and read-only calendar tools without
a daemon, GUI, mailbox database, or hosted intermediary service.

Public binaries contain no mail server, domain, realm, certificate, account, or
password. A user imports or creates a validated local endpoint profile during
`setup`; credentials are stored separately in macOS Keychain.

## How it works

The simplest mental model is a local, typed API adapter. An AI client speaks
MCP, while Exchange speaks EAS. The binary translates between the two without
running a hosted service of its own.

```mermaid
flowchart LR
    User["User"] --> Client["Codex / Claude Code / OpenCode"]
    Client -->|"MCP JSON-RPC over stdin/stdout"| App["eas-mail-mcp"]
    App -->|"EAS 14.1 over HTTPS"| Exchange["Managed Exchange server"]
    App --> Keychain["macOS Keychain"]
    App --> Config["Non-secret account config"]
    App --> Journal["Idempotency journal"]
    App --> Attachments["Temporary attachment cache"]
```

Each MCP client launches its own `eas-mail-mcp serve` process. There is no
daemon and no mailbox database. The process stays alive for the MCP session,
keeps synchronization state in RAM, and exits with the client.

### Terms for application developers

| Term | Practical mental model |
| --- | --- |
| MCP tool | A typed endpoint such as `mail_list` or `mail_send` |
| EAS | The Exchange API used for mail and calendar synchronization |
| WBXML | A compact binary representation of the XML messages used by EAS |
| SyncKey | A server-issued version cursor: "changes since this state" |
| `mail_ref` / `event_ref` | A short-lived process-local handle, not a database ID |
| Cursor | A pointer into one immutable in-memory result snapshot |

### Example read path

For a request such as "show my latest mail", the client calls `mail_list`:

```mermaid
sequenceDiagram
    participant Client as MCP client
    participant App as eas-mail-mcp
    participant Keychain as macOS Keychain
    participant Exchange as Exchange

    Client->>App: Start process
    App->>Keychain: Load credentials and policy state
    Client->>App: initialize with capabilities (name/version are diagnostics)
    Client->>App: tools/call mail_list
    App->>Exchange: OPTIONS once per process
    opt No acknowledged policy is stored
        App->>Exchange: Provision and acknowledge supported limits
    end
    App->>Exchange: FolderSync
    App->>Exchange: Sync Inbox and Sent
    Exchange-->>App: WBXML pages and new SyncKeys
    App->>App: Decode, patch RAM state, sanitize content
    App-->>Client: Structured data / error / warnings
```

`mail_list` refreshes Inbox and Sent unless explicit `folder_ids` are supplied.
`sync_now` can refresh every selected collection. Lists return metadata and a
short plain-text preview; `mail_get` fetches the full body only when requested,
and attachments require separate list and download calls. `mail_search` always
searches Exchange instead of a local index.

The first request in a new process is a cold synchronization. Later requests in
the same process can reuse FolderSync and collection SyncKeys, so they usually
transfer fewer records. Restarting the process intentionally discards that
state.

### State and storage

| Location | Stored data |
| --- | --- |
| Installed binary | Generic EAS transport and MCP runtime; no endpoint data |
| `profiles.toml` | Host, allowed email domains, optional realm, and TLS trust mode |
| `config.toml` | Profile key, email, username, enabled state, write permission |
| macOS Keychain | Password, Device ID, policy state, journal HMAC key |
| Process RAM | Folders, SyncKeys, mail/event data, references, cursors |
| SQLite | Content-free idempotency metadata for write operations only |
| Cache directory | Explicitly downloaded attachments, retained for up to 24 hours |

RAM references and cursors expire after 15 minutes and cannot be shared between
independent Codex, Claude Code, or OpenCode processes. A new list or search call
creates a new immutable snapshot, capped at 100 returned records per page.

### Write safety

Mail writes are disabled per account by default. Once `write_enabled` is set,
client name and version do not add another authorization gate. Every write
requires a caller-provided UUID `idempotency_key`. Before contacting Exchange,
the process records a payload HMAC and `pending` state in SQLite. Reusing the
same UUID with different input is rejected.

Calling a write tool executes it immediately after account, reference, payload,
and idempotency validation. The server does not request a second confirmation or
generate a separate preview. Agents should call a write tool only after an
explicit user request to perform that mutation; asking to draft or review a
message must not trigger the tool. Outgoing bodies are limited to 50,000 Unicode
characters before any journal or network write.

Read requests may retry transient network failures. Mail mutations are not
blindly retried: if the connection fails after a request may have reached
Exchange, the result is `OUTCOME_UNKNOWN` rather than a possible duplicate
message. Generated client configuration does not add write-tool approval
overrides; a possible client-level prompt is controlled entirely by that client.
Neither client policy nor self-reported client identity is an authentication
boundary. See [Security](SECURITY.md) for the full threat model.

### Repository map

- [`crates/app`](crates/app) contains the CLI, MCP tools, runtime, Keychain,
  references, attachment cache, and idempotency journal.
- [`crates/eas`](crates/eas) contains strict HTTPS transport, EAS commands,
  protocol parsing, and the WBXML codec.
- [`crates/profile`](crates/profile) validates portable runtime endpoint profiles.
- [`crates/harness`](crates/harness) provides a scripted Exchange transport and
  black-box stdio MCP tests.
- [`xtask`](xtask) owns repeatable quality, security, profile, bundle, golden,
  performance, and soak commands.

Mail and calendar content is marked as `untrusted_external_content`. The MCP
does not execute it, load remote images, or treat message text as instructions.
Once a tool result is returned, however, the AI client may include it in model
context, so deployment still requires an appropriate data-handling policy.

## MCP tools

Read tools:

- `accounts_list`, `folders_list`, `sync_status`, `sync_now`
- `mail_list`, `mail_search`, `mail_get`
- `mail_list_attachments`, `mail_download_attachment`
- `calendar_list`, `calendar_search`, `calendar_get`

Write tools:

- `mail_mark_read`, `mail_send`, `mail_reply`, `mail_forward`

Writes are disabled per account by default. Every write requires a UUID
`idempotency_key`; a content-free SQLite journal prevents blind replay after an
ambiguous network result. Passwords, Device IDs, policy state, and the journal
HMAC key are stored in macOS Keychain.

`mail_list` synchronizes Inbox and Sent when `folder_ids` is omitted. Other mail
folders remain available through explicit `folder_ids`, while `sync_now` still
refreshes every collection in the requested scope.

## Install and configure

The beta supports macOS 14+ on Apple Silicon and Intel. Windows and Linux are
not supported in `0.2.0`.

```bash
npm install -g eas-mail-mcp@next
eas-mail-mcp setup
```

`setup` imports an existing profile or walks through creating one, adds an
account, checks the live EAS connection, offers to configure detected MCP
clients, and runs redacted diagnostics. A portable profile contains endpoint
metadata but never credentials. For example:

```toml
schema_version = 1
bundle_version = "team-1"

[[profiles]]
id = "work"
display_name = "Work Mail"
host = "mail.example.com"
email_domains = ["example.com"]
username_realm = "EXAMPLE"
device_id_length = 16

[profiles.trust]
mode = "system"
```

It can be imported explicitly before setup:

```bash
eas-mail-mcp profile import ./work-profile.toml
eas-mail-mcp setup
```

Credentials do not go into `.env` or the MCP client configuration. Configure
an additional account with the CLI; replace the sample values below with your
own:

```bash
eas-mail-mcp account add work \
  --profile your-profile-id \
  --email name@example.com \
  --username 'REALM\username'
```

The command prompts for the password without displaying it. Email, username,
and profile ID are stored in
`~/Library/Application Support/EAS Mail MCP/config.toml`; the password is stored
in macOS Keychain under the `eas-mail-mcp` service. Profiles are stored in
`~/Library/Application Support/EAS Mail MCP/profiles.toml`.

The recommended client setup creates a backup, registers the direct Rust
binary, and removes obsolete write approval overrides created by earlier beta
builds:

```bash
eas-mail-mcp client configure codex
eas-mail-mcp client configure claude
eas-mail-mcp client configure opencode
```

The configurator writes the direct Rust binary path, so Node.js is not part of
the MCP runtime. Print that path for manual configuration with:

```bash
eas-mail-mcp native-path
```

Equivalent minimal Claude Code JSON configuration:

```json
{
  "mcpServers": {
    "eas-mail": {
      "type": "stdio",
      "command": "/absolute/path/to/eas-mail-mcp",
      "args": ["serve"]
    }
  }
}
```

Equivalent minimal Codex configuration in `~/.codex/config.toml`:

```toml
[mcp_servers.eas-mail]
command = "/absolute/path/to/eas-mail-mcp"
args = ["serve"]
```

## Build the example

Requirements are macOS 14+ and the Rust toolchain pinned in
`rust-toolchain.toml`.

```bash
cargo xtask profile verify
cargo xtask npm verify
cargo xtask test
```

The public example profile is non-routable and is used only by validation and
tests. It is not embedded in a binary.

## Build npm packages

Build signed-on-machine native tarballs for both supported macOS architectures
and a root launcher package:

```bash
cargo xtask npm pack
```

The output is written to `dist/npm`. Platform packages are exact optional
dependencies of the root package and contain only one native binary. There are
no install or postinstall scripts. See [Runtime profiles](docs/runtime-profiles.md)
for the full profile schema and [Security](SECURITY.md) for the local trust
boundary.

## Engineering gates

```bash
./scripts/bootstrap-tools.sh
cargo xtask check
cargo xtask public-audit
cargo xtask goldens verify
```

The harness covers WBXML, EAS pagination and policy handling, TLS failures,
idempotent writes, MCP stdio framing, cursor expiry, and subprocess resilience.
See [CONTRIBUTING.md](CONTRIBUTING.md) and
[architecture.md](docs/architecture.md).

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your
option.
