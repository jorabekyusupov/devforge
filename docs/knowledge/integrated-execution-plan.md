---
type: Implementation Plan
title: DevForge birlashtirilgan bajarish rejasi
description: Yetti rol tahlili va tashqi agentlar rejasining bogliqlik, parallel ish, UI skill va maqsadli testlar bilan yagona bajarish tartibi.
status: draft
decision_status: proposed
tags: [execution, roadmap, agents, testing, ui, m13, m14, m15]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Birlashtirilgan batafsil reja va ish tartibi topshirigi
  - id: agents
    resource: /m13-external-agents-plan.md
    title: Boshqa chatdagi tashqi agentlar imkoniyatlari va M13.0–M13.9 rejasi
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: M13 M14 M15 tartibi
  - id: completion
    resource: /m7-m12-completion.md
    title: Mavjud funksiyalar va qabul chegaralari
  - id: windows
    resource: /windows-compatibility.md
    title: Platforma va edition cheklovlari
  - id: policy
    resource: /knowledge-policy.md
    title: Bosqich yakunidagi reconciliation
  - id: ui
    resource: ../../.agents/skills/devforge/references/ui-ux-workflow.md
    title: Mac mini installed UI skill routing
  - id: checks
    resource: ../../.github/workflows/check.yml
    title: Amaldagi keng CI tekshiruvlari
---

# Vakolat, manba va holat

User yetti sub-agent tahlili, muhokama va barcha takliflarni hamda boshqa chatdagi
tashqi agentlar rejasini qamragan batafsil bajarish rejasini soradi. Faqat jarayonga
aloqador testlarni bajarish, mustaqil ishlarni sub-agentlarga ajratish va Mac mini
dagi UI skilllardan vazifaga mos foydalanish explicit ish qoidalaridir.[^request]
Feature tarkibi, arxitektura tanlovlari va hajm baholari proposed edi. Keyingi user
ko'rsatmasi rejani bajarish, tegishli testlar, har yakunlangan bosqichni commit/push
qilishni vakolatladi. Public release va system permission o'zgarishi source pushdan alohida.
Planning matni implementation dalili emas; actual natijalar execution-status'da qayd etiladi.

Asos: source111526d ustidagi yetti rol source review va [M13 manba rejasi](m13-external-agents-plan.md).
Reja yozilayotganida knowledge fayllarida boshqa ish oqimining ozgarishlari bor;
ular saqlanadi. Ish boshlashda Git/source qayta tekshiriladi. M7–M12 mavjud core
qayta yozilmaydi. Tarixiy platforma evidence bugungi runtime pass deb olinmaydi.

Manba M13 hujjati tool oilalari, grants va protocol tafsilotlari uchun saqlanadi;
ushbu hujjat ularning boshqa ishlar bilan bogliqligi, navbati va qabulini boshqaradi.
M13.0–M13.9 raqamlar ozgarmaydi. Ichki chat/model/provider/API-key UI scope'da emas.
Nomuvofiqlik topilsa implementationdan oldin manba va ushbu reja birga aniqlashtiriladi;
bir hujjatdagi jumla bilan boshqa hujjatning scope'i jim bekor qilinmaydi.[^agents]

# Ishni boshqarish kontrakti

Har work item uchun: ID, owner, old shart, exact fayl scope, kutiladigan natija,
qabul mezoni, tanlangan testlar va sababi, source revision, qoldiq risk yoziladi.
Holatlar: planned → ready → in_progress → review → validated → integrated.
Tashqi tosiq blocked/unverified sifatida sababi bilan qayd qilinadi; tayyor deb yopilmaydi.
Bir vazifa yakuni butun milestone yakuni emas. Task ma'lumotlari repo ichidagi
mavjud hujjat/evidence oqimida saqlanadi; yangi task SaaS yoki plugin platformasi shart emas.

- Old shart tugamagan dependent kod boshlanmaydi; schema fixture, read-only research va test dizayni parallel bolishi mumkin.
- Shared kontraktni Technical Architect va Tech Lead kelishadi; integrator bitta versiyani e'lon qiladi.
- Ozgarishlar exact reviewed scope bilan; dirty user/boshqa agent fayllari overwrite/reset qilinmaydi.
- Har mutation uchun ownership, stale revision, idempotency va failure/recovery natijasi loyihalanadi; universal undo va'da qilinmaydi.
- Yangi dependency/provider/protocol implementation vaqtida rasmiy manbadan tekshiriladi; eski reja versiyani abadiy pin qilmaydi.
- Har phase close'da code/config/evidence bilan matching, changed, not implemented, unverified ajratiladi; tegishli OKF va log yangilanadi.

# Rollar va parallel sub-agent siyosati

