# Isolated macOS package acceptance

Build the local tarballs with `cargo xtask npm pack`, or supply root and Darwin
ARM64 tarballs downloaded from the accepted staging run. The following check
requires Apple Silicon macOS, Python 3.11 or newer, Node.js/npm, and registry
access to the previous 0.5.1 packages:

```bash
python3 scripts/check-npm-upgrade.py \
  --packages-dir dist/npm \
  --report diagnostics/npm-upgrade.json
```

The script creates temporary npm prefixes, npm configuration/cache, and child
process home directories. It does not change the normal installation, npm
configuration, client configuration, or accounts. npm lifecycle scripts are
disabled. Temporary installations and fixtures are removed on exit.

It checks a clean installation, missing-setup diagnostics, the CLI surface, and
an actual npm update from 0.5.1. Frozen synthetic configuration, profiles, and an
old journal are used to verify byte preservation, UUID inspection, and journal
migration. Package removal must retain those local data files. The report binds
the installed executable to the native archive with SHA-256 and records both
the previous and new executable hashes.

These checks do not initialize or update OS credentials. Separate Rust upgrade
tests cover HMAC/DeviceId preservation, portable references, and no-resend
recovery using `MemorySecretStore`. Neither check establishes a supported
downgrade to 0.5.1.

## Optional production MCP check

On a machine where reading the existing application credential item is
authorized, add `--existing-keychain-read`. This first checks item existence
without exporting its value. If present, the exact packaged executable starts
with zero configured accounts and synthetic local profiles/journal, then
performs initialize, tools/list, accounts_list, operation_get, operations_list,
and stdin shutdown. No mailbox backend is created.

The option must not be used to initialize a new credential store. The script
does not create a test Keychain or change Keychain access rules. An absent or
inaccessible item leaves positive MCP acceptance incomplete; a startup error
is a failed smoke, not a successful session. Independent package-removal checks
still run, and the script returns a nonzero exit status for a requested failed
MCP check. The report contains allowlisted error codes rather than raw logs.

An `AUTH_REQUIRED` result can be specific to the temporary home/environment.
In development acceptance, the same packaged executable passed initialize,
tools/list, account metadata, one operation-list entry, and clean shutdown when
run separately with the normal user environment and existing configuration.
That comparison requires authorization to access existing local state; normal
runtime startup also performs journal recovery and cache housekeeping. Preserve
both results and do not describe the isolated failure alone as an upgrade
regression. See the [acceptance record](releases/1.0.0-acceptance.md).

This narrow session verifies the production executable's protocol and lifecycle
only when it succeeds. Fake-backend harness checks and real client/provider
acceptance are recorded separately. Re-run against the exact staged tarballs
after the release commit; local development packages are not publication
evidence.
