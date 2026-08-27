# ADR 0002: Dependencies and generated specifications

- Status: accepted
- Date: 2026-08-14

## Context

The protocol implementation needs reviewed WBXML tables, reproducible endpoint
profiles, and an MCP implementation that follows the published protocol.

## Decision

Pin Rust 1.95.0 and the official Rust MCP SDK `rmcp` 3.0.1. Keep all EAS WBXML
code pages as declarative TOML under `spec/codepages`; `eas/build.rs` validates
and generates immutable tables only in Cargo `OUT_DIR`.

Use the shared `profile` crate from both `build.rs` and `xtask`. Profile source
and certificates remain build inputs, while only generated constants and
approved PEM bytes enter the runtime binary. `cargo-deny` controls advisories,
licenses, sources, and duplicate versions.

## Consequences

Generated protocol and profile tables cannot drift unnoticed in source control.
Toolchain, SDK, schema, and trust changes are explicit changes requiring golden,
compatibility, security, and cross-architecture checks.

## Windows client command containment

Windows setup uses the safe `process-wrap` 10 std API with only `std` and
`job-object` features. It assigns a suspended command to a Windows Job Object
before resuming it. Unlike 9.1, version 10 terminates the suspended child if job
assignment or resumption fails. The application keeps `forbid(unsafe_code)`.

Client detection and configuration commands are one-shot operations. On a
timeout, monitoring failure, or launcher exit, terminate the whole job before
returning to configuration rollback. Drain stdout and stderr concurrently so
full pipes cannot prevent a command from exiting. The scope guard also handles
output-reader startup failures. Persistent background work is not supported for
these setup commands; this policy does not apply to normal MCP sessions.

The pinned MCP SDK still requires `process-wrap` 9.1. Keep an exact duplicate
exception for that version rather than changing the SDK as part of a Windows
fix. Windows binding exceptions are likewise pinned to the incompatible
versions required by `rtoolbox` and `keyring`. `cargo-deny` continues to reject
other duplicates and check advisories, licenses, and sources for every version.
