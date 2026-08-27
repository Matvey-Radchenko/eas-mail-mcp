# Command-line reference

The operational CLI runs one command through the same `Runtime` used by MCP and
then exits. It uses the same local profiles, operating-system credentials, EAS transport,
validation, portable references, and idempotency journal. It does not start a
daemon or create a mailbox database.

Run `eas-mail-mcp <group> <command> --help` for the complete generated flag
reference.

## Commands

| Group | Commands |
| --- | --- |
| Accounts | `account list` |
| Folders | `folder list` |
| Mail reads | `mail list`, `mail search`, `mail get`, `mail attachments`, `mail download` |
| Mail writes | `mail mark-read`, `mail send`, `mail reply`, `mail forward` |
| Calendar reads | `calendar availability`, `calendar find-slots`, `calendar search`, `calendar agenda`, `calendar get` |
| Calendar writes | `calendar create`, `calendar update`, `calendar delete`, `calendar cancel`, `calendar respond` |

`sync_status` and `sync_now` are MCP-only. Their synchronization state is held
in the MCP process and would not be useful after a one-shot CLI process exits.

## Output

JSON is the default. A successful command writes one MCP-compatible envelope to
stdout:

```json
{
  "data": {},
  "error": null,
  "warnings": []
}
```

Use `--human` for compact terminal output:

```bash
eas-mail-mcp --human account list
eas-mail-mcp --human mail list --limit 10
```

Warnings, errors, write previews, and confirmation prompts go to stderr. This
keeps stdout usable with `jq` and other pipes.

Exit codes are stable:

| Code | Meaning |
| --- | --- |
| `0` | Successful read or completed write |
| `1` | Runtime, network, protocol, or server error |
| `2` | Invalid usage, unavailable interaction, or declined write |
| `3` | Write outcome is `failed`, `partial`, or `unknown` |

## Input modes

Every operational command accepts normal flags or `--input <file|->`. The input
file is the JSON object accepted by the corresponding MCP tool. Unknown fields
are rejected, and command data flags cannot be mixed with `--input`.

```bash
eas-mail-mcp mail get --input get-message.json
printf '%s' '{"mail_ref":"ref1.mail..."}' \
  | eas-mail-mcp mail get --input -
```

For write JSON, `idempotency_key` may be omitted and the CLI generates a UUID.
`--yes` is an execution control and may be used with `--input`.

Plain-text bodies use exactly one of:

```text
--body <text>
--body-file <path>
--body-stdin
```

Calendar cancellation and response comments have equivalent `--comment`,
`--comment-file`, and `--comment-stdin` flags. If stdin carries text or JSON,
the confirmation prompt uses the controlling terminal. Without one, pass
`--yes` or the command exits with code 2.

## Mail

List recent messages or search Exchange directly:

```bash
eas-mail-mcp folder list --account work
eas-mail-mcp mail list --account work --folder '<folder-id>' --limit 50
eas-mail-mcp mail search "quarterly report" --account work --limit 20
```

`--account` and `--folder` are repeatable. For `mail list` and `mail search`,
`--limit` is the total number returned, defaults to 50, and is capped at 10,000.
`--all` consumes every page in the current bounded snapshot before the process
exits. CLI output never exposes `next_cursor`; it reports `results_truncated`
when more results were available than the requested total.

Select a result and reuse its portable reference in another process:

```bash
mail_ref="$(eas-mail-mcp mail search "invoice" \
  | jq -r '.data.items[0].mail_ref')"
eas-mail-mcp --human mail get "$mail_ref"
eas-mail-mcp mail attachments "$mail_ref"
```

Download an attachment using the `attachment_ref` returned by `mail
attachments`:

```bash
eas-mail-mcp mail download 'ref1.attachment...'
```

Write examples:

```bash
eas-mail-mcp mail mark-read "$mail_ref" read
eas-mail-mcp mail send \
  --account work \
  --to person@example.com \
  --subject "Status" \
  --body-file ./message.txt
eas-mail-mcp mail reply "$mail_ref" --body "Thanks" --reply-all
eas-mail-mcp mail forward "$mail_ref" --to person@example.com --body "FYI"
```

`--to`, `--cc`, and `--bcc` are repeatable. No shell-specific comma-list or
array syntax is required.

