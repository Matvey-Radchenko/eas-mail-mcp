# Bounded mail search and conversation reads

`mail_search` retains EAS 14.1 and adds flat optional filters: `from`, `to`,
`received_after`, `received_before`, `is_read`, `has_attachments`, and
`folder_ids`. Address filters match an exact SMTP address, case-insensitively;
display names do not match. `to` checks the To header, not Cc or Bcc. Date bounds
are exclusive RFC3339 instants. Folder identifiers come from `folders_list`.
Multiple folders are encoded as repeated CollectionId children of And, as
specified by [MS-ASCMD CollectionId](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascmd/4d999297-bfec-4f13-8edc-adebf2b60f5c).

EAS Search applies text, date, and collection restrictions on the server. Address,
read-state and attachment predicates are checked against returned metadata.
Unsupported or missing metadata is never silently interpreted as false. Without
search text, both date bounds are required and the period must not exceed 31 days.

Each account is bounded to 1,000 examined candidates and ten logical Search page
requests of at most 100 candidates. Safe transport retries and policy refresh can
add wire calls. The response includes per-account `coverage` with candidate and
call counts, estimated server total when supplied, completeness, and the number
of candidates excluded because required metadata was unknown. A missing server
total cannot establish completeness. Results are ordered by received time,
newest first, within the examined candidates.
EAS Store status 12 is an end-of-retrievable-range warning: returned candidates
are retained, pagination stops, and coverage remains incomplete even if the
estimated total equals the returned count. This follows
[MS-ASCMD Search status](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascmd/4eb1c8d0-60fd-4dfb-9898-2700fe85c956).

`results_truncated` means source coverage was incomplete or candidate metadata
was insufficient. It is separate from `next_cursor`, which pages through the
already-collected snapshot without issuing new Search calls. Coverage and
partial-account warnings persist on every cursor page. No mail collection Sync
is performed by search or by a point lookup of a search result.

## Conversation read

`mail_get_thread` takes a portable `mail_ref`, obtains its opaque Exchange
ConversationId, and searches for that conversation. It returns chronological
messages, retaining opaque ConversationIndex as a tie-breaker. There is no
subject-based grouping or mailbox-wide fallback.

The default message limit is 20, maximum 100. Per-message body limit defaults to
12,000 characters, maximum 50,000. The total body budget defaults to and is capped
at 100,000 characters. `bodies_truncated` and per-message `body_truncated` report
body limits; `results_truncated` covers candidate limits and failed item reads.
Messages after the body budget is exhausted may still have their metadata
returned. Thread search uses the same 1,000-candidate/ten-page hard bound.

Every returned message must carry the seed's ConversationId. If Exchange omits
the identifier, returns an unsupported shape, or cannot verify the conversation,
the operation returns `FEATURE_UNAVAILABLE`. ConversationId is retained as opaque
bytes and the Search request uses the opaque byte-array form described by
[MS-ASCON](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascon/c56c9b3b-aeec-454c-8a4b-90eaec3baedb).
The providers checked during release acceptance accept this request syntax but
return an empty result for known conversation identifiers. Native conversation
reads are therefore unverified on these providers and return
`FEATURE_UNAVAILABLE`, rather than claiming the conversation has no messages.
There is no implicit date-window or full-mailbox fallback.

Search LongId references preserve optional CollectionId/ServerId returned by
Search or ItemOperations. This enables point-based mutation resolution when the
server provides these identifiers, without an implicit mailbox synchronization.
The providers checked during release acceptance omit these identifiers from a
LongId point fetch. Their search references remain readable, but mutations that
require collection/server identifiers return `FEATURE_UNAVAILABLE`. Use an Item
reference explicitly obtained from `mail_list` for those mutations; that tool
performs the requested synchronization. Search does not start that synchronization
implicitly.

CLI: `eas-mail-mcp mail search` exposes the filters; `mail thread REF` exposes
message and body limits. JSON input uses the same names as MCP.

Property writes additionally require completed folder synchronization in the
same MCP session, or the CLI operator's explicit `--sync-folder` flag; see
[mail property writes](mail-properties.md). A point-readable identifier alone
does not establish a valid EAS Sync change context.
