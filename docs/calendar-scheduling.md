# Ranked scheduling

`calendar_find_slots` keeps the existing `participants`, explicit working hours,
and chronological `windows`. Windows still require every participant, including
optional participants, to be free. The additive `suggestions` list contains
concrete meeting starts on a 15-minute grid. Required participants must be free;
optional conflicts are included with participant names and reasons.

Use `participant_options` to assign a role or override local scheduling rules:

```json
{
  "participants": ["owner@example.invalid", "guest@example.invalid"],
  "date_from": "2026-10-19",
  "date_to": "2026-10-30",
  "time_zone": "Europe/Belgrade",
  "working_hours": [{"weekdays": ["mon", "tue", "wed", "thu", "fri"], "start": "09:00", "end": "17:00"}],
  "duration_minutes": 45,
  "buffer_minutes": 15,
  "participant_options": [{"input": "guest@example.invalid", "role": "optional", "time_zone": "America/New_York"}],
  "limit": 10
}
```

An omitted participant option means required. Omitted timezone and working hours
inherit the request values. Inherited wall-clock hours are interpreted in the
participant's own timezone when a timezone override is supplied. Each option
must reference a unique participant input. At least one participant is required.

Meetings must fit inside each participant's working hours. The requested buffer
must be free before and after the meeting, but may extend outside working hours.
Queries include this padding before results are clipped. Buffers range from zero
through 120 minutes in 15-minute steps; duration remains 15–480 minutes in
15-minute steps. `allow_tentative` defaults to false. When enabled, accepted
tentative participants remain explicit in each suggestion.

EAS free/busy precision remains **30 minutes**. A 15-minute candidate grid or
buffer does not create finer server knowledge. Busy, out-of-office, tentative,
unknown and outside-working-hours conflicts are reported separately. Unknown
data and unresolved identities never count as free.

One-off suggestions rank by optional participant conflicts, optional unknown
data, accepted tentative participants, then UTC start. The date range is at most
31 inclusive days; each ResolveRecipients request is at most seven days. The
default limit is 20, maximum 50, applied separately to windows and suggestions.

## Weekly recurring search

`calendar_find_recurring_slots` accepts the same flat input plus `weekday`.
It searches at most 90 inclusive days and 13 weekly occurrences. Every pattern
uses the same local start time in the request timezone, including across DST.
A pattern is excluded if any start falls in a DST gap or fold. All occurrences
must fit the requested general working hours.

Recurring patterns may contain required-participant conflicts. Ranking first
maximizes occurrences without any required conflict, then minimizes required
participant-occurrence conflicts, required unknowns, optional conflicts,
optional unknowns, and accepted tentative participants. Local start time breaks
ties. Each occurrence contains its own date and explicit participant conflicts.
The default limit is five patterns, maximum ten.

Only the selected weekday's working span and buffer are queried: at most 13
logical ResolveRecipients requests. Normal safe-read retries may add wire calls.
A failed interval remains unknown while successful dates are retained. HTTP
throttling stops further requests and marks the remaining dates unknown. If
every request fails, the tool returns the scoped error. Identity changes between
pages or invalid server streams fail the request instead of mixing recipients.

Neither scheduling tool synchronizes calendar collections or reads event bodies.
Returned JSON is capped at 256 KiB; reduce `limit` or participants if this limit is
exceeded. `results_truncated` reports additional valid candidates omitted by the
requested limit; participant summaries and occurrence conflicts describe data
that could not be verified.

CLI: `eas-mail-mcp calendar find-slots` and `calendar recurring-slots` support
`--buffer`; recurring search also requires `--weekday`. Use `--input file.json`
for per-participant options.
