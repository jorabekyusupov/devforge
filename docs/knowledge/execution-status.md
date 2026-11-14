---
type: Project Status
title: DevForge bajarish holati va E00 inventari
description: Joriy vakolat, source baseline, mavjud capability va platforma dalillari, ishlar navbati va maqsadli tekshiruv ledgeri.
status: draft
decision_status: working-direction
tags: [execution, baseline, evidence, e00, delivery]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: Birlashtirilgan ishlar va maqsadli test siyosati
  - id: agents
    resource: /m13-external-agents-plan.md
    title: Tashqi agentlar kontrakti va scope
  - id: completion
    resource: /m7-m12-completion.md
    title: Oldingi funksional qabul va source chegaralari
  - id: windows
    resource: /windows-compatibility.md
    title: OS edition va haqiqiy Windows10 qabul chegarasi
  - id: workspace
    resource: ../../crates/workspace/src/lib.rs
    title: Mavjud shared commands va lifecycle
  - id: shell
    resource: ../../apps/desktop/src-tauri/src/workspace.rs
    title: Desktopdagi mavjud owner va command serialization
  - id: ci
    resource: ../../.github/workflows/check.yml
    title: Amaldagi CI checks
---

# Vakolat va holat sanasi

2026-11-13: user birlashtirilgan reja bo'yicha ishlarni boshlash va har bosqich
yakunida push qilishni topshirdi. Bu oldingi planning-only holatini execution uchun
almashtiradi; oldingi hujjatlardagi planning tarixini o'chirmaydi. Integrator qabul
mezoni yopilgan bosqichni tegishli tekshiruv/evidence bilan commit va push qiladi.
Public release, ishlab turgan boshqa loyihalar yoki system security sozlamalarini
o'zgartirish umumiy push vakolatidan kelib chiqmaydi. Ichki chat/model/provider UI
scope'da emas. Shartli D-backlog qarorlari hali o'z boshlash shartiga ega.[^plan][^agents]

E00 source/readiness inventari integrator tomonidan review qilindi; strict OKF va Git diff
tekshiruvi bilan inventory bosqichi yopiladi. Publication commit Git tarixida. Bu sahifadagi keyingi ishlar
jadvali boshlang'ich queue, boshqa agentning parallel yozayotgan kodiga runtime
qabul bermaydi. Bosqich holati exact yangi source/test natijasi bilan yangilanadi.

# Baseline va saqlanadigan ishlar

Source baseline: `111526dbe3940c66436edd28546adaff08dfc568`, main, 2026-11-13.
Inventory boshida dirty scope: knowledge index/log/roadmap/conversation va yangi
integrated-execution-plan hamda m13-external-agents-plan. Bu rejalashtirish ishlari
saqlanadi; product source baseline deb ularni commit qilingan deb ko'rsatmaymiz.
Handoff/pushda integrator current branch/remote SHA va reviewed pathlarni qayta
tekshiradi. Bu inventar remote CI, publication yoki remote branch holatini qayta
tekshirmadi.

Manifests: `Cargo.toml` modular Rust workspace; `apps/desktop` React/TypeScript,
Tauri shell; SQLite va native credential adapter. Exact versiyalar manifest va
lockfilelarda qoladi. Existing package identity/project bindinglari yangi manager
uchun o'zgartirilmaydi.[^workspace][^shell]

Current local host read-only inventory: macOS27.2.1 build26B131, arm64. `codex`
executable PATH'da mavjud; bu actual MCP client connection yoki client versiyasining
qabul dalili emas. `gh` mavjud. `claude` va `agent-desktop` PATH lookupda topilmadi;
bu appning butun hostda mutlaqo yo'qligi degani emas. GUI skill/permission inventari
UI vazifasi boshlanganda AGENTS bo'yicha bajariladi. Hozir GUI ishlatilmadi.

# Mavjud imkoniyat va saqlanadigan source

| Capability | Mavjud source | Dalil va navbatdagi ish chegarasi |
| --- | --- | --- |
| Typed shared commands, project lifecycle | `crates/workspace/src/lib.rs`, `apps/desktop/src-tauri/src/workspace.rs` | Implemented; desktop hozir global mutex/cancel slot ishlatadi. M13.1 multi-client owner/operations yangi scope, current serializationni bug deb atamaymiz. |
| Runtime install/integrity/process ownership | `crates/runtime/src/package.rs`, `crates/runtime/src/process/` | Implemented staged activation, cancel/leases/readiness/bounded logs. E07 katalog lifecycle/offline qatlam, qayta installer emas. |
| Registry/discovery/runtime context | `crates/projects/src/project.rs`, `crates/projects/src/discovery.rs`, `crates/workspace/src/discovery.rs` | Implemented identity/suppression/project selection. D01 multi-root shartli qo'shimcha. |
| Serving/domain/HTTPS | `crates/serving/src/`, `crates/workspace/src/system_setup.rs` | Implemented reviewed hosts/trust va narrow helper. M13 API va actual production dialog acceptance alohida. |
| DB/cache, backup/restore | `crates/projects/src/database.rs`, `crates/projects/src/backup.rs`, `crates/workspace/src/services.rs` | Implemented engine-specific binding, staged restore. E05 inventory/E06 activation'siz drill va optional retention/schedule yangi qatlam. |
| Setup/PHP diagnostics | `crates/workspace/src/setup.rs`, `apps/desktop/src/components/ProjectSetup.tsx` | Implemented M8 wizard/preparation. E01 inson uchun task checklist; ikkinchi wizard emas. |
| Reproducible config | `crates/projects/src/manifest.rs`, `crates/workspace/src/repro.rs`, `crates/workspace/src/configuration.rs` | Implemented pins/secret refs/review/journal/YAML editor. E02 expected/actual UI; E03 Composer bindingni to'liq mustahkamlash. |
| Inspector/mail/log | `crates/inspector/src/`, `packages/laravel-inspector/` | Implemented capture/log/mail. M13.5 scoped projection/correlation/wait/artifacts; yangi collector qayta yozilmaydi. |
| Xdebug/profiler | `crates/debugger/src/`, `crates/workspace/src/debugging.rs` | Implemented existing primitive-local revision API. User expressions/eval current capability emas. |
| Workspaces | `crates/workspace/src/worktrees/` | Implemented reviewed lifecycle, archive/reactivate/purge va staged external copy. M13.6 faqat facade/adapters va zarur yangi acceptance. |
| CLI | `crates/cli/src/` | JSON chiqishlari mavjud, unified versioned envelope/help/owner bridge yangi M13.2 scope. |
| Desktop/i18n | `apps/desktop/src/`, `shared/locales/` | Implemented RU/UZ/EN registry. E01/E02/E05 va M13.8 UIlar mavjud design-system va task-specific installed UI skill bilan. |
| External agent API | [Manba reja](m13-external-agents-plan.md) | Baseline'da manager/JSON automation facade/MCP implemented emas; 16 oilaning mappingi M13.0da freeze qilinadi. |
| Sharing/deploy/update | [Roadmap](post-mvp-roadmap.md) | M14/M15 proposed; mavjud internal installer auto-update yoki signed public release emas. |

