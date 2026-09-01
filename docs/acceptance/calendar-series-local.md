# Calendar series and directory: local candidate

- Date: 2026-08-27
- Branch: `calendar-series`
- Base: `a547f96af13697090cdaec5833527d2df891358d` from `origin/main`
- Version: unchanged `0.4.0`; local unpublished artifacts, not release approval
- Status: implementation and local packages complete; live acceptance pending

No npm tag, GitHub release, remote branch, installed user MCP, account config,
or credential store was changed. Unrelated changes in the original worktree
were preserved.

## Surface and regression

The candidate has 23 MCP tools and 22 operational CLI commands. It adds bounded
GAL search and extends the five existing Calendar writes with typed recurrence
and explicit scopes. One-off calls and old references remain supported. Agenda
references use the original occurrence start, including after a move.

Tests cover patterns and endings, month-end semantics, DST, all-day events,
partial exceptions, unsupported-field rejection, split ordering, separate journal
checkpoints, concurrency, replay/conflict, and every split failure boundary.
MIME tests cover REQUEST/CANCEL/REPLY, original RECURRENCE-ID, VTIMEZONE, and
attendees present only on an exception. No new mailbox database, daemon,
calendar cache, or second reference mechanism was introduced.

## Gates

Host: Apple Silicon, macOS 26.5.2, Rust 1.95.0.

| Check | Result |
| --- | --- |
| Formatting, Clippy with denied warnings, rustdoc | Passed |
| Rust file limits | Passed; warnings above 300 lines, none above 500 |
| Wire goldens and schemas | Passed |
| Workspace nextest, no retries | 305 passed, 0 skipped |
| Workspace coverage | 87.46% lines, 81.18% functions |
| EAS coverage | 91.23% lines, 85.20% functions |
| Dependency advisories, licenses, sources, bans | Passed |
| Gitleaks source, 26-commit history, private metadata | Passed |
| Operator denylist, current source and Git blobs | Passed |
| Operator denylist, Git author/committer metadata | Inherited blocker described below |
| Binaries and unpacked npm packages | Passed private-string, path, architecture, size, and file-list checks |

`cargo xtask check` passed the test, coverage, and security stages but exited
nonzero at the final public-history audit. The operator denylist matches the
author and committer email in existing commit
`142d54044e8562a20014cd086e05873570c5bea6`, inherited from `origin/main`.
There is no matching source/blob finding. No history rewrite, exception, or
weakened denylist was introduced. Resolve or explicitly accept this historical
metadata issue separately before treating the full gate as green.

## Platform packages

All packages were installed from exact local tarballs with lifecycle scripts
disabled, into isolated prefixes. Installed bytes match the staged binaries,
and `native-path` selects the expected platform package.

| Target | Validation |
| --- | --- |
| macOS arm64 | Release build, ad-hoc signature, minimum target 14.0, npm install and launcher smoke |
| macOS x64 | Release build and signature, x64 Node 24.20.0/npm under Rosetta, exact tarball install and launcher smoke |
| Windows x64 MSVC | cargo-xwin 0.23.0 build; 23 fake-EAS/CLI/MCP tests and 9 MIME unit tests passed in Wine 11.0 |
| Windows npm | Windows Node 24.20.0/npm 11.19.0 in Wine, exact tarballs, generated `.cmd`, native-path, and new command help |

MCP regression includes 24 sequential stdio sessions, cross-process references,
the 23-tool inventory, and nonstandard numeric-format cleanup. No harness or
performance server processes remained after completion.

Wine needed file-backed npm stdout/stderr because its inherited pipe handle
caused Node bootstrap to report `EBADF`. No production workaround was added;
the generated `.cmd` then completed normally. Wine is not native Windows 11
acceptance. Credential Manager, corporate TLS/VPN, physical Intel hardware, and
real mailboxes were not tested. The existing Windows credential-size limit is
unchanged.

## Performance

The existing fake-EAS harness uses 20 cold starts and 200 measured calls after
warmup. It does not contact Exchange or measure live invitation delivery.

| Metric | Observed | Gate |
| --- | --- | --- |
| Cold MCP startup p95 | 5.167 ms | <=150 ms |
| Idle Rust harness RSS | 14.547 MiB | <=20 MiB |
| Rust fake mail-list p95 | 2.283 ms | <=1.15x Python |
| Python baseline p95 | 2.836 ms | Reference |
| Rust/Python ratio | 0.805 | <=1.15 |
| Packaged binary bytes | arm64 8,503,616; x64 9,352,464; Windows 9,596,928 | <=20 MiB each |

## Frozen archives

Version `0.4.0` is retained because version changes were not authorized. These
are not the published `0.4.0` artifacts and must not be uploaded as such.

| Archive | SHA-256 |
| --- | --- |
| `eas-mail-mcp-0.4.0.tgz` | `4e3383fb0526d5a0dfd989dc9883d9ca8c82f55f1382bcddcc2fb9cc9f34ec7e` |
| `eas-mail-mcp-darwin-arm64-0.4.0.tgz` | `7bda8112f5fc3dd797b2535b0887622381d72a512ccd7c4a9f3ec7c522105f77` |
| `eas-mail-mcp-darwin-x64-0.4.0.tgz` | `b34ea64e83407553a0941d2b0c137945db47fb64ecf4ce4200f3a4a0013724bf` |
| `eas-mail-mcp-windows-x64-0.4.0.tgz` | `98e67af6002871ce60a86ba2c1b7617ee179b4d356f8d8b2a17b1ca95059cd19` |

The local handoff also includes a source snapshot without `.git`, `.private`,
build output, credentials, or deployment profiles. Its manifest records a source
tree hash as well as the base commit because the worktree is uncommitted.

## Pending live acceptance

No live EAS read or write was performed. After separate explicit write approval,
test a short personal series without attendees: create, get/agenda, change one
occurrence, split the tail, remove both resulting series, and verify cleanup.
Use the exact local candidate, not a registry install.

Invitations, updates, cancellations, and attendee responses on a real recipient
remain unverified. Their publication requires a separate decision.
