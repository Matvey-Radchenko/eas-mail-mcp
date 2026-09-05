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
| People | `people search` |
| Mail reads | `mail list`, `mail search`, `mail get`, `mail get-many`, `mail thread`, `mail attachments`, `mail download`, `mail auto-reply get` |
| Mail writes | `mail mark-read`, `mail send`, `mail reply`, `mail forward`, `mail move`, `mail delete`, `mail set-flag`, `mail set-categories`, `mail batch`, `mail auto-reply set` |
| Calendar reads | `calendar availability`, `calendar find-slots`, `calendar recurring-slots`, `calendar search`, `calendar agenda`, `calendar get` |
| Calendar writes | `calendar create`, `calendar update`, `calendar delete`, `calendar cancel`, `calendar respond` |
| Recovery | `operation get`, `operation list`, `doctor --check`, `doctor --report` |
| Attachment cache | `cache status`, `cache clear` |

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

Mail, calendar, people, and folder read commands accept normal flags or
`--input <file|->`. The input
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
eas-mail-mcp mail mark-read "$mail_ref" read --sync-folder
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

Send, reply, and forward accept repeated `--attach ./file.pdf`. Files must be
explicit local paths. Limits are 20 files, 25 MiB of raw attachments, and 35 MiB
of encoded MIME. The preview lists each attachment before confirmation.

```bash
eas-mail-mcp mail thread "$mail_ref" --limit 20
eas-mail-mcp mail get-many "$first_ref" "$second_ref"
eas-mail-mcp mail move "$mail_ref" '<destination-folder-id>'
eas-mail-mcp mail delete "$mail_ref"
eas-mail-mcp mail set-flag "$mail_ref" active --sync-folder
eas-mail-mcp mail set-categories "$mail_ref" --category Project --category Review --sync-folder
eas-mail-mcp mail set-categories "$mail_ref" --clear --sync-folder
eas-mail-mcp mail batch --input batch.json --sync-folder
```

`delete` moves to Deleted Items; it does not permanently erase a message.
Moves stay within one account and return a replacement reference. Flags accept
`none`, `active`, or `complete`. Batch reads and writes accept at most 20 items
and report each item's outcome; a batch is not an atomic transaction.

A search reference can be readable while unavailable for point writes. The two
tested servers do not return mutable item locators when fetching Search LongId;
those writes return `FEATURE_UNAVAILABLE`. References from `mail list` carry the
required locator. No hidden mailbox scan resolves this limitation.

See [search and threads](mail-search-and-threads.md) for exact filters and bounded
results, and [auto-reply](auto-reply.md) for scheduled internal/external replies.

## People

```bash
eas-mail-mcp people search --account work --query "Alex" --limit 20
eas-mail-mcp --human people search --account work --query alex@example.com
```

This is a bounded GAL Search, not an address-book export. The query is required;
the default limit is 20 and the maximum is 50. Select one account explicitly
when several are enabled. Results contain only `name` and `email`, with
`results_truncated` when more matches exist.

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

Participant-specific time zones, working hours, required/optional roles, and
meeting buffers can be supplied in MCP-shaped JSON. `calendar recurring-slots`
ranks a shared weekly time across a bounded date range. Exchange free/busy still
has 30-minute resolution; see [scheduling](calendar-scheduling.md).

### Recurring events

```bash
eas-mail-mcp calendar create \
  --account work --subject "Weekly planning" \
  --start 2026-09-07T10:00:00+02:00 --end 2026-09-07T11:00:00+02:00 \
  --time-zone Europe/Belgrade \
  --repeat weekly --repeat-weekday mon --repeat-count 6

eas-mail-mcp calendar update "$occurrence_ref" --scope occurrence --location "Room 4"
eas-mail-mcp calendar update "$occurrence_ref" --scope following --subject "New series"
eas-mail-mcp calendar delete "$personal_series_ref" --scope series
eas-mail-mcp calendar cancel "$meeting_ref" --scope series
eas-mail-mcp calendar respond "$occurrence_ref" accept --scope occurrence
```

