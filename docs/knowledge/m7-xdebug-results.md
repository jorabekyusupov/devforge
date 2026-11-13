---
type: Development Milestone
title: M7.2 Xdebug debugger va function profiler
description: Project-scoped native Xdebug runtime, DBGp boshqaruvi va bounded Cachegrind profiling dalillari.
status: draft
decision_status: confirmed
tags: [m7, xdebug, debugger, profiler, runtime, desktop]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M12 keyin qolgan M7 ishlarini tugatish topshirigi
  - id: core
    resource: ../../crates/debugger/src/lib.rs
    title: Native debugger crate va settings kontrakti
  - id: provider
    resource: ../../crates/debugger/src/provider/mod.rs
    title: Verified Windows DLL va private macOS source build
  - id: recipe
    resource: ../../crates/debugger/src/provider/recipe.json
    title: Reviewed PHP Xdebug SPC va private tools inputs
  - id: workspace
    resource: ../../crates/workspace/src/debugging.rs
    title: Project ownership commands va lifecycle
  - id: native
    resource: ../../crates/workspace/examples/support/debugger.rs
    title: Actual PHP Laravel HTTP CLI va recovery acceptance
  - id: ui
    resource: ../../apps/desktop/src/pages/DebuggerPage.tsx
    title: Uch tilli debugger va function profile sahifasi
  - id: workflow
    resource: ../../.github/workflows/xdebug.yml
    title: Uch native target uchun acceptance workflow
  - id: previous
    resource: /m7-inspector-results.md
    title: Bajarilgan M7.1 Inspector dalillari
  - id: catalog
    resource: ../../runtime/catalog.json
    title: Active va legacy runtime families
  - id: provider-review
    resource: ../../docs/evidence/m7-xdebug/provider-rebuild-review.json
    title: Primary provider rebuild review
  - id: ci-core
    resource: ../../docs/evidence/m7-xdebug/ci-core.json
    title: Accepted core counters va source
  - id: ci-native
    resource: ../../docs/evidence/m7-xdebug/ci-native.json
    title: Accepted native matrix va artifact digests
  - id: boundaries
    resource: ../../docs/evidence/m7-xdebug/source-boundaries.json
    title: Source va preserved paths comparison
  - id: attempts
    resource: ../../docs/evidence/m7-xdebug/ci-attempts.json
    title: Original CI attempts
---

> Davom yozuvi: ushbu hisobot oldingi accepted source'ning tarixiy dalilidir.
> GET/read-only locals, Intel MySQL, GUI editor va archive-only kabi chegaralarning
> kengayishi [M7–M12 completion](m7-m12-completion.md) yozuvida yuritiladi;
> joriy hosted tekshiruv hali yakunlanmoqda.

# Yakuniy holat

M12 yakunidan keyin qolgan M7.2 funksional qabuldan o'tdi. M7.1 Inspector,
Laravel companion va uning avvalgi dalillari saqlandi. M7 endi request/query/job/
log/dump inspection, application profiling, optional native Xdebug debugger va
function profiler'ni o'z ichiga oladi. M13 boshlanmadi.[^request][^previous]

# Runtime va loyiha kontrakti

Normal loyiha PHP tanlovi saqlanadi. Stopped project uchun reviewed install hamda
Off/Debug/Profile sozlamasi mavjud. macOS stock StaticPHP common exportlari oddiy
Xdebug module uchun yetarli bo'lmagani sabab matching CLI/FPM va Xdebug private
prefix'da source'dan build qilinadi. Studio paired SDK spike'da PDO pgsql yo'q edi;
u universal replacement sifatida olinmadi. Windows official matching NTS VS17
x64 DLL publisher SHA256 bilan tekshiriladi.[^provider][^recipe]

Recipe PHP8.4.27/8.5.12, Xdebug3.5.3, SPC va private tools'ni pin qiladi. SPC
resolve qilgan transitive archive SHA256'lari va source locks provenance'da
saqlanadi. Ularning hammasi oldindan pinned yoki build bit-reproducible degan
claim yo'q. macOS output digest/descriptor OS SecretStore key bilan attestation
qilinadi; executable/resource integrity ishlatishdan oldin tekshiriladi. Inventory
poll faqat receipt/presence'ni o'qiydi. Eski attested SDK'lar recipe yangilanishida
avtomatik o'chirilmaydi.[^provider][^recipe]

