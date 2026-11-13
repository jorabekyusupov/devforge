---
type: Implementation Result
title: E03 loyiha boyicha exact Composer binding
description: Schema8 migration, execution va terminal, manifest va worktree hamda credential-free tooling Composer tanlovi.
status: draft
decision_status: working-direction
tags: [e03, composer, migration, exact-pins, execution]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: E03 scope va test siyosati
  - id: store
    resource: ../../crates/projects/src/project.rs
    title: Exact binding va additive schema8
  - id: execution
    resource: ../../crates/projects/src/execution.rs
    title: Invocation va registered credential-free tooling
  - id: repro
    resource: ../../crates/workspace/src/repro.rs
    title: Reviewed manifest setup/export
---

# Natija va chegara

User integrated rejani bajarish va accepted bosqichlar pushini vakolatladi. E03
source/focused-test kesimi bajarildi; boshqa runtime versiyalar yoki katalog defaultlari
o'zgartirilmadi. Joriy catalogdagi Composer2.11.2 saqlanadi. Katalogga keyinchalik
ko'p Composer versiya qo'shilsa mavjud loyiha moving defaultga o'tmaydi.[^plan]

Project store schema8 `composer` package IDni saqlaydi. Schema<=7 recordlar transaction
ichida historical2.11.2 host-target ID bilan backfill qilinadi; project revision va
boshqa JSON metadata saqlanadi. Legacy record deserialize unsupported hostda panic
qilmaydi; actual store/invocation unsupported targetni rad etadi. Future schema rad
etilishi saqlanadi. Bu binary app downgrade uchun umumiy recovery mexanizmi emas.[^store]

`import_with_composer` tanlangan IDni boshidan saqlaydi, `configure_composer` existing
optimistic revisionni ishlatadi. Context invocation va terminal shim/resource lease,
manifest YAML setup/export/import hamda worktree child registration bir xil exact
bindingdan foydalanadi. Child source PHP/Node/Composer'ni oladi.[^execution][^repro]

Redis client install ham project Composer paketini install qiladi. Registered project
uchun bootstrap_for_project pinlarni saqlaydi, ammo transient yangi ID, bo'sh secret/env
references, default services bilan credential-free tooling yaratadi. Yangi unregistered
setup staging uchun eski bootstrap default tanlash semantikasi saqlangan.

# Maqsadli tekshiruvlar

| Qamrov | Buyruq / natija |
| --- | --- |
| Project migration/binding/default/stale | `cargo test -p devforge-projects --test projects`:12passed |
| Manifest exact resolution | `cargo test -p devforge-projects --test manifest`:5passed |
| YAML export va portable pin roundtrip | `cargo test -p devforge-workspace --test repro`:4passed |
| Invocation/shim/lease va credential-free context | `cargo test -p devforge-projects --lib composer_binding_tests`:1passed, final bootstrap fixdan keyin |
| Worktree registration | `cargo test -p devforge-workspace --lib composer_inheritance_tests`:1passed |
| Migration late fix | Tegishli schema migration filterlari:4passed; bu oldingi12testga qo'shiladigan mustaqil total emas |
| Static consumers | `cargo clippy -p devforge-projects -p devforge-workspace --lib -- -D warnings`:passed, final service caller fixdan keyin |

Ikki installed Composer fixture bilan selected resource/argv/terminal shim/lease
tekshirildi. Registered tooling testda catalog defaulti olib tashlandi, secret references
bo'lgan project va SecretStore'ga murojaatni rad qiladigan fixture ishlatildi. Bu real
PHP/Composer process ishga tushirildi degani emas. Dastlab inert executable fixture
native header tekshiruvida yiqildi, fixture actual test executable bilan tuzatildi;
bu product failure yoki real PHP acceptance sifatida ko'rsatilmaydi.

Mustaqil QA source review + integrator review bajarildi. Reviewda services.rsdagi
old default caller topilib tuzatildi; faqat affected fixture/clippy qayta bajarildi.
Old green testlar o'zgarmagan qamrov uchun reused, full workspace suite lokal
qayta bajarilmadi. Affected Rust fayllar formatlandi va scoped diff o'tdi.

# Platforma va reconciliation

macOS arm64da focused compiled/unit fixtures o'tdi. Real PHP/native UI, macOS Intel,
Windows actual execution va minimum OS acceptance bu stage'da qayta bajarilmadi.
Remote code CI natijasi keyingi execution ledgerda aniq run/source bilan qayd etiladi.
Native package/provider oldingi dalili yangi Composer pin native passiga aylantirilmaydi.

Schema/binding claim: changed/implemented/focused tested. CLI/terminal/YAML/worktree
consumers: matching persisted ID. Registered credential omission: tested. Full native
acceptance va app update/downgrade recovery: unverified/M15 alohida gate.

[^plan]: [Integrated scope](integrated-execution-plan.md).
[^store]: [Project metadata](../../crates/projects/src/project.rs).
[^execution]: [Context/invocation](../../crates/projects/src/execution.rs).
[^repro]: [Manifest consumers](../../crates/workspace/src/repro.rs).
