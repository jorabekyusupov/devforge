---
type: Implementation Report
title: M1 Rust Tauri va uch tilli desktop poydevori
description: M1 source, test va native macOS dalillari, Windows hamda qolgan qabul gate'lari.
status: draft
decision_status: confirmed
tags: [m1, rust, tauri, desktop, i18n, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M1 va qayta test qilmasdan push topshirig'i
  - id: evidence
    resource: ../evidence/m1/foundation.json
    title: Buyruq natijalari va native UI kuzatuvlari
  - id: workspace
    resource: ../../Cargo.toml
    title: Rust workspace
  - id: desktop
    resource: ../../apps/desktop/src-tauri/src/lib.rs
    title: Tauri IPC va native shell
  - id: storage
    resource: ../../crates/platform/src/lib.rs
    title: SQLite va native secret adapteri
  - id: locales
    resource: ../../shared/locales/registry.json
    title: Umumiy locale registry
  - id: ci
    resource: ../../.github/workflows/check.yml
    title: macOS va Windows tekshiruv matritsasi
---

# Natija va holat

2026-11-11 kuni foydalanuvchining M1 topshirig'i asosida Rust/Tauri desktop
poydevori yaratildi. `decision_status: confirmed` topshiriqqa tegishli, barcha
targetlar sinovdan o'tganini anglatmaydi.[^request] macOS ARM64 debug `.app`
yig'ildi va haqiqiy native oynada en/ru/uz-Latn interfeys hamda owned menyular
tekshirildi. **M1'ning platformalar bo'yicha qabul gate'i hali to'liq yopilmagan:**
Windows, native Intel va to'liq interaction matritsasi ochiq.[^evidence]

Bu desktop ilova: browser-only preview native backend yo'qligini ko'rsatadi va
tahrirlashni o'chiradi. WebView preview native IPC/persistence dalili o'rnida
ishlatilmadi. M2-M6 runtime/project boshqaruvi hali yo'q.

# Yaratilgan poydevor

| Qism | Source va amaldagi chegara |
| --- | --- |
| Domain | [core](../../crates/core/src/lib.rs): Settings, Theme, dinamik LocaleRegistry, locale resolver, stable error code va storage/secret trait'lari. Tauri dependency yo'q. |
| Platform | [platform](../../crates/platform/src/lib.rs): OS app-data, bundled SQLite, schema v1 migration, preferences saqlash, future/corrupt schema'ni reset qilmaslik, keyring native adapter. |
| CLI | [doctor](../../crates/cli/src/main.rs): shu real storage/diagnostic qatlamidan foydalanadi. Project exec hali yo'q. |
| Desktop | [shell](../../apps/desktop/src-tauri/src/lib.rs): bootstrap/save_settings IPC, native menus/tray, Settings eventi, native theme. Secret IPC yo'q. |
| Frontend | [App](../../apps/desktop/src/App.tsx) va pages: Settings, haqiqiy Diagnostics, hozirgi cheklovni tushuntiruvchi Projects. Fake servis/project ma'lumoti yo'q. |
| i18n | [registry](../../shared/locales/registry.json), uch JSON katalog, i18next/react-i18next; Rust build script ham ayni kataloglarni oladi. Til bo'yicha yopiq enum yo'q. |
| Tekshiruv | [CI](../../.github/workflows/check.yml): macOS/Windows build/check matrix tayyor. Bu push'da foydalanuvchi ko'rsatmasi bo'yicha `[skip ci]`; remote test natijasi mavjud deb ko'rsatilmaydi. |

SQLite foydalanuvchi loyihalarining DB dvigateli emas. Secret store adapteri
startup'da credential'larni tekshirmaydi; alohida native fixture ishlatilib,
o'chirilgan. Runtime supervisor, process/DNS/trust/installer adapterlari keyingi
bosqichlarga tegishli. Oynani yopish tray mavjud bo'lsa yashirishga yozilgan,
Quit chiqadi; haqiqiy close/tray cycle hali tekshirilmagan.[^desktop][^storage]

Rust toolchain task-local cache'ga, shell profile'ni o'zgartirmasdan o'rnatildi.
[Wrapper](../../scripts/with_rust.sh) oddiy Cargo yoki shu isolated toolchain'dan
foydalanadi. Aniq tanlangan versiyalar `rust-toolchain.toml`, manifest va
lockfile'larda; skill'ga doimiy versiya talabi kiritilmadi.[^workspace]

# Til, ko'rinish va UX

Boshlang'ich `system` til/ko'rinish; user tanlovi SQLite'da saqlanadi.
`uz-Latn` (`uz` alias) va ingliz fallback texnik default, userning alohida
yozuv/default tasdig'i deb ko'rsatilmaydi. Explicit script hisobga olinadi:
`uz-Cyrl` lotinga o'tkazilmaydi. Yo'qolgan locale fallback bilan ochilib,
saqlangan asl qiymatni buzmaydi. Qo'shimcha nemis fixture'i data-only
kengaytirishni domain/frontend/tooling testlarda tekshiradi.[^locales][^evidence]

[Dizayn tizimi](../../design-system/devforge/MASTER.md) va generatsiya qilingan
Settings nishoni saqlandi. `ui-ux-pro-max`ning mos fokus/UX qoidalari, `ui-taste`
va `uizze-ui` local playbook'lari qo'llandi. Landing-page qidiruv natijalari
desktop layout sifatida olinmadi. Web Interface Guidelines source auditida
semantic control, localized label, skip link, focus, reduced motion va
tarjima qilinmaydigan yo'llar ko'rib chiqildi. WCAG sertifikatsiyasi yoki
pixel-perfect moslik da'vosi yo'q. Screenshot'lar CUA'da kuzatildi; alohida
native screenshot fayllari repozitoriyda saqlanmagan.

# Bajarilgan tekshirishlar

Quyidagilar **avval bajarilgan** natijalar; userning oxirgi ko'rsatmasidan keyin
test suite'lari qayta ishga tushirilmadi. Boshlangan build/test jarayonlarining
yakuniy chiqishi olindi, qolgan native kuzatuv yakunlandi.[^request][^evidence]

| Tekshiruv | Natija |
| --- | --- |
| `cargo test --workspace` | 7 o'tdi, 1 native credential testi odatiy rejimda ignored. |
| Alohida native credential fixture | 1 o'tdi: set/get/delete va yo'qligini tekshirish; real user credential'i ishlatilmadi. |
| `npm test` | 8 test, 2 fayl o'tdi; component testlaridagi backend fixture, native dalil emas. |
| `node --test scripts/check_locales.test.mjs` | 3 o'tdi; registry, key/placeholder/plural/alias va yangi locale qamrovi. |
| `npm run typecheck` | O'tdi. |
| `cargo clippy --workspace --all-targets -- -D warnings` | O'tdi. Rustfmt source'ga qo'llangan. |
| `tauri build --debug --bundles app` | O'tdi; beforeBuild'da TypeScript/Vite production build ham o'tdi. macOS debug `.app` 32.28 MiB deb hisobot berdi; release performance natijasi emas. |

CLI `doctor` yangi vaqtinchalik `DEVFORGE_DATA_DIR` bilan birinchi smoke'da
o'tdi: macos/aarch64, app0.1.0, schema1, system/system. Bu ilgari o'tgan test
suite'ini qayta bajarish emas. OKF strict: **15 hujjat, 0 error, 0 warning**.

Native CUA kuzatuvi Vite dev-server to'xtatilgandan keyin `tauri://localhost`
embedded app'da bajarildi: en/ru/uz-Latn switch; uch tildagi application menu va
Edit/Tahrirlash/Правка; uz va ru quit/re-launch persistence; light theme
persistence; system locale'ning ruscha aniqlanishi; actual OS/arch/schema/path
diagnostikasi. Cmd+, Diagnostics'dan Settings'ga o'tib heading'ni fokusladi.
Light va System dark ko'rinishi kuzatildi, yakunda dastlabki system/system
tanlovi tiklandi. App'dagi tarjima o'zgarishi loyiha servisiga ta'sir qilmadi:
M1'da bunday servis boshqaruvi umuman yo'q.[^evidence]

# Kod va bilimni solishtirish

| Da'vo | Holat va dalil |
| --- | --- |
| Rust/Tauri faqat tavsiya edi. | O'zgargan: source/manifests/lockfile va native Mac build bor; arxitektura yozuvi yangilandi. |
| Uch tilli shell va persistent preferences bor. | Mos: kataloglar, domain/component testlar va Mac native switch/re-launch dalili. |
| Credential'lar SQLite'dan tashqarida. | Mos: alohida SecretStore adapteri va Mac native ephemeral roundtrip. |
| Windows native desktop ishlaydi. | Tekshirilmagan: source/CI bor, Windows host yo'q. |
| Intel va minimum OS support tasdiqlangan. | Tekshirilmagan: faqat Mac ARM64 27.2. M0 Rosetta runtime dalili desktop build o'rnini bosmaydi. |
| Tray/close, minimum820×620 va to'liq keyboard/accessibility qamrovi. | Qisman source mavjud, to'liq native interaction sinovi bajarilmagan. |
| Runtime/project/supervisor/installer tayyor. | Amalga oshirilmagan; M2-M6 scope. |
| Signed release yoki ommaviy installer. | Amalga oshirilmagan; debug app ommaviy release emas. |

M0 runtime gate'lari ochiq qoladi. Windows MSVC/WebView2, real native menus,
credential manager va persistence alohida Windows mashinada tekshirilishi kerak.
Minimum OS config baseline'i va native Intel ham alohida dalil talab qiladi.
OKF strict validator yangi hujjatlar yakunida bajariladi; uning natijasi product
test yoki semantic haqiqat isboti emas.

[^request]: M1 implementation va keyingi qayta test qilmasdan push topshirig'i.
[^evidence]: Agent-curated command natijalari va CUA kuzatuvlari; platforma cheklovlari alohida yozilgan.
[^workspace]: Core/platform/CLI/desktop minimal crate chegaralari.
[^desktop]: Faqat Settings IPC, native menu/event/theme va tray shell; managed runtime control yo'q.
[^storage]: Preference schema va alohida native secret adapteri.
[^locales]: Locale identifikatorlari va fallback canonical registry'dan olinadi.
[^ci]: Workflow konfiguratsiyasi remote execution dalili emas.
