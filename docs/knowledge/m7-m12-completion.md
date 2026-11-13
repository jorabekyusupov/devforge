---
type: Development Milestone
title: M7–M12 qolgan imkoniyatlarni yakunlash
description: Tayyor funksiyalarni saqlagan holda qolgan debugging, konfiguratsiya, Intel MySQL va workspace lifecycle qismlari.
status: draft
decision_status: confirmed
tags: [m7, m9, m10, m12, completion]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Qolgan ishlarni tugatish va push topshirigi
  - id: previous
    resource: /m7-xdebug-results.md
    title: Oldingi functional acceptance va ochiq cheklovlar
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: M7–M15 scope chegaralari
  - id: commands
    resource: ../../crates/workspace/src/lib.rs
    title: Shared-core buyruqlar
  - id: debugger
    resource: ../../crates/debugger/src/server.rs
    title: Typed DBGp mutation va pause revision
  - id: xdebug
    resource: https://github.com/xdebug/xdebug/blob/3.5.3/src/debugger/handler_dbgp.c
    title: Xdebug property_set ning actual PHP literal kontrakti
  - id: lifecycle
    resource: ../../crates/workspace/src/worktrees/lifecycle.rs
    title: Reviewed archive lifecycle
  - id: remote
    resource: ../../crates/projects/src/backup/remote.rs
    title: External logical dump adapter
  - id: editor
    resource: ../../apps/desktop/src/pages/ConfigurationPage.tsx
    title: Desktop konfiguratsiya muharriri
  - id: git-state
    resource: ../../crates/workspace/src/worktrees/git.rs
    title: Normalized working/index/HEAD va untracked holati
  - id: php-review
    resource: ../../docs/evidence/completion/php-intel-provider-review.json
    title: Intel PHP publisher rebuild autentifikatsiyasi
  - id: packages
    resource: ../../runtime/m9-candidates.json
    title: Imzosi tekshirilgan MySQL oilalari
---

# Vakolat va saqlanadigan ishlar

User M7–M12 qolgan ishlarini bajarish, test, OKF va yakunda pushni topshirdi;
bajarilgan funksiyalar saqlanadi. Asos65948e0 main, yangi isolated checkout.
Parallel project-discovery branchidagi dirty fayllar bu scope'ga kirmaydi.[^request]

# Bajarish ketma-ketligi

1. M7: GET bilan cheklangan trigger'ga methods/headers/body request builder;
   paused primitive locals'ni explicit o'zgartirish, existing stepping/profiler'ni saqlash.
2. M9: Intel uchun verified portable MySQL provider; existing DB'ni auto-upgrade qilmaslik.
3. M10: existing portable schema/plan/setup ustiga localized GUI YAML read/edit,
   validation, reviewed save va setup/recovery.
4. M12: reviewed retained archive reactivation va explicit purge; source/DB/branch
   saqlanishi. Reviewed recursive submodule lifecycle va external PG/MySQL source
   dump'ni yangi managed child DB'ga staged copy qilish.
5. macOS ARM64/Intel va Windows x64 native acceptance; RU/UZ/EN, regression,
   source/evidence reconciliation, strict OKF va final push.

M8/M11'ning accepted funksiyalari qayta yozilmaydi. M13/M14 integrations va M15
release/signing/clean-OS credential/system-dialog gate'lari alohida bosqichlarda.
M7 request/variable changes faqat explicit developer actions; arbitrary shell yoki
remote administration API qo'shilmaydi.[^previous][^roadmap]

# Implementatsiya chegarasi

Shared Rust core desktop va CLI bilan ishlatiladi. Quyidagilar agent tanlagan
implementatsiya yechimlari; har biri foydalanuvchi alohida tasdiqlagan arxitektura
qarori deb talqin qilinmaydi.[^commands]

- **M7**: GET/HEAD/POST/PUT/PATCH/DELETE/OPTIONS, project-local path, bounded
  headers va UTF-8 body. Body1MiB,32 header/16KiB; transport va Xdebug headerlari
  himoyalangan. HEAD bo'sh body yuboradi, UI boshqa methodlar uchun draftni
  xotirada saqlaydi. Request body/header diskka yozilmaydi; response status olinadi.
- Paused simple ASCII-identifier primitive locals uchun string/int/finite-float/
  bool/null assignment bor. Sensitive, nested va truncated qiymatlar read-only.
  Pause revision eski frame/request tahririni rad etadi. Xdebug `property_set`
  data'ni PHP source sifatida baholaydi: client faqat o'zi yasagan primitive
  literal yuboradi; string Base64 orqali inert qiymatga aylanadi, null uchun
  qo'llab-quvvatlanmagan type cast yuborilmaydi. User expression/eval API yo'q.
  Actual PHP canary testi executable ko'rinishdagi matn fayl yaratmasligini va
  integer tahriri bajarilayotgan hisoblashga ta'sirini tekshiradi.[^debugger][^xdebug]
