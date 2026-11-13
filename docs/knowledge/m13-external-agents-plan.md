---
type: Implementation Plan
title: M13 tashqi AI agentlari uchun DevForge integratsiyasi
description: Agentdan mustaqil MCP va JSON CLI, umumiy boshqaruvchi, runtime konteksti va bosqichli qabul rejasi.
status: draft
decision_status: proposed
tags: [m13, external-agents, mcp, cli, automation, implementation-plan]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Tashqi agentlar uchun barcha foydali imkoniyatlarni rejalash topshirigi
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: M13 joylashuvi va M14 M15 chegaralari
  - id: core
    resource: ../../crates/workspace/src/lib.rs
    title: Mavjud shared workspace buyruqlari va supervisor hayot sikli
  - id: cli
    resource: ../../crates/cli/src/main.rs
    title: Mavjud CLI router va turli output shakllari
  - id: execution
    resource: ../../crates/projects/src/execution.rs
    title: Loyiha tanlagan PHP Node va executable argument konteksti
  - id: leases
    resource: ../../crates/platform/src/distribution.rs
    title: Mavjud app operation lease va minimum OS preflight
  - id: inspector
    resource: ../../crates/inspector/src/lib.rs
    title: Request va runtime event konteksti
  - id: mail
    resource: ../../crates/inspector/src/mail.rs
    title: Mavjud per-project SMTP va mail store
  - id: debugger
    resource: ../../crates/workspace/src/debugging.rs
    title: Mavjud debugger control va profile facade
  - id: knowledge-policy
    resource: /knowledge-policy.md
    title: Plan va implementation dalillarini ajratish
  - id: windows
    resource: /windows-compatibility.md
    title: Windows10 minimum va haqiqiy OS qabul chegarasi
  - id: mcp
    resource: https://modelcontextprotocol.io/specification/2026-07-28
    title: Tekshirish paytidagi rasmiy MCP spetsifikatsiyasi
  - id: stdio
    resource: https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio
    title: Local stdio transport va framing
  - id: http
    resource: https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http
    title: HTTP transport versiyalari va localhost talablari
  - id: authorization
    resource: https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization
    title: HTTP authorization kontrakti
  - id: rust-sdk
    resource: https://github.com/modelcontextprotocol/rust-sdk
    title: Rasmiy Rust MCP SDK
---

# Maqsad va vakolat

Foydalanuvchi DevForge imkoniyatlaridan tashqi AI agentlar foydalana olishini va
barcha foydali integratsiyalar uchun reja tuzishni topshirdi. Ichki agent chati,
prompt orqali kod yozdirish oynasi, model/provider ulash va LLM API kalitlarini
boshqarish bu scope'dan chiqarilgan. Bu chegaralar tasdiqlangan; pastdagi
arxitektura va bosqich tafsilotlari taklifdir. Ushbu ish rejalash va OKF yangilash;
M13 product kodi yoki installer yaratish boshlanganini bildirmaydi.[^request]

Maqsad: MCP qo'llaydigan tashqi clientlar uchun standart server, terminal
ishlata oladigan agentlar uchun barqaror JSON CLI. Biror model yoki vendor
SDK'si mahsulotning majburiy dependency'si bo'lmaydi. MCP/terminal imkoniyati yo'q
client uchun avtomatik moslik va'da qilinmaydi. Masofadagi agent mahalliy
`stdio` yoki `127.0.0.1` ga bevosita ulanmaydi: alohida, ruxsatli ulanish yo'li
kerak. Bu AI model provider API integratsiyasi emas.[^mcp][^stdio][^http]

# Joriy source bilan solishtirish

2026-11-13 kuni source111526d read-only tekshirildi; bu safar product testlari
ishlatilmadi. Oldingi runtime dalili joriy agent integratsiyasi ishlashiga dalil
sifatida olinmaydi.[^knowledge-policy]

| Qism | Hozir bor | M13 da qo'shiladi |
| --- | --- | --- |
| Rust workspace | Typed commands, sessions, runtime/service boshqaruvi.[^core] | Barcha callerlar uchun yagona boshqaruv va permission facade. |
| CLI | Runtime/project/site/inspect/debug/config/workspace/distribution; ko'p buyruq JSON chiqaradi.[^cli] | Versioned, bir xil automation contract, discovery/help/schema va facade bilan to'liq coverage. |
| Inspector/debugger/mail | Structured runtime events, native debugger/profile, SMTP va MIME store.[^inspector][^debugger][^mail] | Agent projection, cursor/correlation, bounded resources, wait/filter va redaction. |
| Project execution | Loyiha tanlagan runtime, executable va argument array'i.[^execution] | Trust/grant bilan boshqariladigan jobs, test artifacts va cancellation. |
| Process ownership | GUI Workspace va ayrim foreground CLI lifecycle'lari; operation lease mavjud.[^core][^leases] | Bir nechta agent/GUI o'rtasida single owner, persistent jobs va authenticated local IPC. |
| MCP | Workspace manifest/routerda MCP server yoki SDK topilmadi.[^cli] | stdio server, keyin Streamable HTTP va client compatibility qatlamlari. |

