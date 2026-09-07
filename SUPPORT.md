# Support

Bug fixes and security fixes target the latest stable release. Older versions
can be used temporarily for recovery, but do not have a separate maintenance
branch or promised backport schedule. There is no guaranteed response time or
commercial support SLA.

## Report a problem

Check the [compatibility matrix](docs/compatibility.md), update to the latest
stable package, and reproduce with the smallest read-only request possible.
Include:

- application version, operating system, and architecture;
- the command or tool name and a minimal example with synthetic values;
- the expected and actual behavior, stable error code, and whether it repeats;
- the saved report from `eas-mail-mcp doctor --check --report ./support-report.json`.

Use the [bug report form](https://github.com/Matvey-Radchenko/eas-mail-mcp/issues/new?template=bug_report.yml)
for non-sensitive defects. Do not attach account configuration, endpoint
profiles, client backups, SQLite files, mailbox content, or raw transport logs.
The report file has a separate allowlist schema and excludes identifiers,
addresses, paths, server details, credentials, and raw error text. Review any
additional material before sharing it. See [diagnostics](docs/diagnostics.md).

For a suspected vulnerability, use the private reporting route in
[SECURITY.md](SECURITY.md), not a public issue. Report failed publication
integrity or credential exposure privately as well.

## Recover a write outcome

Keep the original operation UUID. Inspect `operation_get` or
`eas-mail-mcp operation get UUID` and read the affected Exchange state. A
`pending`, `partial`, or `unknown` outcome is not permission to repeat the write
with another UUID. Reusing the original UUID returns the stored result and
prevents a second mutation. Batch items each have their own UUID and outcome.

Do not delete the journal or regenerate the credential-store HMAC key to make
an operation retryable. Cache cleanup is independent and safe for the journal.
If recovery needs another change, review the resulting state first and create
that change deliberately.

## Compatibility policy for 1.x

Public tool names, accepted input fields, default meanings, and CLI exit codes
form the supported interface. Additive tools and output fields can appear in
minor releases; consumers should tolerate additional output fields. Removing
or renaming a supported field or requiring a formerly optional field normally
requires a major version. Validation and security fixes can reject previously
accepted invalid or unsafe input.

Exchange capabilities and effective permissions are external dependencies.
An advertised feature can return `FEATURE_UNAVAILABLE` or an access/policy
error on a particular server. Bounded searches report coverage; incomplete
coverage is not evidence that a mailbox contains no other matches.
