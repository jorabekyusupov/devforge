---
type: Implementation Report
title: M12 branch workspaces
description: Git worktree asosidagi alohida loyiha muhitlari va data-preserving cleanup.
status: draft
decision_status: confirmed
tags: [m12, worktrees, isolation, database, cleanup, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M12 boshlash topshirigi
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: Workspaces scope va keyingi bosqichlar
  - id: core
    resource: ../../crates/workspace/src/worktrees/mod.rs
    title: Reviewed creation receipts and archive cleanup orchestration
  - id: git
    resource: ../../crates/workspace/src/worktrees/git.rs
    title: Bounded Git metadata with no shell interpolation
  - id: host
    resource: ../../crates/runtime/src/process/host.rs
    title: Bounded host tools with owned process lifecycle
  - id: cli
    resource: ../../crates/cli/src/workspaces.rs
    title: Reviewed workspace JSON CLI and operation leases
  - id: ui-native
    resource: ../../scripts/m12_desktop_smoke.py
    title: Native create archive locale and appearance controls
  - id: core-ci
    resource: ../evidence/m12/ci-core.json
    title: Successful two-platform core jobs and default builds
  - id: native-ci
    resource: ../evidence/m12/ci-native.json
    title: Three successful native jobs and verified artifact digests
  - id: attempts
    resource: ../evidence/m12/attempts.json
    title: Preserved fixture failures and bounded corrections
  - id: local
    resource: ../evidence/m12/local-checks.json
    title: Local suite and native followup scope
  - id: config
    resource: ../../crates/workspace/src/repro.rs
    title: Reused M10 manifest definition and exact runtime pins
  - id: sqlite
    resource: ../../crates/projects/src/workspace_sqlite.rs
    title: Conventional local SQLite profile and WAL-aware backup
  - id: ownership
    resource: ../../crates/projects/src/project.rs
    title: Schema5 and generic Forget guard
  - id: ui
    resource: ../../apps/desktop/src/pages/WorktreesPage.tsx
    title: Localized workspaces and reviewed create cleanup UI
  - id: tests
    resource: ../../crates/workspace/tests/worktrees.rs
    title: Stale review trust dirty ownership and cancellation regressions
  - id: native
    resource: ../../crates/workspace/examples/support/worktrees.rs
    title: Real Laravel database and process isolation acceptance
  - id: git-docs
    resource: https://git-scm.com/docs/git-worktree
    title: Official Git linked worktree remove and branch semantics
---

> Davom yozuvi: ushbu hisobot oldingi accepted source'ning tarixiy dalilidir.
> GET/read-only locals, Intel MySQL, GUI editor va archive-only kabi chegaralarning
> kengayishi [M7–M12 completion](m7-m12-completion.md) yozuvida yuritiladi;
> kengaytma uch native targetda qabuldan o‘tgan; aniq source/job chegaralari o‘sha yozuvda.

# Holat va scope

M12 funksional qabuli yakunlandi: macOS ARM64/Intel va Windows x64 backend,
CLI hamda native UI o'tdi. M13 avtomatik boshlanmaydi. Yangi
public installer yoki signed release bu task scope'iga kirmaydi.

# Yaratish kontrakti

Workspace registered non-bare Git project root'dagi committed history'dan
`git worktree add` bilan alohida checkout yaratadi. New yoki existing local branch,
base commit/branch va empty/clone DB tanlanadi. Source dirty changes ko'chirilmaydi;
commit, source profile, runtime pins va domain review digest bilan tekshiriladi.
Reja checkout/download'dan oldin, hamda package tayyorlangach qayta tekshiriladi.
Implicit promisor fetch `GIT_NO_LAZY_FETCH` bilan o'chirilgan. Remote fetch/push/merge
va branch delete avtomatik emas. Gitlink/submodule bor branch yoki retained module
metadata bilan cleanup bu M12 slice'da unsupported; oddiy Git working tree talab qilinadi.

Checkout app data'dagi owned `worktrees/<id>/tree` ichida. Canonical directory,
Git common/admin metadata va receipt mosligi tekshiriladi. Rust orchestration
Tauri'dan mustaqil; host Git metadata va mutation existing M2 process group/Windows
Job adapter'i, bounded output, timeout va cancellation bilan bajariladi. Git uchun
PHP/Node package talab qilinmaydi, host executable package-verified deb ko'rsatilmaydi;
shell interpolation yo'q, Git hooks va fsmonitor o'chirilgan. Trusted repository
review checkout/filter va tanlangan dependency preparation'ni ruxsatlaydi.

Har workspace yangi Project ID, generated unique domain, serving ports, PHP config,
APP_KEY/.env, DB role/credential va tanlangan Redis/mail service'iga ega. Managed
PHP/Node package binaries umumiy immutable store'dan qayta ishlatiladi; selections
alohida. HTTPS source choice saqlanadi, system hosts/CA elevation avtomatik emas;
existing project HTTPS review va fallback URL saqlanadi.

M10 `Manifest`/pin model source registered profile'dan reuse qilinadi. Target
snapshot app data receipt/configuration'da saqlanadi; repository `devforge.yaml`
ustidan yozilmaydi. Source `.env`, vendor/node_modules va ignored uploads
ko'chirilmaydi. Committed `.env` bo'lsa creation rad etiladi. Dependency setup
Composer no-plugins/no-scripts va npm ignore-scripts bilan. Missing dependencies
bilan yaratish tanlansa project'ni keyin existing Prepare orqali tayyorlash mumkin.
Custom environment secret'lari default ko'chirilmaydi; explicit tanlovda yangi
per-project entries olinadi; custom secret qiymatlari .env'ga yozilmaydi, managed
contexts orqali beriladi. Yangi private .env faqat APP_KEY va managed DB/cache/mail
profile qiymatlarini oladi. APP_KEY, managed mail va Redis credential'lari
source'dan nusxalanmaydi. Hardcoded external transports/services umumiy firewall
bilan bloklanmaydi; isolation standard DevForge-managed contexts uchun.

# DB va cleanup chegarasi

Managed PostgreSQL/MySQL empty DB shu instance'da yangi role/database bilan.
Clone M9 verified logical backup -> private import -> staged restore orqali;
source project stopped va serving mutation guard talab qilinadi. External DB
writers'ni DevForge global nazorat qilmaydi. Engine/version compatibility va
MySQL table/trigger/view/routine cheklovlari M9 kabi saqlanadi.

Managed binding bo'lmasa target local SQLite empty DB ishlatadi. Clone faqat
DevForge-initialized conventional `database/database.sqlite`, canonical no-link path
va mos SQLite .env profile uchun. rusqlite existing0.41.1 `backup` feature'i orqali
WAL-aware snapshot olinadi; unrelated dependency version'lari o'zgartirilmaydi.
Unmanaged/remote DB yoki arbitrary SQLite path clone rad etiladi.

Creation receipt stage/error/project ID'ni durable saqlaydi. Failure/cancel partial
files yoki DB'ni avtomatik o'chirmaydi; user inspection va reviewed cleanup orqali
chiqariladi. Cleanup staged/modified/untracked Git changes va Git lock bo'lsa
rad etadi. Detached HEAD va nested registered project ham cleanup'ni bloklaydi.
Archive stage'dan keyingi retry retained working tree'ning HEAD/branch va dirty
holatini yana tekshiradi. Ignored files ham butun checkout bilan `retained` papkaga arxivlanadi;
keyin faqat shu missing worktree Git registry entry'si chiqariladi. `--force`,
global prune, branch delete va DB drop yo'q. Credential, DB generations, backups
hamda archived files retained; disk reclaim/purge bu bosqichda implemented emas.
Archive `.git` file retired admin entry'ga qarashi mumkin: bu active Git checkout
emas; branch common repository'da saqlanadi, files alohida qaytarib olinadi.

Generic Forget ProjectStore darajasida ham managed child uchun bloklangan.
Schema4 ->5 additive optional `worktree` field; old records read qilinadi,
older builds schema5'ni rad etadi. Downgrade/schema rollback M15 gate'i ochiq.

# UI va tekshiruv

Workspaces sidebar route, project filter, active/archive rows, details, real project
link va folder controls bor. Create/cleanup alohida reviewed dialogs, trust/consent,
backend errors va cancellation bilan. RU/UZ-Latn/EN catalogs mavjud. Existing MASTER
system typography/teal/light-dark/window floor saqlandi; four UI skills scoped
workflow'da ishlatildi. Design-system searches operational page'ga mos kelmagan
landing suggestions berdi; ular palette/font/shell o'zgarishi sifatida olinmadi.
[Stable target](../../design-system/devforge/reference/workspaces-target.png)
illustrative1536x1024. Actual backend states va established sidebar/flat controls
targetdagi dekorativ logo/gradient o'rnida saqlanadi; pixel-perfect claim yo'q.

Local final core96 Rust passed/1 credential fixture ignored;31 frontend,3 locale
catalogs/3 locale regressions va6 packaging checks o'tdi. Clippy all-targets va
test-support examples warning-free. Actual macOS Laravel/PG/MySQL/SQLite, Redis,
mail, parallel HTTP/queue va archive backend qabulidan o'tdi; existing-branch,
SQLite empty/Prepare va real CLI alohida followup report'larda. Native WKWebView
create/review/cancel/dirty refusal/archive, RU/UZ/EN light/minimum820x620, dark va
relaunch15 axe case/0 finding bilan o'tdi. Ilk system-theme screenshot'lari dark
bo'lgan; harness explicit light qilib, checkbox/details alignment tuzatildi va
qayta ko'rildi. `agent-desktop` CLI mavjud emas; existing debug-only W3C harness
ishlatildi. External folder opening avtomatlashtirilmadi. Source review yangi
WorktreesPage, App routing va scoped CSS uchun current Web Interface Guidelines
bilan bajarildi. Local main/followup source chegarasi evidence JSON'da; final
immutable CI platform/source reconciliation quyida saqlanadi.
M7.2 Xdebug, Intel MySQL capability gap va M15 minimum-OS/native credentials/system
dialog/signing/licensing/recovery gate'lari ushbu hisobot bilan yopilmaydi.

# Kod bilan bosqich solishtirishi

| Holat | Da’vo | Source va dalil chegarasi |
| --- | --- | --- |
| Mos | Alohida committed worktree, domain va project identity | `worktrees/mod.rs`, `git.rs`; new/existing branch va occupied branch refusal native followup, actual HTTP3 project. |
| Mos | M10 profile/pins va source fayllari saqlanishi | `manifest_definition`, source head/.env assertions, actual CLI review/create/archive. Repository YAML ustidan yozilmaydi. |
| Mos | Empty/copy DB va limited credentials | Existing PG/MySQL logical backup/staged restore; actual data/permissions/parent checks. SQLite WAL unit va real wizard clone/empty-Prepare followup. |
| Mos | Redis/mail va parallel managed services | Per-project cache/queue/mail IDs, real cache/queue/mail/HTTP, source PID survives child cleanup. |
| Mos | Archive cleanup va reachable branch | Full checkout rename, missing-path Git removal, dirty/detached/nested/submodule/ownership guards. Current reviewed HEAD/branch metadata retained; no force/global prune/drop. |
| O‘zgargan | Schema4 ->5 | Optional project `worktree` field, actual schema4 migration unit and generic Forget guard. Older builds refuse schema5. |
| Mos | Localized shared desktop UI | Three catalogs,31 frontend, native15 axe/0 and explicit light/dark/relaunch. External folder launch is not automated. |
| Amalga oshirilmagan | Archive purge/reactivation, submodule lifecycle va remote DB copy | Files/credentials/DB are deliberately retained; branch resume uses a new worktree. These are not silently simulated as cleanup/deletion. |
| Tekshirilmagan | Public distribution va M15 gates | Minimum OS, native credential/system-dialog, signing/licensing, interrupted-install/schema rollback remain separate. |

# Yakuniy platforma va source dalili

Core source `6f68bd4cae13725ab82b1d967a15834b003033fd`,
[run43362034589](https://github.com/jorabekyusupov/devforge/actions/runs/43362034589):
macOS job131677557703 va Windows job131677558144 har birida96 Rust passed/1 ignored,
31 frontend,3 catalogs,6 packaging, fmt/clippy/typecheck, default desktop build va
strict OKF27/0/0 bilan o'tdi. [Core JSON](../evidence/m12/ci-core.json) actual log
hash/counters saqlaydi; ignored credential fixture M15 native credential qabulini
almashtirmaydi.

Initial native run43362038130 uch targetda HTTP fixture libc DNS uchun *.localhost
resolve qila olmagan. [Attempts](../evidence/m12/attempts.json) failure'ni saqlaydi.
`356555fb9e01cd76f13649ce9e54697144f9503c` faqat reqwest test-client loopback mapping
va attempt qaydini qo'shadi: URL/Host header saqlangan, Caddy routing actual. Product
serving yoki system DNS o'zgarmagan. Core qayta bajarilmadi; native matrix to'liq
qayta tekshirildi. `git diff 6f68bd4 356555f` source chegarasini ko'rsatadi.

Native [run43363941554](https://github.com/jorabekyusupov/devforge/actions/runs/43363941554)
source `356555f` barcha3 job'da success:

| Target | Job | Real backend / CLI / GUI | Axe | Katta viewport actual |
| --- | --- | --- | --- | --- |
| macOS ARM64 | 131683780693 | PG/MySQL/SQLite, Redis/mail, parallel services va archive o'tdi | 15 /0 | 1536x656 |
| macOS Intel | 131683781746 | PG/SQLite, Redis/mail, parallel services va archive o'tdi; current MySQL capability yo'q | 15 /0 | 1536x942 |
| Windows x64 | 131683781086 | PG/MySQL/SQLite, Redis/mail, parallel services va archive o'tdi | 15 /0 | 1536x1024 |

Har targetda exact820x620 minimum, RU/UZ-Latn/EN light, explicit dark va preference
relaunch tekshirildi. Hosted Mac screen cap requested1536x1024 ga teng deb
ko'rsatilmaydi; local Mac full1536x1024 alohida o'tdi. UI source/target comparison,
[Mac light](../evidence/m12/workspaces-macos-light.png),
[Uzbek](../evidence/m12/workspaces-macos-uz.png) va
[Windows](../evidence/m12/workspaces-windows-light.png) screenshot'lari ko'rib chiqildi.
Native folder launch va OS credential/system dialogs avtomatlashtirilmadi.

| Artifact | ID | ZIP SHA256 |
| --- | --- | --- |
| m12-macos-arm64 | 14010455348 | `05c9ddb1a6f65f418f5eafdcfc7d2130ca4a5a3e9b881bb3c411db7ece901bf7` |
| m12-macos-x64 | 14010672839 | `2df60b95dc2260d233d1de9769b7d0b2859700c8dcc2014468433728c0a3541a` |
| m12-windows-x64 | 14009344832 | `4479bfd10f6d9f0b747befbfa584329a897c06e6c0b2abd3094ceb5774fc8d49` |

Uch ZIP digest GitHub metadata'ga aynan mos. [Native JSON](../evidence/m12/ci-native.json)
har job uchun5 report va ularning individual hashes'ini saqlaydi: main backend,
existing branch, SQLite empty/Prepare, real CLI, real WebView UI. Raw mail/database,
credentials yoki workspace tree artifact sifatida upload qilinmadi.

Foydalanuvchi navbatdagi ish sifatida M7'ning qolgan qismini yakunlashni so'radi;
bajarilgan M7.1 qayta yozilmaydi. M13 avtomatik boshlanmaydi.

 Local default-feature CLI release va Tauri debug/no-bundle build
o‘tdi; test-support/E2E feature’lari default binary’dan chiqarildi. Bu yangi installer
yoki signed release emas. Yakuniy publication uchun passing product suite qaytarilmaydi.

Private local fixtures va owned artifact download staging tozalandi; public report
va tanlangan screenshot’lar saqlandi. Default-feature binary’lar tiklangan.
