# Phase 4 — Kuzatuvchi hayotini ulash

SUPERGOAL_PHASE_START
Phase: 4 of 7 — Kuzatuvchi hayotini ulash
Task: Ilova ochiq paytda fayl tizimi o‘zgarishlarini yo‘qotmasdan va UI’ni bloklamasdan aniqlash.
Depends on phases: 1, 2, 3
Status: COMPLETE — source/evidence reconciled; see final acceptance audit.

## Work

notify backend yoki mos mavjud dependency bilan cancellable DiscoveryService. Native notification faqat invalidation signal; authoritative shallow scan qayta reconcile qiladi. Root parent va direct children uchun nonrecursive watches, faqat inspection marker fayllari kerak bo‘lsa maqsadli public watcher; full tree watch yo‘q. Initial/focus/manual/periodic reconciliation, bounded coalescing queue, single-flight scan va generation token. Workspace Mutex ichida filesystem walk yoki watcher callback yo‘q.

## Deliverables

- crates/workspace/src/discovery.rs
- crates/workspace/Cargo.toml
- Cargo.lock (if watcher dependency is added)
- apps/desktop/src-tauri/src/workspace.rs
- apps/desktop/src-tauri/src/lib.rs

## Acceptance criteria

- P4.1: Ilova startida UI sahifasi ochilishini kutmasdan saved root scan boshlanadi; close-to-tray holatida davom etadi, Quit’da to‘liq tugaydi.
- P4.2: Root ichida mkdir/copy/rename/delete va direct-child manifest atomic replace natijalari restart yoki Refresh bosmasdan snapshot’da chiqadi.
- P4.3: Watcher event’lari 500ms quiet debounce va 2s max-latency invalidation bilan birlashtiriladi; bitta active scan va bounded dirty flag/queue mavjud.
- P4.4: Root/child watch error yoki overflow 30s periodic shallow fallback va ko‘rinadigan degraded status beradi; app focus va manual rescan recovery qiladi.
- P4.5: 100-child local fixture event’i UI pollingni qo‘shib 5s ichida ko‘rinadi; fallback fixture 35s ichida konvergensiya qiladi; bular proposed acceptance budget, o‘lchov qayd etiladi.
- P4.6: Root switch vaqtida eski watch’lar va pending timer bekor qilinadi; late generation natija persist qilinmaydi; 50 switch stress’da handle/thread soni baseline’ga qaytadi.
- P4.7: Root delete/recreate yoki disconnect/reconnect parent watch/fallback orqali tiklanadi; permission-denied degraded holati eski rows’ni o‘chirmaydi.
- P4.8: 10,000 ta event burst bounded xotira bilan bitta final authoritative natijaga konvergensiya qiladi; user project scripts/caches bo‘yicha recursive event storm kuzatilmaydi.

## Mandatory commands

- `cargo test -p devforge-workspace -p devforge-projects`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all -- --check`

## Evidence required

- Native watch smoke timing, forced failure/fallback, burst/switch/quit lifecycle assertions; backend thread/queue bound hisobotlari; dependency versiyasi va mosligi primary source bilan qayd etiladi.
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

SUPERGOAL_PHASE_VERIFY: 4 PASS
SUPERGOAL_PHASE_DONE: 4
