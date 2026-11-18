---
type: Design Policy
title: UI UX skill'lardan foydalanish
description: To'rtta UI skill'ning vazifasi, ishga qarab ketma-ketligi va desktop tekshiruvi.
status: draft
decision_status: confirmed
tags: [ui, ux, accessibility, desktop, skills]
sources:
  - id: conversation
    resource: /references/conversation-decisions.md
    title: UI UX skill routing va runtime katalog tablari topshirig'i
  - id: runtime-catalog
    resource: ../../apps/desktop/src/components/RuntimeCatalog.tsx
    title: Runtime katalog navigatsiyasi
  - id: runtime-tests
    resource: ../../apps/desktop/src/runtime-catalog.test.tsx
    title: Runtime katalog regression tekshiruvlari
---

# Tasdiqlangan qoida

Foydalanuvchi DevForge UI/UX ishlarida `ui-ux-pro-max`, `ui-taste`, `uizze-ui`
va `web-design-guidelines` skill'larini o'rganib, qachon qaysi biri birinchi
ishlatilishini belgilashni so'ragan. Routing loyiha skill'iga qo'shilgan.[^conversation]

| Skill | Vazifa |
| --- | --- |
| ui-ux-pro-max | UX, design-system, accessibility va tegishli stack bo'yicha qarorlar. |
| ui-taste | Mahsulotga mos vizual yo'nalish, rasmli nishon, polish va craft. |
| uizze-ui | Interfeysni amalga oshirish/tekshirish va kerakli referens yoki asset'lar. |
| web-design-guidelines | WebView UI kodi bo'yicha amaldagi qoida auditi. |

# Routing qarori

Yangi ekran uchun tartib: `ui-ux-pro-max` -> `ui-taste` -> `uizze-ui` ->
`web-design-guidelines` -> vizual va interaction tekshiruvi. Mavjud rasm,
polish, accessibility xatosi va read-only audit uchun boshlanish nuqtasi farq
qiladi; har safar to'rtta skill'ning barcha bosqichlarini bajarish talab emas.

To'liq ketma-ketlik, tashqi skill yo'llari va istisnolar uchun yagona amaliy
manba: [UI/UX workflow](../../.agents/skills/devforge/references/ui-ux-workflow.md).
Ushbu bilim yozuvi tashqi skill matnlarining nusxasi emas.

# Muhim chegaralar

- Tasdiqlangan rasm qayta yaratilmaydi. Yangi vizual ishda mavjud va ruxsatli
  image-generation vositasi bilan bitta tekshirilgan nishon yaratiladi.
- Kichik texnik tuzatish yoki read-only audit uchun yangi konsept rasmi kerak emas.
- Uizze'ning pullik MCP resurslari ixtiyoriy; ularning yo'qligi lokal ishni to'xtatmaydi.
- Desktop vazifasiga mobil navigation yoki safe-area qoidalari avtomatik yuklanmaydi.
- Rasm, kod auditi, brauzer preview'i va real Tauri tekshiruvi turli dalillar.
- Vizual qarorlar backend imkoniyati yoki ishlayotgan servis mavjudligini o'ylab topishga asos emas.

[^conversation]: Foydalanuvchi sanab bergan to'rtta skill va ulardan foydalanish tartibini DevForge skill'iga qo'shish topshirig'i.


# Runtime katalog navigatsiyasi — 2026-11-17

Foydalanuvchi PHP, Node.js, Databases, Cache va Tools kategoriyalari, bir
nechta mahsulot bo'lganda ichki tablar, versiya qidiruvi, faqat o'rnatilganlar
filtri va qayta ochilganda tanlovni eslash taklifini bajarishni tasdiqladi.[^conversation]
Bu mavjud ekran doirasidagi refinement; boshqa vizual tizim yaratilmagan.

Implementatsiya katalogdagi haqiqiy kind'lardan mahsulot tablarini hosil qiladi:
MySQL/PostgreSQL — Databases, Garnet/Redis — Cache, qolganlar — Tools.
Redis uchun guruhlash tayyorligi Redis paketini katalogga qo'shmaydi; Mailpit
ham yangi paket sifatida yaratilmagan. PHP/Node.js ichida versiyalar jadvalda
qoladi. Tanlangan kategoriya va har kategoriya mahsuloti best-effort localStorage'da
saqlanadi; buzilgan yoki eskirgan qiymat xavfsiz fallback qiladi. Qidiruv va
installed filtri joriy mahsulotga birgalikda qo'llanadi; qidiruv boshqa workspace
sahifalaridan ajratilgan. Install/remove/replacement/cancel backend kontraktlari
saqlangan. Tablar ArrowLeft/Right/Home/End va roving focus'ni qo'llaydi.[^runtime-catalog]

Kod bilan reconciliation: yuqoridagi xatti-harakatlar implementatsiyaga mos.
macOS host'da `npm run test -- --run src/runtime-catalog.test.tsx src/workspace.test.tsx`
13/13, `npm run build` va `npm run check:locales` o'tdi; build katta bundle
haqida warning berdi. Yangi testlar filtrlash, mahsulot izolyatsiyasi, klaviatura,
remount persistence, buzilgan preference va RU/UZ boshqaruv matnlarini tekshiradi.[^runtime-tests]
Native macOS Tauri va Windows tekshirilmagan: `agent-desktop` mavjud emas,
Windows target ishlatilmagan. Brauzer fixture preview'i native IPC yoki haqiqiy
paket o'rnatish dalili emas.

[^runtime-catalog]: RuntimeCatalog va WorkspacePage integratsiyasi.
[^runtime-tests]: Maqsadli UI regression suite.


Brauzer dalili: vaqtinchalik fixture backend bilan haqiqiy WorkspacePage
1280px EN/light, 900px RU/dark va 1100px UZ/light'da ko'rildi. Reload tanlovni
tikladi, Home mahsulotni almashtirdi, installed+qidiruv va empty holati o'tdi;
RU/UZ sahifada gorizontal overflow yo'q. Screenshotlar lokal
`output/playwright/runtime-tabs-*.png` ichida (fixture data, native dalil emas).
Scope bo'yicha amaldagi Web Interface Guidelines kodi ko'rib chiqildi;
label, keyboard, focus, locale va empty-state talablari bajarilgan.
Strict knowledge validator: 0 error, 1 oldindan mavjud warning — boshqa ishga
tegishli untracked `report-views-results.md` root indeksdan reachable emas.
Ushbu task bu boshqa hujjatni o'zgartirmadi; strict pass deb ko'rsatilmaydi.


2026-11-17 qo‘shimcha: runtime versiyalari endi alohida patch qatorlari emas,
bitta release-family qatorida ko‘rsatiladi. PHP CLI/FPM birlashtiriladi; oldingi
buildlarni o‘chirish disclosure ichida qoladi. [Branch update kontrakti](runtime-branch-updates.md)
oldingi jadval siyosatining shu qismini almashtiradi.
