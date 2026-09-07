# Mail property writes and explicit synchronization

`mail_mark_read`, `mail_set_flag`, and `mail_set_categories` send a minimal
property Change. They retain other message properties and do not download a
folder implicitly. A property entry in `mail_batch` follows the same rule.

EAS 14.1 requires a completed initial folder synchronization before applying
client changes. An initial SyncKey request followed immediately by Change is
insufficient. A targeted Sync Fetch did not establish this state on the tested
provider: ItemOperations could read the item before Sync0, but Fetch after
Sync0 returned status 8. See [MS-ASCMD Sync](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascmd/89449dc4-678c-4deb-9be2-e1dbbc43e2f5).

In an MCP session, explicitly call `mail_list` for the selected folder first and
use its Item references in that same session. The server retains only the
process-local synchronization key and snapshot already requested by that call.
Each confirmed property change advances the key and updates that snapshot. A
missing or incomplete synchronization context gives `FEATURE_UNAVAILABLE`;
an invalid key gives `SYNC_STALE`. No background Sync or automatic write retry
is performed. `OUTCOME_UNKNOWN` retains the UUID and stops further batch writes
for that account.

Each CLI command starts a new process. For standalone property writes, explicitly
pass `--sync-folder`:

```sh
eas-mail-mcp mail mark-read "$mail_ref" read --sync-folder
eas-mail-mcp mail set-flag "$mail_ref" active --sync-folder
eas-mail-mcp mail set-categories "$mail_ref" --category Project --sync-folder
eas-mail-mcp mail batch --input batch.json --sync-folder
```

The flag permits loading only the selected messages' folders into memory, within
the existing collection pagination and policy limits. It also works alongside
`--input`; it is CLI execution control, not a new MCP input property. The command
checks the same locator and point-read properties again after synchronization,
then presents the write preview. If the item disappeared or changed, it refuses
the write with `SYNC_STALE`; it never finds a substitute by subject. There is no
persistent mail cache. UUID replay skips this preparation and never reissues a
completed write.

Search LongIds without server-provided CollectionId/ServerId cannot be used for
these writes, including with `--sync-folder`. Obtain an Item reference using
`mail_list`. References remain portable locators, but their validity depends on
Exchange: EAS 14.1 does not guarantee ServerId stability after resynchronization
as EAS 16.1 does. Other active sessions can invalidate a synchronization context;
refresh explicitly after a definite stale result. Never refresh and retry an
ambiguous write with a new UUID.

Move and trash use MoveItems and need no property synchronization context.
Always use their returned destination reference. A moved item must be listed in
its destination folder before a later property Change there.