| Rol | Masuliyat va review |
| --- | --- |
| Senior Teach Lead | First-success, CLI/runbook mazmuni, contributor playbook, RU/UZ/EN task piloti |
| Senior Tech Lead / integrator | Dependency/navbat, fayl egaligi, umumiy kontrakt, test de-duplication va integratsiya |
| Senior Software Engineer | Core/adapter implementatsiyasi, exact pins, migration, task runner va performance bottleneck |
| Senior System Analyst | Use case/state/error, ownership, recovery va Given/When/Then mezonlari |
| Senior Business Analyst | Foyda gipotezalari, pilot, auditoriya, parity/differentiator va keyingi scope qarorlari |
| Senior QA | Impact-based test tanlash, contract/fault/native acceptance va evidence traceability |
| Senior Technical Architect | Manager/IPC/lifecycle, API/grant, data recovery va platforma chegaralari |

Bu yetti rol yetti mustaqil developer oqimi degani emas. Integrator har ready itemda
parallel imkoniyatni tekshiradi. Mustaqil, yetarli hajmli ishlar mavjud bolsa sub-agentlarga
ajratadi; umumiy dependency/fayl/DB/process resursi ajralmasa ketma-ket bajaradi va sababini yozadi.

Har sub-agent taskida ID, ownershipdagi fayllar, o'qiladigan kontrakt revisioni,
acceptance, test scope, taqiqlangan shared mutation va handoff formati beriladi.
Bir faylga bir vaqtda bitta writer. Shared lib.rs, locale katalogi, manifests/lockfiles,
CI va knowledge index/log integrator yoki belgilangan yagona owner orqali yig'iladi.
Boshqa agent kontrakt ozgarishini xabarsiz qabul qilmaydi. Mumkin bolsa isolated
worktree va disposable data root ishlatiladi; bu user loyihalari yoki system configga tegmaydi.

Parallel oqimlar:

| Oqim | Qachon boshlanadi | Nima parallel boladi | Integratsiya nuqtasi |
| --- | --- | --- | --- |
| A Core/manager | E00, M13.0 kontraktidan keyin | B CLI schema fixture, C MCP projection fixture | M13.1 live owner qabuli |
| B JSON CLI | M13.0 | A manager va C stdio adapter | M13.2 live CLI faqat A'dan keyin |
| C MCP/diagnostics | M13.0 | Persisted read schema, bounded projection; keyin stdio | M13.3 actual client |
| D Human UI/onboarding | E00 va mavjud M8 API | A/B/C, docs va performance baseline | UI tegishli facade tayyor bolganda ulanadi |
| E QA/platform | E00 | Test dizayni, fixture, OS host/signing/license tayyorgarligi | Har slice relevant native gate |
| F Storage/catalog | E03 kontrakti va resource ownershipdan keyin | Turli modullar bolsa storage inventory va catalog metadata | Har biri alohida reviewed slice |

Ogir CPU/native testlar bitta hostda rejasiz parallel yuritilmaydi. Bir runtime cache,
port, trust store yoki DB fixture'ni bir nechta agent birga ozgartirmaydi. Integrator
shared smoke suite'ni bir marta bajaradi, hamma agent uni takrorlamaydi.

# Test siyosati: faqat ta'sirlangan qamrov

**Har kichik ozgarishdan keyin barcha testlarni qayta-qayta ishga tushirish mumkin emas.**
Default: ozgargan kontrakt + tegishli unit/regression + bevosita consumer. Test tanlash
har task boshlanishida QA/integrator bilan impact map sifatida yoziladi.[^request]

1. Avval mavjud valid baseline/evidence o'qiladi; faqat baseline uchun full suite boshlanmaydi.
2. Bug bolsa kerakli regression orqali reproduce; yangi behavior bolsa observable acceptance yoziladi.
3. Scoped edit → focused test → zarur static check. Passdan keyin shu kod o'zgarmasa aynan o'sha suite takrorlanmaydi.
4. Fixdan keyin failed test va uning bevosita ta'sirlangan regressionlari qayta bajariladi.
5. Integratsiyada faqat ikkala branch tutashgan kontrakt/regression bir marta tekshiriladi.
6. Full workspace yoki keng native matrix faqat shared model/schema/process lifecycle/toolchain/dependency ozgarishi, kop crate ta'siri, tushuntirilmagan regression yoki phase/release risk assessment bilan asoslanganda bajariladi.
7. Docs-only yakunda product suite, native build va downloadlar qayta bajarilmaydi. Strict OKF va scoped diff tekshiruvi yetadi.
8. Infrastructure failure alohida qayd etiladi; retry sababi yoziladi. Flaky/product failure shunchaki rerun bilan yopilmaydi.
9. CI'dagi majburiy checks saqlanadi; scoped local siyosat ularni chetlab o'tish degani emas. CI trigger/path optimizatsiyasi alohida reviewed item, xavfli dependency qamrovini yashirmaydi.[^checks]

