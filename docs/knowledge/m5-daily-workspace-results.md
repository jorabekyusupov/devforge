---
type: Implementation Report
title: M5 kundalik boshqaruv interfeysi
description: Project lifecycle coordinator, native desktop workspaces va M5 verification holati.
status: draft
decision_status: confirmed
tags: [m5, desktop, projects, lifecycle, ui, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M5 bosqichini bajarish topshirig'i
  - id: coordinator
    resource: ../../crates/workspace/src/lib.rs
    title: Framework-independent daily workspace orchestration
  - id: ui
    resource: ../../apps/desktop/src/pages/WorkspacePage.tsx
    title: Real project runtime service log controls
  - id: bridge
    resource: ../../apps/desktop/src-tauri/src/workspace.rs
    title: Worker-thread IPC cache cancellation va graceful Quit
  - id: native
    resource: ../../scripts/m5_desktop_smoke.py
    title: Native WKWebView WebView2 UI assertions
  - id: target
    resource: ../../design-system/devforge/pages/projects.md
    title: M5 visual target and incumbent system decisions
---

# Scope va implementatsiya

Foydalanuvchi M5 bosqichini bajarishni topshirdi.[^request] Oldingi yetkazish
ketma-ketligi bo'yicha code/test/OKF va push davom ettiriladi; M6 installer,
signing yoki boot auto-start bu topshiriqqa kirmaydi.

`crates/workspace` real project/runtime/database services'ni birlashtiradi.
Tauri blocking ishlarni worker'da bajaradi; UI snapshot cached, busy va ready
holatlari ajratilgan. Project serving sessiyalari alohida; bitta project Stop
boshqa project Caddy/PHP/Vite/queue yoki shared PostgreSQL'ni stop qilmaydi.
Database Stop active consumer borida rad etiladi. Quit faqat app-owned
sessiya va PostgreSQL'ni graceful to'xtatgach chiqadi.[^coordinator][^bridge]

Projects/details, Runtimes, Services/logs, Settings va Diagnostics bor.
Import read-only, configure running project'ni scoped restart qiladi.
Runtime install/remove/cancel/verify, DB assign/test, own queue va Vite toggles,
actual log filters, browser/VS Code/folder/terminal launch adapterlari ulangan.
Error, empty, loading, disabled, retry va confirmation holatlari uch katalogdan
olinadi. Raw log/path/name/versions tarjima qilinmaydi.[^ui]

Mavjud MASTER.md identity saqlandi. Generated target1487×1058 content pixels;
placeholder data/version/log timestamps real dalil deb ishlatilmadi. Native
same-size va820×620 capture/interaction qamrovi natijalarda ajratiladi.[^target]

# Native tekshiruv holati

Task-local Mac real Laravel13.42: parallel projects, own PG databases, real queue
job execution, Vite startup/hot cleanup, project A PHP switch while B master PID
stays unchanged, redacted logs va reopen persistence o'tdi. FPM ondemand child
PID naturally retires; isolation testi master process identity bilan tekshiradi.
Native UI loop import error recovery, controls, DB test, version switch,
close-to-tray, ru/uz/en, light/dark, minimum size va normal/forced reopen'ni
bajaradi. Test-only filesystem SecretStore actual keychain UI o'rniga o'tmaydi.
Windows va final source native natijasi pending; workflow mavjudligi pass emas.[^native]

# Lifecycle va verification chegarasi

macOS owned process group uchun private stdin-pipe watchdog app o'lganda cleanup
qiladi; normal Stop uni release qiladi. Windows Kill-on-close Job ishlatadi.
Vite hot file crash recovery faqat private owner receipt, unchanged file stamp/
URL, per-project lock va free private listener holatida amalga oshadi; foreign
hot file yoki live port saqlanadi. App reopen auto-start qilmaydi.

E2E feature debug-only; release instrumentation rad etiladi. Isolated data root
majburiy, WebDriver127.0.0.1'da. IPC mock qilinmaydi. Page-load marker faqat
test build'da; production build'da HTTP test endpoint va fixture SecretStore yo'q.
GUI observations source/unit/browser-preview bilan aralashtirilmaydi.

# Phase reconciliation

- **Mos:** own resource/process boundary, shared project context va three-locale registry.
- **O'zgargan:** M4 foreground-all CLI bilan birga per-project desktop lifecycle bor.
- **Tekshirilmagan:** final hosted native/UI acceptance, minimum OS/Intel va optional external editor availability.
- **Amalga oshirilmagan:** M6 signed installers, boot auto-start, long-term leaf renewal va physical trust metadata cleanup.

[^request]: Joriy M5 ko'rsatmasi.
[^coordinator]: Shared services va per-project ownership invariants.
[^bridge]: Mutex serialization, nonblocking cached status, cancellation va Quit.
[^ui]: Typed real backend commands; illustrative frontend mocks faqat unit fixture'larida.
[^native]: Actual native WebViews; isolated acceptance build'dan tashqarida driver yo'q.
[^target]: ui-ux-pro-max, ui-taste/uizze-ui target/craft va scoped source review.
