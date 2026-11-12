---
type: Implementation Report
title: M9 baza va xizmatlar
description: Owned MySQL Garnet Laravel service profiles va staged logical backup implementatsiyasi.
status: draft
decision_status: confirmed
tags: [m9, database, mysql, garnet, backup, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M9 boshlash topshirigi
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: Database services scope
  - id: orchestration
    resource: ../../crates/workspace/src/services.rs
    title: Desktop CLI shared service orchestration
  - id: mysql
    resource: ../../crates/projects/src/mysql_service.rs
    title: Portable MySQL ownership authentication and limited project users
  - id: cache
    resource: ../../crates/projects/src/cache.rs
    title: Dedicated Garnet processes and scoped Laravel drivers
  - id: backup
    resource: ../../crates/projects/src/backup.rs
    title: Bounded private logical archives and staged restore
  - id: generations
    resource: ../../crates/projects/src/database_store.rs
    title: Retained database generations and active binding transaction
  - id: packages
    resource: ../../runtime/m9-candidates.json
    title: Pinned provider acquisition and authenticity evidence
  - id: ui
    resource: ../../apps/desktop/src/components/ProjectServices.tsx
    title: Localized service configuration backups and retained connections
  - id: native
    resource: ../../crates/workspace/examples/support/services.rs
    title: Actual Laravel cache lock queue Lua and database acceptance
  - id: failure
    resource: ../../crates/projects/examples/services_smoke.rs
    title: Failure isolation process persistence and mysql client escape guard
  - id: local
    resource: ../evidence/m9/local-native.json
    title: Local native working tree evidence
  - id: core-ci
    resource: ../evidence/m9/ci-core.json
    title: macOS Windows core checks with exact source and job IDs
  - id: native-ci
    resource: ../evidence/m9/ci-native.json
    title: Three native platform reports and verified artifact digests
  - id: attempts
    resource: ../evidence/m9/ci-attempts.json
    title: Windows monitor failure and successful followup
  - id: packages-verified
    resource: ../evidence/m9/package-verification.json
    title: Acquisition signature and checksum details
  - id: garnet-provider
    resource: https://github.com/microsoft/garnet/releases/tag/v2.2.11
    title: Official cross-platform Garnet assets
  - id: mysql-provider
    resource: https://dev.mysql.com/downloads/mysql/8.4.html
    title: Official MySQL portable packages
  - id: postgres-provider
    resource: https://www.enterprisedb.com/download-postgresql-binaries
    title: Official EDB binary archive channel
---

# Holat

M9 managed PostgreSQL/MySQL, Garnet, loyiha service profile va logical backup
funksiyalari yaratildi va native qabuldan o'tdi. Apple Silicon va Windows MySQL
bilan, Intel Mac PostgreSQL/Garnet bilan tekshirildi. Intel MySQL capability gap'i
quyida ochiq. Bu public release yoki yangi installer dalili emas.

# Yadro va ma'lumot saqlanishi

- M3 PostgreSQL ma'lumotlari saqlanadi. Project store schema1 ->2 additive migration;
  old project JSON service profile olmasa `project` default, old instance JSON
  engine olmasa PostgreSQL. Old secret references o'zgartirilmaydi.
- `RunningDatabase` enum PostgreSQL/MySQL adapterlarini bir xil workspace boundary
  orqali boshqaradi. Paketlar immutable/shared; data har instance uchun alohida.
  Bir instance ichida loyihalar alohida database/role oladi.
- MySQL8.4.13 ordinary user process, loopback, owned PID/datadir tekshiruvi,
  protected bootstrap SQL, `caching_sha2_password` va pinned local RSA public key.
  Admin credential faqat SecretStore. Project foydalanuvchisida global admin yoki
  boshqa loyiha DB huquqi yo'q. Ownership marker admin-only metadata DB'da;
  Laravel `migrate:fresh` uni o'chirmaydi.
- Baza almashtirish/tiklash oldingi DB va credential'ni saqlaydi. Active binding
  SQLite transaction bilan almashtiriladi. Retained connection UI orqali yana
  faollashtiriladi; project default'ga qaytish binding'ni uzadi, data o'chmaydi.
- Garnet2.2.11 va2.2.10 Redis-compatible provider. Har project uchun alohida process,
  ACL/password, port va data; boshqa project cache instance tanlovi rad etiladi.
  Cache DB1, queue DB0. Default ACL user off; project admin buyruqlarini olmaydi.
  AOF/checkpoint/recovery `FileStream` device bilan ishlaydi. ARM archive'dagi
  Linux-only native I/O library ishlatilmaydi.
- Garnet framework-dependent: managed .NET10.0.15 dependency alohida download,
  verified lease va `DOTNET_ROOT*`; global .NET/PATH o'rnatilmaydi.
- Profile cache `project/file/database/redis`, queue `project/sync/database/redis`.
  Managed contexts/web/worker/terminal'ga qo'llanadi; `.env` avtomatik yozilmaydi.
  Database driver tanlovi loyiha migrations/jadvallari o'rnini bosmaydi.
  Redis uchun Predis kerak: explicit review bilan Composer `^3.7.1`, no scripts,
  no plugins. Bu loyiha composer.json/lock/vendor o'zgarishini foydalanuvchi ko'radi.
  Frontend Node invocation managed `REDIS_*` credentials olmasligi uchun filtrlanadi.

# Backup kontrakti

Managed PostgreSQL/MySQL logical backup: `.dfbackup` ZIP, schema1 manifest,
engine/version/origin project/digest/size va bitta `payload`. Credential yoki loyiha
`.env` fayli archive'ga kirmaydi; SQL data o'zi maxfiy bo'lishi mumkin.

- Bir archive payload1GiB, process5 daqiqa. ZIP path/duplicate/symlink, manifest
  bounds va SHA256 tekshiriladi. Listing faqat kichik header o'qiydi; polling
  har safar barcha SQL payload'ni hash qilmaydi. Import avval private copy qiladi.
- Export yangi faylga atomik yozadi, mavjud fayl ustidan yozmaydi; nom band bo'lsa
  boshqa nom tanlash xabari bor. Import/export shu logical format uchun;
  arbitrary `.sql` yoki physical server directory import'i emas.
- PostgreSQL minimal server artifact'da pg_dump/pg_restore yo'q; EDB18.6 client
  alohida managed package. Faqat kerakli regular member/library/license'lar
  `ZipFiles` orqali olinadi, installer bajarilmaydi. Existing server package
  tanlovi va data directory almashtirilmaydi.
- MySQL InnoDB table/schema/data. View, trigger, routine, event yoki non-InnoDB
  jadval bo'lsa backup rad etiladi; yashirin incomplete backup berilmaydi.
- Restore faqat shu engine va exact server version'ga. Yangi DB + limited role
  yaratiladi, tool shu role bilan ishlaydi. MySQL `--binary-mode`, local-infile off,
  SQL private regular-file stdin, capture false. PG `--no-owner --no-privileges`.
  Password argv/log'ga kirmaydi; private option/pgpass fayli vaqtinchalik.
- Archive SQL bajara oladi: UI explicit trusted archive review qiladi.
  Restore fail/cancel bo'lsa active DB o'zgarmaydi; pending staging row avtomatik
  qayta faollashtirilmaydi yoki data o'chirilmaydi. Recovery/cleanup qo'lda;
  failed data'ni avtomatik delete qilish bu scope'da yo'q.
- DevForge project web/worker avval stopped bo'lishi kerak; tashqi terminal yoki
  uchinchi tomon DB client'larini foydalanuvchi o'zi to'xtatadi. Online backup
  global transaction consistency yoki external workers nazoratini da'vo qilmaydi.
- SQLite/project-owned yoki remote unmanaged DB backup'i bu implementatsiyaga
  kirmaydi; project default oqimi avvalgi konfiguratsiyani saqlaydi.

# Provider va platforma chegaralari

Pinned descriptors/provenance [runtime yozuvida](../../runtime/m9-candidates.json).
MySQL ARM64/Windows arxivlari Oracle detached GPG signature bilan tekshirildi;
Garnet GitHub asset digest, .NET official SHA512, EDB HTTPS + local SHA256 alohida
qayd qilingan. Ularni bir xil authenticity dalili deb talqin qilmang.

Current official MySQL portable package Intel Mac uchun topilmadi. Hozir Intel
Mac PostgreSQL + Garnet bilan ishlaydi; eski MySQL release avtomatik tanlanmadi.
Bu capability gap, universal MySQL support degan da'vo yo'q. macOS minimum package
OS floors, Windows non-ASCII user profile/AppData, signing/notarization va runtime
redistribution/license approval M15 gate'lari bo'lib qoladi. Redis compatibility
faqat bajarilgan Laravel/cache/queue buyruqlari darajasida; Cluster, Sentinel,
Horizon yoki to'liq Redis command conformance tasdiqlanmagan.

# Tekshiruv va reconcile

Local ARM64: MySQL initialize/start/role/backup/staged restore, Garnet ACL/Lua/
SELECT/FLUSH/persistence; Laravel13 cache/lock/queue Lua; parallel PostgreSQL,
MySQL, PHP va ikki Garnet versiyasi o'tdi. Old PG auth `sameuser` guard saqlanishi
uchun restored role/database nomi mos; MySQL32-character role limiti alohida.
Mac socket path limiti private short temporary socket directory bilan hal qilindi.

Local native WKWebView: real IPC backup/restore review va cancel, RU/UZ/EN,
1536x1024 va820x620, dark/light, focus va relaunch o'tdi. Retained-connection
review va focused backend reactivation ham tekshirildi. Final label correction
va output-size monitor frozen-source native qabulida ham bor. File picker OS dialogs avtomat
bajarilmadi; core export/import tekshiruvi alohida. `agent-desktop` mavjud emasligi
sabab mavjud debug-only W3C harness ishlatildi; production'da endpoint yo'q.

Source-guideline review `ProjectServices.tsx`, affected `WorkspacePage.tsx` va CSS
uchun joriy Web Interface Guidelines bilan bajarildi. Existing system shell,
service rows va haqiqiy ma'lumot saqlandi. Generated1536x1024 reference — section
hierarchy/control direction; incidental text/service rows haqiqiy data bilan farq
qiladi, pixel-perfect da'vo yo'q. [Mac screenshot](../evidence/m9/services-macos-light.png),
[Uzbek minimum window](../evidence/m9/services-macos-uz-minimum.png) va
[Windows screenshot](../evidence/m9/services-windows-light.png) ko'rib chiqildi;
qolgan native captures digest bilan qayd etilgan CI artifact'larida.

M10 boshlanmadi. M7.2 Xdebug va M15 system authorization/release gate'lari yopilmadi.

## Windows native birinchi urinish

`e62f2a4` source'da Garnet ACL/Lua/SELECT/FLUSH/restart o'tdi, MySQL identity
check `DatabaseConflict` bilan to'xtadi. Windows MySQL monitor/server fork
qiladi; official `--no-monitor` va `--console` qo'shildi. PID/datadir guard
qisqartirilmadi. `59bc693` native Windows run barcha recovery, Laravel va
WebView2 qabulini o'tkazib tuzatishni tasdiqladi.
Birinchi failure [attempt yozuvida](../evidence/m9/ci-attempts.json) saqlanadi.

# Yakuniy platforma dalili

| Target | Frozen source / native job | Qabul qilingan qism |
| --- | --- | --- |
| macOS ARM64 | `e62f2a4`, [131527132007](https://github.com/jorabekyusupov/devforge/actions/runs/43316891285/job/131527132007) | MySQL/PG/Garnet, real Laravel cache/lock/queue Lua, backup/restore, import isolation/failure guard, retained reactivation, actual WKWebView10 axe/0. |
| macOS Intel | `e62f2a4`, [131527132484](https://github.com/jorabekyusupov/devforge/actions/runs/43316891285/job/131527132484) | PG/Garnet va ikki Garnet versiyasi, Laravel cache/queue, PG staged restore, retained reactivation, actual WKWebView10 axe/0. MySQL paketi yo'q; uning sinovi skip. |
| Windows x64 | `59bc693`, [131532545711](https://github.com/jorabekyusupov/devforge/actions/runs/43318461527/job/131532545711) | MySQL monitor fix, PG/Garnet, Laravel cache/lock/queue Lua, backup/restore, import isolation/failure guard, retained reactivation, actual WebView2 10 axe/0. |

Har native desktop'da RU/UZ/EN katta/minimal light window, English dark theme,
restore review/cancel va real IPC restore, retained review, Tab focus va preference
relaunch tekshirildi. File picker OS dialog'lari avtomatlashtirilmagan. Har uch
native ZIP artifact SHA256'i GitHub metadata digest bilan bir xil; ichidagi backend
va desktop JSON report'lari ham hash qilindi. Native first run umumiy `failure`
holatida qoladi: birinchi Windows job yiqilgan, ikkala Mac job o'tgan. Windows
followup run alohida `success`; failure dalili yashirilmagan.

Core [43316885070](https://github.com/jorabekyusupov/devforge/actions/runs/43316885070)
`e62f2a4` source'da macOS/Windows uchun har birida75 Rust passed/1 credential fixture
ignored,27 frontend,3 locale catalogs/3 locale regressions,6 packaging testi,
Clippy `-D warnings`, native default build va strict OKF24/0/0 o'tdi.
`59bc693` normal non-Windows argument'larini o'zgartirmaydi: Windows-only flags,
private mismatch diagnostics va failure fixture print'i qo'shilgan. Focused local
Clippy va final default-feature Mac build ham o'tdi. Yakuniy Ruscha `_few/_many`
active-project counter so'zlari native capture'dan keyingi locale-only tuzatish;
4 count probe va locale validator bilan tekshirildi, oldingi native report o'sha
matnni tekshirgan deb ko'rsatilmaydi. Passing75/27 suite'lari final push uchun yana
ishga tushirilmadi.

# Code/evidence reconciliation

| Claim | Holat | Dalil/chegara |
| --- | --- | --- |
| Old PostgreSQL/project selections saqlanadi | matching | Additive schema2, default engine/profile deserialization va metadata generation tests; actual PG staged restore barcha3 targetda. |
| MySQL owned process va DB/role isolation | matching / platform change | ARM64 native va Windows followup; Windows monitor disabled, strict PID/datadir verification saqlangan. |
| Project cache/queue va parallel versions | matching | Laravel13 lock/flush/queue Lua; per-project ACL;2.2.10 va2.2.11 parallel, restart persistence barcha3 targetda. RSS/stop-latency budget measured yoki optimized deb da'vo qilinmaydi. |
| Backup activation data saqlaydi | matching | New limited-role DB, failed SQL/shell escape/import isolation ARM/Windows; old generations retained/reactivated. SQLite/unmanaged DB backup implemented emas. |
| Shared cross-platform UI va localization | matching, bounded evidence | Real WKWebView/WebView2,10 axe/0 per target; final Ru plural text probe alohida. Native file dialogs unverified. |
| Intel Mac current MySQL | not implemented | Current Oracle portable binary absent; old release yoki boshqa provider silently selected emas. |
| M15 public/minimum-OS/system-dialog gates | unverified/open | Signing/notarization, redistribution/legal review, Windows non-ASCII user profile/AppData, OS floors va actual elevation dialogs ushbu phase bilan yopilmadi. |

Private local project/database/secret fixtures va isolated GPG agent tozalandi;
local binary default-feature debug build'ga qaytarildi (E2E endpoint feature'i o'chirilgan). App source va public evidence
push qilinadi; yangi distributiv/release bu task bilan nashr etilmaydi. M10
boshlanmadi.