| Ozgarish | Minimal tekshiruv | Qachon kengayadi |
| --- | --- | --- |
| Knowledge/README | strict OKF + tegishli diff/link | Validator kodi ozgarsa uning unittestlari |
| React component | Tegishli Vitest fayli + typecheck | Shared hook/bridge/DTO bolsa uning consumerlari; meaningful UI bolsa scoped native flow |
| Locale | check:locales; parser/registry ozgarsa 3 regression test; affected view | Yangi locale/negotiation bolsa native persistence/fallback |
| Rust module | Tegishli package/test/filter, affected formatting | Public type/behavior bolsa direct consumer crates va adapter contract |
| Composer pin | projects migration/execution, manifest va repro export | Schema/binding consumersga ta'sir bo'lsa focused workspace/CLI/native |
| Manager/IPC | owner/auth/lease/ID/cancel/retry/crash contracts | Changed OS adapter uchun actual native lifecycle; keng shared risk bolsa full core |
| Inspector/mail/debug | Tegishli crate, PHP companion fixture va scoped projection | Changed transport/redaction/retention consumerlari; actual affected Laravel/native |
| Backup/storage | checksum/staged restore/ownership/active references | Engine adapter ozgarsa actual PG/MySQL; active binding preservation |
| Runtime catalog/install | target/pin/digest/staging/recovery | Changed artifact uchun native probe; hamma paketni qayta download qilish shart emas |
| Installer/update | distribution contracts va tegishli package/upgrade smoke | Release RC'da full applicable clean/minimum-OS matrix |

Amaldagi buyruq namunalari: `cargo test -p devforge-projects --test manifest`,
`cargo test -p devforge-workspace --test repro`,
`npm run test -w @devforge/desktop -- src/configuration.test.tsx`,
`npm run typecheck`, `npm run check:locales`.
Yangi manager/MCP test nomlari implementatsiyada yaratiladi; bu reja ularni mavjud deb da'vo qilmaydi.

Evidence ledger: work ID, source/tree yoki tegishli fayllar hash'i, target/OS/build,
tool/fixture versiyasi, buyruq, pass/fail/skipped, artifact va reuse sababi.
Keyingi docs commit source SHA'ni ozgartirsa ham product scope/hash o'zgarmaganini
ko'rsatib oldingi natija qayta ishlatiladi. Kod/fixture/toolchain/target/kontrakt ozgarsa
mos natija invalid qilinadi va faqat shu qamrov qayta tekshiriladi.
Full suite phase close'da ham avtomatik emas: impact analysis zarur qamrovni belgilaydi;
release candidate butun mahsulot qabuli uchun bitta frozen artifact matrix o'tadi.
RC'dan keyingi docs-only o'zgarish native matrixni qayta boshlatmaydi.

# UI: Mac mini skilllari va mavjud dizayn davomiyligi

Har UI owner ish boshida [routing](../../.agents/skills/devforge/references/ui-ux-workflow.md)
va tanlangan skillning haqiqiy SKILL.md faylini o'qiydi. Installed manzillar routingda
bor; boshqa hostda named skill qidiriladi. Qo'llanmagan skill applied deb yozilmaydi.
Bu planning task UI implementatsiyasi emas; hozir konsept/screenshot yaratish talab qilinmaydi.

| Vazifa | Skill yo'li |
| --- | --- |
| Yangi screen/katta redesign | ui-ux-pro-max UX/design-system → ui-taste new-work/operate → uizze-ui implementation → web-design-guidelines scoped review → native visual/interaction |
| Tayyor visual target | ui-taste fidelity → ui-ux-pro-max focused stack/UX → uizze-ui → web-design-guidelines → target comparison |
| Polish/soddalashtirish | ui-taste polish/distill → tegishli UX qidiruv → uizze-ui scoped edit → web-design-guidelines → affected states |
| Keyboard/focus/form/overflow xatosi | ui-ux-pro-max targeted semantic/stack → ui-taste scoped craft → uizze-ui → scoped guidelines → actual reproduction |
| Read-only UI compliance/audit | Vazifaga qarab guidelines-first code review yoki ui-taste rendered-evidence-first UX audit; fixes avtomatik emas |

Oldingi `design-system/devforge/MASTER.md`, page override, components/tokens,
assets va settled target saqlanadi. Har mayda fix uchun master/konsept qayta yaratilmaydi;
yangi visual ishda routingdagi image target qoidasi qo'llanadi. Backend-only tasklarda
tort UI workflow bajarilmaydi. Locale, keyboard, focus/escape, semantic names,
loading/empty/error/success/busy/stale/cancel states, minimum window, long paths,
RU text expansion va supported themes tekshiriladi. Domain logicda matn/language branch yo'q.
Browser preview/native Tauri/system dialog dalillari ajratiladi; macOS screen Windows
qabuli emas. GUI ishidan oldin AGENTS'dagi agent-desktop skill/permission qoidalari bajariladi;
macOS permissions userning exact tasdigisiz o'zgartirilmaydi.