Jadval source inspectionni bildiradi. Actual runtime pass tarixiy artifact bilan
cheklangan; har bir row bugun native rerun qilindi degani emas.[^completion]

# Platforma va artifact evidence inventari

| Target | Saqlangan dalil | Current qabul chegarasi |
| --- | --- | --- |
| macOS15 ARM64 | Native source08dd836, run43453644852/job131982029879; `docs/evidence/completion/ci-native.json` | Historical backend/CLI/WKWebView functional pass. Current local macOS27.2.1 va minimum13.5 qabuli o'rniga o'tmaydi. |
| macOS15 Intel | Native source08dd836, run43453067442/job131980050440; shu matrix | Historical Intel functional pass. Minimum OS/production credentials/signed upgrade ochiq. |
| Windows Server2025 x64 | Sourcef13c106, run43457642556/job131995900378; shu matrix | Historical ordinary-token native backend/WebView2. Windows10/11 desktop proof emas. |
| Windows installer | Installer sourcebb763e6, harness661b76b, run43474984058/job132056387771; `docs/evidence/windows10/ci-installer.json` | Historical success, installer SHA256 `af159ea75aa99b0207403a6756442e3bb31118fe930c00e209f469ca5ef5734f`; `windows10_native_verified:false`. |
| Windows10 22H2/19045 x64 | Preflight/NSIS floor va [compatibility](windows-compatibility.md) | Actual desktop/runtime host avvalgi qaydda yo'q; bu inventory yangi hostni aniqlamadi. M15 gate unverified. |
| Windows11 desktop | Installer policy qabul qiladi | Server natijasidan actual Win11 desktop pass olinmaydi; actual host/job kerak. |
| Linux/Windows ARM | Baseline supported matrixga kiritilmagan | D07 explicit future scope. ARM emulation native x64 support sifatida ko'rsatilmaydi. |

Provider exception: Garnet/.NET10 Windows10 Home/Pro22H2 uchun umumiy vendor support
va'dasi yo'q; LTSC/Enterprise lifecycle alohida. PHP/Node/MySQL/PG prerequisites,
loader, credential/Job/ACL/trust actual targetda tekshiriladi. Bu inventar eski
vendor ma'lumotini yangi live verification deb ko'rsatmaydi; provider tanlash yoki
release vaqtida rasmiy talablar yangidan tekshiriladi.[^windows]

Production native credential roundtrip test `crates/platform/src/lib.rs`da ignored.
Native E2E FixtureSecrets real OS credential service va user authorization dialogini
isbotlamaydi. Settings/project schema migrationlari mavjud, lekin butun appning
versiyalararo update/rollback qabuli hali yangi M15 ishidir.

# Qaror va tashqi dependency registeri

| ID | Holat/owner | Qaror yoki resurs |
| --- | --- | --- |
| R01 | Required, Architect/Tech Lead | M13.0da facade schema, grants, read/live/persisted freshness, side-effect va ownership/lifecycle freeze. |
| R02 | Required, Architect + Engineer | M13.1 headless launch/IPC peer validation tajribasi; always-on daemon oldindan majburiy emas. |
| R03 | Required, QA/platform | Actual Win10 19045, Win11 desktop, minimum macOS13.5 ARM64/Intel hostlarining mavjudligini aniqlash; hozir tayyor deb belgilanmagan. |
| R04 | Required for release, release owner | Signing identities/key custody/rotation, notarization/Authenticode va redistribution/legal review; mavjudligi tekshirilmagan. |
| R05 | Working proposal, BA/Teach/QA | Birinchi pilot: aralash macOS/Windows Laravel jamoasi; HTTP first-success, DB/queue/mail manifestga qarab. Haqiqiy testerlar hali tayinlanmagan. |
| R06 | Working proposal, Tech Lead/QA | Codex + ikkinchi vendor actual MCP client + terminal JSON fallback. Local Codex executable bor; versiya, client2 va actual connection acceptance hali yo'q. |
| R07 | Contract clarification, Architect | Bounded evaluate existing primitive-local API'ni anglatadi; haqiqiy expressions alohida execution grant/design qarorisiz qo'shilmaydi. |
| R08 | Conditional, BA | D01–D07 boshlash shartlari pilot/real repo/provider/business qarori bilan; umumiy implementation vakolati shartlarni avtomatik yopmaydi. |

R03/R04 release target acceptance'ini to'sishi mumkin; schema, core, scoped tests,
docs va independent implementationni to'xtatmaydi. Blocking holatning egasi va
talab qilinadigan external evidence har stage handoffda saqlanadi.

# Boshlang'ich work queue

Holatlar ushbu E00 inventory topshirilishidagi dependency readiness, product completion emas.
Parallel agent boshlangan yoki tugagan itemni integrator evidence bilan yangilaydi.