- **M9/M10 portability**: Oracle imzosi tasdiqlangan8.4.11 server/client/dump
  oilasi macOS ARM64, Intel va Windows x64 uchun exact common tanlov. Oldingi
  ARM64/Windows8.4.13 paketlari va project pinlari saqlangan; avtomatik downgrade
  yo'q.8.4.13 manifest Intel'da jim8.4.11'ga o'tmaydi. Yangi DB selector har
  engine ichida yangi versiyani oldin ko'rsatadi. Release patch/licensing siyosati
  M15 gate'i bo'lib qoladi.[^packages]
- Intel stock PHP8.5.12 artifact'i tekshirish vaqtida upstream'da qayta
  yig'ilgani aniqlangan. Publisher sidecar va actual archive SHA mosligi,
  payload hash hamda Rosetta x86_64 CLI/FPM8.5.12 NTS probe tekshirildi.
  Legacy descriptor/hashlar saqlanib, explicit `staticphp20261113` replacement
  oilasi qo'shildi; mavjud project avtomatik o'tkazilmaydi.[^php-review]
- **M10 GUI**: existing manifest schema/plan/setup ustidagi uch tilli YAML editor;
  read/edit,64KiB schema-validated atomic save, file SHA asosidagi stale-write
  guard va local old-version backup. Saving setup'ni bajarmaydi. Saved plan'dagi
  downloads, secret availability va DB action ko'rib chiqiladi; trust va `.env`
  write alohida tanlov. Local secret qiymati input'dan tozalanadi. Existing journal
  bilan setup retry/cancellation saqlangan. CLI ham ishlaydi.[^editor][^commands]
- **M12**: archive'dan aynan o'sha project ID/path, keyinchalik o'zgartirilgan
  site/queue settings, secret refs va DB binding bilan reactivation. Linked branch
  HEAD o'zgargan yoki band bo'lsa to'xtaydi. Receipt v2 va `restoring`/`purging`
  retry holatlari bor; eski v1 receipt o'qiladi. Content review ignored `.env` va
  SQLite'ni ham qamraydi;100k file/64GiB/depth100 chegaralari bor. Purge faqat
  owned retained files'ni o'chiradi; managed DB, credential, source va branch
  qoladi. Deletion boshlanishidan oldin cancel va stale checks bor; boshlangan
  deletion sinxron tugaydi yoki keyingi reviewed retry bilan davom etadi.[^lifecycle]
- Windows CRLF checkout ustiga archived LF faylni tiklash `git status`da
  normalized content o'zgarmagan bo'lsa ham false-dirty holat yaratishi native
  sinovda topildi va lokal `core.autocrlf=true` bilan takrorlandi. Dirty guard
  read-only cached HEAD diff, working/index diff va untracked list'ni solishtiradi;
  index yoki faylni stage/normalize qilmaydi. Staged-only, actual tracked,
  untracked va dirty submodule guardlari saqlangan.[^git-state]
- Submodule bor commit uchun self-contained clone va recursive submodule init
  ishlatiladi. Trust review `.gitmodules` va ichki manbalarni qamraydi. Dirty
  submodule cleanup'ni bloklaydi; archive/restore Git metadata'ni saqlaydi; purge
  oldin root/submodule history bundle yaratadi. Oddiy linked worktree'ga keyin
  qo'lda qo'shilgan submodule metadata'sini avtomatik conversion qilish bu yo'l
  emas: yangi submodule workspace yaratish reviewed isolated clone orqali bo'ladi.
- External PG/MySQL source uchun host/port/database/user/local password ref va
  CA file ko'rib chiqiladi. External host uchun verified TLS, plaintext esa faqat
  literal loopback IP uchun. Password argv/log'da bo'lmaydi; private options file
  ishlatiladi. Secretning opaque revision'i review'ni bekor qiladi, password hash
  plan'ga kiritilmaydi. PG read-only logical dump; MySQL table/data scope,
  trigger/routine/eventlarsiz.1GiB/300s dump va staged local restore bor;
  connection/CA/restore xatosi oldingi active binding'ni saqlaydi. Portable source
  DB nomi ASCII identifier va63 belgigacha; arbitrary version/engine conversion
  va production remote administrator bu feature emas.[^remote]

# Tekshirish holati

Product candidate: `fd8c73e1edabd327c35067fa4e35960425049b02`.
Lokal macOS27.2.1 ARM64'da117 Rust testi o'tdi;1 explicit native credential
roundtrip testi M15 uchun ignored.38 frontend testi, locale/fmt/clippy va
fixture-feature clippy o'tdi. Native PG/MySQL, TLS wrong-CA refusal, archive/
restore/purge, recursive submodule va CLI sinovlari o'tdi; `core.autocrlf=true`
qo'shimcha native qabuldan ham o'tdi. Native debugger PHP8.4/8.5, barcha primitive
mutation, inert executable-looking string, stepping, HTTP builder va profiler
sinovidan o'tdi. HTTP native fixture GET/POST/PUT/PATCH/DELETE body, HEAD/OPTIONS
empty-body holatlarini tekshiradi; transport unit testi OPTIONS'da ham UTF-8
payload byte'larini tekshiradi. Source PHP'ning method body talqini transport
byte'lari bilan bir claim emas.

