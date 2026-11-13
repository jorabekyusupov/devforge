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

# Yakuniy tekshirish va source chegarasi

M7–M12 qolgan funksional scope yakunlandi. Yakuniy product source
`f13c10697dc4f55a6b4b02f27fe5c4120cdba50b`; docs/evidence-only close commit undan
keyin keladi. Inson ko'rib chiqqan `verified` metadata qo'shilmadi.

| Target | Native source | Run / job | Natija |
| --- | --- | --- | --- |
| macOS15 ARM64 |08dd836|43453644852 /131982029879|passed|
| macOS15 Intel |08dd836|43453067442 /131980050440|passed|
| Windows Server2025 x64 |f13c106|43457642556 /131995900378|passed|

Har bir targetda19 debugger backend check va12 completion backend check,
shared-core CLI,13 debugger va9 completion axe case/0 violation o'tdi. M7 actual
avtomatik topilgan va explicit tayyorlangan Laravel loyihasida bajarildi:
PHP8.4/8.5, primitive mutation, inert executable-looking string, stepping,
redaction/isolation, request/profiler va Forget/Restore configuration preservation.
GET/POST/PUT/PATCH/DELETE body bilan, HEAD/OPTIONS native fixture'da empty body
bilan tekshirildi; real TCP unit testi OPTIONS UTF-8 byte'larini ham tekshiradi.
HTTP serverning body talqini yuborilgan transport byte'lari bilan bir xil claim emas.

Core run43457648747 yakuniy f13c106 source'da ikkala OS'da muvaffaqiyatli:
macOS134 Rust, Windows131 Rust, har birida44 frontend testi, locale/typecheck,
fmt/clippy, distribution tooling va actual native desktop build.1 explicit OS
credential roundtrip testi har ikkala platformada M15 uchun intentionally ignored.

- [Native matrix va artifact digestlar](../evidence/completion/ci-native.json).
- [Core counts va checks](../evidence/completion/ci-core.json).
- [Source projection](../evidence/completion/platform-source-boundary.json):
  f13c106'da macOS production source08dd836 bilan bir xil; o'zgarish Windows-only
  erase diagnostics va acceptance Git actor'ida. Unix wrapper, boshqa Rust,
  frontend, manifests va runtime catalog bir xil. Mac'da keyingi test-helper actor
  qayta native bajarildi deb da'vo qilinmaydi.
- [Bajarilgan source saqlanishi](../evidence/completion/source-preservation.json):
  M2 runtime, M7.1 Inspector, M8 setup, M11 mail/log va accepted Discovery source
  main996a11d bilan bir xil. Bu har bir tarixiy feature uchun yangi barcha native
  gate'lar qayta yopildi degani emas.
- [Attempt tarixi](../evidence/completion/attempts.json): failure va fixlar yashirilmagan.

Actual WKWebView va WebView2 screenshots saqlandi va ko'rib chiqildi. RU/UZ/EN,
light/dark, requested minimum820x620, captured actual viewport, YAML read/edit/save/
review, secret clearing, archive same-ID reactivation va explicit purge tekshirildi.
Windows outer/client size farqi va real viewportlar raw report'larda bor; ular
requested size bilan aralashtirilmaydi. `agent-desktop` mavjud emasligi uchun
existing debug-only native W3C harness ishlatildi, OS permission o'zgartirilmadi.
FixtureSecrets production OS credential adapterini tasdiqlamaydi.

# Concurrent main bilan integratsiya

Parallel accepted delivery main996a11d5015c8433397cf6b6c09682cd883251a1'ga
kirdi. Isolated milestone branch unga merge qilindi; original checkout
almashtirilmadi. Runtime descriptorlar ikkala source'dan qiymati bilan saqlangan.
Discovery availability guard yangi config/source entry point'larda qo'llanadi;
owned archive restoration yangi katalog filesystem identity'sini o'sha project ID'ga
mos yangilaydi. Replaced directory read/save'da rad etilishi va restored identity
unit regressiyalari bor. Discovery funksiyalari qayta yozilmadi.

# Platforma failure'lari va recovery

Windows CRLF checkout ustiga LF archive fayli tiklanganda porcelain false-dirty
holati lokal `core.autocrlf=true` bilan takrorlandi; read-only normalized staged/
working/untracked diff yechimi actual staged-only o'zgarishni ham bloklaydi.

Windows GUI purge fixture'da Git commit elevated Python actor ostida bajarilganidan
keyin storage failure chiqdi. Fixture Git commit endi native app bilan aynan bir
restricted token ostida bajariladi; privilege oshirilmaydi. Owned-handle unlink
read-only Git object hardlink'ining source atributi yoki data'sini o'zgartirmaydi;
Windows unit va yakuniy actual UI purge o'tdi. Foreign ACL yoki ochiq file handle
uchun avtomatik ACL o'zgartirish/escalation qo'shilmadi; failure bo'lsa purging
journal, managed DB va recovery history qoladi. Read-only atribut failure'ning
har bir OS sababini alohida traced deb talqin qilinmaydi.

Mac transaction lock explicit unlock guard bilan bo'shatiladi; duplicated descriptor
saqlanganda ham keyingi lock ochilishi unit testi bilan tekshirilgan. Parallel fork
descriptor inheritance observed manifest_busy mexanizmiga mos, ammo aynan CI fork'i
traced deb ko'rsatilmaydi. Edit control read-busy paytida DOM'dan olib tashlanmaydi.

Birinchi08dd836 ARM64 job native assertion'ga yetmasdan aws-lc archive yozishda
runner I/O error bilan to'xtagan. Same-source fresh ARM64 run muvaffaqiyatli; dastlabki
run red holati va alohida successful retry source matrix'da ochiq ko'rsatilgan.

# Phase reconciliation

| Claim | Holat | Dalil |
| --- | --- | --- |
| M7.1 Inspector |matching, saqlangan|source preservation|
| M7.2 request/locals kengaytma |changed, implemented, native passed|uch target debugger backend/CLI/GUI|
| M8 |matching, saqlangan|setup source unchanged; merged core regressions|
| M9 common runtime |changed, implemented, native passed|GPG3 target; actual MySQL common backup/restore|
| M10 GUI |changed, implemented, native passed|existing planner; uch target native editor/secret/review|
| M11 |matching, saqlangan|mail/log source unchanged; merged core regressions|
| M12 kengaytma |changed, implemented, native passed|archive/submodule/remote TLS/staged DB/CLI/native UI|
| Discovery interop |matching + owned restore guard|auto-discovered M7, filesystem identity regressions|
| M13/M14 |boshlanmagan|ushbu topshiriq ularga o'tmagan|
| M15 release |unverified, ochiq|clean/minimum OS, native credential/system dialog, signing, distribution, installer/update/rollback|

# Qabul chegarasi

Submodule-bearing branch yangi isolated clone orqali boshqariladi; oldingi linked
worktree'ga qo'lda qo'shilgan metadata'ni automatic conversion qilish yo'q. MySQL
external copy table/data scope; trigger/routine/event va engine/arbitrary-version
conversion yo'q. Locals simple ASCII identifiers, primitive values va bounded
display/mutation bilan cheklangan; arrays/objects/arbitrary expression editing yo'q.
Bu normal funksional scope chegaralari public release yoki barcha OS/device'larda
cheksiz muvofiqlik va'dasi emas. Existing DB/project pinlari avtomatik almashtirilmagan.

Structural OKF close:30 document,0 error,0 warning (strict). Bu test/source dalilini
bog'laydi; o'zi runtime yoki human verification hisoblanmaydi.

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