| Work ID | Inventory holati | Old shart / navbatdagi qabul |
| --- | --- | --- |
| E00 | integrated | Inventory/decision/test map: strict34/0/0, source push99d6a9e; remote main tasdiqlandi. |
| M13.0 | in_progress | E00 integratsiyasi; 16 oilaning contract mappingi, schema fixtures va lifecycle ADR. |
| M13.1 | in_progress | M13.0; native single-owner/auth/ID/retry/cancel/Quit/headless acceptance. |
| M13.2 | planned | M13.0 schema parallel, live CLI M13.1dan keyin. |
| M13.3 | planned | M13.0 projection fixture parallel; actual client live owner/CLI contractdan keyin. |
| M13.4 | planned | Owner/grants/versioned API; adapterlar start/stop→runtime→env→DB→HTTPS reviewed slices. |
| M13.5 | planned | Read projection M13.0dan parallel mumkin; integrated scope/freshness/health/mail qabul. |
| M13.6 | planned | Execution/debug/workspace kontrakti, actual isolated scenario va preservation. |
| M13.7 | planned | HTTP auth/origin/revoke/version; remote bridge alohida M14. |
| M13.8 | planned | Frozen contracts ustiga connection/grants/audit UI, locale/native UX. |
| M13.9 | planned | M13 slice evidence reconciliation va applicable client/OS matrix. |
| E01 | ready | E00/M8 source, human first-success checklist va UI routing; M13ni kutmaydi. |
| E02 | ready | Existing M10 plan/health facts ustida expected/actual scope; live facade availability alohida. |
| E03 | validated | [Exact Composer source/focused qabul](composer-binding-results.md); native target followup ochiq. |
| E04 | in_progress | Current-state/tooling/README drift va contributor runbook, product tests talab qilmaydi. |
| E05 | in_progress | Ownership/resource kontrakti; read-only storage inventari, keyin cleanup preview. |
| E06 | planned | E05 va staged DB ownership; activation'siz drill, keyin retention/scheduling. |
| E07 | planned | E03 exact bindings va trust policy; catalog/offline lifecycle. |
| E08 | planned | M13.6 execution, real profile talabi; scheduler/Reverb, keyin Horizon. |
| E09 | ready | Reproducible release workload baseline dizayni; optimize faqat actual o'lchovdan keyin. |
| E10 | ready | Declared Laravel11/12/13/PHP/provider meaningful matrix dizayni; actual hostlar ajratiladi. |
| M14.1 | planned | Project/site/operations kontrakti; IDE/DB adapters. |
| M14.2 | planned | Verified bitta tunnel provider, target/access/expiry/revoke. |
| M14.3 | planned | Tunnel stop/Quit/crash/restart/private endpoint exposure test. |
| M14.4 | planned | Remote agent bridge talab qilinsa TLS/auth/revoke; local M13dan mustaqil. |
| M15.1 | planned | All metadata store inventory va cross-version migration/recovery. |
| M15.2 | planned | Actual credentials/system dialogs, ordinary-token lifecycle. |
| M15.3 | planned | Actual network/disk/permission/interruption/recovery faults. |
| M15.4 | planned | R03 actual minimum/current OS matrix. |
| M15.5 | planned | R04 signing/licensing/provenance/SBOM/key custody. |
| M15.6 | planned | Updater signature + OS signing + schema/data recovery. |
| M15.7 | planned | Frozen RC applicable final matrix; earlier unchanged scope evidence reuse. |
| D01 | conditional | Multi-root real-user demand, identity/watch/conflict scope. |
| D02 | conditional | Real pnpm/Yarn/Bun repository/lockfile need. |
| D03 | conditional | Scout/search use case va bitta provider feasibility/license. |
| D04 | conditional | Deploy target/value/authorization tanlovi. |
| D05 | conditional | External DB masking schema/policy talabi. |
| D06 | conditional | Business/support/pricing research; account/subscription avtomatik emas. |
| D07 | out_of_current_scope | Linux/Windows ARM/Docker-first/plugin marketplace explicit talab bilan qayta ko'riladi. |

# Maqsadli tekshiruv va natijani qayta ishlatish

E00 uchun product code o'zgarmadi: yangi full suite, runtime download, native app,
installer yoki CI ishga tushirilmadi. Integrator strict OKF + scoped diff bilan docs
stage'ni tekshiradi. Validatordan o'tish source/runtime truth yoki human review emas.

Oldingi suhbatdagi local135 Rust/44 frontend, boshqa tooling natijalari continuation
ma'lumoti; bu E00 agenti tomonidan fresh run emas. Source hash/scope mosligi ledgerga
yozilmaguncha ularni yangi implementationning pass'i qilib ko'chirmaymiz.

| Keyingi scope | Birinchi tanlanadigan tekshiruv | Qamrovni kengaytirish sababi |
| --- | --- | --- |
| M13.0 contracts | Yangi serialization/unknown/version/error fixtures; changed DTO consumers | Shared Command/DTO behavior o'zgarsa direct consumers. |
| E03 Composer | projects migration/execution, manifest resolve, workspace repro export | Binding/schema ta'siridagi CLI/terminal/native selection. |
| E04 docs | Strict OKF va relevant diff/link | Validator kodi o'zgargandagina validator unit suite. |
| E01/E02 UI | Affected Vitest/typecheck/locales va relevant native task | Shared hook/bridge/DTO yoki keyboard/native state o'zgarsa affected consumers. |
| M13.1 owner | Targeted IPC/lease/auth/ID/retry/cancel/crash fixtures | Changed OS adapter actual native acceptance; keng core ta'sir bo'lsa bitta broad suite. |
| E09/E10 harness | Fixture/config inspection, kichik representative smoke | Actual workload/compatibility expectationni yopish uchun declared target. |

Har run ledgeri source/tree hash, changed paths, target/OS/build, tool/fixture version,
command, result, artifact, reuse/invalidation sababini saqlaydi. Bir xil testni har
agent takrorlamaydi; shared integration qabulini integrator bir marta bajaradi.
CI majburiy checks saqlanadi, local scope siyosati ularni chetlab o'tishga ruxsat emas.[^ci]

