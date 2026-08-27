# npm release process

The npm workflow stages immutable candidates; it never makes a package public
by itself. Staged publishing requires npm 11.15 or newer, an existing package,
and maintainer 2FA. Configure each package's trusted publisher for
`npm stage publish` only.

Before the first `0.4.0` staged release, create
`eas-mail-mcp-win32-x64` once in npm and configure its trusted publisher for
this repository and the `Stage npm release` workflow. Apply the same access,
2FA, and provenance policy as the existing native packages.

## Build and test locally

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

Packaging is host-aware: macOS builds the two Darwin native packages and
Windows 11 x64 builds `eas-mail-mcp-win32-x64`. Run the local acceptance loop on
both operating systems. On Windows, the packaged PE x64 executable uses bundled
SQLite and the static MSVC CRT; it is intentionally not Authenticode-signed in
`0.4.0`.

Each active MCP stdio connection owns one server process. During the manual
check, verify that opening one connection creates one process and that closing
the connection removes it. Repeating at least 24 sessions must not accumulate
server processes; the harness enforces the same lifecycle automatically.

## Stage and inspect exact artifacts

Push the accepted commit to `main`, then run `Stage npm release` with `latest`
for a stable version. Separate macOS and Windows jobs build and audit the
artifacts; the staging job then submits all four tarballs. Nothing is public
yet.

List and download each staged package:

```bash
npm stage list eas-mail-mcp
npm stage list eas-mail-mcp-darwin-arm64
npm stage list eas-mail-mcp-darwin-x64
npm stage list eas-mail-mcp-win32-x64
npm stage download <stage-id>
```

Install the downloaded root tarball and the native tarball matching the test
machine. This tests the exact bytes awaiting approval. On macOS:

```bash
npm install -g ./eas-mail-mcp-darwin-arm64-*.tgz ./eas-mail-mcp-0.*.tgz
eas-mail-mcp setup
```

On Windows PowerShell:

```powershell
npm install -g .\eas-mail-mcp-win32-x64-0.4.0.tgz .\eas-mail-mcp-0.4.0.tgz
eas-mail-mcp --version --verbose
eas-mail-mcp native-path
eas-mail-mcp doctor
eas-mail-mcp setup
```

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
& (Join-Path $prefix 'eas-mail-mcp.cmd') doctor
```

After the registry smoke succeeds, point `next` at the same stable version.
Create the matching Git tag and source-only GitHub release only after both tags
resolve to the accepted artifacts. Provider-expansion pilots do not replace the
generic profile, security, stdio, or package gates above.