## Calendar

Availability and common-slot calculation accept repeatable participants and
working-hour groups:

```bash
eas-mail-mcp calendar availability \
  --participant a@example.com \
  --participant b@example.com \
  --from 2026-08-24 \
  --to 2026-08-28 \
  --time-zone Europe/Belgrade \
  --working-hours mon,tue,wed,thu,fri@09:00-18:00

eas-mail-mcp calendar find-slots \
  --participant a@example.com \
  --participant b@example.com \
  --from 2026-08-24 \
  --to 2026-08-28 \
  --time-zone Europe/Belgrade \
  --working-hours mon,tue,wed,thu,fri@09:00-18:00 \
  --duration 60
```

Use `calendar search` for text and `calendar agenda` for a compact date range:

```bash
eas-mail-mcp calendar search "planning" --account work
eas-mail-mcp --human calendar agenda \
  --account work \
  --from 2026-08-24 \
  --to 2026-08-30 \
  --time-zone Europe/Belgrade
```

Fetch one full event with the returned `event_ref`:

```bash
eas-mail-mcp calendar get 'ref1.event...'
```

Create a timed event or an all-day event:

```bash
eas-mail-mcp calendar create \
  --account work \
  --subject "Planning" \
  --start 2026-08-25T10:00:00+02:00 \
  --end 2026-08-25T11:00:00+02:00 \
  --time-zone Europe/Belgrade \
  --required person@example.com

eas-mail-mcp calendar create \
  --account work \
  --subject "Day off" \
  --all-day-start 2026-08-28 \
  --all-day-end 2026-08-29 \
  --time-zone Europe/Belgrade
```

Attendee flags `--required`, `--optional`, and `--resource` are repeatable.
`calendar update` accepts the same schedule and attendee flags, plus
`--clear-attendees` and `--clear-reminder`. Rare structures, including attendee
display names, can be supplied through MCP-shaped JSON.

```bash
eas-mail-mcp calendar update 'ref1.event...' --location "Room 4"
eas-mail-mcp calendar delete 'ref1.event...'
eas-mail-mcp calendar cancel 'ref1.event...' --comment "Cancelled"
eas-mail-mcp calendar respond 'ref1.event...' accept --comment "Accepted"
```

Recurring series and individual occurrences remain read-only.

## Portable references

Mail, event, and attachment references use the versioned form
`ref1.<kind>.<base64url-json>`. They contain only the local account ID and the
minimum Exchange locator needed to fetch the item. They do not contain a body,
subject, recipients, password, or other credential.

References are intentionally opaque: store or pass the complete string without
decoding or editing it. They are not HMAC-signed and have no local TTL because
all processes running as the same OS user are already inside the trusted local
boundary. A reference normally remains useful until Exchange removes or changes
the target; stale references return `NOT_FOUND` or `SYNC_STALE`.

MCP and CLI use the same codec, so a reference returned by either mode can be
used by the other, including after the original process exits. Page
`next_cursor` values are different: they point to immutable RAM snapshots, live
for 15 minutes, and are intentionally kept inside one MCP process. The CLI
consumes those cursors internally for `--limit` or `--all`.

## Write confirmation and retries

CLI writes first validate account write access and input, resolve references,
and build the final operation. The complete preview is written to stderr with
control and ANSI characters escaped. No journal row or mutation request is
created before confirmation.

The prompt is enabled by default. `--yes` is the explicit non-interactive mode:

```bash
eas-mail-mcp mail send ... --yes
```

After confirmation, the runtime takes the per-account lock and verifies the
target again. A changed target returns `SYNC_STALE` before mutation. The CLI
generates and returns an idempotency UUID for every write; retain it when an
operation must be retried deliberately:

```bash
eas-mail-mcp mail send ... \
  --idempotency-key 9d45957c-86c1-4c25-a823-22c56d6a19d1
```

A completed duplicate returns the stored result. Reusing the UUID with a
different payload returns `IDEMPOTENCY_CONFLICT`. A `partial` or `unknown`
outcome must be reconciled instead of retried with a new UUID.

MCP writes still execute immediately once the MCP tool is called. They share
the same validation, reference resolution, locking, stale checks, and journal,
but user review must happen in the agent workflow before the write-tool call.
