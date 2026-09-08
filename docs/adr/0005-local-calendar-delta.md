# ADR 0005: Opt-in persistent Calendar Sync

- Status: implemented in the local calendar-sync build; not an upstream release decision
- Date: 2026-09-08

## Decision

Add `calendar_sync` and CLI `calendar sync`. Only `account_id` is required.
Omitting `sync_key` resumes the private saved state or initializes it automatically.
An explicit key is an optional expected-state assertion for exactly one collection;
it must match the saved snapshot. It cannot substitute for missing event data.
`full` explicitly rebuilds selected collections and conflicts with `sync_key`.
Existing agenda/search methods retain their stateless behavior.

Persist master events and their keys together in versioned per-account JSON under
the application support directory's `calendar-sync` subdirectory. This is an
explicit, narrowly scoped exception to ADR 0001's in-memory calendar policy.
Calling this method opts into storing calendar metadata, including titles,
locations, participants, recurrence and exceptions. Do not log payloads or keys.
Use private directories/files, link checks, and atomic replacement. Credentials
remain in the credential store; no event content is added to SQLite. Account
removal and a supported remote-wipe response purge the calendar snapshot too.
Attachment-only `cache clear` does not remove this opt-in snapshot.

Use the existing cross-process account lock shared with mutations. Apply each
Sync page to a candidate, then commit its data and returned key together. Missing
fields preserve old values; explicit empty fields clear them. Delete and
SoftDelete remove the corresponding master. A failed write leaves the previous
on-disk key and data together. A failed request never implies an empty calendar.

An invalid key or a Change without a matching baseline starts one bounded rebuild
per invocation. Keep the old complete snapshot while persisting replacement pages
separately. Promote only after the last page. Resume unfinished pages on the next
call. `max_pages` defaults to 20 per collection, with a 30-second page deadline.
Corrupt, incompatible, or differently bound caches fail closed, without overwriting
them. Recovery from those storage errors requires deliberate local intervention.

Keep the policy-enforced server filter independent of the local display horizon.
The default display is today minus 90 days through today plus 90 days, inclusive;
an explicit window may span at most 366 days. Expand masters and exceptions locally
on each call, so moving the window does not reset a valid SyncKey. Server retention
policy can still limit historical coverage. Return progress and the local projected
snapshot; returning unchanged local rows is not a network re-download. Metadata-only
Sync does not guarantee notes: `body_available=false` tells clients to preserve
their existing notes. It does not authorize clearing a body.

## Consequences and validation

The local macOS bridge no longer performs weekly agenda scans or per-event detail
reads. It publishes accounts independently, retains available rows on failure, and
uses `ready`/`complete` to distinguish initial loading from a complete empty result.
The JSON response remains subject to existing MCP output-size limits; CLI clients
must also impose a response bound. Large MCP projections may require narrower dates.
Unsupported recurrence data still fails projection rather than silently losing it.

Wire fixtures cover omitted keys, restart with persisted state, sparse changes,
deletions, empty deltas, invalid-key recovery and rejection of unrelated keys.
Cache unit tests cover explicit field clearing, SoftDelete, partial replacement,
permissions and failed atomic replacement. These tests do not establish live
Exchange compatibility or delivery of invitations. No write flow is changed here.