App/operation lease concurrent supervisor egasini kelishtiruvchi broker emas.
Har tool call uchun yangi `Workspace` yaratish yoki CLI'ni wrapperdan qayta-qayta
ishga tushirish service lifetime muammosini yechmaydi. Bu source inspectiondan
chiqqan arxitektura xulosasi; ayni race amalda reproduce qilindi degan da'vo emas.

# Taklif qilinadigan arxitektura

`Tashqi agent -> MCP adapter yoki JSON CLI -> local control API -> yagona manager -> mavjud Rust yadro`

Desktop ham shu manager bilan ishlaydi. MCP biznes mantiqni, credentiallarni
yoki alohida supervisorni egalab olmaydi. Haqiqiy IPC/policy boundary uchun
`devforge-control` moduli/crate'i va Rust MCP adapter uchun alohida modul/crate
ko'rib chiqiladi; nomlar implementationda aniqlanadi. Microservice platformasi
yoki umumiy plugin marketplace qurish talab qilinmaydi.

Manager bir foydalanuvchi/DevForge data root uchun bitta. macOS Unix domain
socket, Windows named pipe yoki teng kuchli user-scoped IPC ishlatiladi;
peer/ACL tekshiruvi majburiy. Client nomi yoki project path matnining o'zi grant
emas. Agent o'chganda manager Windows client Job Object/process tree'i bilan
birga nobud bo'lmasligi native test bilan isbotlanadi. Per-user ishga tushirish
mexanizmi va crash recovery M13.1 spike'ida tanlanadi.

Mavjud Quit ma'nosi ochiq saqlanadi: foydalanuvchining explicit Quit'i managed
amallar va owned servislarni tartibli to'xtatadi, clientlarga sababni qaytaradi.
Ulangan agentlarning retry'i darhol manager'ni yana ko'tarmaydi. Oddiy MCP client
disconnect'i butun muhitni to'xtatmaydi. Har job uchun disconnect/cancel siyosati
aniq: vaqtinchalik job xavfsiz nuqtada bekor qilinadi yoki oldindan tanlangan
durable rejimda operation ID orqali kuzatiladi. Bu lifecycle o'zgarishi alohida
regressiya va migration tekshiruviga ega.[^core][^leases]

# Agentga ochiladigan imkoniyatlar

Quyidagi nomlar tool oilalari uchun taklif; hozirgi CLI buyruqlari deb
ko'rsatilmaydi. Har biri faqat grant berilgan loyiha va imkoniyatlar bilan ishlaydi.