# E00 reconciliation va handoff

| Claim | Reconciliation |
| --- | --- |
| M7–M12 existing behavior | matching source inventory + historical evidence; current native acceptance deb ko'rsatilmaydi. |
| M13/M14/M15 | planned; yangi manager/MCP/sharing/updater ishlayapti degan da'vo yo'q. |
| Execution/push authorization | changed: current user planningdan implementatsiya va har accepted stage pushga o'tishni topshirdi. |
| OS/platform/release | unverified qismlar ochiq; current local arm64 host metadata read-only tekshirildi. |
| E00 exit | inventory/decision register/test strategy/ready queue prepared; integrator documentation validation va publication evidence bilan yopadi. |

Root integrator indeks/log/provenance va exact reviewed-path commit/pushni boshqaradi.
E00 agenti boshqa fayllarni o'zgartirmadi, Git mutation qilmaydi.

[^plan]: [Birlashtirilgan reja va work IDlar](integrated-execution-plan.md).
[^agents]: [Tashqi agentlarning mavjud va yangi imkoniyatlari](m13-external-agents-plan.md).
[^completion]: [Tarixiy native/source matrix](m7-m12-completion.md).
[^windows]: [Windows/edition cheklovlari](windows-compatibility.md).
[^workspace]: [Shared workspace core](../../crates/workspace/src/lib.rs).
[^shell]: [Desktop serialization va native adapters](../../apps/desktop/src-tauri/src/workspace.rs).
[^ci]: [Amaldagi CI](../../.github/workflows/check.yml).

# TP01 — Docs-only CI qamrovi

Amaldagi check matrix job nomlari saqlandi. Docs-only allowlist uchun faqat scoped
CI selector regression va strict OKF ishlaydi; code/tooling/unknown/mixed/manual
ozgarishda oldingi barcha product checks qoladi. Push before va PR base farqi,
rename/deletion/new branch/missing history fail-open full tanlanadi.
`scripts/ci_scope.py` uchun 8 focused unittest, actual temporary Git diff va
GITHUB_OUTPUT integration agent tomonidan o'tdi. Workflow YAML/gating va diff review
o'tdi. Remote docs-only policy run43486644920/source0089b2f macOS va Windows joblarida
success: selector/8test/strict OKF o'tdi, product test/build steps skipped ekanligi
job-step natijalari bilan tekshirildi. Code-change full branch run6940822 alohida.
Product tests lokal qayta bajarilmadi, chunki bu ozgarish app behavior'iga tegmaydi.

# E04.1 — Developer hujjatlari

README current-state/MySQL/desktop kirishlari sourcega moslandi; CONTRIBUTING
scoped tests, isolated fixtures, ownership, evidence reuse va UI routing playbook
qo'shildi. Validator qo'llanmasidagi no-CI/runtime-not-created eskirgan da'volari
tuzatildi; sanali tarixiy natijalar saqlandi. Uch hujjatning36 local link targeti va
scoped whitespace review agent tomonidan o'tdi; integrator diff review qildi.
E04.1 documentation slice validated; E04ning termin/glossary va actual RU/UZ/EN
user pilot qismi planned. Product code/test/build bu slice uchun bajarilmadi.

# M13.0.1 — Control contract foundation

`crates/control` (devforge-control) qo'shildi: api_version1 envelope/ID, canonical
registered binding va scoped resource containment; mustaqil scopes/expiry/revoke;
project/action/revision/digest/expiry review; client/project/key/digest idempotency;
operation state/cancel/freshness. [ADR](../architecture/control-plane.md).
Embedded inventory16oila/153operation uchun core/new/unsupported, scope/effect/review
policy saqlaydi; hammasi available:false, live tools deb e'lon qilinmaydi.

Scoped validation: `cargo test -p devforge-control --test contracts`13passed;
`cargo fmt -p devforge-control -- --check` va
`cargo clippy -p devforge-control --all-targets -- -D warnings` o'tdi.
Bu framework-independent kontrakt qabuli; macOS/Windows authenticated IPC yoki
actual MCP client/native acceptance emas. Cargo lockda faqat local crate qo'shildi,
uchinchi tomon dependency versiyalari o'zgarmadi. Existing runtime testlari rerun qilinmadi.

M13.0.1 validated; toliq M13.0 hali in_progress. M13.0.2da per-tool typed payload
schemas, bounded pagination/output, adapter dispatch/error mapping va Rust–TS contract
sync qoladi. M13.1 live owner gate yopilmadi. Plan mavjudligi manager/MCP bor degani emas.

Mustaqil QA reviewdan keyin non-live policy inventory tuzatildi: project code
bajarishi mumkin bolgan start/restart/setup/create/select/capture amallari ExecutionTest
grantini ham talab qiladi; Inspector companion install Xdebug SDK installidan ajratildi.
Read-plan va apply effect/reviewlari, mail delete effekti va family/operation union
consistency regressiyalari qo'shildi.13test final inventoryga tegishli; old10test
natijasiga emas. Bu live authorization/IPC acceptance degani emas.

# E03 handoff

[Composer natijasi](composer-binding-results.md) schema8 va barcha aniqlangan
consumerlarni, initial fixture failure va reviewda topilgan Redis caller fixni qayd
etadi. Local focused projects12/manifest5/repro4, invocation1/worktree1 va scoped
clippy; late fixlarda faqat invalid scope rerun. Real PHP/UI/Windows/Intel native
qabul yangidan da'vo qilinmaydi. Source push alohida E03 commitda.

# E04.2 — Task runbook va uch tilli atamalar (2026-11-14)

