# npm release process

The npm workflow stages immutable candidates; it never makes a package public
by itself. Staged publishing requires npm 11.15 or newer, an existing package,
and maintainer 2FA. Configure each package's trusted publisher for
`npm stage publish` only.

The `eas-mail-mcp-windows-x64` package was bootstrapped for `0.4.0`. Keep its
trusted publisher, access, 2FA, and provenance policy aligned with the existing
native packages.

## Build and test locally

Prepare the isolated Python baseline using [the benchmark instructions](../benchmarks/README.md)
before the performance command. Run timing measurements after concurrent builds
and tests have finished.

From a clean release commit, run:

```bash
cargo xtask check
cargo xtask npm pack
cargo xtask live
cargo xtask perf --python benchmarks/.venv/bin/python
cargo xtask npm install-candidate
eas-mail-mcp setup
```

`install-candidate` installs the root and matching native tarballs from
`dist/npm` into the normal global npm prefix. Exercise first-run setup,
multi-account setup, client configuration, MCP reads, operational CLI reads,
portable references across CLI processes, and permitted self-writes. Restart
configured clients after changing their MCP configuration.

Live fixture drivers stop further writes and skip automatic cleanup after
`partial`, `unknown`, or `OUTCOME_UNKNOWN`. Preserve the reported operation UUID
and inspect the journal and server before any recovery action. A confirmed
failure can still trigger cleanup of a confirmed-created fixture. Record any
retained fixture instead of treating an interrupted lifecycle as a pass.

Before changing an existing installation, run the
[isolated package upgrade check](package-acceptance.md). It uses temporary npm
prefixes and synthetic local state to check clean install, 0.5.1 upgrade, and
uninstall retention. Record its exact archive hashes and any incomplete MCP
check separately from real setup and provider acceptance.

Packaging is host-aware: macOS builds the two Darwin native packages and
Windows builds `eas-mail-mcp-windows-x64`. Record each validation environment and
its evidence in the version's acceptance document. For 1.0, the agreed route is
native macOS live acceptance, native Windows CI, and a Wine/Whisky package smoke.
A physical Windows machine or Intel Mac is not an additional promotion gate;
do not describe emulation or cross-compilation as native hardware acceptance.
On Windows, the packaged PE x64 executable uses bundled SQLite and the static
MSVC CRT; it is intentionally not Authenticode-signed. See the
[compatibility matrix](compatibility.md) and [1.0 acceptance record](releases/1.0.0-acceptance.md).

Each active MCP stdio connection owns one server process. During the manual
check, verify that opening one connection creates one process and that closing
the connection removes it. Repeating at least 24 sessions must not accumulate
server processes; the harness enforces the same lifecycle automatically.

## Stage and inspect exact artifacts

Push the accepted commit to `main`, then run `Stage npm release` with `latest`
for a stable version. Separate macOS and Windows jobs build and audit the
artifacts; the staging job then submits all four tarballs. Nothing is public
yet.

Run the required eight-hour read-only soak against the exact staged native
binary, preserving its durable report:

```bash
cargo xtask soak --hours 8 \
  --application ./candidate/bin/eas-mail-mcp \
  --report ./diagnostics/soak.json
```

For 1.0.0 only, the operator later approved a one-hour maximum, superseding
the earlier four-hour allowance. The accepted release source retains the
eight-hour default and its original four-hour exception flag; it has no
one-hour xtask option. A separately derived validation harness, bound to the
accepted commit and a recorded patch/hash, supplies the strict one-hour 1.0.0
policy. It runs the exact staged application without rebuilding it or changing
any package. See the [release acceptance record](releases/1.0.0-acceptance.md#approved-100-soak-duration-exception)
for provenance and the actual outcome; do not pass a one-hour flag to the
accepted `cargo xtask soak` command.

The derived report uses `release-1.0.0-operator-approved-one-hour`. No failed or
interrupted duration is accumulated. No new cycle starts after the deadline;
an in-flight cycle that cannot finish within it fails acceptance. Hash, warning,
read, and shutdown checks remain in force. The normal eight-hour requirement
remains unchanged for later releases.

Use the actual extracted candidate path (`eas-mail-mcp.exe` on Windows).
`--application` prevents rebuilding the application. The report records the
binary SHA-256, start time, elapsed duration, and progress. Its sessions are
synthetic MCP SDK sessions; client-name strings do not establish real GUI-client
acceptance. Record actual client connection checks separately.

List and download each staged package:

```bash
npm stage list eas-mail-mcp
npm stage list eas-mail-mcp-darwin-arm64
npm stage list eas-mail-mcp-darwin-x64
npm stage list eas-mail-mcp-windows-x64
npm stage download <stage-id>
```

Install the downloaded root tarball and the native tarball matching the test
machine. This tests the exact bytes awaiting approval. On macOS:

```bash
npm install -g ./eas-mail-mcp-darwin-arm64-1.0.0.tgz ./eas-mail-mcp-1.0.0.tgz
eas-mail-mcp setup
```

On Windows PowerShell:

```powershell
npm install -g .\eas-mail-mcp-windows-x64-1.0.0.tgz .\eas-mail-mcp-1.0.0.tgz
eas-mail-mcp --version --verbose
eas-mail-mcp native-path
eas-mail-mcp doctor --check
eas-mail-mcp setup
```

The examples name 1.0.0 explicitly. For a later release, use that exact accepted
version for both packages. Record the source commit, stage IDs, package hashes,
workflow run, and available npm provenance for all four packages. npm provenance
establishes build origin; it is separate from operating-system code signing.

If the setup or runtime check fails, reject every staged package and bump the
version before creating another candidate. Do not approve a partial fix under
the same version.

## Publish and registry smoke

After acceptance, approve all three native packages first and the root package
last. Approval requires maintainer 2FA and is the action that makes each package
public.

Immediately verify root-package resolution in a clean npm prefix:

```bash
PREFIX="$(mktemp -d)"
npm install -g --prefix "$PREFIX" eas-mail-mcp@latest
"$PREFIX/bin/eas-mail-mcp" --version --verbose
"$PREFIX/bin/eas-mail-mcp" native-path
```

Repeat the clean-prefix smoke on Windows PowerShell:

```powershell
$prefix = Join-Path $env:TEMP ("eas-mail-mcp-" + [guid]::NewGuid())
npm install -g --prefix $prefix eas-mail-mcp@latest
& (Join-Path $prefix 'eas-mail-mcp.cmd') --version --verbose
& (Join-Path $prefix 'eas-mail-mcp.cmd') native-path
& (Join-Path $prefix 'eas-mail-mcp.cmd') doctor --check
```

After the registry smoke succeeds, point `next` at the same stable version.
Create the matching Git tag and source-only GitHub release only after both tags
resolve to the accepted artifacts. Provider-expansion pilots do not replace the
generic profile, security, stdio, or package gates above.