| Oila | Qo'shiladigan amallar |
| --- | --- |
| Health/capabilities | App/OS/runtime holati, port conflict, available tools/provider capabilities, actionable doctor va qo'llanmaydigan imkoniyat sababi. |
| Projects/discovery | Scoped list/get, safe scan/import, effective PHP/Node/DB/service/domain ma'lumoti, manifest inspection, forget/restore review. |
| Runtimes/PHP | Catalog/install/probe, loyiha bo'yicha PHP/Node tanlash, extension/INI diagnostikasi, dependency consumerlar va xavfsiz remove plan. |
| Services/processes | Project start/stop/restart; DB/cache, Vite/queue/mail holati, readiness, output va ownership. Boshqa loyiha/shared consumer saqlanadi. |
| Environment/config | Redacted get/check; env.example farqlari, patch preview/apply, secret reference bind, devforge.yaml import/export/setup/status. |
| Domains/HTTPS | Owned URL/port, domain plan, sertifikat holati, conflict, review qilingan hosts/trust apply va cleanup. OS elevation tor helperda qoladi. |
| Database | Metadata/connection test, owned test DB/role, bind/switch, schema inspect va cheklangan read-only query. Clone, backup, export/import/restore generation review bilan. |
| Logs/errors | Oxirgi exception, stack/file/line/context, level/time/search filters, cursor pagination va yangi xatolarni kutish. |
| Inspector/performance | Request/job/CLI trace, SQL/N+1/duplicate/slow queries, dump/dd, HTTP/cache events, capture toggle, profile summary va before/after solishtirish. |
| Debugger/profiler | Capability/status, optional Xdebug install/mode, breakpoints, start/pause/step/continue, stack/scopes/locals, bounded evaluate; mutation existing revision va alohida grant bilan. Profile/history/export. |
| Mail | SMTP holati, scoped list/show/wait, sender/recipient/subject/time filters, verification/reset URL ajratish, attachment metadata/controlled export, reviewed delete. |
| Execution/tests | Loyiha runtime'i bilan PHP/Artisan/Composer/Node/npm/Git invocation; test runner, exit code, bounded output, timeout/cancel, artifact references. |
| HTTP verification | Managed site'ga HTTP request/probe, readiness/TLS/response summary va trace correlation. Redirect/host chegarasi saqlanadi; tokenlar reference orqali beriladi. |
| Workspaces | Plan/create/list, own/shared/clone/empty DB, isolated services/domain/ports; conflict/revision check bilan merge, reviewed archive/restore/purge. |
| Operations/events | Job ID/status/progress/cancel, durable receipts, resumable bounded event cursor, expired cursor uchun aniq resync holati. |
| Agent context/artifacts | Project summary/runbook, runtime capability, diagnostics bundle va allowed source/test artifact URI'lari. Barcha filesystemga umumiy o'qish eshigi ochilmaydi. |

DB schema/query va mail wait/link extraction kabi yetishmayotgan amallar yangi
typed adapter sifatida qo'shiladi; mavjud UI borligi agent API tayyorligini
bildirmaydi. Browser ochish, sahifani bosish va kod yozishni tashqi agent o'z
vositalari bilan bajaradi. DevForge to'g'ri URL, execution context va server
dalillarini beradi. Kelgusida frontend console/network artifact bridge qo'shish
mumkin; buning uchun chat, Studio yoki o'z browser automation engine'i shart emas.

# Umumiy kontrakt va ruxsatlar

1. JSON CLI: `api_version`, `request_id`, project/operation ID, `ok`, typed
   `data/error`, warnings, pagination. Error code va field nomlari tilga bog'liq
   emas; inson uchun message/UI en/ru/uz katalogidan. Eski CLI consumerlari uchun
   explicit compatibility saqlanadi; yangi envelope yashirin breaking change emas.
2. MCP: standart JSON-RPC envelope saqlanadi, tool input/output schema, structured
   result va truthful annotations. `stdout` faqat protocol; log `stderr`da.
   Core tool/resource yo'li barcha clientlarda; task/subscription/elicitation
   kabi optional feature bo'lmasa polling/cursor fallback bor.[^stdio][^mcp]
3. Har chaqiruv project ID, tekshirilgan canonical root va grant bilan bog'lanadi.
   Traversal, junction/symlink escape, boshqa project ID yoki private key resource
   orqali chegarani aylanib o'ta olmaydi. Client-reported roots authority emas.
4. Observer, project-manage, execution/test va sensitive-operation ruxsatlari.
   Loyiha/ruxsat bir marta berilib saqlanishi va bekor qilinishi mumkin; oddiy
   har tool call uchun qayta dialog talab qilinmaydi. Default scope tor va o'qish.
5. Env/credentiallar default masked; child processga broker ichida inject qilinadi.
   Destructive DB/workspace/runtime amali va system trust uchun scoped plan,
   revision/digest, muddati va haqiqiy authorization kerak. Agent yuborgan
   `confirm:true` yoki tool annotation foydalanuvchi roziligini isbotlamaydi.
6. Request idempotency key, optimistic revision va resource-specific lock.
   Retry ikkinchi DB/project/process yaratmaydi; busy/conflict/recovery alohida kod.
7. Output byte/row/time limit, pagination, cancellation va redacted audit trail.
   Xom log, mail yoki source ichidagi matn instruction emas; undan yangi grant
   chiqarilmaydi. Redaction mutlaq maxfiylik kafolati deb ko'rsatilmaydi.
8. SQL read-only alohida cheklangan DB role/connection bilan. `SELECT` prefixi
   yoki rollback o'zi write sandbox emas; DB'ga xos DDL/side-effect cheklovlari
   tekshiriladi. Admin DB credential agentga berilmaydi.
