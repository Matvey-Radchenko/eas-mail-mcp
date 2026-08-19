# Runtime profiles

Profiles describe approved Exchange ActiveSync endpoints. They are portable
local configuration, not credentials and not build inputs. Public binaries do
not contain operator endpoints.

## Location

On macOS, the active registry is stored at:

```text
~/Library/Application Support/EAS Mail MCP/profiles.toml
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
schema_version = 1
bundle_version = "operator-1"

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

`id` is a stable lowercase key stored in account config. Hosts and domains are
lowercase DNS names without scheme, port, IP address, path, wildcard, or
trailing dot. `username_realm` is optional. Device ID length is 16 or 32 ASCII
characters.

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
