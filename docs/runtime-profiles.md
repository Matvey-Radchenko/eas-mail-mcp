# Runtime profiles

Profiles describe approved Exchange ActiveSync endpoints. They are portable
local configuration, not credentials and not build inputs. Public binaries do
not contain operator endpoints.

## Location

The active registry is stored at:

```text
macOS:   ~/Library/Application Support/EAS Mail MCP/profiles.toml
Windows: %LOCALAPPDATA%\EAS Mail MCP\profiles.toml
```

Use the CLI instead of editing the active file directly:

```bash
eas-mail-mcp profile import ./team-profile.toml
eas-mail-mcp profile add
eas-mail-mcp profile validate
eas-mail-mcp profile list
eas-mail-mcp profile export ./backup-profile.toml
eas-mail-mcp profile remove work
```

## Schema

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

`id` is a stable lowercase key stored in account config. Hosts and domains are
lowercase DNS names without scheme, port, IP address, path, wildcard, or
trailing dot. Device ID length is 16 or 32 ASCII characters.

`identity.mode` controls the Basic Auth username collected by the setup wizard:

- `email` uses the mailbox email and does not ask for another login;
- `username` stores the entered username unchanged after validation;
- `realm_username` requires `realm`, accepts either a short login or the same
  realm-prefixed login, and stores canonical `REALM\username`.

`username_hint` is optional local guidance displayed by the wizard. It is not a
credential. Schema v1 files remain accepted: `username_realm` maps to
`realm_username`, while its absence maps to `username`. The next profile
modification or export atomically rewrites the active file as schema v2.

Trust mode is either the system store or one inline public certificate:

```toml
[profiles.trust]
mode = "exclusive_pem"
pem = """-----BEGIN CERTIFICATE-----
...
-----END CERTIFICATE-----
"""
sha256 = "00:11:...:FF"
```

Exclusive mode disables system roots for that profile. The PEM must contain
exactly one certificate and no private key. `profile add --pem root.pem`
calculates the fingerprint and embeds the public certificate into the portable
profile automatically.

Profile files never contain passwords, Device IDs, policy keys, HMAC keys, or
personal account configuration. Replacing a conflicting ID requires
`profile import --replace` and confirmation. Replacement is rejected if it
would invalidate an existing account or change its Device ID length.

Profiles cannot change the protocol version, DeviceType, URL path, redirect
policy, or TLS verification. The runtime always uses EAS 14.1,
`DeviceType=EasMailMCP`, HTTPS, and `/Microsoft-Server-ActiveSync`.
