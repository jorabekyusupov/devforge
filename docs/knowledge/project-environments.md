---
type: Architecture Decision
title: Har loyiha uchun development muhiti
description: Loyiha bo'yicha PHP, Node.js va DB tanlovi, umumiy paketlar va izolyatsiya.
status: draft
decision_status: working-direction
tags: [projects, php, nodejs, database, isolation]
sources:
  - id: conversation
    resource: /references/conversation-decisions.md
    title: Loyiha bo'yicha DB PHP va Node.js muhokamasi
  - id: m9
    resource: /m9-services-results.md
    title: Managed database cache queue and retained generations
---

# Muhit biriktirish modeli

Foydalanuvchi har loyihaga alohida DB, PHP va Node.js biriktirilishini
aniqlashtirgan. Suhbatda quyidagi model taklif etilgan:[^conversation]

| Resurs | Loyihaga biriktiriladigan qism | Umumiy ishlatilishi mumkin bo'lgan qism |
| --- | --- | --- |
| PHP | Tanlangan versiya va loyihaga tegishli konfiguratsiya. | Bir xil versiyaning o'rnatilgan paketi. |
| Node.js | Tanlangan versiya va loyiha buyruqlarining muhiti. | Bir xil versiyaning o'rnatilgan paketi. |
| DB | Alohida baza, ulanish va foydalanuvchi/huquqlar. | Mos loyihalar uchun bitta server instance. |
| Cache/queue | M9 Garnet instance, credential va driver tanlovi project uchun alohida; cache DB1, queue DB0. | Binary paket va managed .NET runtime umumiy. |

Alohida sozlama har loyiha uchun alohida binary nusxasi yoki alohida DB serveri
degani emas. Boshqa server versiyasi yoki kuchliroq izolyatsiya kerak bo'lsa,
alohida instance biriktirish M3 PostgreSQL va M9 MySQL adapterlarida bor.
M9 Garnet jarayoni har loyiha uchun alohida; provider/platform dalili va
chegaralari [M9 natijalarida](m9-services-results.md).

# Kutiladigan kontrakt

- Bir loyiha versiyasini almashtirish boshqa loyihaning tanlovini o'zgartirmaydi.
- Web so'rovlari, Composer, Artisan va queue buyruqlari loyihaga biriktirilgan
  PHP va tegishli konfiguratsiyaga amal qiladi.
- DevForge orqali ochilgan terminal va ishga tushirilgan Node/npm/Vite jarayonlari
  loyiha muhitidan foydalanadi; kompyuterdagi har qanday tashqi terminal avtomatik
  shu muhitga o'tadi, deb va'da berilmagan.
- DevForge'ni yoki paketlar katalogini yangilash mavjud loyiha tanlovini va DB
  ma'lumotlarini o'z-o'zidan almashtirmaydi.
- SQLite'ning ilova metama'lumotlarini saqlashi foydalanuvchi loyihasi uchun
  qaysi DB dvigateli tanlanishini belgilamaydi.

# M3 implementatsiyasi

M2 exact package identity va leased executable resolver'ni yaratdi;
[runtime natijalariga](m2-runtime-core-results.md) qarang. Ikki Node versiyasi
macOS va Windows'da parallel vaqtinchalik HTTP fixture'da tekshirildi.
[M3](m3-project-environments-results.md) Project binding, SQLite registry,
per-project INI/env, scoped CLI va PostgreSQL DB/role qatlamini qo'shdi.

Terminal shell/shim va resolver M3'da yaratildi; native oyna GUI acceptance'i
alohida. `.env` faqat explicit apply va private backup bilan yoziladi. PHP
serving/pool, proxy va domen/HTTPS integratsiyasi M4 da yaratildi; bu
bo'limning dastlabki yozilishi paytida ular keyingi scope edi.
PostgreSQL'ni MVP'dagi yagona boshlang'ich DB qilish
[alohida taklif](mvp-proposal.md); bu umumiy runtime modelining majburiy qismi emas.

[^conversation]: Foydalanuvchining muhitlar haqidagi savoli va assistantning dizayn javobi; mavjud ishlaydigan implementatsiya sifatida qayd etilmagan.

# M9 joriy kengaytma

MySQL/PostgreSQL instance tanlovi, cache/queue driver'lari, per-project Garnet
va staged backup/restore qo'shildi. DB switch/unbind oldingi database va SecretStore
references'ni saqlaydi; retained connection qayta faollashtiriladi. Restore fail
bo'lsa active binding saqlanadi. `.env` avtomatik o'zgarmaydi; tashqi terminalga
automatik global tanlov berilmaydi. Engine-specific platform gap'lari, SQLite va
unmanaged DB backup chegarasi [M9 kontraktida](m9-services-results.md).