Compiler prefix short private temporary directory'da; app data'dagi spaces
build'ni buzmaydi. Rust ownership managed process tree tugagach cancellation'da
ham temporary prefix'ni tozalaydi. Apple CLT Python HTTPS probe TLS protocol
xatosi bergani uchun builder secure HTTPS-only system curl va SHA256 ishlatadi.
Product builder doctor, Homebrew yoki PECL'ni chaqirmaydi. Birinchi source build
uzoq davom etishi mumkin; keyingi loyihalar matching SDK'ni qayta ishlatadi.
Upstream GitHub API rate limit build failure/retry'ga sabab bo'lishi mumkin;
oldingi runtime va project data saqlanadi.[^provider]

Generated project INI mode/trigger/output'ni boshqaradi; project `.env`, normal
PHP pin va Node tanlovi o'zgarmaydi. Frontend Node environment'dan Xdebug qiymatlari
ajratiladi. Stop/quit owned socket, request va launch receipt'ni yopadi; stale
unlocked receipt faollashmaydi. Schema6 settings'ni default Off bilan qo'shadi,
old project records/pins saqlanadi. Old build yangi schema'ni refuse qiladi.
Yangi import/worktree local debug session'ni meros olmaydi.[^workspace][^core]

# Debugger va profiler

DBGp listener authenticated ephemeral loopback port'da. Breakpoint add/remove,
Continue, Step into/over/out, paused request Stop, stack frame va bounded read-only
locals/source mavjud. Control queue16 bilan cheklangan; resume engine ACK'idan
oldin Running ko'rsatib duplicate step'ni Busy qiladi. Source viewer faqat project
ichidagi PHP faylni o'qiydi. Known values va sensitive variable nomlari mask
qilinadi; bu noma'lum developer data'sini avtomatik aniqlash kafolati emas.
Missing/moved breakpoint source Off'ga o'tishni bloklamaydi.[^core][^workspace]

Built-in trigger faqat owned local GET path uchun. Arbitrary eval, variable
mutation, POST builder va external IDE attach ushbu slice'ga kirmaydi. Public
CLI `debug status/source/profile/install/mode` read/config API'dir. Explicit CLI
profile capture native fixture'da Context.debug_trigger bilan tekshirilgan.
IDE integrations keyingi roadmap'da qoladi.[^workspace][^native]

Cachegrind parser self/inclusive vaqt, calls, memory va total/peak summary'ni
ko'rsatadi; top500 functions qidirish, retained history, confirmed delete va
no-clobber raw export mavjud. Raw export source paths/function nomlarini saqlaydi.
UI known values/pathlarni sanitize qiladi.32MiB per-file, history20/128MiB;
supervisor100ms intervalda own prefix uchun32MiB/128MiB/25 files budjetini kuzatib,
limitda tegishli owned process tree'ni restart qilmasdan to'xtatadi. Bu har baytdagi
OS disk quota emas. Linked history/receipt/cache paths refuse qilinadi. Boshqa
process active profile owner bo'lsa CLI status uning history'sini prune qilmaydi.
Failed owned session Debugger sahifasidan Stop qilinib tiklanadi.[^core][^workspace]

# Tekshiruv paytidagi provider rebuild

Normal stock PHP arxivlari testlar davomida upstream'da almashtirildi. be6abd3
ARM64 fresh fixture IntegrityMismatch'da faollashishni to'xtatdi. Primary CLI
object Last-Modified2026-11-13T08:59:22Z edi. Yangi CLI/FPM archive bytes publisher
SHA'lariga mos yuklandi; local PHP8.5.12/xdebug=false/PDO mysql,pgsql,sqlite va FPM
probe o'tdi. Keyin Intel PHP8.4 ham almashgani aniqlandi; barcha8 macOS sidecar
qayta tekshirildi.[^catalog][^provider-review]

Barcha pre-existing descriptor va SHA'lar o'zgarishsiz saqlandi. O'zgargan
PHP8.4 ARM64/Intel hamda PHP8.5 ARM64 uchun alohida staticphp20261113 family va
superseded→replacement mapping qo'shildi. Existing installed pins ishlaydi; eski
missing ID'ga network install refuse qilinadi, verified cached legacy archive
CLI'dan qabul qilinadi. Runtimes'dagi explicit replacement install loyiha tanlovini
o'zgartirmaydi. Unlocked portable manifest active family'ni tanlaydi; explicit
old ID/hash pin saqlanadi. CLI/FPM family va actual PHP patch versiyasi alohida
assert qilindi. URL hali mutable: yana rebuild bo'lsa integrity gate reject qiladi,
yangi review kerak. Immutable feed/distribution gate M15'da qoladi.[^catalog][^provider-review]

# Qabul dalillari

Core source23fedbee495540ec2a92c4009771e71a0287172f,
run43403571569: macOS108 Rust/1 ignored, Windows106 Rust/1 ignored; har OS36
frontend,3 locale regression,6 packaging checks, fmt/clippy va default-feature
native debug/no-bundle build o'tdi. Ikki Unix-only link guard testi son farqini
beradi. Ignored test native credential UI fixture'si; OS dialog qabulini
anglatmaydi. Strict OKF28 hujjat/0 error/0 warning — faqat strukturaviy dalil.[^ci-core]