9. Executable allowlistning o'zi PHP/Artisan/npm/Composer hook'larini xavfsiz
   qilmaydi: ular loyiha kodini bajaradi. Project trust, execution grant, aniq
   argument/cwd/env va process/output limit birga ishlaydi. Unrestricted shell
   MCP default tool'i bo'lmaydi. Terminali keng huquqli agentning tashqi shell
   amallarini MCP grantlari to'liq cheklaydi deb va'da qilinmaydi.[^execution]

# Ulanish va client mosligi

Avval local stdio, keyin Streamable HTTP. Rasmiy Rust SDK (`rmcp`) implementation
uchun nomzod; aniq mos stable versiya manifest/lockfileda tanlanadi.[^rust-sdk]
Tekshirish paytida rasmiy latest2026-07-28 edi; undan oldingi clientlar uchun
version/capability compatibility alohida tekshiriladi. Shu yangi spetsifikatsiya
session/HTTP oqimini o'zgartirgan: eski handshake/session modelini barcha
versiyalarga bir xil tatbiq qilish mumkin emas. DevForge grant/job session'i
protocol session'idan mustaqil. Versiya bu reja bilan abadiy pin qilinmaydi.[^mcp][^http]

HTTP local endpoint faqat loopback, autentifikatsiya, Origin validation va
client/project scope bilan. OAuth-capable clientlar uchun standart metadata/flow,
mos local client uchun out-of-band berilgan scoped credential varianti; token
model provider kaliti emas. Public `0.0.0.0` yoki umumiy tunnel avtomatik ochilmaydi.
Masofaviy agent ulanishi M14 bilan bog'liq optional secure bridge/TLS/auth qabuli;
local M13 core undan mustaqil tugaydi.[^http][^authorization][^roadmap]

Codex, Claude Code, Cursor, Gemini CLI, Copilot va OpenCode uchun clientga xos
yo'riqnoma/config generator taklif qilinadi. Haqiqiy format o'sha clientning
rasmiy hujjati bilan implementation vaqtida tekshiriladi. Generic MCP JSON va
JSON CLI fallback majburiy; barcha clientlar hozir testdan o'tgan deb aytilmaydi.
Config mavjud MCP entries yoki AGENTS.md'ni almashtirmaydi: project-scope diff,
managed snippet va backup bilan. Global PATH yoki agent sozlamasi avtomatik
qayta yozilmaydi.

# Bajarish bosqichlari

| Bosqich | Natija va qabul mezoni |
| --- | --- |
| M13.0 — Contract va inventar | Har tool uchun existing/new mapping, schema/error/scope/side-effect, ownership/lifecycle ADR va acceptance fixture. Scope faqat tashqi agent. |
| M13.1 — Umumiy manager | Authenticated user IPC, single owner, operation store, project locks/grants, headless start va explicit Quit/crash recovery. GUI+2 client bir service'ni ikki marta ochmaydi. |
| M13.2 — JSON CLI | Versioned outputs/help/schema/capabilities, status/list/inspect; facade orqali chaqiruv, eski CLI compatibility. GUI yopiq bo'lsa ham to'g'ri natija. |
| M13.3 — stdio MCP | Tools/resources, project binding, pagination/redaction, protocol compatibility va generic config. MCP Inspector va kamida2 actual clientda read-only oqim. |
| M13.4 — Muhit amallari | Runtime/project/service/process/env/domain/HTTPS va DB amallari; reviewed apply, readiness, cancellation va audit. A loyihadagi amal B/shared DB'ni buzmaydi. |
| M13.5 — Diagnostika va mail | Log/error/query/HTTP/job/cache/dump/profile projection, mail wait/link, request correlation va before/after cursor. Yangi request dalili eski natija bilan aralashmaydi. |
| M13.6 — Test/debug/workspace | Managed exec/tests, isolated DB/workspace, HTTP probe, debugger control, backups/recovery va artifacts. Tuzatish tashqi agentda, tekshirish real muhitda. |
| M13.7 — HTTP MCP | Streamable HTTP, auth/Origin/revocation, eski-yangi protocol mosligi va unsupported feature fallback. Remote bridge alohida M14 dependency. |
| M13.8 — Ulanish UX va docs | en/ru/uz Settings: agent ulash, loyiha/ruxsat tanlash, active operations/audit, revoke. Client config export, curated guide, packaged binaries. Chat/provider/model picker qo'shilmaydi. |
| M13.9 — Yakuniy qabul | Quyidagi client/OS/end-to-end matritsasi, M7–M12 regressiyasi, source/evidence reconciliation va strict OKF. |

Har bosqichda faqat kerakli test va OKF yangilanadi; bitta bosqich tugagani
butun M13 tugaganini bildirmaydi. M14/M15 qayta raqamlanmaydi.