[Birinchi loyiha](../guides/first-project.md) mavjud M8/Workspace/Configuration/Inspector
controls asosida HTTP→yangi Inspector event, runtime/secret/port konflikt, conditional
DB/queue/mail va cancellation retained state oqimini tushuntiradi.
[Atamalar](../guides/terminology.md):43 katalog key/129 RU/UZ/EN label exact source
mosligi va46local link tekshirildi, scoped diff o'tdi. READMEga kirish havolalari va
current schema8 Composer chegarasi qo'shildi. Native user pilot yoki yangi checklist
UI bu docs bilan implemented/verified deyilmaydi. E04 materiallar qismi integrated;
ona tilidagi user review/pilot natijalari hali unverified. Product suite qayta bajarilmadi.

Publication: E00 99d6a9e, TP01 6940822, E04.1 0089b2f, M13.0.1 5e77a2a va
E03 ae62902 origin/main'ga push qilingani remote natija bilan tekshirildi. Code CI
runlari alohida source'ga bog'liq; green docs-only CI native product qabuli emas.

# M13.0.2 — Typed projections va checked wire artifacts (2026-11-14)

16oila uchun strict read-only projection DTO/schema va153operation uchun typed/planned/
unsupported mapping qo'shildi.16projection typed, qolganlari explicit unavailable;
live handler yo'q. Prepared read grant, canonical binding, safe revision, cursor va
operation owner/original scopesni tekshiradi; output original project/resource bilan
qayta tekshiriladi. ReadBudget capped counting writer bilan row/byte/cooperative timeout
limitini saqlaydi. Unicode scalar text limiti standard schema maxLengthga mos; wire
vaqt Unix ms, safe integerlar JS number aniqligi doirasida.

`cargo test -p devforge-control --test tool_contracts`:11passed, final QA fixlardan
keyin; scoped clippy o'tdi. Generated JSON/TS drift shu suite'da tekshirildi. Root
`tsc --noEmit --strict --target ES2022 --module ESNext --moduleResolution bundler
--skipLibCheck --lib ES2022 crates/control/contracts/control-contracts.ts` bajardi:pass.
Checked artifacts uchun local .gitattributes text/eol=lf qo'shildi, git check-attr bilan
ikkala faylda tasdiqlandi; boshqa repo fayllari line endings o'zgarmadi.

Independent QA review operation-owner, project binding va schema Unicode masalalarini
topdi; source tuzatildi va tegishli11test bilan yopildi. Old contract foundation
semantikasi saqlangan qamrovda old13test natijasi reused; lokal full workspace
testi/build/native run qayta bajarilmadi.

M13.0.2 source/contracts slice validated, toliq M13.0 hali ochiq: remaining planned
mutation payloadlari, haqiqiy adapter error/dispatch mapping va final contract freeze
qoladi. Manager/authenticated IPC/MCP/client/native acceptance amalga oshmagan.
Next ready: M13.0.3 planned tools uchun typed mutation kontraktlari, so'ng M13.1 owner
spike/integratsiya; already typed read contract fixture'lari reuse qilinadi.

# Remote CI reconciliation (2026-11-14)

E03 sourceae62902 run43487502800 macOS va Windows matrix success: original scoped
local checksdan tashqari repository code-CI fmt/clippy/full Rust/frontend/locales
va embedded desktop binary build o'tdi, strict35/0/0. Native OS credential testi
ignored qolgan; Windows runner actual Win10/11 yoki interactive app proof emas.
M13.0.1 source5e77a2a run43487380006 va TP01 source6940822 run43486608801 ham success.
E04.2 sourcea2b77c9 run43488625057 docs-only success, product steps skipped. Ushbu
remote natijalar final M13.0.2 kodining CI pass'i deb ko'rsatilmaydi; yangi source
run'i alohida. Existing broad checks code pushda saqlandi, local suites takrorlanmadi.

# M13.0.3 — Typed commands va domain adapter (2026-11-14)

Initial12command: services start/stop/restart; runtime install/remove/select;
environment plan/apply/YAML save; workspace archive/restore/purge. Inventory155operation:
16projection,11mutation,1pure plan; qolganlari planned/unsupported, barcha live_available:false.
Request fingerprint canonical root/current revision/action/typed args bilan real SHA256
providerda hisoblanadi; trusted grant/project trust/backend review va exact args
fingerprint alohida tekshiriladi. Client confirm/consent/trusted fieldlari authority emas.

Yangi devforge-control-adapter prepared requestni mavjud Workspace::Command yoki
configuration_plan method descriptoriga map qiladi, lekin Workspace ochmaydi va
amal bajarmaydi. Current project/root/revision va workspace source-owner qayta
tekshiriladi; backend digest control reviewdan ajratilmaydi. Legacy consent/trusted
faqat verified receipt asosida synthesize qilinadi. Domain errors stable scope/code
va bound project bilan qaytadi, foreign IDs/raw messages rad etiladi. Production
SHA256 helper known vectors bilan tekshirildi.

Independent QA ProjectName trim-empty/200UTF8-byte tafovutini topdi: control va
adapter core validate_name bilan moslashtirildi. Standard schema maxLength scalar
count; x-maxUtf8Bytes200 qo'shimcha server cheklovi ekani izohlangan. Preparedname
invalid bolsa side-effectful Configure command yaratilmaydi. NonUTF8 fingerprint
root panic emas InvalidInput qaytaradi; APFS invalid directory fixture'ni yaratolmagani
uchun Unix persisted-PathBuf unit fixture ishlatildi.

Focused checks:8initial mutation tests; lateName boundary1, nonUTF8unit1 va changed
generated-drift1 pass; projection11passed, o'zgarmagan qamrov reused. Root adapter
dispatch7passed (12tool mapping, review/current identity/workspaceowner, domain errors,
name agreement, SHA vectors), scoped control/adapter clippy, targeted generated TS
compile va strict OKF o'tdi. Root initial test hash formatting compile error fixture
provider tuzatildi; product runtime failure sifatida ko'rsatilmaydi.

M13.0 toliq freeze hali ochiq: remaining planned tool payloadlari/actualhandler
mapping qo'shilganda acceptance yangilanadi. M13.1 infrastructure parallel boshlandi;
GUI/CLI/MCP hali yangi managerga ulanmagan. Native runtime/user data bilan mutation
bajarilmadi. Existing runtime/package selections saqlandi.

