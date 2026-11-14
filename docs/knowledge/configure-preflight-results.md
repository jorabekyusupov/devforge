---
type: Implementation Result
title: Configure refusal oldidan running sessionni saqlash
description: Invalid name va debugger PHP-change siyosatini stopdan oldin umumiy validator bilan tekshirish, native red-green dalili.
status: draft
decision_status: working-direction
tags: [configure, validation, lifecycle, regression, native]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: Existing behaviorni saqlash va tegishli test siyosati
  - id: project
    resource: ../../crates/projects/src/project.rs
    title: Shared configuration validation
  - id: workspace
    resource: ../../crates/workspace/src/lib.rs
    title: Configure stopdan oldingi guard
  - id: test
    resource: ../../crates/workspace/src/configure_tests.rs
    title: Explicit cached native regression
---

# Muammo va natija

M13.0.3 review invalidname typed contract tafovutini topdi. Typed adapter oldindan
tekshirgan bolsa ham raw Workspace::Command::Configure (mavjud GUI/direct caller)
ProjectStore.configurega yetguncha running sessionni stop qilardi. Invalidname va
active debugger policy bilan PHP pinini o'zgartirish o'sha kech rad etishdan zarar ko'rdi.

Project::validate_configuration name, inspection/runtime compatibility va debugger
PHP-change siyosatini bitta side-effect-free helperda saqlaydi. Direct ProjectStore
va shared Workspace bir xil tekshiruvdan foydalanadi; Workspace uni stopdan oldin
bajaradi. Existing revision va domain guards saqlanadi. Bu config/file/SQL applyning
umumiy atomic rollback kafolati emas; concurrent mutation yoki keyingi I/O/start
failure boshqa recovery qamroviga tegishli.

# Haqiqiy native RED → GREEN

Actual reviewed cache'dan PHP CLI/web, Node va Caddy tanlangan paketlari fresh
TempDir storega nusxalandi va verified executablelar ishlatildi. Isolated Laravel-shaped
public PHP fixture OS-assigned loopback portda HTTP canary berdi; haqiqiy Serving/Session
ishladi. Bu empty session yoki fake backend asserti emas.

Old source uchun invalidname actual sessionni olib tashlagani regressionda RED
bo'ldi. Debugger Settings real save_debugger API bilan valid policy sifatida saqlandi;
boshqa compatible PHP piniga Configure debug_runtime_change qaytarib actual sessionni
yo'qotgani ham RED bo'ldi. Tuzatishdan keyin ikkala GREEN: aynan shu child PIDlar,
HTTP canary javobi, project revision/PHP va debugger metadata saqlandi. Debugger
holati policy-refusal proof; actual Xdebug SDK activation sinovi deyilmaydi.

# Tekshiruv va chegaralar

Explicit cached store belgilangach:

```sh
DEVFORGE_CONFIGURE_RUNTIME_STORE="/path/to/reviewed-runtime-store" \
  scripts/with_rust.sh cargo test -p devforge-workspace --lib configure_tests -- --ignored --nocapture
```

Natija:2native test passed. Direct store refusal regression:
`cargo test -p devforge-projects --test projects debugger_policy_refuses_direct_store_php_change_without_mutating_record` —1passed.
`cargo clippy -p devforge-projects -p devforge-workspace --lib --tests -- -D warnings` o'tdi.
Scoped whitespace review o'tdi; full local suite/GUI/othernative tests takrorlanmadi.

Native cache env talab qilgani uchun testlar default suite'da ignored; broad
`--ignored` bilan barcha native/keychain testlarni ishga tushirish kerak emas.
Downloader, user loyihalari, hosts/trust/global config yoki old cache o'zgarmadi.
Actual native evidence macOS ARM64; Windows/Intel native HTTP regression unverified.
Compiler/source guards boshqa targetda bir xil bo'lsa ham bu native pass o'rnini olmaydi.

# Reconciliation

Static refusal-before-stop: changed, implemented, native tested. Direct store
policy: matching, targeted tested. PID/HTTP/metadata preservation: tested for
shu ikki refusal family. General apply rollback, actual debugger session switching
va unavailable target acceptance: unverified, alohida qamrov.