# Bajarish navbati va old shartlar

Asosiy dependency: **E00 → M13.0 → M13.1 → M13.2 → M13.3 → M13.4 → M13.5 → M13.6 → M13.7 → M13.8 → M13.9 → M14 → M15.**
Schema/projection/UI fixture'lari oldin parallel ishlab chiqilishi mumkin; yuqoridagi
navbat live integratsiya va qabul navbatidir. M13.5 read projection M13.4 bilan
mustaqil modullarda parallel bolishi mumkin, lekin har biri o'z old shartini yopadi.
E01/E02/E04 va compatibility/performance baseline E00'dan keyin parallel; E03 katalog
kengayishidan oldin; E05–E10 optional ishlar M13ni to'liq kutdiradigan paket emas.

## E00 — Baseline, qaror va risk inventari

Owner: Tech Lead + System Analyst; QA va BA parallel review.

1. Current Git/dirty scope, manifests va historical evidence inventory; M7–M12 funksiyalarini qayta implementatsiya qilmaslik uchun mapping.
2. Feature/support jadvali: implemented/planned/proposed/unverified, OS/arch/edition/provider/prerequisite va source/artifact.
3. Windows10/11 actual host, macOS minimum13.5, native credential/dialog hamda signing/license masullarini aniqlash; yo'q resurs aniq blocker.
4. Pilot auditoriyasi: aralash macOS/Windows Laravel jamoasi taklifi; first-success HTTP, DB/queue/mail manifestga qarab. Client juftligi va versiyasini tanlash.
5. Har keyingi item uchun acceptance va test impact map; scope bahosi E00/M13.0dan keyin. Yetti rol yetti developer deb muddat hisoblanmaydi.

Exit: inventory, decision register, test ledger va ready queue mavjud. Test: docs checks;
old product suite baseline niyatida qayta yuritilmaydi.

## M13.0 — Kontrakt, facade va scope freeze

Owner: Architect + System Analyst; Engineer va QA fixturelarni parallel tayyorlaydi.

1. Manba rejadagi 16 tool oilasining existing/new/unsupported mappingi, input/output/error/capability/side effect/grant/bounds.
2. api_version, request_id, operation_id, project/revision, typed data/error, warnings va cursor; MCP standart envelope alohida saqlanadi.
3. Rust/TS turlarini generated schema yoki contract fixture bilan sync tekshirish; Tauri domain ichiga kirmaydi.
4. Plan/digest expiry, optimistic revision, idempotency key va resource conflict modeli. Read/live/persisted origin/freshness.
5. Bounded evaluate aniqligi: existing primitive locals qoladi; haqiqiy expression eval alohida execution grant/design qarorisiz qo'shilmaydi.
6. SQL read-only role/connection va DB-specific limits; SELECT prefix/rollback o'zi sandbox emas. Project exec hooks uchun trust + execution grant.

Exit: versioned contract fixtures va ownership/lifecycle ADR. Test: schema serialization,
unknown/version/input/error fixtures; existing affected Command/DTO consumers only.

## M13.1 — Yagona owner, IPC va operatsiyalar

Owner: Core Engineer; Architect review; QA native harness parallel.

1. Desktop/current CLI composition ustida umumiy control facade; MCP alohida Workspace/Supervisor egasi bo'lmaydi.
2. User-scoped authenticated IPC; macOS socket/Windows named pipe yoki equivalent peer/ACL verification.
3. Durable operation ID/status/progress/target-cancel/result/retry; avval conservative serialization. Resource locks zarur scope'da.
4. GUI-absent headless manager spike: launch/ownership/crash/reconcile; always-on OS daemon oldindan majburiy emas.
5. Window close, explicit Quit, client EOF/kill, reconnect, interrupted/unknown state; Quitdan keyingi retry auto-relaunch siyosati.
6. Grants/revoke va audit; running operation revocation siyosati yangi amallarni rad etishdan alohida.

Exit: GUI+2 client bitta service owner; retry bitta effect; A cancel Bga tegmaydi;
shared consumers saqlanadi; restart status truthful. Test: targeted IPC/lease/process,
operation persistence va har changed OS adapter native lifecycle. B/C adapter development
parallel, live acceptance owner qabuli tugagach.

## M13.2 — JSON CLI va human help

