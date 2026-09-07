# Outgoing attachments

`mail_send`, `mail_reply`, and `mail_forward` accept an optional `attachments`
array. Each entry has an absolute local `path`, an optional `filename`, and an
optional `content_type` such as `application/pdf`. The filename defaults to the
file's basename and the MIME type defaults to `application/octet-stream`.

```json
{
  "account_id": "work",
  "to": ["colleague@example.invalid"],
  "subject": "Report",
  "body": "Attached is the report.",
  "attachments": [{"path": "/absolute/path/report.pdf", "content_type": "application/pdf"}],
  "idempotency_key": "00000000-0000-4000-8000-000000000001"
}
```

CLI send, reply, and forward accept repeated `--attach FILE` flags. CLI paths can
be relative to the current directory; JSON paths must be absolute. For example:

```sh
eas-mail-mcp mail send --account work --to colleague@example.invalid \
  --subject Report --body 'Attached is the report.' --attach ./report.pdf
```

One operation supports at most 20 regular files, 25 MiB of combined raw bytes,
and 35 MiB of complete MIME. Empty files are supported. Links, reparse points,
directories, and special files are rejected. Display filenames must fit 255
UTF-8 bytes and cannot contain directory separators or control characters. MIME
types cannot contain parameters or header controls.

The CLI preview includes each filename, MIME type, size, and SHA-256 digest.
Commit rereads the files and rejects a changed preview before sending. The exact
prepared byte buffers are then sent; the backend never reopens attachment paths.
SmartReply and SmartForward retain the server's normal original-message behavior.

Attachment bytes remain in operation-local memory. They are not copied into a
persistent outgoing cache or journal. The journal stores an HMAC of the input and
attachment metadata/digests, so changed content conflicts with a reused UUID.
A retry therefore needs the original files to remain available. If a file is no
longer available, inspect `operation_get` or `operation get` using the original
UUID. Never use a new UUID merely to retry an uncertain send.

Omitting attachments preserves the pre-1.0 canonical input and plain-text MIME
form, so existing operation UUIDs continue to replay correctly.