`--repeat` accepts `daily`, `weekly`, `monthly`, or `yearly`. Other selectors are
`--repeat-interval`, repeatable `--repeat-weekday`, `--repeat-day`,
`--repeat-week` (1-4 or 5 for last), and `--repeat-month`. Choose exactly one ending:
`--repeat-count`, `--repeat-until YYYY-MM-DD`, or `--repeat-forever`.
The same selectors work with all-day schedule flags and `calendar update`.

Recurring writes require `--scope series|occurrence|following`; responses only
accept `series|occurrence`. Obtain an occurrence reference from `calendar agenda`
before targeting one occurrence or the remaining tail. Text search returns the
master reference. See [recurrence semantics and JSON examples](calendar-series.md).

## Portable references

Mail, event, and attachment references use the versioned form
`ref1.<kind>.<base64url-json>`. They contain only the local account ID and the
minimum Exchange locator needed to fetch the item. They do not contain a body,
subject, recipients, password, or other credential.

An agenda occurrence also carries its original UTC start. Moving that occurrence
does not change this identity. Removing it, truncating its old series, or changing
the pattern can make an old reference stale. Old references without an occurrence
identifier remain valid master references.

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

## Diagnostics and recovery

```bash
eas-mail-mcp doctor --check
eas-mail-mcp doctor --report ./diagnostic-report.json
eas-mail-mcp operation get 9d45957c-86c1-4c25-a823-22c56d6a19d1
eas-mail-mcp operation list --account work --status unknown --limit 20
eas-mail-mcp cache status
eas-mail-mcp cache clear --account work --yes
```

`doctor --check` exits with code 1 when an enabled account is unhealthy. Plain
`doctor` preserves its successful diagnostic exit for per-account failures.
`--report` writes a separate allowlisted report without account identifiers,
email addresses, server names, local paths, or credentials. Normal doctor output
is local diagnostic information; share the report file instead.
For fresh per-account status through MCP, use `accounts_status`.

`operation get/list` read the local journal without loading profiles or
credentials and without contacting Exchange. They do not resend operations.
List accepts `pending`, `succeeded`, `failed`, `partial`, or `unknown`, with a
maximum limit of 100. These recovery commands and cache commands use flags,
not MCP-shaped `--input`.

`pending` means that no final outcome was saved; the owner may still be active
or may have stopped before saving its result. Recovery changes an abandoned
pending record to `unknown` only after obtaining its account's process lock.
Never treat `pending`, `unknown`, or a zero step mask as permission to resend.

`completed_steps` is a bit mask, not a count. For single mail operations and
automatic replies, bit `1` records Exchange's acknowledgement. Historical
mail operations written by 0.5.1 can have a zero mask even after success; their
saved status remains authoritative and migration does not rewrite them.
For calendar operations, combine the following values to interpret the mask:

| Value | Confirmed calendar step |
| --- | --- |
| 1 | Calendar item changed |
| 2 | Current attendees notified |
| 4 | Removed attendees notified |
| 8 | Meeting response applied |
| 16 | Reply notification sent |
| 32 | New series created |
| 64 | Original series truncated |
| 128 | Original series attendees notified |

An automatic-reply acknowledgement does not prove read-back matched the request:
inspect its final `status`, which remains `partial` if verification failed.

Cache removal affects downloaded files only. Cleanup after 24 hours is lazy,
not a background timer. See [diagnostics and cache](diagnostics.md) and the
[update, recovery, and uninstall guide](getting-started.md).

MCP writes still execute immediately once the MCP tool is called. They share
the same validation, reference resolution, locking, stale checks, and journal,
but user review must happen in the agent workflow before the write-tool call.

Property changes need completed folder synchronization. The CLI `--sync-folder`
flag explicitly permits loading the selected folders before preview; without it,
a fresh process returns `FEATURE_UNAVAILABLE`. Replay skips synchronization.
See [mail property writes](mail-properties.md) for limits and stale-reference handling.
