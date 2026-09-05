# Performance gate

Run from the repository root on macOS with the pinned Rust toolchain and Python
3.11 or newer. The current harness reads process RSS through Unix `ps`; it is
not a Windows performance measurement. All mail data is synthetic. The harness
does not load user profiles, access operating-system credentials, or contact
Exchange.

Create the ignored environment once; no global Python packages are required:

```bash
python3 -m venv benchmarks/.venv
benchmarks/.venv/bin/python -m pip --isolated install -r benchmarks/requirements.txt
benchmarks/.venv/bin/python -m pip check
cargo xtask perf --python benchmarks/.venv/bin/python
```

`xtask perf` builds the release application and two benchmark binaries, then
prints a JSON report. Run it after other builds and tests finish, with the
machine otherwise idle, to reduce measurement contention. Record the source
commit, OS/architecture, Python version, installed dependency versions, and the
report with the release acceptance evidence.

The measurements use 20 MCP startup samples, 20 warmup calls per server, and 200
`mail_list` samples per server, with 100 generated messages per call. Every
response must contain exactly 100 items and no MCP/application error. The Python
script is the reference implementation.

Both latency sessions connect before warmup and remain alive throughout the
comparison. Warmup and measured requests run in adjacent pairs, alternating
Rust/Python and Python/Rust order. Each pair executes sequentially, so benchmark
requests do not compete with each other. The former method measured all Rust
calls before starting Python; a short background-load change could therefore
be attributed entirely to one implementation. Pairing balances the order and
spreads both implementations across the same time interval. It reduces that
source of bias; it does not eliminate OS scheduling noise or prove the cause of
an earlier outlier.

The gate still compares the Rust p95 with the Python p95 over all 200 calls;
it does not substitute a median or a percentile of per-pair ratios. Every
startup and measured latency is retained in each completed JSON report. Pair
records include order and per-server start offsets, in milliseconds from the measured
phase start. Percentiles, ratios, and bounds use unrounded values. No sample is
trimmed or retried. An invalid response, nonfinite/nonpositive measurement, or
failed bound fails the run.

For a controlled release comparison, finish local builds/CLI checks first and
predeclare the number of runs. Preserve every result, including failures, and
check executable hashes before and after the complete series. Existing reports
from the sequential method remain historical evidence and must not be relabeled
as paired-method results.

The production application is inspected for binary size; measured startup, RSS,
and mail latency belong to the Rust synthetic server, not a live Exchange account.

| Metric | Required bound |
| --- | --- |
| Rust MCP startup p95 | At most 150 ms |
| Rust idle RSS after warmup | At most 20 MiB |
| Production native binary | At most 20 MiB |
| Rust/Python `mail_list` p95 ratio | At most 1.15 |

A failed bound makes the command fail after emitting its measurements. Preserve
the failing report and investigate the cause; do not select only a favorable
repeat or relax a threshold merely to make a candidate pass.
