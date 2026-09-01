# Recurring calendar and directory search

These capabilities are included in `0.5.0`. Live recurring invitations were not
validated before release and remain a documented acceptance exception.

## Find a colleague

`people_search` and `people search` perform one bounded EAS Search against GAL:

```json
{"account_id":"work","query":"Alex","limit":20}
```

Only `name` and `email` are returned. `query` must contain 1-256 printable
characters; `limit` defaults to 20 and cannot exceed 50. `results_truncated`
indicates additional matches. Multiple enabled accounts require an explicit
account, and no directory cache or address-book synchronization is created.

## Create a series

Add `recurrence` to the existing `calendar_create` input:

```json
{
  "account_id": "work",
  "subject": "Planning",
  "schedule": {
    "kind": "timed",
    "start": "2026-09-07T10:00:00+02:00",
    "end": "2026-09-07T11:00:00+02:00",
    "time_zone": "Europe/Belgrade"
  },
  "recurrence": {
    "frequency": "weekly",
    "interval": 1,
    "weekdays": ["mon", "wed"],
    "end": {"mode": "count", "count": 6}
  },
  "idempotency_key": "11111111-2222-4333-8444-555555555555"
}
```

Omit attendees for a personal event. Attendees, roles, body, reminders, and
all-day schedule inputs work as before. CLI JSON can omit the idempotency UUID.

| Frequency | Selectors |
| --- | --- |
| `daily` | `interval` days |
| `weekly` | `weekdays`, defaulting to the initial local weekday |
| `monthly` | `day_of_month`, or `week_of_month` with `weekdays` |
| `yearly` | `month` plus the same numbered-day or relative-weekday selector |

`interval` is 1-999. Relative ordinals are 1-4 or 5 for the last matching day.
Date selectors default to the initial local date; the initial start must itself
match the rule. Endings are mutually exclusive: `{"mode":"count","count":6}`,
`{"mode":"until","date":"2026-12-31"}`, or explicit `{"mode":"never"}`.
Counts include deleted occurrences. All-day end dates remain exclusive.

Exchange uses month-end when a numbered day does not exist: a monthly rule for
the 31st also occurs on February's last day. The Rust expansion and invitation
RRULEs use the same semantics, rather than silently skipping those months.
Timed series keep local wall time across DST. Ambiguous/nonexistent times and
overlapping occurrences are rejected rather than silently shifted.

## Select the mutation scope

| Scope | Meaning |
| --- | --- |
| `series` | Change the master and retain its exceptions |
| `occurrence` | Override or remove the selected original occurrence |
| `following` | Change/remove the selected occurrence and the remaining tail |

Recurring update/delete/cancel require an explicit scope. One-off calls without
scope retain their old behavior. Responses support only `series` and
`occurrence`; there is no bulk response to all following instances.

Use an `event_ref` returned by agenda for `occurrence` or `following`. It includes
the original start, even after an instance moves. Search and create return a
master reference; `calendar_get` resolves either kind in Rust. Do not decode or
edit references. A deleted instance or a changed pattern can return `SYNC_STALE`.
Recurring responses use Calendar references; the old mail-invitation reference
path remains for one-off invitations.

An occurrence patch contains only explicitly changed fields. Unchanged fields
continue to inherit the series; existing overrides retain their meaning. A
series/tail schedule change is rejected if exception identities or ordinals
cannot be preserved unambiguously. There is no force-reset option. Changing only
an occurrence's timezone or clearing only its inherited reminder is not supported
by this EAS 14.1 mapping; use the series scope instead.

## Splitting and failure recovery

An update with `following` performs these steps under the per-account lock:

1. Create the new tail with a fresh UID and unconfirmed attendee responses.
2. Limit the old UID to its prefix and retain only its earlier exceptions.
3. Send the new invitation and old-series update after both item changes.

Deleting/cancelling a tail only limits the old series. Selecting the first
occurrence is equivalent to `series`. The old prefix uses an explicit EAS count,
so a prefix larger than 65,535 occurrences is rejected before mutation.

Exchange does not provide a transaction across these steps. `completed_steps`
distinguishes `new_series`, `truncate_old_series`, and notification checkpoints.
If the second item change fails, the result can be `partial` with two series
temporarily present. An ambiguous network result is `unknown`. Do not retry
either outcome with a new UUID: inspect the calendar and reconcile the completed
steps. Replaying the same UUID reports the recorded outcome without resending.

All writes still require account `write_enabled`. CLI previews and confirms;
MCP executes a permitted explicit tool call immediately. No event content,
attendee list, or recurrence data is persisted in the operation journal.

## Acceptance boundary

Unit, wire-golden, fake-EAS, MCP, and CLI tests exercise the protocol and failure
paths. Personal-series and live-recipient checks were deferred for `0.5.0` by
the release owner; failures discovered in post-release validation require a
follow-up release rather than an automatic retry.

See the [0.5.0 acceptance record](releases/0.5.0-acceptance.md) and the
[local candidate evidence](acceptance/calendar-series-local.md) for the test
matrix and accepted limitations.

Protocol references: [GAL Search](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascmd/8211179b-14f3-44ab-9de6-b69ca2a48c4e),
[recurrence constraints](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascal/dabc38cf-7f14-4f51-8c88-717dace42de5),
[month-end differences](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-oxcical/4f8a95e3-542a-4c8b-88b3-1b00355286e7),
and [Exchange's unsupported THISANDFUTURE parameter](https://learn.microsoft.com/en-us/openspecs/exchange_standards/ms-stanxical/7ae77b54-ab32-406f-b6f8-4101a2a729c2).