Owner: CLI sub-agent; M13.0dan schema, M13.1dan live owner talab etiladi.
Versioned envelope, error/exit code, capabilities/schema/help, read-only status/list/inspect,
operation status/cancel va eski consumer compatibility. Global/subcommand --help va
misollar state yaratmasdan ishlaydi; shell completion/help metadata ham shu kontraktdan.
Exit: CLI facade orqali GUI yo'q holatda ham truthful natija; stdout JSON, diagnostics stderr.
Test: CLI contract/help/exit fixtures + facade integration; barcha native runtimelar emas.

## M13.3 — stdio MCP read-only

Owner: MCP sub-agent; CLI bilan parallel adapter yozilishi mumkin.
Protocol version/capability compatibility, project binding, bounded tools/resources,
pagination/expired-cursor resync, generic config va JSON CLI fallback. Secrets masked;
logs/mail/source instructions sifatida olinmaydi. MCP Inspector + tanlangan kamida
ikki actual client va terminal fallback. Client config yaratilishi actual connection emas.
Exit: scoped current context ishlaydi; denied project, stale cursor, EOF va noisy output
fixtures o'tadi. Test: MCP protocol/adapter va actual read-only client flow.

## M13.4 — Boshqariladigan muhit amallari

Owner: Core/adapter sub-agentlar, shared command schema yagona ownerda.
Kesimlar ketma-ket: start/stop/restart → runtime install/select → env/YAML setup →
DB bind/clone/restore → domains/HTTPS/system helper. Har kesim oldingi M7–M12 core
funksiyalarini facade orqali ishlatadi; yangi side effect uchun reviewed plan/grant.
Secrets manager ichida inject qilinadi, admin credential agentga berilmaydi.
Exit: stale/replayed/revoked apply rad; existing project data/pins va shared service
saqlanadi; failed/cancelled natija aniqligi. Test: har adapterga mos focused regression;
DB/system privilege uchun actual native affected flow, butun suite har kesimda emas.

## M13.5 — Health, diagnostika va mail

Owner: Inspector/mail sub-agent; Teach Lead actionable messages, QA redaction.
Existing PHP/Composer/env/process/serving/DB factsni issue→dalil→ta'sir→next action
projectionga yigish. Request/job/HTTP/cache/SQL/N+1/dump/profile/log/error filters,
correlation va before/after cursor; mail list/show/wait/link, attachment metadata,
controlled export/delete. Support bundle/runbook/artifact URI, preview/redaction va
local export; raw env/private key/DB dump avtomatik kirmaydi, auto-send yo'q.
Exit: yangi request eski eventdan farqlanadi, A konteksti Bga chiqmaydi; canary secret
redaction va bounds o'tadi. Test: relevant inspector/PHP/mail/projection fixtures;
affected actual Laravel capture va client flow. Doctor yoki bundle yangi mustaqil
backend sifatida ikkinchi marta yozilmaydi.

## M13.6 — Execution, HTTP, debugger va workspaces

Owner: execution/debug/workspace alohida scoped sub-agentlar; umumiy operation kontrakti frozen.
Trusted project PHP/Artisan/Composer/Node/npm/Git invocation, timeout/output/exit/cancel;
test artifactlari; managed site HTTP probe/redirect/TLS/correlation; owned test DB,
workspace create/merge/archive/restore/purge va staged backup/recovery. Existing Xdebug
step/stack/primitive locals/profile revision qoidalari saqlanadi.
Exit: actual known failure testdan tashxisgacha kuzatiladi; isolated workspace/DB,
argv/env/ownership va source preservation; arbitrary shell default tool emas.
Test: tegishli execution/debug/worktree/repro fixtures va bitta affected end-to-end
scenario; bir xil common lifecycle suite integrator tomonidan bir marta.

## M13.7–M13.9 — HTTP, ulanish UI va integratsiya qabuli

M13.7 owner MCP: loopback auth/Origin, protocol compatibility, revoke va scoped
credentials; public bind/tunnel avtomatik emas. Remote bridge M14 alohida dependency.
Test: HTTP transport/auth/origin/compatibility va changed platform endpoint behavior.

M13.8 owner UI + Teach Lead: uch tilli project/grant tanlash, config diff/export,
active operations/audit/revoke, client instructions va packaged executable paths.
Existing MCP entries/AGENTS/PATH overwrite qilinmaydi. UI routing yuqoridagi tartibda.
Test: affected React/locales + native connection/permission UX, Unicode/space paths.

M13.9 owner integrator + QA: actual client/OS/contract matrix, M7–M12 ta'sirlangan
regressiyalar va exact artifact/source mapping. Relevant old green natijalar hash/scope
bilan reuse; shared manager sabab keng risk qolsa core/native matrix bir marta.
Minimum OS yo'q bolsa gap ochiq, full M13 target acceptance da'vosi qilinmaydi.
Exit: local integration holati va qolgan targetlar aniq; M14 boshlash uchun uning kerakli
kontraktlari validated; unverified scope yashirilmaydi.

