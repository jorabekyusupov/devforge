---
type: Development Milestone
title: E02 va E05 metadata report UI komponentlari
description: Published Rust DTOlar uchun typed pure-props environment, storage va cleanup preview presentation slice.
status: draft
decision_status: confirmed
tags: [e02, e05, ui, reports, metadata, verification]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: E02 E05 va maqsadli test siyosati
  - id: environment
    resource: ../../crates/workspace/src/environment_report.rs
    title: Rust EnvironmentReport DTO
  - id: storage
    resource: ../../crates/workspace/src/storage.rs
    title: Rust Inventory DTO
  - id: preview
    resource: ../../crates/workspace/src/cleanup_preview.rs
    title: RuntimeCleanupPreview DTO
  - id: views
    resource: ../../apps/desktop/src/components/EnvironmentReportView.tsx
    title: Environment pure presentation
  - id: storage-view
    resource: ../../apps/desktop/src/components/StorageReportView.tsx
    title: Storage va preview pure presentation
  - id: tests
    resource: ../../apps/desktop/src/report-views.test.tsx
    title: Scoped UI contract regression
---

# Slice holati

E02/E05 uchun published Rust report shakllariga mos TypeScript DTOlar va pure-props
React komponentlari yaratildi. Bu bosqich yangi route, fake live API, Rust command,
backend read yoki o'chirish oqimini qo'shmaydi. Actual native read endpoint wiring
va native product flow alohida integratsiya talab qiladi; E02/E05 to'liq yakunlandi
deb olinmaydi.[^plan]

`EnvironmentReportView` report/null, loading, errorCode va optional read-only onRefresh
qabul qiladi. `StorageReportView` inventory/null, optional preview va shu read state
kontraktini oladi. Wire types Rust report, OfflineReport, resource/reference va
instantaneous lease DTOlari bilan mos; bu presentation typed props'i o'zi runtime
JSON decoder yoki endpoint authorization emas.

# Truthfulness va interaction

- Metadata comparison, exact catalog resolution, receipt mavjudligi va native health
  birlashtirilmaydi. Environment readiness flag true bo'lsa ham UI native sog'lomlik
  yoki offline ishlashni verified qilmaydi.
- Secret uchun faqat env name/reference alias va metadata registration ko'rinadi;
  registration credential mavjudligini isbotlamaydi. Cached extension current probe emas.
- Empty/null/error/loading holati successful empty reportga aylantirilmaydi. Retained
  report refresh/error vaqtida previous observation deb yoziladi; unknown schema fields render qilinmaydi.
- Storage total va resource logical/apparent bytes; disk allocation yoki freeable joy
  emas. Unsafe JS integer exact byte qiymati sifatida ko'rsatilmaydi.
- Bounded scope completeness, registry completeness, ownership, saqlanish sababi,
  observed references va coverage uncertainty alohida ko'rsatiladi.
- Cleanup preview blocked/review_candidate/unknown va lease state'ni beradi;
  review_candidate delete authorization yoki apply token emas. Apply/delete action yo'q.
- User-facing category/reason matnlari primary va RU/UZ/EN kataloglarda; unknown raw
  codes compact optional technical-details ichida. Literal IDs/keys/values tarjima qilinmaydi.
- Table caption va column/row headerlari, native details/summary, visible focus,
  semantic read refresh hamda escaped React text ishlatiladi. Long strings wrap qiladi.

# UI workflow va dalil

Existing design-system MASTER va incumbent operational screenshots/conventions
saqlandi. Installed ui-ux-pro-max focused feedback/semantic React guidance,
ui-taste polish/operate/craft, uizze-ui local implementation va yangi fetched
web-design-guidelines affected code review qo'llandi. Yangi visual system/image
concept, paid retrieval yoki native fidelity da'vosi yo'q.

Focused `src/report-views.test.tsx`:10 passed. TypeScript va uch locale semantic
validator o'tdi. Tests metadata/native distinction, secret/cache uncertainty,
nullable/loading/error/schema, stale refresh, incomplete inventory, unknown refs,
unsafe integer, candidate/blocked lease, no destructive controls, escaped values
va RU/UZ copy'ni qamradi. Whole product suite qayta yuritilmadi. Final CSS-only
alignment correction shu focused render/browser tekshiruvi bilan tekshirildi;
unchanged logic uchun green test natijalari reuse qilinadi.

Owned1423 Vite portdagi temporary synthetic fixture real komponentlarni render
qildi: EN/light1180x800, RU/dark820x620 blocked preview, UZ/light820x620 candidate.
Uch holatda scope main bo'yicha WCAG2A/AA/2.1AA axe0, horizontal overflow false.
Technical summary click va Space keyboard toggle/focus tekshirildi. Hamma caption,
raw ID, long package string va translated text screenshotlarda ko'rildi. Ko'rilgan
alignment muammosi scoped dd text-align:start bilan tuzatildi va affected screenshotlar
yangilandi. Fresh guidelines affected source review'da qolgan actionable finding yo'q;
bu butun ilova yoki native accessibility conformity da'vosi emas.

Local screenshotlar `output/playwright/reports/` ichida; fixture title synthetic
browser ekanini aniq belgilagan. Ular native backend ishini isbotlamaydi. Temporary
preview files, owned dev server va browser tekshiruvdan keyin olib tashlanadi.
Agent-desktop mavjud emasligi sabab native GUI permissions o'zgartirilmadi.

# Qolgan ish

Root native bridge va project-scoped read wiring, loading/error/retry state mapping,
actual report fixtures, macOS/Windows Tauri oqimi va product-level acceptance'ni
keyingi integratsiyada bajaradi. Shim/API response validation va authorization
presentation componentga yuklanmaydi. Native OS/credentials/Windows10, all six
locale/theme combinations, real user pilot va ona tilidagi copy review unverified.

[^plan]: [Birlashtirilgan reja](integrated-execution-plan.md).
[^environment]: [Environment DTO](../../crates/workspace/src/environment_report.rs).
[^storage]: [Storage DTO](../../crates/workspace/src/storage.rs).
[^preview]: [Cleanup preview](../../crates/workspace/src/cleanup_preview.rs).
[^views]: [Environment view](../../apps/desktop/src/components/EnvironmentReportView.tsx).
[^storage-view]: [Storage view](../../apps/desktop/src/components/StorageReportView.tsx).
[^tests]: [Scoped tests](../../apps/desktop/src/report-views.test.tsx).
