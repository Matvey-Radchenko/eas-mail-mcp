# Contributing

## Source of truth

Workspace lints, `clippy.toml`, `.config/nextest.toml`, and `cargo xtask check`
are executable policy. `AGENTS.md` only points contributors and coding agents
to these rules.

Rust 1.95.0 is pinned in `rust-toolchain.toml`. Install local tools with:

```bash
./scripts/bootstrap-tools.sh
```

On Windows PowerShell, run `./scripts/bootstrap-tools.ps1` instead.

## Code boundaries

- `profile` validates portable runtime profile manifests and trust material.
- `eas` owns WBXML, EAS commands, profile-backed endpoints, and HTTPS transport.
- `app` owns account configuration, platform credential storage, idempotency,
  CLI, and MCP tools.
- `harness` owns fake I/O and black-box process drivers.
- `xtask` owns engineering gates and release assembly.
- `fuzz` is a separate nightly-only workspace.

Production crates never depend on `harness`. Name modules after domain
capabilities; do not add catch-all `service`, `utils`, or `helpers` modules.
Introduce traits only at I/O boundaries.

## Style gates

- Production code forbids unsafe code, `unwrap`, `expect`, `panic`, `todo`,
  `unimplemented`, `dbg!`, and direct print macros.
- Narrow lint exceptions use `#[expect(lint, reason = "...")]`.
- Functions are limited to 100 lines and cognitive complexity 20.
- Handwritten Rust files warn above 300 physical lines and fail above 500.
- Public production library items require rustdoc.
- Code, rustdoc, ADRs, and architecture documents are written in English.

## Test loop

```bash
cargo xtask test
cargo xtask goldens verify
cargo xtask profile verify
cargo xtask npm verify
cargo xtask check
```

Nextest runs without retries. Golden fixtures are updated only through
`cargo xtask goldens accept` after reviewing canonical XML and WBXML changes.

Before staging an npm release, also run the packaging and extended gates:

```bash
cargo xtask npm pack
cargo xtask live
cargo xtask perf --python benchmarks/.venv/bin/python
cargo xtask npm install-candidate
```

The eight-hour read-only soak is required before promoting a stable release.
The exact npm candidate must be installed and accepted before staged packages
are approved. Follow [the npm release process](docs/releasing.md).

The one-time `0.2.0` soak and pilot exceptions are documented in its public,
provider-neutral release acceptance record. They do not change future gates.

## Security rules

Never add runtime endpoint overrides, plaintext credentials, TLS bypasses,
cross-origin redirects, client spoofing, or mailbox fields to logs or SQLite.
Treat mail and calendar content as untrusted external input. A mutation with an
ambiguous result must return `OUTCOME_UNKNOWN` and must not be retried blindly.

Never commit real deployment profiles. Run `cargo xtask public-audit --denylist
.private/public-audit-denylist.txt` before publication when an operator-specific
denylist exists.
