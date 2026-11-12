---
type: Implementation Report
title: M8 loyihani tez tayyorlash
description: Laravel Git dependency setup PHP diagnostics recent favorites va HTTPS review implementatsiyasi.
status: draft
decision_status: confirmed
tags: [m8, onboarding, laravel, git, php, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M8 ni boshlash topshirig'i
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: M8 functional scope
  - id: setup
    resource: ../../crates/workspace/src/setup.rs
    title: Creation preparation cancellation recovery and project metadata
  - id: env
    resource: ../../crates/projects/src/env_file.rs
    title: Optimistic env writes backups and non-rotating APP_KEY
  - id: context
    resource: ../../crates/projects/src/execution.rs
    title: Verified runtime context and owned Git child processes
  - id: php
    resource: ../../crates/projects/src/php_settings.rs
    title: Validated project-only INI settings
  - id: system
    resource: ../../crates/workspace/src/system_setup.rs
    title: Digest-bound HTTPS review and existing narrow helper plans
  - id: authorization
    resource: ../../crates/serving/src/authorize.rs
    title: Fixed-helper OS authorization adapters
  - id: ui
    resource: ../../apps/desktop/src/components/ProjectSetup.tsx
    title: Three-language operational setup dialogs
  - id: native
    resource: ../../crates/workspace/examples/support/onboarding.rs
    title: Actual Laravel Git PHP SQLite and cancellation acceptance
  - id: composer
    resource: https://getcomposer.org/doc/03-cli.md
    title: Official create-project install and platform requirement commands
  - id: npm-policy
    resource: https://raw.githubusercontent.com/laravel/laravel/13.x/.npmrc
    title: Actual Laravel template script policy
  - id: local-backend
    resource: ../evidence/m8/local-native.json
    title: Local working-tree native backend acceptance
  - id: local-desktop
    resource: ../evidence/m8/local-desktop.json
    title: Local actual WKWebView controls and13 axe cases
  - id: local-checks
    resource: ../evidence/m8/local-checks.json
    title: Actual local checks and working-tree evidence boundary
  - id: core-ci
    resource: ../evidence/m8/ci-core.json
    title: Actual macOS Windows core CI conclusions
  - id: counters
    resource: ../evidence/m8/test-metrics.json
    title: Per-OS counters parsed from actual job logs
  - id: windows-probe
    resource: ../evidence/m8/windows-bootstrap.json
    title: Controlled native Windows bootstrap INI path reproduction
  - id: audit
    resource: ../evidence/m8/ui-audit.md
    title: Scoped UI source and visual review
  - id: hosted-arm
    resource: ../evidence/m8/hosted-macos-arm64.json
    title: Frozen-source native ARM64 backend and WKWebView
  - id: hosted-intel
    resource: ../evidence/m8/hosted-macos-x64.json
    title: Frozen-source native Intel backend and WKWebView
  - id: hosted-windows
    resource: ../evidence/m8/hosted-windows-x64.json
    title: Final native Windows backend and WebView2
  - id: native-matrix
    resource: ../evidence/m8/ci-native.json
    title: Actual required jobs and retained failed/cancelled attempts
  - id: artifacts
    resource: ../evidence/m8/artifact-verification.json
    title: Three final archive digests and report-equality receipts
  - id: source-map
    resource: ../evidence/m8/source-reconciliation.json
    title: Controlled source differences and failure reconciliation
---

# Holat

Foydalanuvchi M8'ni boshlashni topshirdi. Laravel wizard, Git clone,
Composer/npm/.env/APP_KEY, PHP sozlamalari va diagnostika, recent/favorite va
HTTPS setup review yaratildi. M8 functional acceptance macOS ARM64/Intel va
Windows x64'da actual backend va real native GUI bilan o'tdi. M7.2 Xdebug va M15 release gate'lari
ochiq qoladi.[^request][^roadmap]

# Yaratish va tayyorlash

Laravel Composer create-project, Git esa installed Git orqali olinadi.
PHP/Node/Composer managed paketlari integrity lease bilan tekshiriladi. Starter
cheklovi runtime/catalog.json defaults'ida, UI'da permanent versiya emas.
Bootstrap unregistered private temporary store'da; global PHP/Node/PATH
o'zgarmaydi. Git static owned Node launcher orqali M2 group/Windows Job'ga kiradi,
shell interpolation yo'q. URL credentials, ext helper, invalid branch va reserved
Windows folder nomlari refuse. HTTPS/SSH va absolute local repository qabul
qilinadi. Existing Git credentials/known-host sozlamalari ishlatiladi;
interactive login yoki recursive submodule setup qo'shilmagan.[^setup][^context]

Existing destination almashtirilmaydi. Random owned staging va create-new
publication concurrent user fayliga yozmaydi. Max100000 entry, depth128;
special Windows source links refuse. Bu filesystem va SQLite orasida all-or-nothing
transaction emas: disk/crash/publication xatosida yangi partial papka qolishi
mumkin. User fayllari avtomatik o'chirilmaydi. Registered dependency failure
project_id beradi; retry faqat prepare qiladi. Process/stage cancellation owned
descendant'larni to'xtatadi, bajarilib bo'lgan fayllar saqlanadi.[^setup][^native]

Composer install plugins'siz, scripts faqat tanlansa. Npm lock bo'lsa ci, bo'lmasa
install; default ignore-scripts. Allow scripts project npm policy'sini bekor
qilmaydi: Laravel13 .npmrc ham ignore-scripts=true beradi. Native failing-script
branch o'z policy'sini false qilgan holda actual nonzero exit/retry'ni tekshiradi.[^composer][^npm-policy][^native]

.env faqat yo'q bo'lsa example'dan olinadi. Empty APP_KEY32 OS-random byte'dan
base64 yaratiladi; existing key rotate qilinmaydi. CRLF, UTF-8 BOM va export
whitespace hisobga olinadi; duplicate key yoki concurrent edit refuse. Existing
.env o'zgarishiga private backup qo'yiladi. Key DTO'ga chiqmaydi, log redactor'ga
beriladi. SQLite initial migration faqat DevForge yaratgan Laravel receipt'iga
va project database.sqlite fayliga bog'langan; Git project's DB'si avtomatik
migrate qilinmaydi.[^env][^setup]

# PHP va kundalik boshqaruv

Recent/favorite metadata persist qilinadi. Old schema1 preferences serde default
bilan ochiladi; import/start/open/visit recency'ni yangilaydi. Forget code yoki
DB'ni o'chirmasdan navigation metadata'sini olib tashlaydi.[^setup]

Memory, upload/POST, execution timeout va timezone project record'da. INI injection
va nomuvofiq limit refuse; timezone selected PHP bilan tekshiriladi. CLI/web bir
project INI'dan foydalanadi, SAPI defaults va application ini_set PHP semantikasida
qoladi. Save faqat selected running project'ni restart qiladi. Diagnostika actual
PHP version/SAPI/INI/extensions/PDO va Composer lock platform requirements'ni
ko'rsatadi; vendor/env/key faqat presence flag. Windows INI mavjud SQLite
DLL'larini ham yuklaydi.[^php][^context][^native]

# HTTPS oynasi

Actual owned domains, fixed hosts target va own CA fingerprint ko'rsatiladi.
Ticket/digest/project/owner binding,10min review va explicit consent tekshiriladi;
refresh consent'ni yangilaydi. M4 helper ikki fixed plan: owned hosts va own CA
trust. macOS/Windows authorization adapterlari bor; UI/app admin sifatida
ishlamaydi. Success uchun actual hosts/trust qayta tekshiriladi. Serving status
uchun project restart kerak bo'lishi mumkin.[^system][^authorization]

Local native test review, consent guard va client-only CA bilan actual TLS'ni
tekshirdi; real administrator/keychain/UAC prompt bosilmadi. Native authorization,
denied prompt va signed-helper acceptance M15'da ochiq. Read-only review OS trust
qo'shildi degani emas.[^native]

# Dalillar va qolgan tekshirish

Local actual create/clone/Composer/npm/key/SQLite, failed dependency/retry,
CLI+web PHP, other-project PID preservation, TLS, cancellation descendant cleanup
va persisted navigation o'tdi. Real WKWebView GUI13 axe holatida0 finding bilan,
ru/uz/en minimum window va dark/light'da o'tdi. Frozen hosted native backend/UI
ham uch targetda13 tadan axe holati/0 finding bilan tugadi.[^hosted-arm][^hosted-intel][^hosted-windows]

| Target | Source va actual job | Natija |
| --- | --- | --- |
| macOS ARM64 |93bec5d, [job131463645100](https://github.com/jorabekyusupov/devforge/actions/runs/43298665397/job/131463645100) | Backend + WKWebView passed,13 axe/0 finding |
| macOS Intel |93bec5d, [job131463644465](https://github.com/jorabekyusupov/devforge/actions/runs/43298665397/job/131463644465) | Backend + WKWebView passed,13 axe/0 finding |
| Windows x64 |5f12739, [job131469975676](https://github.com/jorabekyusupov/devforge/actions/runs/43300470639/job/131469975676) | Backend + WebView2 passed,13 axe/0 finding |

Assembled qabul matritsasi ishlatilgan:93bec5d workflow umumiy failure holatini
eski Windows guard urinishidan saqlaydi, final Windows keyingi successful run'dan.
Old failure/cancelled job success deb ko'rsatilmagan.93bec5d→5f12739 source diff
faqat setup.rs canonical guard va uning unit regression'i; UI/PHP/IPC protocol
o'zgarmagan. Final core ikkala OS'da5f12739 bilan o'tdi.[^native-matrix][^source-map]

Uch final ZIP GitHub artifact SHA-256'ga teng, extracted native/desktop JSON'lari
job log report'lariga teng. Local logical1523x1032/scale2, hosted ARM1523x656,
Intel1523x942 va Windows1523x1032 actual viewport bilan qayd etildi. Minimum
outer820x620 window content'i OS'ga qarab kichikroq; overflow/scroll actual
tekshirildi. Local/full target va capped hosted capture teng deb ko'rsatilmaydi.[^artifacts][^audit]

Local67 Rust passed/1 native credential fixture ignored,25 frontend va3 locale
regression o'tdi. Canonical ownership regression qo'shilgach final core CI
43300464022 source5f12739 macOS/Windows'da68 Rust/1 ignored va25 frontend,
3 locale regression,6 packaging contract, fmt/clippy/typecheck va default native desktop build bilan
o'tdi. Counts har OS uchun, matrix soni bilan ko'paytirilmagan.[^core-ci][^counters]

Local native/captures source freeze'dan oldingi working tree'ga tegishli; final
BOM parser, alert/return focus va opening-revision regressionlari keyingi67/25
checks bilan tekshirildi. Hosted native frozen-source evidence alohida
to'ldiriladi; local report'ga tekshirilmagan commit SHA qo'yilmadi.[^local-checks][^local-backend][^local-desktop][^audit]

# Windows bootstrap topilmasi

ba69da7 native ARM64 full source/setup bosqichidan o'tib, cancellation port-fayl
yo'li relative bo'lgani uchun fixture'da to'xtadi. e2de6ac fixture root'ini
canonical absolute qildi. Windows shu eski run'da source creation'ni tugatmadi.

Controlled Windows PHP8.5.12/Composer2.11.2 probe'ida ASCII INI loaded/openssl/curl/zip
va source creation o'tdi. Unicode INI `php_ini_loaded_file=false` va extensions
false bilan qoldi; Composer TLS'ni o'chirmasdan fail qildi. ASCII managed config
va Unicode cwd/destination kombinatsiyasi actual Laravel source yaratdi.
93bec5d bootstrap config/cache'ni source parent'dan private app-data temp store'ga
ko'chirdi. Project source Unicode yo'lda qoladi; TLS bypass ishlatilmadi.[^windows-probe]

First29ccaae diagnostic Python env whitelist SYSTEMROOT'ni uppercase sabab
qoldirgan; uning ASCII curl failure holati valid control emas. Corrected probe
43298659600/job131463624959 va93bec5d shu xatoni tuzatgan. CI runner user profile
ASCII; non-ASCII Windows user-profile/app-data path acceptance hali alohida M15
tekshiruvi, universal Unicode path coverage deb ko'rsatilmaydi.[^windows-probe]

93bec5d Windows'da source/Composer/npm/key bosqichlari o'tib, SQLite ownership
guard'ida to'xtadi: database dir dunce canonical form'da, persisted project root
esa std::fs canonical verbatim form'da edi.5f12739 containment'ni ikki tomonda
bir xil std::fs canonical form bilan tekshiradi; executable/ENV boundary'dagina
path simplified qilinadi. Portable regression unit qo'shildi, outside-project
directory guard saqlandi. Mac93bec5d va Windows5f12739 native dalillari alohida
yoziladi; Unix'da bu ikki canonical form bir xil semantikaga ega.

# Kod bilan solishtirish

| Da'vo | Holat | Kod va dalil |
| --- | --- | --- |
| New Laravel/Git va scoped Composer/npm preparation | Mos | setup.rs, execution.rs va actual native source/clone/prepare |
| Existing APP_KEY saqlanishi, BOM/CRLF, backup va log masking | Mos | EnvPlan unit regressionlari va actual Composer script redaction |
| Project PHP INI, CLI/web values va other-project PID preservation | Mos | PhpSettings/Context va native CLI+HTTP assertions |
| Favorite/recent saqlanishi | Mos | Preferences defaults, navigation va native reopen |
| Bootstrap config joylashuvi va Windows SQLite containment | O'zgargan | Controlled Windows probe, app-data staging va canonical-form regression |
| HTTPS exact review, consent va helper adapters | Mos | system_setup.rs/authorize.rs, native review/guard va client-only TLS |
| Actual OS authorization dialogs, non-ASCII Windows profile, minimum OS/signing | Tekshirilmagan | M15 manual/native release acceptance gate'lari |
| Recursive submodule/OAuth login wizard, M7.2 Xdebug va M9 providers | Amalga oshirilmagan | Bu M8 implementatsiyasiga qo'shilmadi |

Hosted acceptance `.github/workflows/onboarding.yml` orqali, core check esa
`check.yml` orqali yuritiladi. Source branch immutable saqlandi. App UI testida
debug-only embedded WebDriver va private fixture credential adapter bor; default
build'da ular yo'q. File-picker va system authorization dialoglari automation'da
bosilmagan. Local private fixture report'lar olinib, task process'lari tugagani
tekshirilgach tozalangan; default-feature desktop binary qayta build qilingan.

Yakuniy strict OKF23 hujjat,0 error/0 warning bilan o'tdi. Bu strukturaviy
tekshirish; native acceptance o'rniga o'tmaydi. Yakuniy docs/evidence commit
uchun o'tgan product suite'lari qayta ishlatilmaydi. M8 installer qayta
paketlanmadi; M6 tester distributivi yangisi deb ko'rsatilmaydi. Public release,
signing, minimum OS, system authorization dialogs va non-ASCII Windows profile
acceptance M15'da ochiq qoladi.

Test-tooling tuzatishlari: Node version v prefiksi, template npm policy,
icon button aria-label va disabled fieldset readiness hisobga olindi.
Parallel Tauri feature build/rustdoc artifact collision source bug deb
belgilanmadi; final test/build ketma-ket bajarildi.

[^request]: «M8 ni boshla barcha ishlar tugagandan song»; oldingi test/OKF/final-push tartibi saqlandi.
[^roadmap]: M8 yangi raqamlashda; M9-M15 tafsilotlari alohida reja.
[^setup]: Ownership-aware destination, explicit review and retry.
[^env]: No key rotation or public key-value DTO/log output.
[^context]: Managed leases, static Git bridge, cancellation and private INI.
[^php]: Project storage/selected runtime; system PHP o'zgarmaydi.
[^system]: Review va OS effect, hosts va trust ajratilgan.
[^authorization]: Actual system authorization prompts manual acceptance gate.
[^ui]: Semantic controls, incumbent tokens, illustrative target not runtime data.
[^native]: Isolated task fixture; user Herd/hosts/global CA o'zgarmagan.
[^composer]: Selected Composer create/install/platform-check commands.
[^npm-policy]: Allow does not bypass project's own script policy.
[^local-backend]: Isolated actual Laravel/Git/SQLite/TLS and descendant cancellation.
[^local-desktop]: Real native controls and no invoke mocking;13 axe cases.
[^local-checks]: Exact local counters, scope notes and private fixture cleanup receipt.
[^core-ci]: Both actual job conclusions success on frozen5f12739.
[^counters]:68 Rust/25 frontend/3 locale/6 packaging per OS,1 ignored credential fixture.
[^windows-probe]:93bec5d controlled cases with correct SYSTEMROOT; no user secrets/project code.
[^audit]: Stable illustrative target and actual logical viewport compared; no pixel-perfect claim.
[^hosted-arm]:93bec5d actual native ARM64 job, no invoke mocking.
[^hosted-intel]:93bec5d actual native Intel job, no Rosetta substitution.
[^hosted-windows]:5f12739 actual Windows x64, native WebView2 and runtime packages.
[^native-matrix]: Actual per-job conclusions and retained original attempt statuses.
[^artifacts]: Final ZIP digests checked before accepting their public reports/captures.
[^source-map]: Exact source diff and actual fixture/Windows product fixes reconciled.
