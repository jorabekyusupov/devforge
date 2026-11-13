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
| M13.1 | planned | M13.0; native single-owner/auth/ID/retry/cancel/Quit/headless acceptance. |
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
| E03 | in_progress | Exact Composer project binding/migration/execution/export; catalog refreshdan oldin. |
| E04 | in_progress | Current-state/tooling/README drift va contributor runbook, product tests talab qilmaydi. |
| E05 | planned | Ownership/resource kontrakti; read-only storage inventari, keyin cleanup preview. |
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
o'tdi. Remote Actions yangi policy qabuli hali pending; product tests lokal
qayta bajarilmadi, chunki bu ozgarish app behavior'iga tegmaydi.
