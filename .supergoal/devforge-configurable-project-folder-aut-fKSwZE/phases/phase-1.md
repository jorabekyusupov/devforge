# Phase 1 — Sozlama kontraktini kengaytirish

SUPERGOAL_PHASE_START
Phase: 1 of 7 — Sozlama kontraktini kengaytirish
Task: Loyiha papkasining yagona, saqlanadigan va eski sozlamalarga mos kontraktini yaratish.
Depends on phases: none
Status: COMPLETE — source/evidence reconciled; see final acceptance audit.

## Work

Core Settings ichida discovery sozlamasini default policy bilan ifodalash; OS home resolution platform qatlamida qoladi. JSON preferences uchun additive serde default va kerak bo‘lsa versioned migratsiya. Rust/TS Bootstrap DTO, mavjud test fixture va save oqimini moslashtirish. Faqat parent directory policy; UI va watcher keyingi bosqichlarda.

## Deliverables

- crates/core/src/lib.rs
- crates/platform/src/lib.rs
- apps/desktop/src/bridge.ts
- apps/desktop/src-tauri/src/lib.rs

## Acceptance criteria

- P1.1: Eski language/theme JSON yangi maydonlarsiz yuklanadi va ikkala qiymat saqlanadi.
- P1.2: Default policy macOS va Windows uchun foydalanuvchining home katalogiga Devforge segmentini qo‘shadi; disk ildizidagi /Devforge yaratilmaydi.
- P1.3: Custom absolute yo‘l yoki boshlang‘ich ~ qo‘llab-quvvatlanadi; relative, bo‘sh, NUL/control character va faylga ishora qiladigan yo‘l actionable error beradi.
- P1.4: Til/theme o‘zgarishi discovery yo‘lini, discovery o‘zgarishi til/theme qiymatini yo‘qotmaydi; stale full-settings yozuvlari uchun regression testi bor.
- P1.5: Default katalogni yaratish bir martalik initialization yoki explicit create bilan bajariladi; mavjud katalog va uning tarkibi o‘zgarmaydi.
- P1.6: Custom path mavjud bo‘lmasa Save uni jim yaratmaydi: Create folder amali yoki boshqa yo‘l tanlash talab etiladi.
- P1.7: Saqlash/validation xatosida oldingi saqlangan yo‘l saqlanadi; app startup settings xatosini muvaffaqiyat deb yashirmaydi.

## Mandatory commands

- `cargo test -p devforge-core -p devforge-platform`
- `npm run typecheck`
- `cargo fmt --all -- --check`

## Evidence required

- Legacy JSON/reopen, home resolver (har target), invalid path va saved-value preservation test natijalari; old/new settings payload namunasi (secrets yo‘q).
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

SUPERGOAL_PHASE_VERIFY: 1 PASS
SUPERGOAL_PHASE_DONE: 1
