# ADR 0004: Recurring calendar and directory search

- Status: accepted for `0.5.0`; live invitation validation deferred
- Date: 2026-08-27

## Decision

Add one bounded `people_search` tool and CLI `people search` over Search/GAL.
Require one selected account and return only directory name/address pairs. Do
not add a contacts sync, database, daemon, or cache.

Keep the existing five Calendar write methods and extend their input with
optional recurrence and explicit scope. Single-event calls retain their behavior.
The old public recurrence/exception map fields remain; a typed internal model
preserves presence, category/sensitivity overrides, and attendee metadata.
Unknown or lossy data makes an item read-only rather than being silently dropped.

Extend the existing stateless `ref1.event` payload with an optional original UTC
occurrence start. This is the only reference codec for MCP and CLI. A moved
instance keeps its identity; get/prepare resolve it against the current master.
No TTL, signature, or new persistent reference store is introduced.

Represent a single-instance update/deletion as an EAS Exception. Split a following
update into new-tail creation and old-prefix truncation, using a new UID for the
tail. Only then send the new REQUEST and old-series update. Preserve unambiguously
mapped exceptions, but never copy accepted participant responses into a new UID.
Use no `RANGE=THISANDFUTURE`. Following removal only truncates; its first-instance
case is equivalent to whole-series removal. Responses use MeetingResponse
InstanceId plus a matching RECURRENCE-ID reply, and never support following scope.

Reuse the prepare/commit path, account advisory lock, and journal. Add separate
step bits, not event content, to the existing journal schema. Recheck full source
revision after CLI confirmation and recheck idempotent replay after lock
acquisition. A partial split cannot be rolled back atomically and is not retried.

Enable the existing `icalendar` crate's parser feature to construct VTIMEZONE
through its public component API. The added `nom-language` dependency is recorded
in `Cargo.lock`; no GUI, runtime service, or recurrence database is introduced.

## Consequences

The candidate exposes 23 MCP tools and 22 operational CLI commands. It preserves
old object references, schemas of unrelated tools, and one-off writes. Existing
exception mapping is strict: ambiguous remapping must be rejected, not guessed.
CLI confirmation and `write_enabled` remain unchanged. The same-user trust
boundary and untrusted-content marking also apply to directory results.

Tests cover local month-end/DST semantics, wire serialization, split checkpoints,
MIME, independent processes, and concurrency. They do not prove live Exchange
invitation delivery. The release owner accepted this limitation for `0.5.0`;
post-release live validation remains required evidence for removing it.
