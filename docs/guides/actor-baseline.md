# Workspace actor metadata baseline

This harness measures the real `WorkspaceActor` against disposable metadata. It
creates minimal Laravel-shaped folders and submits real `Import` and `Favorite`
commands; it does not seed the database directly or replace Workspace with a fake.
It selects host-target PHP/Node package IDs from the actual actor snapshot. Those
packages are **not installed or executed**. No Composer scripts, application code,
DB server, mail server, Inspector, GUI or WebView is started.

Run from the repository root with the configured toolchain:

```sh
scripts/with_rust.sh cargo run --release -p devforge-manager --example actor_baseline
```

The default workloads are 1, 10 and 100 projects in separate fresh data roots. An
explicit invocation can select up to five counts, each between 1 and 200:

```sh
scripts/with_rust.sh cargo run --release -p devforge-manager --example actor_baseline -- 1 10 100
```

The binary writes a single JSON report to stdout only after every requested case
passes its metadata invariants and cleanup. It accepts no project/data/output
path and never writes a report over an existing file. Capture stdout using your
own chosen destination if needed. Compiler output is on stderr; failures produce
only a coarse JSON error code on stderr with a nonzero exit. An incomplete output
is not a passing report. Bad counts are rejected before fixture allocation.

Temporary roots are owned by the process and removed after each case. Unix uses
`/tmp` to respect Darwin's socket-path limit. Windows uses the ordinary user's
TempDir without modifying its owner, ACL, security policy or process token. A
Windows host whose fresh TempDir does not satisfy production ownership rules
fails closed with `owner_acquisition_failed`; use a correctly configured ordinary
user rather than weakening manager validation. macOS execution does not establish
Windows compilation/runtime acceptance.

## What is measured

| Field | Meaning |
| --- | --- |
| `actor_start_ms` | Real actor initialization, metadata stores and initial snapshot; no client transport/bootstrap or GUI launch. |
| `registration_wall_ms` | Fixture creation, sequential import submission, operation polling, result retrieval and env-preservation checks. |
| `import_end_to_end` | Submission through terminal success, including actor scheduling, real metadata work, snapshot refresh and 1 ms status polling. |
| `favorite_end_to_end_including_refresh` | Twenty real sequential Favorite operations across registered projects; this is not an isolated snapshot-computation duration. |
| `cached_snapshot_read` | 2,000 sequential authenticated in-process reads of the cached Arc snapshot; not JSON serialization, network IPC, contention or GUI response. |
| `periodic_refresh_observation_wait_including_250ms_schedule` | Five waits for a changed observed-at generation, including actor's existing refresh schedule and 1 ms observation polling. This is **not** snapshot compute time or a performance budget. |
| `idle.process_cpu_seconds` | Native current-process CPU delta over one second of actor idle/periodic refresh; includes the harness process threads, no managed services. |
| `idle.one_core_percent` | CPU seconds / actual wall seconds × 100; one logical core basis, not divided by machine core count. |
| `idle.process_lifetime_peak_rss_bytes` | Unix `getrusage(RUSAGE_SELF)` peak RSS, normalized to bytes (macOS bytes, other Unix KiB). This is process-lifetime peak and can accumulate across cases; not current or case-exclusive RSS. Windows is null because the existing dependency features do not enable memory counters. |
| `shutdown_ms` | Actor shutdown/join. Fixture cleanup happens afterwards and must also succeed. |

Windows CPU uses existing `GetProcessTimes`; Unix CPU uses `getrusage`. Failed or
unavailable native counters remain null and are not treated as zero. The report
states `debug` versus `release`, package version, OS, macOS version when available,
architecture and available logical parallelism. Pair captured reports with exact
source revision, hardware model/RAM, OS build and invocation in the execution
evidence ledger. A build alone is not benchmark evidence.

## Isolation and measurement limits

Authentication uses an in-memory SecretStore fixture and a synthetic peer identity.
This measures the actor's existing authorization checks, not kernel peer identity,
production keychain, transport authentication or actual client compatibility.
Workspace receives a denying SecretStore and the harness requires zero Workspace
secret calls. Each imported `.env` remains unchanged. Final invariants require the
expected project count, healthy cache, no active work, installed runtimes, running
project processes, serving instances, DB instances or cache instances.

The p50/p95 fields are nearest-rank sample summaries, not confidence intervals.
The small import sample at count 1 and refresh sample of 5 are descriptive only.
No ms/RAM/CPU product budget is inferred from these numbers. This is a single local
metadata-only baseline, not sustained idle, many backups, noisy log/query/mail,
whole-process-tree release GUI, managed runtime readiness, packaged-app startup or
cross-platform acceptance. Those remain later E09 workloads. Compare subsequent
runs only with matched source/profile/host/workload; do not rerun this measurement
for unrelated documentation changes or treat a single noisy sample as regression.
