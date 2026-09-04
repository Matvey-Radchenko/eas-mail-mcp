# Public audit and historical identity metadata

`cargo xtask public-audit --denylist .private/public-audit-denylist.txt` checks
the current public files, every reachable commit's contents, history patches,
and historical blobs. Author and committer identity metadata are checked
separately. Findings identify the source and commit without printing the denied
term. Git replacement objects, text conversion, and colored patch rendering do
not substitute for the stored objects during the audit.

## A narrowly approved historical exception

An operator may explicitly approve retaining the identity metadata of one
already-public historical commit. This does not permit secrets in files or
messages, remove denylist terms, or rewrite history. The approval must identify
the complete commit SHA and the SHA-256 fingerprint of its exact author and
committer header bytes.

The optional, ignored local file is
`.private/public-audit-history-metadata-allowlist.json`. Its schema is:

```json
{
  "schema_version": 1,
  "entries": [
    {
      "commit_sha": "<complete lowercase commit SHA>",
      "metadata_sha256": "<64 lowercase SHA-256 hex characters>"
    }
  ]
}
```

The fingerprint input is the raw `author ...` header, one LF byte, the raw
`committer ...` header, and one LF byte, in that order. It includes every byte
of both headers, including names, addresses, timestamps, and timezone offsets.
No normalization, mailmap, date formatting, or Unicode conversion is applied.
Headers are read from `git cat-file commit` only before its first empty line;
message text cannot masquerade as identity metadata. Other headers, including
signatures, remain subject to the normal content scan.

The file must be a Git-ignored regular file no larger than 64 KiB. Unsupported
schemas, extra fields, abbreviations, duplicate commit entries, fingerprint
mismatches, or entries outside the audited history fail closed. An exact match
suppresses only that commit's author/committer scan. The same text in any other
commit, message, patch, blob, working file, binary, or npm archive remains denied.

Prepare a reviewable proposal containing only the commit SHA and metadata
fingerprint under ignored `diagnostics/`. Do not copy it into the active local
allowlist until the operator approves the specific exception. The mechanism
does not approve a proposal by itself. Without the allowlist, the audit retains
its original rejection of matching historical identity metadata.

The exception file is local acceptance evidence and must not be committed or
packed. Artifact audits and Gitleaks do not consult it. Record the separately
approved exception in release evidence and retain the full denylist for all
subsequent checks.