# Tashqi agentlar imkoniyatlarining toliq mappingi

Bu jadval boshqa chat rejasining birorta oilasi tushib qolmasligini tekshiradi.[^agents]

| Oila | Bajarish ID | Minimal acceptance |
| --- | --- | --- |
| Health/capabilities | E00, M13.0/.5, E02 | unsupported/unverified sababi, actionable doctor |
| Projects/discovery | M13.3/.4 | scoped identity/import/forget/restore, path boundary |
| Runtimes/PHP | E03, M13.4, E07 | install/select/probe/remove plan, pins saqlanishi |
| Services/processes | M13.1/.4 | readiness, owner/shared consumers, scoped stop |
| Environment/config | M13.4, E02 | masked read, env.example diff, YAML/secret refs, reviewed apply |
| Domains/HTTPS | M13.4, M15 | owned URL/trust/hosts plan, narrow elevation helper |
| Database | M13.4/.6 | role-limited schema/query; clone/backup/restore review |
| Logs/errors | M13.5 | bounded filters/cursor, stack/file/line, wait |
| Inspector/performance | M13.5, E09 | request/job/HTTP/cache/SQL/dump/profile va correlation |
| Debugger/profiler | M13.6 | existing scoped/revision API, eval scope aniqligi |
| Mail | M13.5 | wait/link/filter/attachment, controlled export/delete |
| Execution/tests | M13.6, E08 | trusted runtime context, exit/cancel/artifacts |
| HTTP verification | M13.6 | project-local host/redirect/TLS va trace |
| Workspaces | M13.6, E05 | isolated DB/domain, merge/archive/restore/purge guards |
| Operations/events | M13.1–.9 | IDs, durable result, bounded cursor/reconnect/resync |
| Agent context/artifacts | M13.3/.5/.6 | scoped summary/runbook/artifact, umumiy filesystem access emas |

# Mahsulot va muhandislik qoshimchalari

Har item scope'i M13da mavjud feature bilan takrorlanmaydi; uning ustidagi foydalanuvchi
yoki operational qatlam sifatida ishlab chiqiladi. Har biri alohida ready/review gate.

| ID / owner | Qadamlar, dependency va exit | Tegishli tekshiruv |
| --- | --- | --- |
| E01 Teach/UI | E00dan keyin mavjud M8 ustiga existing repo/Git/YAML task checklist; resume va conditional DB/queue/mail; HTTP→Inspector first-success. M13ni kutmaydi, keyin facadega ulanadi. | setup/onboarding affected React + native task flow; APP_KEY/data preservation |
| E02 System/UI | M10 expected/actual/portability exceptions; extension/pin/secret-presence/DB/OS edition capability va sabab. M13.5 health source'idan foydalanadi; silent fallback yo'q. | manifest/repro/environment focused + cross-device fixture; values masked |
| E03 Engineer | Catalog kengayishidan oldin per-project exact Composer ID; old record migration; execution/setup/terminal/export/import bir xil binding. | projects schema/execution + manifest/repro export, ikki versiya/default-change fixture |
| E04 Teach/BA | E00dan parallel README/current-state/tooling docs drift; first-PR/contributor ownership/test/evidence playbook; uch til termin/glossary va real task review. | docs checks; catalog content o'zgarsa locale check; product suite yo'q |
| E05 Storage Engineer | Operation/ownership kontraktidan keyin runtime/DB generation/backup/archive/mail/log/profile bytes/active refs/last use inventory; keyin reviewed cleanup preview. Purge existing DB/credential/branch semantikasini jim kengaytirmaydi. | owned paths/leases/active bindings va UI inventory; unknown ownership o'chirilmaydi |
| E06 Backup Engineer | E05dan keyin activation'siz disposable restore drill → result/receipt/cleanup → retention preview → optional scheduling. Running loyiha yashirin to'xtatilmaydi, yagona foydali backup yo'qotilmaydi. | checksum/corrupt/cancel/engine-version, actual changed PG/MySQL restore, active binding unchanged |
| E07 Runtime Engineer | E03dan keyin catalog revision/provenance/capabilities/deprecation/revocation → reviewed refresh/signature/trust/rollback → offline readiness → cache bundle. Secrets/DB alohida; current pins saqlanadi. | target/digest/pin/receipt/offline/bad-signature; faqat changed artifact native probe |
| E08 Process Engineer | M13.6dan keyin test/Pint/named worker va talabga mos scheduler/Reverb; keyin Horizon. Fixed typed profillar va YAML; general plugin/arbitrary shell emas. | process readiness/cancel/env/INI/log/isolation va selected actual Laravel profile |
| E09 QA/performance | E00dan baseline: 1/10/100 va zarur 200 loyiha, backup/event burst; release process tree CPU/RAM/start/status/UI latency/quit; app vs managed services alohida. Keyin bottleneck bolsa cursor/lazy detail/event refresh. | repeatable workload va per-target report; optimizationdan keyin faqat tegishli comparison |
| E10 QA/compatibility | E00dan harness: declared Laravel11/12/13 va mos PHP; PG/MySQL/SQLite, queue/mail/log/dump/exception. Pairwise meaningful coverage, actual supported/untested jadval. | changed companion/runtime kombinatsiyalari; bitta Laravel13 pass boshqalarga proof emas |

