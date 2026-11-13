# Phase 2 — Chegaralangan skanerni yaratish

SUPERGOAL_PHASE_START
Phase: 2 of 7 — Chegaralangan skanerni yaratish
Task: Runtime tanlanmagan bo‘lsa ham Laravel loyihasini xavfsiz aniqlash.
Depends on phases: 1
Status: COMPLETE — source/evidence reconciled; see final acceptance audit.

## Work

Tauri’dan mustaqil shallow scanner va typed candidate DTO yaratiladi. Mavjud inspect()/manifest limitlarini qayta ishlatish yoki read-only tekshiruv chegarasini ajratish. Only direct child directories, maqsadli composer/package/public index metadata. Root scan natijasi complete/partial/unavailable/cancelled holatlarini farqlaydi. Chunk, cancellation, entry cap va per-entry errors.

## Deliverables

- crates/projects/src/discovery.rs (new)
- crates/projects/src/project.rs
- crates/projects/src/lib.rs

## Acceptance criteria

- P2.1: Laravel fixture artisan/public/index.php/composer.json belgilariga ko‘ra topiladi; mos kelmagan katalog successful Laravel deb ko‘rsatilmaydi.
- P2.2: Bir daraja skanerlanadi: nested app, vendor va node_modules tarkibi rekursiv o‘qilmaydi; dot-directory va DevForge staging/data kataloglari kandidat bo‘lmaydi.
- P2.3: PHP/Node paketlari o‘rnatilmagan fixture ham needs_runtime candidate sifatida chiqadi; hech qanday download boshlanmaydi.
- P2.4: Qisman ko‘chirilgan loyiha keyingi skanda to‘liq bo‘lgach topiladi; invalid yoki oversize manifest boshqa loyihalarning natijasini to‘sib qo‘ymaydi.
- P2.5: Scan script, composer/npm, artisan, Git hook va .env amallarini bajarmaydi; byte/sentinel tekshiruvi project fayllari o‘zgarmaganini isbotlaydi.
- P2.6: Symlink/junction/reparse child boshlang‘ich scope’da avtomatik follow qilinmaydi; sabab ko‘rsatiladi va explicit Import saqlanadi.
- P2.7: Root unavailable yoki entry cap holatida partial/unavailable qaytadi; bu holat barcha eski loyihalar missing degan natijaga aylantirilmaydi.
- P2.8: Tekshiruv uchun fake clock/temporary tree ishlatilib cancellation va 10,000 direct-child cap aniq tasdiqlanadi; cap natijasida truncation ko‘rsatiladi.

## Mandatory commands

- `cargo test -p devforge-projects`
- `cargo fmt --all -- --check`

## Evidence required

- Golden, no-runtime, malformed, incomplete-copy, unreadable-child, no-recursion/no-side-effects, cancellation va cap fixture natijalari.
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

SUPERGOAL_PHASE_VERIFY: 2 PASS
SUPERGOAL_PHASE_DONE: 2