| Target | Accepted source va native job | Natija |
| --- | --- | --- |
| macOS ARM64 | c858354, [job131832528582](https://github.com/jorabekyusupov/devforge/actions/runs/43409632986/job/131832528582) | Backend/CLI/WKWebView,12 axe/0 |
| macOS Intel | c858354, [job131832019709](https://github.com/jorabekyusupov/devforge/actions/runs/43409476063/job/131832019709) | Backend/CLI/WKWebView,12 axe/0 |
| Windows x64 | eefac76, [job131820182067](https://github.com/jorabekyusupov/devforge/actions/runs/43405940038/job/131820182067) | Backend/CLI/WebView2,12 axe/0 |

Har target actual PHP8.4/8.5 Xdebug load, Laravel13/PostgreSQL HTTP breakpoint,
step/paused Stop, masked locals/source, HTTP/CLI function profiles, scoped export/
delete, cross-project denial, other-project PID preservation, normal PHP/.env/pin
recovery va missing source'dan Off o'tishini tekshirdi. Public CLI success JSON va
active external owner history guard ham o'tdi. GUI genuine IPC bilan live BP add/
remove, profile/filter/delete-cancel, actual budget failure→Stop recovery, ru/uz/en,
minimum window, dark/relaunch'ni tekshirdi.[^native][^ci-native]

Bu assembled acceptance matrix. Core23fedbe'dan keyingi production logic o'zgarmadi:
keyingi diff reviewed macOS catalog data, native test helper, workflow va docs'dir.
Windows normal packages o'zgarmagan; uning qabul source'i alohida ko'rsatilgan.
M7.1 source/report paths Git diff bilan unchanged tekshirildi.[^boundaries]

Mac SDK'lar source6205198 job'larida production builder orqali yig'ildi va keyingi
qabulda cache'dan olindi. Current builder SHA256
41d7e7d6fb136c242b4a01488a9c5d58e058e7f9daa22553bcad68d350086749
producer bilan bir xil. Har final artifact raw ZIP digest'i GitHub digest'iga mos;
report va provenance SHA'lari ci-native.json'da. Private DB/env/SecretStore yoki raw
profile artifactga kiritilmadi. CI cache faqat public runtime store'ni saqlaydi;
marker/receipt va expected checksum aktivatsiyada yana tekshiriladi.[^ci-native]

Hosted ARM content1536x656, Intel1536x942, Windows1536x1024; local ARM1536x1024.
Minimum820x620, ru/uz/en va overflow assertions o'tdi. Hosted ekran chegaralari
illustrative target bilan pixel-perfect tenglik deb ko'rsatilmaydi. Actual PNG'lar
ko'rildi; yangi sahifa existing master va fresh Web Interface Guidelines review
bilan tekshirildi. Native Save dialog avtomatlashtirilmadi; bounded export backend'da
haqiqiy fayl bilan tekshirildi.[^ui][^ci-native]

Final debug/e2e Start metrics ARM36.95/26.74s, Intel151.06/131.55s,
Windows17.53/16.66s bo'ldi. Ular SDK verification va fixture services'ni o'z ichiga
oladi; release latency benchmark emas. Default-feature local CLI va desktop
binary'lari test-support/e2e'siz qayta build qilindi.[^ci-native]

# Saqlangan muvaffaqiyatsiz urinishlar

- Local SPC doctor kutilmagan auto-fix/Homebrew jarayonini boshlagan; aynan task
  process tree to'xtatildi, global tool install tugamadi. Product builder doctor
  ishlatmaydi. Local unauthenticated SPC GitHub403 va keyingi interrupted retry
  production CI builder success deb ko'rsatilmadi.
- Initial native UI resize ikki animation frame kutishda timeout berdi. Bounded
  frame-or-timer ACK qo'shildi; functional/axe assertions saqlandi.
-6205198 macOS core immediate post-Stop remove'da Busy oldi.8 local parallel suite'da
  takrorlanmadi, baseline deb e'lon qilinmadi. Cleanup1s bounded wait oladi; live
  lease Busy assertion va production semantics unchanged. Duplicated/inherited
  handle ehtimoliy sabab, o'lchangan root cause emas; [Rust1.99.1 kontrakti](https://github.com/rust-lang/rust/blob/1.99.1/library/std/src/fs.rs).
- Intel debug Start120s test budjetidan oshdi. Start-only300s budget/elapsed report
  qo'shildi; readiness va security assertions o'zgarmadi. Release backend oldin ham
  o'tgan. Debug hashing xarajati ehtimoliy izoh, profiling proof emas.
- Windows/Intel test persisted BP count2'ni DOM render deb qabul qilib li13'ni juda
  erta qidirdi. Actual failure PNG'da li13 keyin ko'ringan. Driver actual DOM'ni
  kutadi; accepted native retries o'tdi.
- Upstream mutable archive IntegrityMismatch'lari va yangi build review yuqorida
  saqlandi. Old run failure/cancelled holatlari success'ga qayta yozilmadi.[^attempts]

# Kod bilan solishtirish va chegara

| Da'vo | Holat | Source/dalil |
| --- | --- | --- |
| M7.1 inspection/application profiling | Mos | Unchanged Inspector/companion/report, source-boundaries.json |
| Optional verified SDK, modes, schema6, private INI | O'zgargan | debugger provider + projects execution + three native target |
| Breakpoints/steps/locals/source/request Stop | Mos | Native real Laravel/PHP/DBGp va GUI |
| Function profiles, limits/export/delete/isolation | Mos | Cachegrind/store, M2 budget regression, native/GUI12 |
| Legacy pins va reviewed provider rebuild | O'zgargan | Immutable original descriptors, separate families, family/pin/UI tests |
| Uch locale, minimum window va theme/relaunch | Mos | Registry catalogs,36 frontend va three native12/0 |
| Signed release/minimum OS/native credential dialogs | Tekshirilmagan | M15 gate'lari |

M7 functional qabul yopildi. M15'dagi signing/notarization/Authenticode, licensing,
minimum/clean OS, native credential/system-dialog, interrupted-install/disk faults,
update/schema rollback va immutable runtime feed review ochiq. Intel MySQL server
capability gap M9'da qoladi. Laravel11/12 actual acceptance M7.1'dagi old chegara;
bu Laravel13 fixture uni yopmaydi. Public installer yoki runtime release chiqarilmadi.

# Takrorlash

`.github/workflows/xdebug.yml` verified SDK cache bilan normal fixture, native
backend/CLI va debug-only W3C UI'ni ishlatadi. `check.yml` default dispatch all;
`suite=native,native_target=windows-x64|macos-arm64|macos-x64` failed target'ni
alohida qaytaradi. Qabuldan keyingi docs-only push product suite'ni takrorlamaydi. Private local
fixture/DB/env/SecretStore va source-build/download staging own process'lar
qolmagani tekshirilgach o'chirildi; boshqa loyiha data'siga tegilmadi.

[^request]: User: avval M12 to'liq, keyin qolgan M7; bajarilgan M7.1 saqlansin.
[^previous]: Original M7.1 source/job va dalillar o'z tarixiy chegarasida saqlanadi.
[^core]: Bounded protocol/session/parser/store; eval va property mutation yo'q.
[^provider]: OS-specific managed packages va existing owned process adapters.
[^recipe]: Critical source/tool checksum'lar va actual transitive provenance.
[^workspace]: Project revision/consent/lifecycle va common CLI/Tauri boundary.
[^native]: Disposable actual Laravel13/PostgreSQL va real PHP/Xdebug fixture.
[^ui]: Generated target illustrative, existing master va inspected actual native PNG'lar.
[^catalog]: Optional superseded mapping; legacy package identities o'zgarmaydi.
[^provider-review]: Primary publisher checksums, bytes/probes va subsequent native acceptance.
[^ci-core]: Exact source/job log counters va default-feature build statuses.
[^ci-native]: Per-target source/job/artifact/provenance digest mapping.
[^boundaries]: Git path/source comparisons, SDK builder fingerprint va immutable catalog descriptors.
[^attempts]: Original failure/cancellation holatlari va sabablarining alohida qaydi.


# Discovery bilan main integration — keyingi qabul

Discovery bilan birlashtirilganda project schema6 ikki history sabab schema7’ga
additive ko‘tarildi; debugger sozlamalari va discovery ledger birgalikda saqlanadi.
Yetishmagan reviewed Intel PHP8.5 family’lari qo‘shilib, old48 descriptor unchanged
qoldi. Source38847ce actual ARM64/Intel/Windows qabulida M7 aynan avtomatik topilgan
loyihada ishladi; explicit fixture preparation secret binding’larni meros qilmadi.
[Hisobot](../evidence/discovery-m7-integration/README.md) new source/job/hash boundary,
20UI/22axe0 per target va actual current-catalog downloads’ni beradi. Old M7.1 va
SDK builder/recipe source o‘zgarmadi. Yuqoridagi original schema6/source dalili o‘z
tarixiy chegarasida saqlanadi; current main integration yangi qabul bilan bog‘lanadi.
