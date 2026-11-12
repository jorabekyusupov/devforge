---
type: Implementation Report
title: M10 takrorlanadigan muhit
description: Portable devforge.yaml exact package pins local secret links reviewed CLI setup va recovery jurnali.
status: draft
decision_status: confirmed
tags: [m10, reproducible, configuration, cli, secrets, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M10 boshlash topshirigi
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: Reproducible environment CLI scope
  - id: manifest
    resource: ../../crates/projects/src/manifest.rs
    title: Bounded versioned YAML and exact cross-platform catalog pins
  - id: orchestration
    resource: ../../crates/workspace/src/repro.rs
    title: Redacted plan digest setup journal and retained service bindings
  - id: cli
    resource: ../../crates/cli/src/config.rs
    title: CLI export init plan secret setup status and stdin credentials
  - id: environment
    resource: ../../crates/projects/src/environment.rs
    title: Local secret references in managed execution contexts
  - id: dotenv
    resource: ../../crates/projects/src/env_file.rs
    title: Reviewed subset patch optimistic writes and private backups
  - id: store
    resource: ../../crates/projects/src/project.rs
    title: Additive metadata schema3 and legacy project records
  - id: leases
    resource: ../../crates/platform/src/distribution.rs
    title: Exclusive setup lease shared with GUI and CLI operation leases
  - id: native
    resource: ../../crates/workspace/examples/repro_smoke.rs
    title: Real CLI two-machine stores Laravel PostgreSQL Garnet HTTP and failure recovery
  - id: workflow
    resource: ../../.github/workflows/reproducible.yml
    title: Native ARM64 Intel and Windows acceptance
  - id: example
    resource: ../examples/devforge.yaml
    title: Credential-free pinned portable example
  - id: yaml
    resource: https://docs.rs/serde-saphyr/2.1.1/serde_saphyr/
    title: Official YAML parser options budget and opt-in feature boundaries
---

# Scope va holat

Foydalanuvchi M10 ni boshlashni so'radi. Roadmap'dagi CLI-first kontrakt yaratildi;
local va hosted native acceptance holati quyida yuritiladi. M10 yangi desktop
config editor emas: mavjud GUI orqali tayyorlangan loyiha keyin ishga tushiriladi.
M11 avtomatik boshlanmaydi. Schema/pin/journal detali assistant implementation
yechimi; ularning har biri alohida user technology-version talabi emas.

# Portable va lokal chegarasi

`devforge.yaml` schema1: project name/domain/HTTPS/Vite/worker preference, exact
PHP/Node/Composer versions, optional PostgreSQL/MySQL/Garnet/.NET selection,
PHP settings, cache/queue drivers, fixed dependency flags va environment -> local
alias mapping. `pins` bundled catalog ID -> SHA256 bo'lib barcha mavjud target'lar
uchun export qilinadi. PHP Windows provider `-1` reviziyasi portable PHP semver'dan
alohida, exact package ID/digest'da saqlanadi. App tanlaydigan Caddy provider ham
pin bilan tekshiriladi; arbitrary proxy/provider URL kiritilmaydi.

YAML'da raw credential, local store item ID, machine path, port, database role yoki
instance ID bo'lmaydi. Local aliases canonical checkout path hash'iga bog'langan
OS SecretStore namespace'da saqlanadi. Ikkinchi checkout/device o'z qiymatini
kiritadi. Registered legacy project environment qiymati shu local project'dan
fallback bo'lishi mumkin; secret export'da o'qilmaydi yoki ko'chirilmaydi.
Reference update opaque revision bilan prior review'ni eskirtiradi. Runtime
contexts credential link'ni resolve qiladi; YAML/context diagnostics qiymatni
chiqarmaydi. Project code o'ziga ajratilgan environment qiymatlarini o'qiy oladi.

Parser serde-saphyr2.1.1, Rust MSRV declared compatibility tekshirildi. Faqat
serialize/deserialize features; external include/properties/robotics yoqilmagan.
64KiB input,16 depth/4096 nodes,1 document; alias/anchor/merge, duplicate keys,
unknown fields va unsupported tags rad etiladi. Error snippet saqlanmaydi.
Version ranges yoki YAML orqali executable/shell/hook buyruqlari berilmaydi.

# CLI review va execution

- `config export <project-id> [destination]` va `config init <root> <php> <node> <composer>`
  yangi fayl yaratadi; mavjud fayl ustidan yozmaydi. [README](../../README.md)
  team workflow va stdin secret kiritishni ko'rsatadi.
- `config plan <root> [--write-env]` schema1 JSON: exact packages/downloads,
  settings/dependency flags, local secret presence, redacted key actions va
  template/key-generation flags. Project files va jarayonlar o'zgarmaydi;
  normal app metadata migration va lokal lease/journal directory paydo bo'lishi mumkin.
- Review digest manifest, project/site/binding state, lockfiles, `.env` snapshot,
  local secret revisions va download holatiga bog'langan. Stale plan yoki missing
  pin/secret uchun yangi review kerak; old digest silent reuse qilinmaydi.
- `config setup <root> --review <digest> --trust [--write-env]`: CLI exclusive
  operation lease oladi. GUI'ni Quit qilish kerak; close-to-tray yetarli emas.
  Existing serving session uchun ham project lease ishlatiladi. Rust callers
  session/manifest leases va caller-owned lifetime'ni hurmat qiladi.
- Package download/extraction, registration/runtime/PHP/site settings, local
  references, optional environment, database, dependencies, services va env patch
  bosqichlari `config status` ko'rsatadigan private schema1 journal'da saqlanadi.
- Composer no-plugins/no-scripts, npm ignore-scripts. Predis faqat explicit flag va
  absent client holatida o'rnatiladi; bu composer.json/lock/vendor'ni o'zgartiradi.
  Application package lock'lari PHP library va Node dependency'larini boshqaradi.
  SQL migrations, arbitrary setup hooks, global PATH, hosts/trust/elevation yoki
  background serverni CLI'dan detach qilish avtomatik bajarilmaydi.

# Ma'lumot va recovery

Managed DB bir xil paketda bo'lsa qayta ishlatiladi; boshqa paket uchun old data
va binding saqlanadi, yangi local instance/database tanlanadi. Bu DB migration
emas. Cache own project/provider/runtime'ga mos bo'lsa qayta ishlatiladi. Host
MySQL capability gap'i, shu jumladan Intel Mac current binary yo'qligi, plan'da
unsupported deb chiqadi; boshqa provider yoki eski versiya yashirin tanlanmaydi.

`--write-env` bo'lmasa template copy, APP_KEY generation va profile patch ham
bajarilmaydi. Flag bo'lsa key actions redacted, existing nonempty APP_KEY saqlanadi,
unrelated lines va local values saqlanadi. Faqat manifest'da ko'rilgan environment
nomlari hamda managed DB/cache/queue keys yoziladi. Stale/duplicate/symlink/reparse
files rad etiladi; replacement private backup + optimistic snapshot bilan.
M10 idempotent patch alohida `apply_if_changed`; old M3 explicit `EnvPlan.apply`
backup kontrakti saqlangan. Multiline local secret runtime'da mumkin, dotenv write
uchun rad etiladi. Template copying flag'i butun `.env.example` nusxasini anglatadi;
preview raw secret values yoki template'ning to'liq content diff'ini chiqarmaydi.

Bu additive setup: manifest'dan olib tashlangan nom boshqa local variables yoki
credential'larni delete qilmaydi; local `.env` cleanup foydalanuvchi nazoratida.
Journal cross-filesystem/SQL transaction emas. Failure/cancel bosqich va stable
error code'ni saqlaydi; old DB data o'chirilmaydi. Ready service ID saqlangan bo'lsa
retry reuse qiladi. Crash initialization'dan keyingi pending/orphan instance uchun
manual recovery kerak; data directory forced reinitialize qilinmaydi.

Project metadata schema2 ->3 marker va `environment_references` default field.
Legacy records yangi app'da o'qiladi; eski app schema3'ni rad etadi. Bu tested
application downgrade/rollback emas; M15 gate ochiq. Secret credential roundtrip
va system dialogs bu phase'da production OS credential UI acceptance degani emas;
CI fixture adapter faqat explicit test-support build'da mavjud.

# Tekshiruv holati

Focused parser/pin, unsafe env/ref, stale review, subset dotenv write, lease va
legacy JSON tests bor. Local native CLI ikkita alohida app-store va checkout'da
Laravel13, exact packages, PostgreSQL/Garnet, secret isolation, rerun preservation,
key preservation, failed dependency retry va cancellation'ni tekshiryapti.
Final HTTP va hosted three-target dalillari phase-close'da qo'shiladi. Bu literal
Mac archive'ni Windows'ga ko'chirish emas: har OS'da ikkita mustaqil store/path
scenario va portable pins barcha target'lar uchun tekshiriladi.

GUI source o'zgarmadi; M10 uchun yangi native GUI/axe pass da'vo qilinmaydi.
Existing desktop build va frontend regression checks alohida. M9'dagi10 axe/cases
historical evidence bo'lib qoladi. M7.2 Xdebug va M15 release/minimum-OS/signing/
licensing/native-dialog gate'lari ushbu task bilan yopilmaydi.
