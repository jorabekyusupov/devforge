---
type: Verification Result
title: E10 companion framework compatibility fixture
description: Actual Laravel12/13 managedPHP8.4 Collector/mail contract, Laravel11 Composer advisory blocker va scoped evidence.
status: draft
decision_status: working-direction
tags: [e10, laravel, inspector, mail, compatibility]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: Declared support va actual matrix farqi
  - id: runner
    resource: ../../scripts/companion_compatibility.py
    title: Isolated Composer and native adapter runner
  - id: native
    resource: ../../crates/inspector/examples/companion_compatibility.rs
    title: Actual Rust Collector va SMTP/MIME ingestion
  - id: fixture
    resource: ../../packages/laravel-inspector/tests/compatibility.php
    title: Actual Foundation/framework API contract fixture
---

# Natija

| Fresh fixture | Managed PHP | Status |
| --- | --- | --- |
| Laravel11 ^11.0 candidates | 8.4.27-staticphp20261113 | Composer2.11.2 advisory policy blocked dependency resolution |
| Laravel12.72.0 | same PHP8.4.27 | passed |
| Laravel13.42.0 | same PHP8.4.27 | passed |

Actual reports [11](../evidence/companion-compatibility/laravel11-php84-macos-arm64.json),
[12](../evidence/companion-compatibility/laravel12-php84-macos-arm64.json),
[13](../evidence/companion-compatibility/laravel13-php84-macos-arm64.json) va
[provenance](../evidence/companion-compatibility/provenance.json).

Laravel11 status installed-existing11 runtime yoki companion failure emas; yangi
synthetic fixture Composer policy bilan resolve qilinmadi. Guard chetlab o'tilmadi,
PHP/framework silently upgrade qilinmadi va companion declared ^11 constraint
shundan kelib chiqib olib tashlanmadi. Exact advisory diagnostics reportda.

12/13 uchun actual Foundation Application/HTTP Kernel 200/500, SQLite query,
sync queue success/failure, Log/dump/dd/exception, opt-out/project isolation va
redaction 19Collector event bilan tekshirildi. Framework Mail productning actual
authenticated Rust MailServer'iga yuborildi; MailStore MIME va attachment byte'lari
tekshirildi. Mock Collector/SMTP substitute emas. Full Laravel skeleton, actual
network web server, Tauri/native desktop, async queue yoki PG/MySQL proof emas.

# Izolyatsiya va qayta ishlatish

Runner PHP/Composer receipt descriptorlarini current bundled catalog bilan, actual
selected bytesni receipt executable/resource hashes bilan tekshiradi. Global host
PHP/Composer/INI/env, real user app va credentials ishlatilmaydi. Composer home/cache,
app, productstore temp, scripts/plugins off; network faqat --allow-download explicit.
Existing owned fixture cache copied/reused, global Composer cache yozilmadi. Advisory
checks default saqlanadi. Child auth credentials synthetic va ephemeral.

Python9validator/redaction/provenance unit tests pass; PHP lint, focused Rust example
build/fmt va `cargo clippy -p devforge-inspector --example companion_compatibility
--locked -- -D warnings` passed. Shared lock unstable bo'lganda clippy to'xtadi;
root lock resolve qilgach shu check o'tdi, thirdparty versions upgrade qilinmadi.

Actual matrixdan keyin runner diagnostic proxy userinfo/raw/decoded knowncredential
redaction va future outputbinary/fixture sourcehash maydonlari kuchaytirildi. Faqat
Python affected tests qayta bajarildi; actual12/13 va11blocked runlar qayta bajarilgan
deb aytilmaydi. Provenance at-run runner/fixture/native source va binary hashesni,
postrun change sababini alohida saqlaydi; private absolute sessionpaths yo'q.

# Qolgan gate

Actual host macOS ARM64. Intel/Windows, PHP8.3/8.5, PG/MySQL/asyncqueue, full app/network
serving va native UI unverified. Bu harness faqat per-major selected fixtures bilan
ishga tushiriladi, har ordinary pushda barcha frameworklar download/install qilinmaydi.
M7 transport/runtime source o'zgarmasa old valid evidence reused, changed companion
APIlarda faqat tegishli major/provider slice qayta tekshiriladi. No public release.
