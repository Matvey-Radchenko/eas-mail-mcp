# Automatic replies

`mail_get_auto_reply` reads the selected account's Exchange automatic-reply
settings. `mail_set_auto_reply` sets them with the same local write permission,
account lock, durable UUID idempotency, and explicit CLI preview used by other
mutations. Reply text is not stored in the operation journal.

```bash
eas-mail-mcp mail auto-reply get --account work
eas-mail-mcp mail auto-reply set --account work --state enabled \
  --internal-message-file ./internal-reply.txt
eas-mail-mcp mail auto-reply set --account work --state scheduled \
  --starts-at 2026-10-05T09:00:00+02:00 --ends-at 2026-10-09T17:00:00+02:00 \
  --internal-message-file ./internal-reply.txt \
  --external-audience known --external-message-file ./external-reply.txt
eas-mail-mcp mail auto-reply set --account work --state disabled
```

Omitting `external_audience` means `none`, which explicitly disables external
replies when enabling or scheduling. `known` selects external contacts and `all`
selects all external senders. Known and unknown external senders share one reply
message, matching Exchange's implementation. Enabling requires a non-empty
internal message; an external audience also requires an external message.
Messages are plain text with a maximum of 10,000 characters per message.
Disabling preserves stored reply messages and does not accept message changes.

Scheduled timestamps require explicit RFC3339 offsets. They are converted to
UTC before transmission; the server controls activation without a local process
remaining open. The end must be later than the start and the current time.

All commands support JSON input with `--input FILE` or `--input -`. Mutations
ask for confirmation after showing the account, current settings, new interval,
audiences, and messages; `--yes` skips the prompt. Keep the returned operation
UUID when recovering from an interrupted call.

After every acknowledged update, the application reads the settings again.
`succeeded` means the effective settings matched the request. `partial` means
Exchange accepted the update but verification failed or found a mismatch, for
example because administrator policy suppressed external replies. `unknown`
means the update may have reached Exchange. Reusing the same idempotency key
returns the historic outcome without sending another update. Read the current
settings before deciding whether a new update is needed. CLI partial/unknown
results use exit code 3.

Settings are exchanged using EAS 14.1 [Settings/Oof](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascmd/b5a1ed99-a7ac-4d0b-aacb-40ac792d0a91).
Read-back is required because an [OofMessage update can report success while
administrator policy prevents external replies](https://learn.microsoft.com/en-us/openspecs/exchange_server_protocols/ms-ascmd/9ca3ab46-894a-4c63-9a7c-653b77ec4856).
