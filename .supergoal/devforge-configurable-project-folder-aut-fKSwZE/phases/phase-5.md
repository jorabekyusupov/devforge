# Phase 5 — Settings oqimini yetkazish

SUPERGOAL_PHASE_START
Phase: 5 of 7 — Settings oqimini yetkazish
Task: Foydalanuvchi papkani tanlashi va aniqlash natijasini boshqarishi uchun mavjud UI’ni kengaytirish.
Depends on phases: 1, 2, 3, 4
Status: PLANNED — implementation has not started.

## Work

Mavjud Settings uslubida Loyiha papkasi: label+path draft, native picker, explicit Save, Reset to default, Create folder kerak bo‘lsa, Scan now. Status/count/last scan/retry. Projects ro‘yxatida candidate va registered statuslarini farqlash; existing Prepare/Start/Import oqimlarini qayta ishlatish. New Project wizard parent default saved root’dan, foydalanuvchi o‘zgartira oladi. Backend DTO tor typed commands; UI’dan filesystemga to‘g‘ridan to‘g‘ri ruxsat qo‘shilmaydi.

## Deliverables

- apps/desktop/src/pages/SettingsPage.tsx
- apps/desktop/src/components/ProjectDiscoverySettings.tsx (new, if extracted)
- apps/desktop/src/pages/WorkspacePage.tsx
- apps/desktop/src/components/ProjectSetup.tsx
- apps/desktop/src/bridge.ts
- apps/desktop/src/workspace.ts
- apps/desktop/src/App.tsx
- apps/desktop/src/styles.css
- apps/desktop/src/discovery.test.tsx (new)
- shared/locales/{en,ru,uz-Latn}.json

## Acceptance criteria

- P5.1: Text input va native folder picker bir xil draft’ni yangilaydi; Cancel picker current/saved qiymatni o‘zgartirmaydi; Save keyin relaunch’da yo‘l qaytadi.
- P5.2: Save explicit: har keystroke active watcher’ni o‘zgartirmaydi; invalid input inline xato, pending holatda double-submit bloklanadi; failed Save eski active root’ni saqlaydi.
- P5.3: Default qiymat ~/Devforge, Windows’da resolved native path ko‘rsatiladi; Reset default oldingi user folder contentini ko‘chirmaydi yoki o‘chirmaydi.
- P5.4: Scan now single-flight ishlaydi; scanning/empty/success/degraded/error/unavailable holatlar ko‘rinadi, error’ning retry amali bor.
- P5.5: Candidate needs-runtime/missing/conflict/ignored/relocation-pending holatlari oddiy stopped/ready bilan aralashtirilmaydi; Start faqat haqiqiy registered ready loyiha uchun ishlaydi.
- P5.6: Forget qilingan entry uchun Restore oqimi va root o‘zgargandagi detached loyihalar tushuntiriladi; existing manual Import tashqaridagi loyihalarni qo‘shadi.
- P5.7: New Project default parent saved root’dan olinadi; publish/setup staging katalogi tayyor bo‘lguncha discovery uni register qilmaydi; create/discovery race bitta project yaratadi.
- P5.8: Russian, Uzbek, English catalogs label/error/status/a11y matnlar bilan teng; dark/light/system va minimum desktop window’da long Unicode path horizontal overflow bermaydi.
- P5.9: Keyboard Tab, visible focus, picker cancel/return focus va aria-live status ishlaydi; loyiha nomlari/yo‘llari tarjima qilinmaydi.

## Mandatory commands

- `npm run typecheck`
- `npm test`
- `npm run check:locales`
- `npm run build`
- `cargo test -p devforge-workspace`

## Evidence required

- React interaction/async error tests, locale lint, incumbent va updated Settings screenshot’lari; ui-taste/uizze-ui scoped implementation, web-design-guidelines affected-source review. Native system picker interaction phase 6’da alohida tekshiriladi.
- Print each criterion as pass/fail/not-verified with source/test evidence; counts and exit codes only summarize real runs.
- Before close, reconcile affected knowledge with actual code and current evidence. Do not mark planned behavior implemented.
- Run strict knowledge validator when affected durable knowledge is updated, using prepared tooling.
- During implementation print SUPERGOAL_PHASE_VERIFY, then SUPERGOAL_PHASE_DONE only when all required criteria pass. Planning creates no such completion markers.

## Scope and failure rules

- User requested planning first. This document is a work specification, not permission to implement or publish.
- No global memory edits, unrequested dependency upgrades, unrelated project/system changes, or copying Herd proprietary code/assets.
- Future execution: investigate failure, focused retry, then focused fix; preserve original criteria. Missing target/access remains an honest open gate.