Actual WKWebView'da debugger13 va completion9 axe case/0 violation; RU/UZ/EN,
light/dark, minimum820x620, YAML read/edit/save/review, secret input clearing,
archive reactivation va explicit purge o'tdi. `agent-desktop` mavjud emasligi
sabab existing native W3C harness ishlatildi; OS permission o'zgartirilmadi.
Production credential adapter bu fixture secret store bilan tasdiqlanmaydi.

`15a9377` core run43439756815 macOS/Windows'da to'liq o'tdi; native
run43439751906 ARM64 job131933982082 ham barcha backend/CLI/GUI checks bilan
muvaffaqiyatli. O'sha run Intel'da upstream stock PHP hash mismatch, Windows'da
CRLF false-dirty holatini aniqladi; bu failures yashirilmadi. Tuzatilgan c5c3231
uchun hosted native43444634035 va core43444639614 yakunlanmoqda. Oldingi
6f11ed2/96cc405 failures DBGp literal/null hamda test fixture path tuzatishlaridan
oldingi source hisoblanadi. Runtime source builder/recipe o'zgarmadi; SDK cache
ularning source hashiga bog'langan.

Keyingi c5c3231 run Windows'da archive/TLS qismlaridan o'tib, submodule'ning
normal CRLF checkout'i uchun LF-only test expectation'ini aniqladi. Test initial
normalized matnni va archive/restore'dan keyingi aynan original byte'larni
solishtiradi. ARM64'da backend/CLI o'tdi, UI read-busy paytida Edit tugmasi
DOM'dan olinishi flaky click'ni ochdi; button endi joyida disabled qoladi va native
harness editable holatni kutadi. macOS core'da serial configuration save'dan keyin
transient manifest_busy kuzatildi: transaction guard explicit unlock bilan
yakunlanadi; duplicated descriptor saqlanganda ham keyingi lock ochilishini unit
test tekshiradi. Parallel fork'da descriptor inheritance ehtimoli mexanizmga mos,
ammo aynan o'sha CI fork'i traced deb ko'rsatilmaydi.

fd8c73e native run43447799027 va core43447805176 yakunlanmoqda.

Structural OKF29 document/0 error/0 warning; bu desktop yoki human verification
emas. Final phase closure hosted matritsa va committed public reports bilan
bog'lanadi.

# Phase reconciliation

| Claim | Source bilan holati | Dalil chegarasi |
| --- | --- | --- |
| M7.1 Inspector | matching, saqlangan | Companion source qayta yozilmadi; yangi debugger undan ajratilgan |
| M7.2 kengaytma | changed/implemented | Latest hosted native acceptance kutilmoqda |
| M8 | matching, saqlangan | Onboarding/product implementation o'zgarmadi; umumiy regression bor |
| M9 common runtime | changed/implemented | GPG3 target; latest native common-version matrix kutilmoqda |
| M10 GUI | changed/implemented | Existing planner reused; latest native UI matrix kutilmoqda |
| M11 | matching, saqlangan | Mail/log source qayta yozilmadi; umumiy regression bor |
| M12 kengaytma | changed/implemented | Local backend/CLI/UI dalili; latest hosted matrix kutilmoqda |
| M13/M14 | boshlanmagan | Ushbu topshiriq ularga o'tmagan |
| M15 release | unverified/ochiq | Signing, clean/minimum OS, native credential/system dialog, installer/update/rollback gate'lari alohida |

[^request]: Joriy user topshirigi: qolgan ishlar to'liq, test, OKF, final push; bajarilganlar saqlansin.
[^previous]: Oldingi accepted M7.1/M7.2 normal operation scope saqlanadi.
[^roadmap]: Qolgan functional scope ushbu topshiriq bilan vakolatlandi; public release emas.

[^commands]: Shared-core Command va desktop/CLI adapterlari actual source.
[^debugger]: Local name/value allowlist, generated literal, pause revision va scoped source.
[^xdebug]: Xdebug3.5.3 property_set handlerning cast/data talabi; caller matni literal qilinadi.
[^lifecycle]: Owned path/project guards, content review, Git history va retry states.
[^remote]: Verified dump executables, private auth files, TLS va staged restore.
[^editor]: Read revision, reviewed save/setup va localized UI.
[^packages]: Oracle signature fingerprint va immutable package hashes kandidat provenance'da.

[^git-state]: Git content guard read-only; CRLF va staged-only regression sinovlari mavjud.
[^php-review]: Actual artifact/payload SHA, publisher sidecar va explicit replacement metadata.
