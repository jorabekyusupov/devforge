# DevForge bilim bazasi tarixi

## 2026-11-12

- **M5 yakuni**: [Kundalik workspace](m5-daily-workspace-results.md) real project/runtime/database/queue/Vite/log controls bilan yaratildi. Final e501b29 source macOS WKWebView va Windows WebView2 native workflow,9 tadan axe holati,53 Rust/13 frontend/3 locale per-OS checks va desktop build bilan o'tdi. Project A lifecycle B master PID/shared PG'ni saqladi; tray/Quit, forced-crash cleanup/hot recovery va reopen tasdiqlandi. Hosted Mac viewport cap aniq qayd etildi; native credential/optional external apps/minimum OS/signing M6 gate'lari alohida.

## 2026-11-11

- **M5 implementation**: [Kundalik boshqaruv](m5-daily-workspace-results.md), shared coordinator, scoped lifecycle va real UI controls yaratildi. Native Mac backend/UI sinovlari yuritilmoqda; hosted Windows/final acceptance pending.

- **M4 yakuni**: [Serving natijalari](m4-serving-results.md), owned domains, Caddy/PHP-FPM/CGI/Vite, scoped CA va fixed-target hosts/trust helper yaratildi. Source6f3033e native Mac Chrome va Windows Edge'da Laravel13.42/DB/TLS/secure-cookie/upload/WSS HMR/restart/fallback hamda cleanup bilan o'tdi. Full CI har OS'da51 Rust,8 frontend,3 locale va desktop build bilan o'tdi. Mac nontrusting metadata retention, native credential UI va M5/M6 gate'lari alohida saqlandi.

- **M3 yakuniy native dalil**: Full CI64e9911 macOS/Windows'da39 Rust testi, frontend/locale va native binary build bilan o'tdi. Actual Mac8/Windows6 paket,2 project, Laravel13.42 migration/worker, PHP8.4->8.5, PG/PDO/isolation/restart va env backup sinovlari o'tdi. Windows CMD quoting hamda credential roundtrip testi o'tdi; token/path/fixture regressiyalari va ochiq GUI/minimum OS gate'lari [hisobotda](m3-project-environments-results.md) saqlandi.

- **M3 loyiha muhitlari**: [Natijalar](m3-project-environments-results.md) va `crates/projects` yaratildi: read-only import, PHP/npm constraint'lari, persistent binding/context, secret references, explicit env patch/backup va owned PostgreSQL DB/role. Mac'da39 Rust testi va2 loyiha native smoke o'tdi; yakuniy hosted qamrov alohida dalil bilan qo'shildi.
- **M2 runtime yadrosi**: [natijalar](m2-runtime-core-results.md), pinned catalog, staged package store, cancellation/limits, executable leases, process-group/Job adapterlari, readiness va bounded restart/loglar, CLI install/probe/remove yaratildi. macOS fixture/regression va 8 real paket smoke dalillari saqlandi. Windows CI hamda to'liq lifecycle/release qamrovi hisobotda alohida ko'rsatiladi.
- **M2 hosted CI**: macOS26 ARM64 va Windows hosted job'lari28 Rust testi, frontend/locale, fmt/clippy, native desktop binary build va OKF strict bilan o'tdi. M1'ning tarixiy Windows build gate'i yangi dalilga bog'landi; GUI/credential/minimum OS talablari yopilgan deb ko'rsatilmaydi.
- **M2 real Windows regressiyasi**: publisher smoke npm script argumentidagi Windows verbatim path uchun EISDIR topdi. Canonical containment'ni saqlagan holda external executable/cwd path soddalashtirildi. Yakuniy37777ed CI ikkala OS'da o'tdi; hosted Mac8 va Windows6 paket, har birida11 probe va2 parallel Node HTTP sinovi o'tdi. Dastlabki failure va fix dalillari saqlandi.
- **M1 poydevori**: Cargo workspace, Tauri/React desktop shell, SQLite migration, OS secret adapter, CLI doctor, shared en/ru/uz-Latn kataloglari va CI matrix yaratildi. [M1 hisobotida](m1-foundation-results.md) oldin o'tgan testlar, native macOS app/menu/re-launch dalillari va Windows/Intel/minimum OS cheklovlari ajratildi. Foydalanuvchining qayta test qilmasdan push ko'rsatmasi sabab avvalgi test suite'lari qayta bajarilmadi; bu push uchun CI skip belgisi ishlatiladi.
- **M0 dalillari**: [Runtime natijalari](m0-runtime-findings.md), aniq artifact/target kataloglari va izolyatsiyalangan smoke runner'lar qo'shildi. 23 artifact publisher SHA-256'ga mos keldi; ikki muqobil manba metadata-only qoldi. ARM64 va x64/Rosetta'da ikki PHP, ikki Node, Laravel/PDO/HTTP/TLS/Vite transport sinovlari o'tdi. 44 tooling/validator testi o'tdi; Windows, native Intel/minimum OS va release gate'lari ochiq saqlandi.
- **MVP reja**: Platforma/runtime feasibility'dan installer va uch tilli qabul matritsasigacha M0-M6 [bajarish rejasi](mvp-phase-1-plan.md) yaratildi. Scope va yangi texnik tanlovlar taklif, implementation hali boshlanmagan.
- **Til talabi**: Rus, o'zbek va ingliz tillari hamda kengayadigan locale registry/catalog modeli [i18n kontraktiga](localization.md) yozildi; skill va tekshirish qoidalari bog'landi. Ilova tarjimalari va runtime hali yaratilmagan.
- **Validator**: [read-only skript](../../scripts/validate_knowledge.py), uning development dependency'lari va regression testlari qo'shildi; [foydalanish yo'riqnomasi](validation-tooling.md) indeksga bog'landi.
- **Yuritish**: Muhim vazifa yakunidagi knowledge-impact tekshiruvi, strict validatsiya va bosqich oxirida kod/test dalillari bilan solishtirish [siyosatga](knowledge-policy.md) hamda skill'ga qo'shildi.
- **Yaratish**: OKF 0.2 formatida [indeks](index.md) va mavzular bo'yicha bilim yozuvlari yaratildi.
- **Qarorlar**: [mahsulot talablari](product-scope.md), [muhandislik siyosati](engineering-policy.md) va [UI/UX siyosati](ui-ux-policy.md) qayd etildi.
- **Ajratish**: [arxitektura](architecture.md) va [loyiha muhitlari](project-environments.md) ishchi yo'nalish, [MVP](mvp-proposal.md) esa tasdiqlanmagan taklif sifatida belgilandi.
- **Manba**: [suhbat qaydi](references/conversation-decisions.md) yaratildi; inson tekshiruvi yoki runtime natijasi o'ylab topilmadi.
- **Integratsiya**: AGENTS.md va DevForge skill'iga bilim bazasini o'qish, tegishli yozuvni yangilash va statuslarni ajratish qoidasi qo'shildi.
