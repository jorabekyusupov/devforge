---
type: Implementation Report
title: M11 mail va kengaytirilgan loglar
description: Authenticated local SMTP MIME preview va M7 asosidagi structured log exception viewer.
status: draft
decision_status: confirmed
tags: [m11, mail, smtp, mime, logs, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M11 boshlash topshirigi
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: Mail catcher va rich log viewer scope
  - id: mail
    resource: ../../crates/inspector/src/mail.rs
    title: Bounded authenticated loopback SMTP persistent inbox and MIME
  - id: logs
    resource: ../../crates/inspector/src/logs.rs
    title: Full retained event filtering pagination and scope
  - id: orchestration
    resource: ../../crates/workspace/src/mail.rs
    title: Owned mail lifetime and project-scoped mailbox operations
  - id: environment
    resource: ../../crates/projects/src/mail.rs
    title: Standard Laravel SMTP overrides with local credentials
  - id: schema
    resource: ../../crates/projects/src/project.rs
    title: Schema4 and default-disabled mail field
  - id: capture
    resource: ../../packages/laravel-inspector/src/Capture.php
    title: Context redaction standalone logs exception location and trace
  - id: install
    resource: ../../crates/inspector/src/lib.rs
    title: Recognized old companion upgrade with backups and edit protection
  - id: ui
    resource: ../../apps/desktop/src/pages/ActivityPage.tsx
    title: Localized inbox attachment preview and structured log workspace
  - id: native
    resource: ../../crates/workspace/examples/support/activity.rs
    title: Real Laravel SMTP queue and log acceptance
  - id: desktop
    resource: ../../scripts/m11_desktop_smoke.py
    title: Native W3C controls HTML sandbox remote-request trap and locale checks
  - id: local-checks
    resource: ../evidence/m11/local-checks.json
    title: Local core and default-feature build results
  - id: local-native
    resource: ../evidence/m11/local-native.json
    title: Local real Laravel SMTP and queue acceptance
  - id: local-desktop
    resource: ../evidence/m11/local-desktop.json
    title: Local WKWebView controls and axe results
  - id: native-ci
    resource: ../evidence/m11/ci-native.json
    title: Three successful native jobs and verified artifact digests
  - id: core
    resource: ../evidence/m11/ci-core.json
    title: Final macOS Windows full core and default debug builds
  - id: attempts
    resource: ../evidence/m11/ci-attempts.json
    title: Initial Windows legacy upgrade failure and scoped correction
  - id: parser
    resource: https://docs.rs/mail-parser/1.0.1/mail_parser/
    title: Official MIME parser API and encoding support
  - id: sanitizer
    resource: https://github.com/cure53/DOMPurify/releases/tag/3.4.17
    title: Official stable HTML sanitizer release
---

# Scope va holat

M11 SMTP catcher va Mail/Logs ish sahifasi funksional qabuldan o‘tdi:
macOS ARM64/Intel va Windows x64 real backend/native UI dalillari hamda source
reconciliation quyida saqlanadi. M12 avtomatik boshlanmaydi. No new public installer/release.

# Mail service va data chegarasi

Native Rust SMTP sink existing inspector/core boundary'da, per project loopback
listener. Tashqi provider/Go runtime yoki OS service install yo'q. Standard EHLO,
AUTH PLAIN/LOGIN, MAIL/RCPT/DATA, dot-stuffing, RSET/NOOP/QUIT; authsiz DATA qabul
qilinmaydi. SMTPUTF8 va8BITMIME qabul qilinadi, STARTTLS taklif qilinmaydi.
Har server faqat127.0.0.1, max8 connection, idle15s/connection120s/100 command,
max100 recipient/2MiB raw message/50 attachment. Service hech qachon DNS lookup,
external SMTP forwarding yoki mail relay qilmaydi.

Credential per-project OS SecretStore'da; project JSON, session, status va UI'da
raw password yo'q. Username project ID. Fixed allocated local port private metadata'da
saqlanadi; restart boshqa listener'ga ulanmaydi yoki uni to'xtatmaydi. Lock va
instance receipt active session'ni ajratadi; thread readiness va owner Drop/join
listener lifetime'ni boshqaradi. Mail enable/disable uchun project web/worker stopped
bo'lishi kerak; window close existing tray lifetime'ni o'zgartirmaydi, Quit serverni
to'xtatadi. Project forget faqat uning mail listener'ini to'xtatadi; inbox saqlanadi.

Per-project SQLite inbox marker/ownership bilan, private directory, secure_delete
va auto_vacuum. Latest200 messages va64MiB raw content retention. Envelope recipient
ham saqlanadi. Mail HTML/text/attachments user preview uchun raw content'dan parse
qilinadi, avtomatik secret-redaction emas: local email o'zi maxfiy data saqlashi mumkin.
Mail list search Unicode lowercased subject/sender/recipient,25/page. MIME parser
mail-parser1.0.1 `full_encoding`, declared Rust1.98 MSRV with toolchain1.99 compatibility.
Existing resolved zeroize1.9.1 va unrelated runtime catalog/dependency versiyalari
saqlandi; MIME parsing uchun yangi dependencies alohida lockfile'da.

Standard Laravel managed context `MAIL_MAILER=smtp`, loopback host/port, username,
password va scheme'ni beradi. `MAIL_URL`/encryption Laravel null sentinel bilan
clearlanadi: empty URL native test'da transport override xatosi keltirgan.
`.env` GUI'dan avtomatik yozilmaydi. M10 `mail: optional bool` export/plan/setup va
explicit env patch orqali ishlaydi; absent field old manifest uchun existing mail
choice'ni saqlaydi. Frontend contexts `MAIL_*` olmaydi. PHP tooling dependencies
uchun mail service shart emas; actual runtime mail context active receipt talab qiladi.
Bular standard MAIL_* routing; custom/hardcoded application transport'larini
network firewall kabi bloklash da'vo qilinmaydi.

Project metadata schema3 ->4: absent JSON `mail` false; older app yangi schema'ni
rad etadi. Tested downgrade/rollback emas, M15 gate ochiq.

# Preview va export

Mail & logs sahifasi existing semantic theme, system typography va sidebar'da.
RU/UZ/EN catalogs, registry orqali future locale; original mail data tarjima qilinmaydi.
HTML preview DOMPurify3.4.17 HTML-only allow profile, active tags/forms/URLs va
resource attributes olib tashlangan; empty sandbox, no-referrer va child CSP
script/connect/image/default sources none. Remote resources/links yuklanmaydi.
Inline images HTML'da ochilmaydi, attachment sifatida alohida ko'riladi. HTML/plain
body preview65,536 chars bilan cheklangan va truncation disclosed.

Attachment image PNG/JPEG/GIF/WebP yoki text/JSON/XML preview. Text64KiB byte preview;
SVG, PDF, archive va executable format'lar active embed qilinmaydi; unsupported preview
uchun Save fallback. Parts2MiB raw message chegarasida. Native Save dialog user path
oladi; private tempfile + non-overwrite write; filename traversal part ID sifatida
ishlatilmaydi. Native file-picker dialogs avtomatik tekshirilgan deb da'vo qilinmaydi.
Mail delete/clear project-scoped va UI review bilan; disable/restart inbox'ni o'chirmaydi.

Stable target: [activity reference](../../design-system/devforge/reference/activity-target.png),
1536x1024; illustrative fixtures. Established shell/presentation preserved, unsafe
mail button/link'lari targetdagi dekorativ ma'nodan actual isolated preview'ga
moslashtirilgan. Pixel-perfect claim yo'q. ui-ux-pro-max search'ning blue/Poppins
palette/font taklifi existing MASTER ustidan yozilmadi; four installed UI skills
scoped workflow'da ishlatildi, paid Uizze resources olinmadi.

# Log va exception

M7 opt-in authenticated event store saqlangan. Full bounded retained history'dan
log/exception query, level/type/1h-6h-24h filters, Unicode substring va50/page.
Request/job context correlation Inspector'dagi context filter'ga olib boradi.
Structured context va exception class/message/file basename/line/stack argument'larsiz
ko'rsatiladi. Console/standalone logs ham per-context olinadi; offline circuit
instrumentation failure'ni application failure'ga aylantirmaydi.

Companion0.2.0 meaningful structured context, SMTP/Redis known credential redaction,
exception location va log scopes qo'shadi. Context sensitive-key/value sanitization
M7 kabi bounded/heuristic, universal PII/secret detector emas. DB QueryException
bindings yoki raw SQL values transport qilinmaydi. Recognized0.1.0 bundle SHA'lari
bilan backup + staged replacement update qilinadi. Windows Git checkout CRLF sabab known legacy hash bilan mos kelmagan; faqat
tanilgan UTF8 bundle source uchun LF/CRLF canonical comparison qo‘shildi, unit
fixture har OS’da CRLF bilan yoziladi. User-edited/foreign files yoki
symlinks overwrite qilinmaydi. Existing M7 event history va provider opt-in saqlanadi.

# Tekshiruv holati

Local core SMTP auth, cross-project denial, MIME charset/HTML/text/attachment,
message limit, persistence/port/shutdown, foreign-root refusal tests o'tdi. Log
search complete retained history va redacted context bilan tekshirildi; recognized
legacy upgrade va user edits refusal test'i bor. Native Laravel CLI/queue mail,
log/exception va attachment export fixture bor. Initial prototype UI harness race,
invalid PNG fixture va explicit capture-enable sequence tuzatildi; ular product
success evidence sifatida ko'rsatilmaydi. Parallel Cargo feature build/doc-test
artefact collision'dan keyin local checks ketma-ket bajariladi.

Local `cargo test --workspace`89 passed/1 ignored; frontend29, locale3 va
packaging6 checks o'tdi; fmt/clippy/typecheck ham o'tdi. Default-feature CLI va
embedded-frontend Tauri **debug/no-bundle** build final product source bilan
tayyorlandi. Bu release build yoki yangi tester installer emas. Full suite yakuniy
hujjat/publication commit uchun qayta ishga tushirilmaydi.

# Source bilan bosqich solishtirishi

| Holat | Da'vo | Source va dalil |
| --- | --- | --- |
| Mos | Bounded authenticated local SMTP, MIME va scoped inbox | `crates/inspector/src/mail.rs`, auth/isolation/limits/persistence unit tests va `activity-native` real Laravel/queue fixture. |
| Mos | Managed mail env, M10 boolean va secret isolation | `crates/projects/src/mail.rs`, `crates/workspace/src/mail.rs`, `manifest.rs`; native env-unchanged, frontend-credential-filter va manifest-no-credential assertions. |
| Mos | Mail HTML/plain/attachment va Logs/context UI | `ActivityPage.tsx`, `InspectorPage.tsx`, `activity.test.tsx`, native W3C smoke. Safe HTML iframe, trap, preview va context navigation tekshiriladi. |
| Mos | Structured capture va full retained log filters | `packages/laravel-inspector/src/Capture.php`, `crates/inspector/src/logs.rs`; standalone/queue/exception assertions hamda context-before-limit unit. |
| O'zgargan | Schema3 ->4, companion0.1 ->0.2 | `crates/projects/src/project.rs` va inspector materialize; old JSON default false, known bundle staged backup/upgrade, CRLF canonical check. |
| Amalga oshirilmagan | Arbitrary custom SMTP transportni global bloklash; PDF/SVG active preview | Managed MAIL_* routing mavjud; network firewall va active document renderer bu scope'da yo'q. Unsupported attachment Save bilan olinadi. |
| Tekshirilmagan | OS Save dialog va signed installer/update/rollback | Backend export byte equality tekshirildi; native file-picker GUI, minimum OS, native credentials va distribution recovery M15 gate'lari. |

# Core CI va source chegarasi

Final core source `f18da61e19eecea2019ec8872d2981c62e8316d7`, run
[43341821991](https://github.com/jorabekyusupov/devforge/actions/runs/43341821991).
macOS job131611711110 va Windows job131611711449: har OS89 Rust passed/1 ignored,
29 frontend,3 catalogs,6 packaging, fmt/clippy/typecheck va embedded frontend bilan
default-feature debug/no-bundle desktop build o'tdi. Strict OKF26/0error/0warning.
Per-job counters va log hashes [core evidence](../evidence/m11/ci-core.json)'da.
Ignored credential test native SecretStore/system-UI acceptance o'rnini bosmaydi.

Dastlabki `ab07756c3ad7d51dc58a59e5d0dbdf41efc0ed3f` core run43340463033'dagi
Windows legacy-upgrade CRLF failure'i [attempts](../evidence/m11/ci-attempts.json)'da
saqlandi. Final fix faqat known UTF8 bundle LF/CRLF comparison va CRLF test fixture:
`git diff ab07756 f18da61 -- crates/inspector/src/lib.rs`. Normal SMTP/MIME/UI source
o'zgarmagan. Native acceptance run43340467842 oldingi `ab07756` source'da; CRLF
upgrade fix final core matrix'da alohida tekshirildi. Bir run barcha keyingi
commitlarni tekshirgan deb da'vo qilinmaydi.

# Native platforma qabuli

[Run43340467842](https://github.com/jorabekyusupov/devforge/actions/runs/43340467842)
uch targetda muvaffaqiyatli tugadi. Har birida actual managed PHP/Node/PostgreSQL,
Laravel CLI va queue orqali SMTP mail, log va exception yuborildi; keyin haqiqiy
WKWebView/WebView2 desktop controls debug-only W3C harness orqali tekshirildi.

| Target | Job | Backend / UI assertions | Axe holatlari / topilma | Katta viewport actual |
| --- | --- | --- | --- | --- |
| macOS ARM64 | 131607221666 | 11 /8 o'tdi | 15 /0 | 1536x656 |
| macOS Intel x64 | 131607221882 | 11 /8 o'tdi | 15 /0 | 1536x942 |
| Windows x64 | 131607221487 | 11 /8 o'tdi | 15 /0 | 1536x1024 |

Har targetda ru/uz-Latn/en, minimum820x620, light/dark, saved preference relaunch,
HTML/plain, image/text preview, clear-review cancel, structured log filter/redaction,
exception stack va request/job context navigation tekshirildi. Malicious HTML script
execute bo'lmadi va remote-resource trap har targetda0 so'rov qayd etdi. Native
attachment export byte equality o'tdi; OS Save dialog GUI avtomatlashtirilmadi.

Hosted Mac katta oynasi runner ekran balandligi bilan cheklangan; requested1536x1024
bilan actual teng deb ko'rsatilmaydi. Minimum820x620 uch platformada exact o'tdi;
local Mac1536x1024 ham tekshirildi. Screenshots:
[local Mac mail](../evidence/m11/mail-macos-light.png),
[Mac exception](../evidence/m11/logs-macos-exception.png),
[Intel mail](../evidence/m11/mail-macos-intel.png),
[Windows mail](../evidence/m11/mail-windows-light.png).

Uch artifact ZIP SHA256 GitHub digest bilan aynan mos:

| Artifact | ID | SHA256 |
| --- | --- | --- |
| m11-macos-arm64 | 13999582772 | `4513310116ecd67c0791574d164dab477db951e23c0865d0d75d9a59fa91b821` |
| m11-macos-x64 | 14001292483 | `00ff4e9f4fb5ba1a2a9fb97e877a5f433c84f673ac7b85b2caa36317bf72c4d9` |
| m11-windows-x64 | 13999198222 | `8d3d15b581c4be2c09ceba22ed7820b201de8e6adb94cc603e6af1a176b1e7ff` |

[Native evidence](../evidence/m11/ci-native.json) per-job backend/desktop report,
JSON SHA256, actual viewport va artifact digest'larini saqlaydi. Workflow
`.github/workflows/activity.yml`; backend command `native_workspace activity-native`,
desktop command `python scripts/m11_desktop_smoke.py ...`. Test harness default
production features'da yo'q; alohida default CLI/desktop build o'tdi. Local private
fixture xizmatlari yakunlanganidan keyin fixture directory o'chirildi. Faqat public
reports va synthetic screenshot'lar commit qilinadi; mail DB/credentials upload yo'q.
Yakuniy docs push uchun product suite takrorlanmadi.

M7.2 Xdebug, M15 signing/minimum-OS/native credential/system-dialog/redistribution
masalalari yopilmadi. GUI screenshots yoki unit pass public release dalili emas.
