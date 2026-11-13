# Planning rationale

## Goals

One configurable project root; default OS-home/Devforge; unattended Laravel discovery;
usable Settings and honest native macOS/Windows proof. Planning only this turn.

## Constraints

Reuse current Rust/Tauri/React/SQLite contracts. Preserve package IDs, DB/secret/worktree
ownership and .devforge.test domains. Do not copy Herd source or assets. No global
system mutation, process starts, dependencies install, project .env writes from scans.
User asks plan first, no code, CI dispatch or publishing. No global memory updates.

## Risks

1. Identity merge can attach old DB/secrets to unrelated code. Stable OS identity,
   unique canonical path, source associations, conservative review and session guard.
2. Lost events/partial scan can fabricate removal or stale registration. Generation,
   one-flight coordinator, bounded invalidation and complete/partial distinction.
3. Native paths/watchers/dialog behavior differs. Actual macOS ARM64/Intel and Windows
   x64 evidence from same final source; no cross-compile-as-runtime claims.

## Dependencies

Settings model -> read-only scanner -> registry reconciliation -> watcher -> UI ->
native evidence -> final polish/audit. Phase 3 is the critical correctness boundary.
Project currently requires selected runtimes, so needs-runtime discovery candidate
must not be forced into Project with fake IDs. Existing filesystem identity is Unix
inode vs Windows path; real rename preservation needs a Windows identity addition.
Settings and workspace are separately locked; coordinator must avoid lock inversion.

## Open questions already resolved as proposals

One root only; direct children; Laravel only; explicit Save; default folder created
once; no destructive cleanup; existing domain contract; Start/Prepare explicit.
Rename stable domain remains; unsafe/ambiguous relocation becomes review. Symlink
children skipped, explicit Import still works. Network path event reliability uses
periodic shallow recovery, without claiming universal filesystem coverage.

## Memory hits

No rollout loaded. Current checkout is ahead of historical memory; no old phase
status or prior push cadence was imported. This task authorizes planning documents.

## Tools/skills

Devforge + supergoal; ui-ux-pro-max focused form/status search. Current Settings is
the visual baseline, no redesign or image generation needed at planning time.
Future UI implementation follows repository routing with ui-taste/uizze-ui and
web-design-guidelines. Existing M8/M12 native harness enables target-specific checks.

## Best practices checked

notify 8.2.2 primary docs: events may be missed on some filesystems, editor atomic
saves vary, deleted parent monitoring needs parent path; polling is fallback and
can be expensive. Plan consequently uses bounded shallow invalidation and periodic
reconciliation, not recursive dependency watches or correctness based on event order.
Exact dependency selection stays in manifests/lockfile at implementation time.

## Self-critique

Falsifiability: explicit criterion IDs and fixture/time budgets, no generic “works”.
Atomicity: configuration, scanning, reconcile, watch, UI, native verification and
final polish each has its own verifiable result; tests align with ownership.
Weakest dependency: reconcile before watch activation. Refined rename assumptions,
incomplete-copy invalidation, actual system-picker gate before presenting plan.
