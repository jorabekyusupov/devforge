---
type: Architecture Decision
title: DevForge arxitektura yo'nalishi
description: Rust/Tauri stack va yadro, interfeys, CLI hamda OS adapterlari chegaralari.
status: draft
decision_status: working-direction
tags: [architecture, rust, tauri, react, sqlite]
sources:
  - id: conversation
    resource: /references/conversation-decisions.md
    title: Texnologiya va arxitektura muhokamasi
---

# Ishchi yo'nalish

Suhbatdagi yakuniy texnik tavsiya va loyiha skill'ining boshlang'ich yo'nalishi:
**Rust + Tauri + React/TypeScript + SQLite**. Bu tanlov aniq dependency versiyalari
yoki o'rnatilgan stack mavjudligi haqida dalil emas.[^conversation]

| Qism | Vazifa |
| --- | --- |
| Tauri desktop shell | Oyna va desktop integratsiyalari. |
| React/TypeScript | Interfeys, boshqaruvlar va foydalanuvchiga ko'rinadigan holatlar. |
| Mustaqil Rust yadro | Loyihalar, runtime paketlari va servislar bo'yicha umumiy mantiq. |
| Fon boshqaruvchisi | Oynadan mustaqil boshqarish zarur bo'lgan servislar hayot sikli. |
| CLI | Yadro imkoniyatlaridan terminal orqali foydalanish. |
| SQLite | DevForge sozlamalari va metama'lumotlari; foydalanuvchi loyihalarining DB dvigateli emas. |
| OS adapterlari | macOS/Windows jarayonlari, yo'llar, startup, DNS, trust store va huquqlar. |

# Sabab va chegaralar

Go/Wails, Rust/Tauri va boshqa variantlar muhokama qilingan. Go majburiy emas.
Foydalanuvchi AI bilan birga ishlab chiqishni belgilab, dasturchi tajribasi va
o'rganish vaqtini stack tanlovining mezonidan chiqarishni so'ragan. Yakuniy
tavsiya resurs nazorati va desktop integratsiyasini ustun qo'ygan.[^conversation]

Yadro Tauri va UI turlariga bog'lanmasin. Modullar amaldagi ehtiyojga qarab
ajratiladi; mikroservislar yoki umumiy plugin platformasi oldindan talab emas.
Har ikkala OS uchun alohida integratsiya kerak; bitta platformada ishlash
ikkinchisining ishlashini isbotlamaydi.

Ilova, runtime binarlari, konfiguratsiya va loyiha/database ma'lumotlarining
egalik chegaralari ajratiladi. Yangilash foydalanuvchi ma'lumotlarini saqlashi
kerak. Batafsil ish qoidalari repozitoriydagi
[runtime yo'riqnomasida](../../.agents/skills/devforge/references/runtime-contracts.md).

# M1 implementatsiyasi

Foydalanuvchining keyingi M1 topshirig'i asosida Cargo workspace va Rust/Tauri
poydevori yaratildi. `crates/core` settings/locale kontraktlarini, `crates/platform`
SQLite va OS credential adapterini, `crates/cli` doctor buyrug'ini,
`apps/desktop` esa React presentation va Tauri shell'ni saqlaydi.
`bootstrap`/`save_settings` IPC va `native-navigate` eventi minimal kontraktdir.
SQLite ishlarining command yo'li Tauri `spawn_blocking` orqali bajariladi.
Aniq versiyalar manifest, lockfile va toolchain faylida.

M1'da macOS ARM64 native app tekshirildi. M2'da hosted macOS/Windows test va
native binary build dalili qo'shildi; Windows GUI hali tekshirilmagan.
[M1 natijasi](m1-foundation-results.md) kod va dalil chegaralarini saqlaydi.

# Ochiq masalalar

M2'da [runtime yadrosi](m2-runtime-core-results.md) `crates/runtime`da yaratildi:
immutable package store, process supervisor, OS process tree adapterlari va CLI
ulanishi mavjud. U Tauri'dan mustaqil; desktop hali bu qatlamni boshqarmaydi.

M3'da [loyiha muhitlari](m3-project-environments-results.md) `crates/projects`da
yaratildi: registry, inspection, context resolver, secret references va owned DB
lifecycle. Project CLI shu qatlamni chaqiradi. GUI command'lari M5'da ulanadi;
bloklovchi library amallari o'shanda worker thread'da bajarilishi kerak.

Web-server/proxy integratsiyasi, DNS, huquqli yordamchi jarayon, standalone
supervisor daemon va installer hali amalga oshirilmagan. Ularning oldingi
takliflari ishlayotgan feature deb talqin qilinmaydi.

[^conversation]: Suhbatdagi yakuniy Rust/Tauri tavsiyasi va undan keyin yaratilgan skill; alohida benchmark yoki formal stack tasdig'i yo'q.
