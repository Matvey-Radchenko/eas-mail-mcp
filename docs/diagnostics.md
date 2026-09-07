# Diagnostics and downloaded attachments

`eas-mail-mcp doctor` checks local configuration and probes enabled accounts. Its
normal diagnostic output keeps the historical exit code 0 for per-account
failures. Use `eas-mail-mcp doctor --check` in scripts: it exits 1 if an enabled
account fails, no enabled account is ready, or profile configuration is missing.
Disabled accounts are reported without contacting Exchange.

To save a report for support, run:

```bash
eas-mail-mcp doctor --check --report ./support-report.json
```

The destination directory must already exist. The report includes application
version, operating system, architecture, aggregate health, stable error codes,
and advertised capabilities. It excludes account identifiers, usernames, email
addresses, domains, server details, paths, credentials, profile hashes, and raw
error text. On macOS the report has owner-only file permissions. Regular CLI
diagnostic output still includes local account identifiers; share the saved
report when reporting a problem.

The `accounts_status` MCP tool independently probes selected accounts. It retains
per-account results even when every account is unavailable. A successful probe
checks capabilities and folders without downloading message or calendar content.
Server capabilities and the local permission to write are separate fields.
Effective server-side write permission is reported as unknown (`null`):
diagnostics do not send a test mutation to verify it. Advertised commands can
still be restricted by mailbox permissions or administrator policy.

Downloaded attachments become eligible for cleanup after 24 hours. Cleanup is
lazy: it runs at runtime startup and before another attachment download. There
is no background timer, so expired files can remain while the application is
idle. Files can also disappear earlier after explicit cache clearing or remote
wipe processing. Copy files elsewhere if they need to be retained.

```bash
eas-mail-mcp cache status
eas-mail-mcp cache clear --account work
eas-mail-mcp cache clear --yes
```

`cache status` reports file counts, total bytes, and expired usage without pruning
files. `cache clear` asks for confirmation unless `--yes` is supplied. It removes
downloaded attachments only; it does not remove account configuration, credentials,
or the operation journal. Without `--account`, it clears downloads for every
account, including accounts removed from configuration. Cache writes, expiry,
status, and clearing share an interprocess lock. An explicit clear can remove a
download immediately after another process has returned its path.
