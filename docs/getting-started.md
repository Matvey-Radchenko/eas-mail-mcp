# Getting started

This guide covers npm installation, local endpoint profiles, multiple accounts,
and MCP client configuration on macOS and Windows.

## Requirements

- macOS 14 or later on Apple Silicon or Intel, or Windows 11 x64;
- Node.js 18 or later with npm;
- an Exchange ActiveSync 14.1 endpoint using Basic Auth over TLS;
- access to any network, VPN, system CA, or public CA certificate required by
  the Exchange operator.

The runtime supports only HTTPS and the standard
`/Microsoft-Server-ActiveSync` path. It does not support OAuth, Microsoft Graph,
IMAP, redirects, custom paths, TLS bypasses, or client identity spoofing.

## Install from npm

Install the stable package globally:

```bash
npm install -g eas-mail-mcp@latest
eas-mail-mcp --version
eas-mail-mcp native-path
```

The root npm package selects the matching native package. It has no
`install` or `postinstall` script. Node.js launches administrative commands,
but configured MCP clients execute the printed Rust binary directly, so Node.js
does not remain in the active MCP connection.

## Run the setup wizard

```bash
eas-mail-mcp setup
```

On a first run, the wizard:

1. Imports a supplied endpoint profile or creates one interactively.
2. Selects a profile and asks for email, the profile-specific login, and a
   hidden password.
3. Checks the profile and TLS connection, authentication, EAS 14.1
   capabilities, Provision policy, and FolderSync.
4. Saves the account and operating-system credentials only after verification succeeds.
5. Offers to enable mail and calendar writes. They are off by default.
6. Offers to add another account.
7. Configures detected Codex, Claude Code, and OpenCode clients with backup and
   rollback protection.
8. Runs redacted diagnostics.

If a check fails, the wizard lets the user change the profile, email, login, or
password and retry. Failed credentials are not persisted.

Run `eas-mail-mcp setup` again at any time to add or repair accounts, update a
password, change write access, manage profiles, configure clients, or run
diagnostics.

## Endpoint profiles

A profile describes one approved EAS endpoint. It is portable configuration,
not a credential file: it may contain server and domain metadata and an
optional public CA certificate, but never a password or account secret.

Example schema v2 profile:

```toml
schema_version = 2
bundle_version = "operator-1"

[[profiles]]
id = "work"
display_name = "Work Mail"
host = "mail.example.com"
email_domains = ["example.com"]
device_id_length = 16

[profiles.identity]
mode = "realm_username"
realm = "EXAMPLE"
username_hint = "Short corporate login"

[profiles.trust]
mode = "system"
```

Identity modes are:

- `email`: use the mailbox email as the Basic Auth username;
- `username`: ask for and store a standalone username;
- `realm_username`: accept a short login and canonicalize it as
  `REALM\username` without relying on shell escaping.

Import and validate a profile before running the wizard when an operator has
provided one:

```bash
eas-mail-mcp profile validate ./team-profile.toml
eas-mail-mcp profile import ./team-profile.toml
eas-mail-mcp setup
```

Use `eas-mail-mcp profile add` for an interactive profile builder. The other
profile commands are `list`, `validate`, `export`, and `remove`. See
[Runtime profiles](runtime-profiles.md) for the full schema, exclusive public CA
trust mode, validation rules, and migration behavior.

## Accounts and write access

The wizard supports multiple accounts, including multiple accounts using the
same profile. It generates a unique local account ID when necessary.

The recommended management entry point is the repeatable setup menu:

```bash
eas-mail-mcp setup
```

Focused commands are also available:

```bash
eas-mail-mcp account list
eas-mail-mcp account add
eas-mail-mcp account update-password <account-id>
eas-mail-mcp account set-writes <account-id> on
eas-mail-mcp account set-writes <account-id> off
eas-mail-mcp account remove <account-id>
```

`account add` without flags uses the same interactive flow as `setup`. Scripts
can provide all required flags and `--password-stdin`. An incomplete non-TTY
call fails with `INTERACTIVE_REQUIRED`; credentials should never be placed in
command arguments, `.env`, profile files, or MCP client configuration.

Enabling writes allows both mail and calendar mutations for that account. A
write tool executes when the AI client calls it; the server does not display a
second confirmation dialog. Keep writes off unless the client and its operating
instructions provide the desired user-control model.

## Configure MCP clients

The setup wizard lists detected clients and offers to register EAS Mail MCP in
each one automatically; no manual MCP configuration is required. It confirms
each updated client and reminds you to restart it. The same operations can be
run explicitly:

```bash
eas-mail-mcp client configure codex
eas-mail-mcp client configure claude
eas-mail-mcp client configure opencode
```

Each operation creates a backup, registers the direct native binary path, and
rolls back on failure. Restart the client afterward: editing configuration does
not terminate already running stdio sessions.

### Manual configuration

Automatic configuration is preferred. For a manual setup, first obtain the
machine-specific native path:

```bash
eas-mail-mcp native-path
```

Use that absolute path in one of the following configurations.

Codex, `~/.codex/config.toml`:

```toml
[mcp_servers.eas-mail]
command = "/absolute/path/to/eas-mail-mcp"
args = ["serve"]
```

Claude Code, `~/.claude.json`:

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

OpenCode, `~/.config/opencode/opencode.json` or `opencode.jsonc`:

```json
{
  "mcp": {
    "eas-mail": {
      "type": "local",
      "command": ["/absolute/path/to/eas-mail-mcp", "serve"],
      "enabled": true
    }
  }
}
```

Do not put email, username, password, endpoint metadata, or environment secrets
in these client entries. The native process loads non-secret account metadata
from local config and credentials from Keychain or Windows Credential Manager.

## Verify the installation

```bash
eas-mail-mcp profile list
eas-mail-mcp account list
eas-mail-mcp doctor
```

After restarting the AI client, verify the MCP with the read-only
`accounts_list` and `folders_list` tools. Do not use a mail or calendar write as
an installation test unless the account owner explicitly enabled and requested
that operation.

The same account can be checked without an AI client:

```bash
eas-mail-mcp --human account list
eas-mail-mcp --human folder list
eas-mail-mcp --human mail list --limit 5
```

See the [command-line reference](cli.md) for mail and Calendar reads, portable
references, writes with confirmation, JSON automation, and exit codes.

Every active MCP connection owns one `eas-mail-mcp serve` process. Multiple
client tasks can therefore create multiple lightweight processes. They should
exit when their stdio connections close; restart the client after removing or
changing its MCP entry.

## Local data

Non-secret configuration and cache files are stored under:

```text
macOS configuration: ~/Library/Application Support/EAS Mail MCP
macOS cache:         ~/Library/Caches/EAS Mail MCP
Windows:             %LOCALAPPDATA%\EAS Mail MCP
```

macOS Keychain or Windows Credential Manager stores passwords, Device IDs,
policy state, and the HMAC key used by the content-free idempotency journal.
Mail and calendar data are not stored in a local database. Explicitly
downloaded attachments use the application cache and expire automatically.

### Windows 0.4.0 credential capacity

All accounts share one Credential Manager entry. Windows limits its credential
blob to [2,560 bytes](https://learn.microsoft.com/en-us/windows/win32/api/wincred/ns-wincred-credentialw);
the current backend encodes the entire JSON secret bundle as UTF-16. Passwords,
Device IDs, policy state, account IDs, and the HMAC key all count toward this
limit, so there is no fixed maximum number of accounts.

An oversized update returns `STORAGE_ERROR` with a size-limit message instead
of suggesting that the store is locked. The previous credential entry remains
unchanged. Remove unused accounts through the CLI before retrying. Do not
shorten passwords or move secrets into configuration files to work around the
limit. This restriction does not apply to macOS Keychain.

The local profile and account files are trusted against accidental corruption,
not a malicious process running as the same operating-system user. See
[Security](../SECURITY.md) for the complete boundary.

## Update or uninstall

Update to the latest stable release:

```bash
npm install -g eas-mail-mcp@latest
```

The native executable path normally remains stable, but restart configured MCP
clients so their active processes use the new binary.

Remove the npm package:

```bash
npm uninstall -g eas-mail-mcp
```

Npm uninstall intentionally preserves local profiles, account configuration,
the idempotency journal, and credential-store items. Remove accounts and profiles with
the CLI before uninstalling when those settings should not remain.

## Troubleshooting

| Error or symptom | Meaning and next check |
| --- | --- |
| `AUTH_REQUIRED` | Exchange rejected the credentials. Re-enter the password and verify the email/login format selected by the profile. |
| `ACCESS_DENIED` | Credentials may be valid, but EAS access, device policy, or an allowlist prevents the operation. Contact the operator. |
| `CONFIG_INVALID` | The profile or account metadata failed strict validation. Run `profile validate` and do not weaken endpoint or TLS rules. |
| `INTERACTIVE_REQUIRED` | A command without a TTY omitted required scripted arguments. Run the interactive wizard or provide the documented non-secret flags and `--password-stdin`. |
| `STORAGE_ERROR` with a per-entry size-limit message | On Windows `0.4.0`, the combined secret bundle exceeds Credential Manager capacity. Remove unused accounts; unlocking the store or re-entering the same password will not fix the size limit. |
| TLS or network failure | Check the required network/VPN and the profile's approved CA. Do not disable certificate or hostname verification. |
| MCP is not visible | Run `client configure`, restart the client, then check `doctor` and the client's MCP diagnostics. |
| Many MCP processes | Check how many client tasks or sessions are active. Each stdio connection has one process; stale processes should disappear after the owning client exits. |

For a safe agent-assisted installation workflow in Russian, see
[Инструкция для ИИ-агента](agent-installation.ru.md).
