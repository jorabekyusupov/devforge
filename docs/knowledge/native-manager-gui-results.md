---
type: Implementation Result
title: M13.1 thin desktop manager integration
description: Actual native WebView cutover and isolated owner lifecycle acceptance on macOS ARM64.
status: draft
decision_status: working-direction
tags: [m13, desktop, manager, native, lifecycle]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: Required M13.1 owner and client acceptance
  - id: adapter
    resource: ../../apps/desktop/src-tauri/src/workspace.rs
    title: Thin native adapter preserving old handlers
  - id: bridge
    resource: ../architecture/manager-native-bridge.md
    title: Native client lifetime and explicit authentication
  - id: evidence
    resource: ../evidence/m13-thin-cutover/result.json
    title: Sanitized actual native acceptance and source limits
---

# Implemented boundary

The desktop now owns NativeBridge and its separate client lease. It constructs no
Workspace/Supervisor/discovery service and has no local-owner fallback. The old20
handlers and local dialogs/window actions remain, with explicit connect and pure
reconciliation added. host_pid remains the GUI PID. Native credentials remain in
Rust. Cold startup uses the compiled fixed CLI authority; warm connection requires
an explicit user action, with no startup credential probe.

Commands await actual results, including large images and trusted partial-create
recovery. Cancellation targets the exact intent/receipt. Reconciliation stays
available while remote work is busy and after reauthentication; it never replays
an unconfirmed effect. Discovery retries preserve newer pending folder choices.
A shared RAII counter covers startup and manual connection attempts, so immediate
Quit cannot race a late detached owner into surviving an apparent successful exit.

Quit ACK is not cleanup success. The existing private inhibition marker now has a
fresh per-start shutdown identity and pending/succeeded/failed outcome. Controller
writes the result after actor cleanup while retaining the legacy lease. SDK/GUI
confirmation reads the matching owner outcome while holding that lease; stale,
missing or failed completion cannot be reported as successful cleanup.

# Required local evidence

The actual macOS ARM64 Tauri WebView used real IPC, not invoke mocks.13 checks
passed: cold bootstrap/no control-store startup probe, real managed PHP HTTP,
commands/GUI host PID, project/favorite, large debug read, configuration read,
two independent native clients, same-key retry/same receipt, GUI crash/relaunch,
explicit warm connection, single owner, Close survival and confirmed Quit.
Manager/Caddy/PHP PIDs stayed identical across GUI and both clients; all exited
after the successful final cleanup. Only receipt-matching cached packages were
copied into a disposable fixture; no downloads, OS credential store or system
security permission changes were used. PHP served a synthetic route; this is not
an actual Laravel-vendor compatibility claim.

Focused checks passed:3 Rust adapter consumers,8 frontend workspace tests,
TypeScript,3 locale catalogs, desktop e2e/library tests clippy and SDK-harness
clippy. The Windows path assertion changed to canonical path equivalence and its
single affected facade selector passed locally. Shutdown added2 result tests and
reran4 directly affected quit regressions. Other unchanged suites were not rerun.

[Result/provenance](../evidence/m13-thin-cutover/result.json) records a dirty build,
not an immutable whole-product release artifact. The final equivalent lint/test
changes and excluded deferred report catalogs are explicit. Reproduction artifact
remains output/native/m13-thin-cutover/smoke.py locally; it is not a new product API.
[Observed warm native view](../evidence/m13-thin-cutover/warm-connected.png).

# Remaining target gates

New Windows native UI, production credential dialogs, native terminal/file
choosers, minimum OS and signing remain unverified. Hosted Windows IPC/component
CI is separate from Windows10/11 desktop acceptance. M13.1 local functional flow
is validated; the target matrix remains open. External JSON CLI/MCP tools and
external grant dispatch are subsequent planned stages, not enabled by this GUI.