# End-to-end qabul matritsasi

- Transport: haqiqiy stdio client va HTTP client, protocol parser, output schema,
  supported eski/yangi protocol, pagination, cancellation, restart va retry.
  Optional extension yo'q clientda core workflow ishlashi kerak.
- Client: kamida Codex + boshqa vendor MCP client + faqat terminal/JSON CLI
  client. Qolgan nomlar alohida tested/unverified jadvalda, o'rnatilgan actual
  client versiyasi bilan. Config generatsiyasi actual ulanishning o'rniga o'tmaydi.
- OS: macOS ARM64 va Intel; Windows x64 current desktop va minimum Win10
  22H2/build19045. CI Server2025 alohida evidence; userda Win10 host/VM yo'q,
  bu gate actual host kelguncha unverified qoladi. macOS minimum13.5 actual
  acceptance ham current hosted OS muvaffaqiyati bilan yopilmaydi.[^windows]
- Lifecycle: GUI + ikki agent, shared service consumer, port conflict, client
  kill/EOF, manager crash, stale PID, explicit Quit, cancel/reconnect. Windows
  Job va macOS process owner/IPC huquqlari actual native tekshiriladi.
- Isolation: A/B projects, denied/revoked grant, stale plan/revision, path escape,
  untrusted repo execution, duplicate retry, redacted token/cookie/env/mail,
  audit va bounded noisy output. Revoke yangi amallarni to'xtatadi; running
  operationni cancel qilish natijasi alohida ko'rsatiladi.
- Functional: runtime install/select; project start/readiness/TLS; owned test
  DB/migration; queue/Vite failure output; actual Laravel HTTP SQL/N+1;
  mail verification link; Xdebug step/locals/profile; workspace+clone DB;
  staged backup/restore va boshqa loyiha/project-data saqlanishi.
- Packaging: release stdio stdout toza, absolute executable paths Unicode/space
  bilan, app update/uninstall vaqtida active job lease, credentials saqlanishi;
  protocol/API compatibility ma'lum. Signing/licensing va minimum OS M15 bilan
  alohida, qayta yozilmagan release gate'lar.

# Rejalash yakunidagi reconciliation

| Da'vo | Holat |
| --- | --- |
| M7–M12 core imkoniyatlari qayta ishlatiladi | mos: source bor; agent API hali yaratilmagan. |
| M13 ichki chat/provider yo'nalishi | o'zgargan: joriy user topshirig'i bilan tashqi agent integratsiyasiga toraytirildi. |
| MCP/broker/stable agent contract | amalga oshirilmagan: shu hujjat implementation rejasi. |
| Agent/client/OS matritsasi | tekshirilmagan: ushbu planning ishida runtime/client suite ishlatilmadi. |

Hozirgi vazifa natijasi plan va OKF; faqat strict knowledge validator ishlatiladi.
Product test/CI, binary build, runtime sozlash, commit yoki push planning scope'ga
avtomatik qo'shilmaydi. Implementation alohida bajarish bosqichidir.

Planning tekshiruvi: tayyor tooling Python bilan
`scripts/validate_knowledge.py --strict` —32 documents,0 errors,0 warnings;
`git diff --check` otdi. Bu format/havola tekshiruvi; MCP yoki native platforma
qabuli emas. Source/model/provider konfiguratsiyasi o'zgartirilmadi.

[^request]: [Tashqi agent integratsiyasi topshirigi](references/conversation-decisions.md).
[^roadmap]: [M7–M15 roadmap](post-mvp-roadmap.md).
[^core]: [Shared workspace core](../../crates/workspace/src/lib.rs).
[^cli]: [CLI router](../../crates/cli/src/main.rs).
[^execution]: [Project execution](../../crates/projects/src/execution.rs).
[^leases]: [App va operation lease](../../crates/platform/src/distribution.rs).
[^inspector]: [Inspector](../../crates/inspector/src/lib.rs).
[^mail]: [Mail store](../../crates/inspector/src/mail.rs).
[^debugger]: [Debugger facade](../../crates/workspace/src/debugging.rs).
[^knowledge-policy]: [OKF siyosati](knowledge-policy.md).
[^windows]: [Windows compatibility](windows-compatibility.md).
[^mcp]: [MCP specification](https://modelcontextprotocol.io/specification/2026-07-28).
[^stdio]: [stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio).
[^http]: [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http).
[^authorization]: [Authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization).
[^rust-sdk]: [Official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk).
