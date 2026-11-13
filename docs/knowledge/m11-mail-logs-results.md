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
  - id: parser
    resource: https://docs.rs/mail-parser/1.0.1/mail_parser/
    title: Official MIME parser API and encoding support
  - id: sanitizer
    resource: https://github.com/cure53/DOMPurify/releases/tag/3.4.17
    title: Official stable HTML sanitizer release
---

# Scope va holat

M11 user topshirig'i bilan boshlandi. SMTP catcher va Mail/Logs ish sahifasi
implementatsiyasi bor; native qabul va source reconciliation holati quyida
saqlanadi. M12 avtomatik boshlanmaydi. No new public installer/release.

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
bilan backup + staged replacement update qilinadi. User-edited/foreign files yoki
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

Final native platform/GUI evidence va end-of-phase source table qo'shiladi.
M7.2 Xdebug, M15 signing/minimum-OS/native credential/system-dialog/redistribution
masalalari yopilmadi. GUI screenshots yoki unit pass public release dalili emas.
