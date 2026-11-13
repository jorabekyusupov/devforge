# Phase 6 — Native platformalarni tekshirish

SUPERGOAL_PHASE_START
Phase: 6 of 7 — Native platformalarni tekshirish
Task: OS watcher, path identity, Tauri IPC va real serving’ni har targetda bajarib isbotlash.
Depends on phases: 1, 2, 3, 4, 5
Status: COMPLETE — source/evidence reconciled; see final acceptance audit.

## Work

M12/M8 native harness pattern’ini qayta ishlatib alohida fixtures/data dirs/ports. Matrix macOS ARM64, macOS Intel, Windows x64. Actual watcher events orqali default/custom/add/rename/delete/resume, actual WebView Settings input-save-relaunch, Prepare/Start real HTTP. Native picker selection/cancel return-focus alohida actual system-dialog evidence; mock tanlov bu gate o‘rnini bosmaydi. Native lane authorization mavjud bo‘lsa CI; push/run scope bo‘lmasa aynan tested source artifact bilan existing authorized remote route, bo‘lmasa ochiq gate.

## Deliverables

- crates/workspace/examples/support/discovery.rs (new)
- crates/workspace/examples/native_workspace.rs (existing dispatcher path verify)
- scripts/discovery_desktop_smoke.py (new)
- scripts/discovery_native_smoke.py (if needed)
- .github/workflows/discovery.yml (new)
- docs/evidence/project-discovery/ (public reports only)

## Acceptance criteria

- P6.1: macOS ARM64’da built Tauri app, watcher, saved path, startup/relaunch va discovered Laravel HTTP fixture actual ishlaydi; OS/arch/source hash hisobotda bor.
- P6.2: macOS Intel’da ayni final source hash bilan native backend va WebView checks bajariladi; ARM64 natijasi Intel o‘rniga yozilmaydi.
- P6.3: Windows x64’da ayni final source hash bilan WebView2, watcher, backslash/drive case, case-only rename, Cyrillic/spaces/non-ASCII parent va long path tests bajariladi.
- P6.4: Har targetda real folder picker selection/cancel tekshiriladi yoki alohida NOT VERIFIED gate yoziladi; native dialogga xizmat qiladigan IPC mock bilan pass belgilanmaydi.
- P6.5: No-runtime candidate, installed-runtime register, duplicate/manual import, Forget+Restore, corrupt manifest recovery, root permission/missing va switch-during-scan stsenariylari native hisobotda bor.
- P6.6: Actual watcher event/overflow fallback va tray/quit lifecycle tekshiriladi; App e2e feature production HTTP control endpointini ochmaydi.
- P6.7: Fixture HTTP selected PHP orqali 200 beradi; existing .devforge.test/.localhost-port serving modeli saqlanadi; hosts/CA native fixture faqat owned isolated muhitda.
- P6.8: Reports/screenshots tested commit + uncommitted artifact digest/source archive bilan bog‘lanadi; zip report provenance tekshiriladi va secrets/raw .env yuklanmaydi.
- P6.9: Target yoki dialog automation mavjud bo‘lmasa no green completion: bajarilgan mustaqil checks va ochiq native gates aniq saqlanadi; cross-compile runtime deb ko‘rsatilmaydi.

## Mandatory commands

- `npm run tauri -- build --debug --no-bundle --features e2e`
- `cargo run --release -p devforge-workspace --example native_workspace --features test-support -- discovery-native discovery-fixture`
- `python scripts/discovery_desktop_smoke.py discovery-fixture target/debug/devforge-desktop target/release/examples/native_workspace`
- `cargo test --workspace`
- `npm test`

Native commands execute independently on macOS ARM64, macOS Intel and Windows x64.
On Windows pass the actual `.exe` application/launcher paths. The discovery-native
subcommand and desktop smoke script are phase deliverables; they do not exist yet.
Run/dispatch only through an authorized native route; no publishing is authorized here.

## Evidence required

- Har target uchun alohida JSON assertions, source hash, native Settings/Projects screenshots, picker evidence, HTTP response, watcher/fallback timings va cleanup result; skipped/cancelled lane pass emas.
- Print each criterion as pass/fail/not-verified with source/test evidence; counts and exit codes only summarize real runs.
- Before close, reconcile affected knowledge with actual code and current evidence. Do not mark planned behavior implemented.
- Run strict knowledge validator when affected durable knowledge is updated, using prepared tooling.
- During implementation print SUPERGOAL_PHASE_VERIFY, then SUPERGOAL_PHASE_DONE only when all required criteria pass. Planning creates no such completion markers.

## Scope and failure rules

- User requested planning first. This document is a work specification, not permission to implement or publish.
- No global memory edits, unrequested dependency upgrades, unrelated project/system changes, or copying Herd proprietary code/assets.
- Future execution: investigate failure, focused retry, then focused fix; preserve original criteria. Missing target/access remains an honest open gate.

## Actual phase close

All original phase criteria PASS; source/test classifications and platform evidence are in
[PHASE_RECONCILIATION](../PHASE_RECONCILIATION.md) and [acceptance audit](../acceptance-audit.json).

SUPERGOAL_PHASE_VERIFY: 6 PASS
SUPERGOAL_PHASE_DONE: 6
