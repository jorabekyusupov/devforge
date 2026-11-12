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
---

# Holat

Foydalanuvchi M8'ni boshlashni topshirdi. Laravel wizard, Git clone,
Composer/npm/.env/APP_KEY, PHP sozlamalari va diagnostika, recent/favorite va
HTTPS setup review yaratildi. Local macOS native backend va GUI o'tdi; hosted
matritsa yakuniy dalil bilan to'ldiriladi. M7.2 Xdebug va M15 release gate'lari
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
ru/uz/en minimum window va dark/light'da o'tdi. Final hosted source/job IDs,
counters, archive digest va strict OKF natijasi yakunda qo'shiladi.

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
