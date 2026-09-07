# Online meeting and attendee updates

PR #8 permits adding participants to an existing organizer series whose master
contains server-managed online meeting links or an appointment reply time.
Calendar synchronization initializes `Supported` with the EAS 14.1 required
calendar properties and the attendee/status properties the client writes.
OnlineMeetingConfLink, OnlineMeetingExternalLink and AppointmentReplyTime are
excluded: they remain server-managed (ghosted) and are never emitted in command
requests. Synchronization keys remain process-local; fresh and recovered
calendar sessions both negotiate this set at SyncKey 0.

Roster-only additions notify added people, removals notify removed people, and
role changes notify the affected people. A participant previously invited only
to an exception receives a series invitation when added to the master. Changes
to time, place, content or recurrence notify current participants, including
when the same request also changes the roster. Reordering the roster causes no
notification. UUID replay does not send another message.

Unchanged exceptions are omitted from Calendar Change, keeping their server
values. A changed exception whose reminder is explicitly disabled cannot be
safely rewritten by supported EAS 14.1 providers. Such an edit fails validation
before any mutation; an explicit numeric reminder can replace the override.
Creating a new tail cannot preserve server-managed online metadata or ambiguous
cleared exception categories/reminders, so these cases also fail before the
first mutation. Nonempty online metadata on exceptions remains unsupported.
These limits do not prevent adding attendees to the existing whole series.

## Local acceptance

Deterministic tests cover:

- A biweekly series with 37 attendees, seven exceptions with disabled reminders,
  and all three server-managed fields: adding three people preserves metadata
  and exceptions, sends only one message to the additions, and replays safely.
- Existing attendees receiving schedule, location, subject, body and role
  changes for recurring and nonrecurring meetings.
- Calendar initialization ordering, required Supported fields, excluded
  server-managed fields, and absence of renegotiation on nonzero SyncKey.
- Scripted EAS mailbox reads and writes that reject any unexpected request:
  an existing online series is changed without forbidden metadata tags or
  unchanged exception nodes, while the local result retains those values.
- Rejecting a changed disabled reminder and unsafe new tail before any write.

The tests use synthetic accounts and mailbox data. They do not claim a new
live Exchange/provider acceptance or verify an actual external meeting link.

## Protocol references

- [MS-ASCMD Supported](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascmd/a492869c-dad0-4ea7-a2ba-2a252a386ee4)
- [MS-ASCMD Change](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascmd/3e2b243a-d052-407f-bfc0-ee0de82e1e01)
- [MS-ASCAL OnlineMeetingConfLink](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascal/aa63e887-2e0c-487f-a1a9-d4466708a31b)
- [MS-ASCAL AppointmentReplyTime](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascal/7d079aee-5edd-4c26-96ba-d2b954034dbd)
- [MS-ASCAL Reminder](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascal/d9b081e0-91e1-4ec3-a317-4ebd0ccd4489)
