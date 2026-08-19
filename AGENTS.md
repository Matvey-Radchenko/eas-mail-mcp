# Agent instructions

Follow `CONTRIBUTING.md`, workspace lints, and `cargo xtask check`. Architecture
and security decisions live in `docs/architecture.md`, `SECURITY.md`, and
`docs/adr/`. Never add runtime endpoint overrides, TLS bypasses, plaintext
secrets, real deployment profiles, tracked `.private/` files, or production
dependencies on `harness`.
