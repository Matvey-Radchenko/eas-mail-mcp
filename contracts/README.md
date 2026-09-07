# Public contract baseline

`v1.0.json` records the behavior-bearing MCP and CLI surface. It contains no
account configuration, mailbox data, credentials, runtime IDs, or package patch
version. MCP documentation text and ordering of schema sets are excluded;
property names, literal values, tuple order, constraints, defaults, output
schemas, and tool behavior annotations remain significant. CLI snapshots include
commands, flags, positional arguments, value counts, defaults, choices, aliases,
and conflicts. Runtime validation and effects are additionally covered by tests.

Run `cargo xtask contract verify` to capture the actual in-memory MCP server and
Clap command tree and compare them with the baseline. A mismatch reports the
first changed JSON pointer. `cargo xtask check` includes this gate. The capture
uses an empty runtime in temporary storage and makes no Exchange requests.

`cargo xtask contract compatibility` checks directionality separately: old valid
requests must remain accepted, while new responses must fit the old output
schema. Optional additions are accepted when the old schema permits them; new
required inputs, removed required outputs, narrowed enums, and incompatible
bounds or item types fail. Existing CLI arguments and commands are preserved.
The checker resolves local references and handles a conservative schema subset;
changed exclusive unions and unsupported constructs require manual review.
It fails closed instead of claiming a general JSON Schema containment proof.
Exact `verify` still requires an intentional baseline update for compatible drift.

After reviewing an intentional public API change, run
`cargo xtask contract accept` and review the baseline diff together with the
implementation. CI must never accept the baseline automatically. An accepted
snapshot is a record of review, not proof that a change is backward compatible.

The baseline also supplies the expected tool names for stdio tests and the live
artifact smoke harness. They compare exact names instead of maintaining separate
counts. Rebuild the harness after updating the baseline, since it is embedded at
compile time.

MCP success and runtime failures retain matching structured content and JSON text
fallbacks. Fatal runtime errors set `isError=true`; partial reads keep data and
warnings with `isError=false`. Single-write replay states `failed` and `unknown`
set `isError=true`, while `partial` preserves data plus a machine-readable warning.
Reading operation history never turns an unknown historic state into a tool error.

Public input/output fields, reference and cursor lifetimes, limits, and error
semantics are documented in the [CLI reference](../docs/cli.md),
[architecture](../docs/architecture.md), and [feature limits](../docs/compatibility.md).
The 1.x compatibility policy is in [SUPPORT.md](../SUPPORT.md). Retained completed
UUIDs prevent execution for 90 days after the last recorded update; routine
cleanup never expires unknown or partial outcomes. See
[update and recovery](../docs/getting-started.md#update-and-recover).