# M13.1.1 — Local owner/IPC infrastructure (2026-11-14)

[Manager lifecycle ADR](../architecture/manager-lifecycle.md): alohida manager.lock,
lifetime OperationLease, private endpoint, kernel same-user peer auth, bounded
framing/connection/deadline va dedicated detached launch. Bu generic transport;
Workspace/GUI/grants/public tool handler ulanmagan. Incoming role/client_id authority emas.

Local macOS real process tests: owner +2client, graceful stop, crash/rebind va
launcherexitdan keyin owner survival o'tdi; original9test +subprocessfixture1ignored.
Fixture parent tests tomonidan explicit ishlatilgan, native keychain ignoredtest emas.
Windows saturation source review17→18transient pipe slot tuzatildi;16active cap
saqlanadi, ortiqcha client ownerni to'xtatmaydi. Saturation/idle regression actual
macOSsocketda o'tdi; Windowspipe path actualCI followup bo'ladi.

Independent QA shared OperationLease symlinkni follow qilishini topdi. Root isolated
fixtureda oldsource uchun3failure reproduce qildi (faqat disposable tmp resources).
Platform AppLease/OperationLease regular/nlink va nofollow/reparse flags bilan
himoyalandi, dangling symlink tashqarida fayl yaratmaydi. Platform distribution
focused8test, clippy o'tdi; managerlevel yangi dangling-link test va tegishli
owner/installer/unsafe filters qayta o'tdi, othergreen results reused. Existing
locklarga chmod/repair qilinmadi. libc/windows-sys locked versiyalari qayta ishlatilgan.

Windows compile/runtime yangi CI source bilan alohida tekshiriladi. RestrictedJob
capability failure detached survival proof emas. Root path/privateACL/peer tests
source-macOS dalili minimumOS/interactiveGUI/signing acceptancega aylanmaydi.
M13.1.2 keyingi capability/auth module explicit firstaction va independent grants
chegarasini qo'shadi; keyin single Workspace actor, durableoperations vaGUI/CLI
forwarding. Full M13.1 gate hali ochiq.

# TP02 — Rust dependency impact CI (2026-11-14)

Rust .rs-only change uchun cargo metadata'dagi barcha dev/build/target local
dependency reverse closure olinadi. CLI→desktop runtime edge explicit qo'shilgan.
Test/clippy/fmt argumentarray orqali selected packagesda; frontend/native build
faqat desktop consumer bo'lsa. Manifest/lock/toolchain/unknown/manual/metadata failure
FULL fallback, docs-only TP01 o'zgarishsiz.10selector test +old8scope test passed,
actual cargo metadata read-only sample bilan leaf/control/manager/CLI closure review
qilindi; product suite lokal rerun qilinmadi.

M13.0.3 source8e36be8 run43559303146 success. M13.1.1 source79ed2a6 run43559549217
Windows compile/clippydan o'tdi, lekin manager Owner::acquire UnsafeEndpoint sabab
infrastructure tests fail; macOS job passed. Root-owner/elevated-fixture hypothesis
native sanitized probe bilan tekshiriladi, production validation yumshatilmaydi.
Bu failure yashirilmaydi va full M13.1 target qabuli deb ko'rsatilmaydi.

## Windows native diagnostic probe

Manager test-only common root/security probe owner/TokenUser matching, reparse,
protected DACL va ACE flags/mask booleansni chiqaradi; rawSID/path/credentials yo'q.
Production owner/ACL validation o'zgarmadi, fixture SetOwner qilinmadi. Probe commit
sababi79ed2a6 failure qatlamini nativeWindowsda aniq isbotlash; bu platform gate
yopilganini bildirmaydi. Faqat manager source/tests+docs, TP02 shu paketni tanlaydi.

## Windows fixture cause confirmed and corrected

Native probe968b234 run43561300852: new_root owner_matches_user=false,
failed_acquire_control exists=false. Demak rad etish private pipe/ACLdan oldingi
root owner preconditionda; elevated runner TempDir default ownerTokenUserga mos emas.
Faqat yangi disposable TempDir owner'i TokenUserga OWNER_SECURITY_INFORMATION bilan
moslandi; DACL/UAC/token/production source o'zgarmadi. Avval originalforeignowner
production Owner'da rad etilgani va root bo'sh qolgani negativecase bilan tekshiriladi.
Rootbuilder keyin before/after booleanlarni beradi. NativefixCI hali pending;
unrelated producttests ishlamasdan TP02manager closure tekshiriladi. Oldred runlar
43559549217/43561274129/43561300852 saqlanadi, successfulrerun deb almashtirilmaydi.

# Configure shared preflight fix

[Native refusal natijasi](configure-preflight-results.md): raw GUI/core pathdagi
invalidname va debuggerPHP-change refusal actual running sessionni stop qilgani
RED; common Project::validate_configuration stopdan oldin ishlagach2GREEN, exactPID/
HTTP/revision/runtime metadata saqlandi. Directstore1test+scopedclippy green.
Actual Xdebugactivation yoki Windows/Intel native pass deyilmaydi; user/globaldata
tegilmadi va boshqa product suites qayta bajarilmadi.

Windows managerfixturef84360b run43562216307 macOS/Windows manager-only selectedCI
success. Native namedpipe owner/peer/frame/connection process testlari shu Windows
runnerda o'tdi; rootOwner sourcevalidation yumshatilmagan. Bu minimumWin10/11 yoki
Workspace/GUI2client/nativekeychain/fulldetachedjob capability umumiy qabuli emas.

# M13.1.2 — Desktop capability/auth policy