E01/E02/E05 UIlari yuqoridagi installed skill routing bilan; yangi implementation uchun
loading/empty/error/recovery states va accessibility ham acceptance tarkibidir.

# M14 — Ulashish va tashqi integratsiyalar

Old shart: M13dan zarur operation/auth/project/site kontraktlari validated.

1. M14.1: IDE/DB client provider adapterlari, command/URL/credential exposure tekshiruvi; UI va adapter mustaqil bolsa parallel sub-agentlar.
2. M14.2: bitta verified tunnel provider; explicit site/port, access control, expiry, revoke va active status. Website sharing control API sharing emas.
3. M14.3: stop/Quit/crash/restart/expiry/revoke lifecycle; inspector/debug/mail/DB/control tasodifan ochilmaydi.
4. M14.4: remote agent zarur bolsa alohida secure bridge/TLS/auth/revocation; local M13 natijasidan mustaqil qabul.
5. Deploy providerlari D04 qaroridan keyin alohida kesim; oddiy tunnel scope'ini kutdirmaydi.

Test: affected adapter unit/contracts, actual provider flow, two-project routing,
revoked/expired link va no-private-endpoint exposure; full M7–M12 suite avtomatik emas.
Exit: share session target/lifetime aniq, actual exposure yopilishi isbotlangan.

# M15 — Yakuniy beta, update va tarqatish

E00da boshlangan host/signing/license tayyorgarligi shu yerda actual qabul bilan yopiladi.

1. M15.1: metadata store inventory (preferences SQLite, project store, workspace JSON, operation store, secret refs); N−1→N upgrade va downgrade siyosati. Existing migrationlarni qayta yozmaslik.
2. M15.2: production native credential create/read/rotate/delete; locked/denied/cancel/unavailable; hosts/trust/system dialog, ordinary-token lifecycle va explicit cleanup.
3. M15.3: disk-full/network/proxy/TLS/locked-file/permission/stale-lock/interrupted install/update/recovery; pins/projects/DB/secret references preservation.
4. M15.4: minimum13.5/current macOS ARM64/Intel, Windows10 19045 va Windows11 desktop; Server2025 alohida evidence; provider edition support aniqligi.[^windows]
5. M15.5: runtime provenance/SBOM/notices/redistribution/license review, signing/notarization/Authenticode va update key ownership/rotation. Signed manual distribution auto-updatedan oldin kelishi mumkin.
6. M15.6: updater signature + OS signing + schema/data recovery uch alohida gate; corrupt update/refused downgrade/interruption fixtures.
7. M15.7: bitta frozen RC artifact uchun applicable end-to-end/install/upgrade/uninstall/native/locale/performance matrix; pilot defect repairdan keyin faqat invalid qamrov rerun.

QA/OS targetlar isolated hostlarda parallel; signing key va release manifesti yagona
ownerda. Testlar yuqoridagi itemga tegishli; final RC matrixning har iteratsiyada
qayta boshlanishi shart emas. Actual target, identity yoki license gate yo'q bolsa
shu release scope blocked/unverified; macOS yoki Server pass bilan yopilmaydi.
Exit: aniq support matrix va source/artifact/evidence; public publish alohida vakolat.

# Keyingi takliflar ham qamrovdan tushmaydi

Quyidagilar yoqotilgan yoki rad etilgan task emas, boshlash sharti bor decision backlog.
M13/M14/initial release old sharti qilib qo'shilmaydi.

| ID | Qaror va boshlash sharti | Keyingi ish/test |
| --- | --- | --- |
| D01 | Multi-root discovery: real userda bir nechta root ehtiyoji pilotda tasdiqlansin | identity/suppression/relocation conflict model → scanner/UI; root-switch/watch regression |
| D02 | pnpm/Yarn/Bun: real repository lockfile va talab | bittadan manager adapter, exact version/hooks/trust/platform test; npmni jim almashtirmaslik |
| D03 | Meilisearch yoki Typesense: Scout use case va provider feasibility | bitta provider/license/runtime/data lifecycle; actual search/readiness/stop/backup ehtiyoji |
| D04 | Deploy providers: target va foyda tasdiqlansin | bitta provider preview/auth/secrets/rollback/external-write scope; contract va disposable deploy |
| D05 | External DB data masking: raw copydan tashqari talab | schema/policy-aware masking, canary/relation validation; avtomatik toliq anonimlik va'dasi yo'q |
| D06 | Open-source/free/paid/support modeli, pilot auditoriyasi | BA tadqiqot va support/license xarajatlari; pricing/account/subscription qarorsiz qurilmaydi |
| D07 | Linux/Windows ARM, Docker-first yoki umumiy plugin marketplace | joriy scope tashqarisi; explicit talab va feasibility bo'lsa alohida roadmap, mavjud native stackni almashtirmaslik |

# Pilot va hajmni baholash

BA/Teach/System/QA uch gipotezani o'lchaydi: first-success; second-machine reproducibility;
scoped external-agent diagnosis. Known N+1/DB/queue/mail faults va controlled missing
runtime/secret/port conflict ishlatiladi. Success/failure, vaqt, yordam, retry, to'g'ri
root cause, irrelevant calls va stale-context xatolari yoziladi. Numeric performance,
conversion yoki narx targetlari baseline/tadqiqotdan keyin; soxta talab yaratmaymiz.
Data-loss/isolation/secret-disclosure critical holati qabulni bloklaydi.

M13.0dan keyin har ready item faol engineer-kun diapazoni bilan baholanadi; QA/native
host/signing/vendor kutish alohida. Har iterationga bitta integratsiya qilinadigan slice,
parallel oqimlar va test scope olinadi. Tooling/protocol/headless spike yakunlanmasdan
butun loyihaga qat'iy kalendar va'dasi berilmaydi.

# Yetti rol takliflarining traceability jadvali

| Rol | Takliflar qayerda bajariladi |
| --- | --- |
| Teach Lead | Current-state/docs drift va contributor E04; checklist E01; CLI help M13.2; explanatory doctor/bundle M13.5; team onboarding E02; locale/task pilot |
| Tech Lead | E00 support ledger; M13.0–.1 ops/owner; E07 catalog/offline; E06 backup policy; M13 API/mutations; M14 adapter/tunnel; M15 evidence/update |
| Software Engineer | M13.0 Rust/TS contracts; M13.1 ops; E03 Composer; M13.5 recovery; E07 lifecycle; E08 tasks; E05–.06 storage/drill; E09 performance |
| System Analyst | M13 state/review/cancel; E02 drift; E05 ownership; E06 drill; M14 session; E08 profiles; D01–.02 roots/package managers |
| Business Analyst | Pilot/value/positioning; E01–.02 first-success/portability; E05–.06 recovery; E08 profiles; D03 search; E07 offline; D04–.06 deploy/masking/business |
| QA | Impact-based reusable regression; E10 framework matrix; E09 performance; M13/M14 contract tests; M15 actual OS/credential/fault/update/licensing |
| Architect | Existing modular Rust boundary; M13 facade/owner/API/projection; M15 metadata migration; E07 maintenance; M14 exposure; M13.5 support artifact |

# Har slice uchun yakuniy topshirish

Integrator exact changed paths va behavior, bajarilgan relevant testlar, reuse qilingan
natija/sababi, native platform status, qoldiq blocker va keyingi ready IDni topshiradi.
Review scope bilan test tanlovi mosligini QA tekshiradi. OKF matching/changed/not
implemented/unverified reconcile qilinadi va strict validator prepared interpreterda
ishlaydi. Structural pass runtime proof emas. Code review/visual/native evidence bir xil
natija deb aralashtirilmaydi.[^policy]

Tarixiy planning bosqichida faqat hujjat/link/diff/strict tekshirildi. Keyingi bajarish
vakolati bilan product ishlari boshlandi; har slice actual natija va tekshiruv scope'i
bilan yopiladi. Reja mavjudligi implementatsiya tugaganini bildirmaydi.

[^request]: [Joriy ish tartibi topshirigi](references/conversation-decisions.md).
[^agents]: [Tashqi agentlar uchun batafsil manba reja](m13-external-agents-plan.md).
[^roadmap]: [M13–M15 ketma-ketligi](post-mvp-roadmap.md).
[^completion]: [Mavjud core va chegaralar](m7-m12-completion.md).
[^windows]: [OS/provider acceptance chegaralari](windows-compatibility.md).
[^policy]: [Knowledge reconciliation siyosati](knowledge-policy.md).
[^ui]: [UI skill routing](../../.agents/skills/devforge/references/ui-ux-workflow.md).
[^checks]: [Majburiy CI checks](../../.github/workflows/check.yml).