[Auth ADR](../architecture/manager-auth.md): constructor filesystem identityni
tekshiradi, SecretStore'ni probelamaydi. Faqat trusted explicit desktop action key
provision/rotate/revoke qiladi; authenticate existingkeyni tekshiradi, callerrole/tokenni
adopt qilib key yaratmaydi. CSPRNG256bit zeroizing credential/session, const-time
compare, root/kernelpeer (+PID mavjud bo'lsa) binding, bounded8default/32hard cache
va fixedTTL. External project grants bundan alohida. Nativeprovider/wire/GUI ulanmagan.

Blocking store methods faqat dedicated auth workerda; server eventloopda ishlatilmaydi.
QA slowstore time bugini topdi: pre-storeguard saqlangan holda post-completioncooldown/
expiry actualelapsedga bog'landi; denied/unavailable va rotate/delete failure ham
finalize qilinadi. Crossprocesskeychange keyingi permittedauth yokiTTLdagina
kuzatilishi mumkin, immediate globalrevoke deyilmaydi. HeldDesktopAuthority ham
consumptionda check_current bilan expiry tekshiradi.

Original13memory/denystorefixture pass. Timingfixdan keyin2delayedregression+4changed
fixture qayta o'tdi, old9unaffected qamrov reused; heldproofexpiry1newtest o'tdi.
Scopedmanagerclippy/diff green; fullsuite/keychain/UI yoki prompt chaqirilmadi.
In-processmarker policy wiregrant emas; actualcontroller aynan shu chegaralarni
saqlamaguncha fullDesktopCommand ochilmaydi. Actor/desktoppubRPC integration keyingikesim.

# M13.1.3 — Single Workspace actor va durable receipts

[Actor ADR](../architecture/manager-actor.md): one-shot WorkspacePermit rootManagerLease
va installerOperationLease'ni workerning realWorkspace cleanupigacha ushlab turadi.
32waiting+1active boundedqueue; cached snapshot va targetcancel Workspace lockdan
mustaqil. Actual isolated Workspace metadata Import/Favorite/Visit ishlatilgan,
DBcontention/busy/cancel uchun realSQLitelock ishlatiladi, fakebackend yo'q.

Clientidentity root+kerneluserdan serverderived (PIDemas, GUIrelaunchga barqaror).
ExhaustiveCommandsubject project-ID mosligini tekshiradi; globalfaqatdesktop-root,
worktree/cacheambiguous rawtargetlar UnsupportedSubject, existingGUIroute o'zgarmadi.
FreshDesktopAuthority admissionlock/SQLwrite-lock va COMMIToldida qaytatekshiriladi.
Acceptedjob keyingiexpiry/revokedan so'ngdavom etishi policyexplicit, executiontime
externalgrants revalidation haliyo'q. Workerunexpectederror cache'da safeActorError.

OperationsSQLite faqat IDs/key/fingerprint/state/finitecode/timestamp; rawcommand,
secret/log/mail payload yo'q. StartupQueued→Interrupted, Running/CancelRequested→Unknown;
neverautoreplay. Samekey/samefingerprint oldreceipt, boshqarequest conflict. Success
value va validatedexactfailure scope/code/subject/retryable faqat64KiBperitem/128row/
1MiBtotal RAM LRU; eviction/restartResultUnavailable. Durablefailureclassification
finite; GUIerror tafsilotlariDBga yozilmaydi.

Final focused actorintegration9passed +retention/policyunit2passed, managerclippy
alltargets-Dwarnings pass. QA A/Bsubject, expired-duringadmission va cacheworkererror
masalalari tuzatildi; failureDetail eviction/restart realmetadata testibilantasdiq.
OriginalDarwinlongtemp-path fixture source/tmpga moslandi, prod103bytepolicyyumshamadi.
Nativeevidence localmacOSARM64; yangiWindows/Intelactoracceptance CI/targetda alohida.
No GUI/runtime/download/Keychain yokiuserproject mutation. PrivateIPC64KiB va actor
256KiBcommand limit existingM7 1MiBHttpbodyga yetmaydi: authenticatedlargebodychannel
va trustedworktreetargetmapping yopilmaguncha GUImigrationready emas.

# M13.1.4 — Private controller, bootstrap va native renewal

[Controller ADR](../architecture/manager-controller.md) va [bootstrap ADR](../architecture/manager-bootstrap.md):
realOwner/oneActor/listenerready kombinatsiyasidan private pipe orqali serverissued
256bitroot/instancebound oneuse token. Actualpipe bo'lmaganTTY/disk/NUL rad; Root
credentials argv/env/disk/log/resultsga yozilmaydi. Stdoutkeyframedan keyinNULda,
handleinheritance cheklangan; WindowsUCRTfd1 vaWin32STDOUT birlikda boshqariladi.
MandatoryexeSHA yangiGUIlaunchAPI, timeoutfaqatknownnewUNreadychild; existing/ready/
unknownOwner o'ldirilmaydi. Defaultnormalserve credential chiqarmaydi.

NativeGUItokenrenewal explicitvalidsession/peer/root/generation asosida atomikrotate;
pertokenfixedTTL va immutable12hdefault/24hmaxauthoritycontext. NativeguiAPI keyfallback
ham renewablecontext, ordinary1.2authfixednonrenewable. Renewablecontextcrossprocess
revokewindow nextstoreauth/ownernotification yokiabsolutecontextdeadlinegacha bo'lishi
mumkin; bu yangiworkingpolicyamendment,10mininstantrevoke deb ko'rsatilmaydi.

StrictprivaterouteHealthdan boshqa hammasidaSession; nostartupSecretStoreprobe,
NativeIOspawn_blockingda, busycachetry_lock. Quitmutex/persistinhibitbeforestop,
Autostartinhibited, ExplicitUserStarttrustedRustintentclear; OSsignalhandlebir xil
gatedcommit, persistencefailurealive, stalehandlenewownerga yozmaydi. LegacyAppLease
exclusion oldGUI/newmanager parallelWorkspace'ni bloklaydi. RawlegacyCLIguard uchun
LegacyExclusion faqatmanager.lock, configexclusive bilanselfdeadlockyo'q.

Checks: realController7passed; actualprivatebootstrapcustomharness5passed; newGUIauth3,
actualready/revoke2, olddetach1; lateWindowsunitfixturechange2affectedcase rerun;
MacUID+kernelLOCAL_PEERPID2clientcase1; LegacyExclusion/configexclusive1; scopedmanager
clippyalltargets passed. JSONbufferZeroizingadded, generalValueheapcopiesfullyzeroized
degan claimyo'q. Startup/root tests memory/denystore; realKeychain/GUI ishlatilmadi.

39770f8Windows43567704236:7actorcasespass, contentionfixture2secRunningobservation
ProjectStore3secidleSnapshotreadbounddanqisqa bo'lgani uchunfail. Faqatobservation6sec
qilindi, product3sec/500mscachecancel/32queue va finalsuccessassertlar saqlandi.
TargetedcaseMacpassed; newWindowsnativehopqaroriCI'daalohida, oldfailurehistorysaqlandi.

Stillgates: noactualGUIclientmigration; private64KiB/actor256KiB existingM7largebody
uchun yetmaydi, worktree/cachetrustedtargets/partialspecificerrors vaDiscoveryowner
wiring ham yopilmaguncha GUIreadyemas. NoMCP/externalgranttool liveadvertisement.

## Bootstrap custom-harness Windows fixture correction

QA ownerfixtureauditda bootstrapcustomharnessroot() ham plainTempdir ekanini topdi.
Unitfixture avvalprivatechildga tuzatilgan, endicustomharness ham publishedcommon::root
helperdan foydalanadi (freshTempDirowneronly, Unix/tmp). Faqat testfile o'zgardi;
custombootstrap5targetedcase va scopedtestclippy green, otherauth/unitcases reused.
ProductionACL/rootownercheck o'zgarishsiz. Windows yangi CI snapshotda tekshiriladi.

## Windows private request enum layout correction

425adec run43569830478Windowsclippy Request::Submit.command inlineCommand uchun
large_enum_variant rad etdi (native tests haliyetmagan). FieldBox<Command> vaactor
forwarding derefga almashtirildi; serdeJSONwire o'zgarmadi. Faqat2linesource,
controller7targetedtests va scopedmanagerclippy green. NativeWindowsyangihoptasdiq
kutiladi; bu failurefixture yokiAuthbuzilishi deb qaytatalqinqilinmaydi.

# M13.1.4 CLI composition entry

[Private manager CLI](../guides/private-manager-cli.md): actualcompiledCLI manager
serve/health/help, explicit/automatic start intent, privatepipeonlybootstrap before
rootwrites, nativeStoresconstructednostartupquery, OSsignalgracefulQuit. Transitional
LegacyExclusion preventsnewmanager+oldrawforegroundOwner; metadatareadwhitelist
retained, configexclusive notselfconflicting. Globalhelp returnscommandJSONbefore
appdata/credential initialization.

Focused CLI routing6tests pass (no filesystemonhelp/healthmissingowner, invalidflags,
file/nullbootstraprefusal, rawmutationexclusion/catalogread, exclusiveleasecompat).
New actualCLIcoldbootstrap→PID/rootverify→health→redeem→Quit→AutoStartinhibited case1
passedmacOSARM64. RootusedactualproductionCLI/nativeStoreconstructors, noNativeStore
authcall/request; earlierDenystorepipelineproves coldpathNoProbe. Cli alltargets
clippy passed; broad frontend/product suites notlocallyrerun. Other target newCLI
execution CIpending. GUI/publicJSONCLI/MCP forwarding notclaimed.

# E09.1 — Release actor metadata baseline

[Harness guide](../guides/actor-baseline.md), [sanitized JSON](../evidence/actor-baseline/2026-11-14-macos-arm64.json)
va [provenance](../evidence/actor-baseline/2026-11-14-macos-arm64-provenance.json).
Realrelease1/10/100metadata workload localmacARM64da o'tdi; noinstalledruntimes/
services/GUI/NativeStore/workspace-secretcalls va cleanup invariants passed.
100projects: registration1.94s; Favoriteinclrefreshp95~12.50ms; cachedreadp95~0.00121ms;
periodicobservationp95~309.89ms SCHEDULINGWAIT (computeemas).1sidleCPU~5.21%onecore;
processlifetimepeakRSS14.37MB (percaseexclusiveemas). No productSLAchosen.
Focusedexampleclippypass,3invalidargboundschecks. ReportsourceHEAD2ff0002plusdirty
parentwork/harness, binary/output hashes recorded; notcleanGUIbenchmark.
WindowsCPUcodeGetProcessTimes yozilgan, RSSnull; actualWindows/GUI/process-tree/
logmailqueryburst/longidle budget laterE09gates. SourceartifactNativeRoot paths yoki
credential yo'q. Parentunchanged tests/wholeproduct notrerun.

## CLI Windows import-gating fix

6ab2ca6Windowsrun43570885602 clippy testmanager_entry.rsda Unix-only ikkihelper
importi unused bo'lgani uchun native testsdanoldin to'xtadi. Importlarga cfg(unix)
qo'shildi, aynan shu platformagated testsga mos; sourcebehavior/JSON/prodflags o'zgarmadi.
ScopedCLItestclippy passed; unrelatedtest qayta bajarilmadi. WindowsnewCIhopmustverify,
oldfailurehistorypreserved.

# E05.1 readonly inventory backend

[Storage natijasi](storage-inventory-results.md): API/ownedmarkers/refs/logicalsize,
boundedregistry/FS, incompleteunknowncoverage. No cleanup/delete/eligibility/secret
contentread/productiondatascan.8storage +initialmetadata1 passed; QA SQLquerybyte
sourcefinding2phaseoctet_length corrected +targetedmetadata2/Unicode1/ref1/DB1pass,
finalmetadatahandles1/link1pass. Scopedclippy/fmt/strict. Remaining E05UI/cleanup
preview E06drill/retention alohida; hardlinklogicalsize notfreeable, nonatomicsnapshot.
