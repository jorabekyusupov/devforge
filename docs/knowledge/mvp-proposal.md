---
type: Product Proposal
title: DevForge birinchi MVP taklifi
description: Tavsiya etilgan birinchi ish jarayoni, funksiyalar va hali tasdiqlanmagan chegaralar.
status: draft
decision_status: proposed
tags: [mvp, scope, proposal, open-questions]
sources:
  - id: conversation
    resource: /references/conversation-decisions.md
    title: Birinchi MVP haqidagi assistant tavsiyasi
---

# Taklif holati

Ushbu yozuv dastlabki MVP tavsiyasini saqlaydi. Uch til va platformalarni
boshidan hisobga olgan joriy [batafsil bajarish rejasi](mvp-phase-1-plan.md)
alohida yozildi. Reja tuzish implementation scope'ini avtomatik tasdiqlamaydi.

Foydalanuvchi birinchi MVP bosqichi uchun tavsiya so'ragan. Assistant quyidagi
scope'ni taklif qilgan; suhbatda bu ro'yxatni yakuniy tasdiqlash yoki uni yozishni
boshlash topshirig'i bo'lmagan. Bu hujjat roadmap majburiyati emas.[^conversation]

# Taklif qilingan yakuniy ish jarayoni

Mavjud Laravel loyihasini qo'shish -> PHP/Node/DB biriktirish -> lokal domen
orqali brauzerda ochish -> holat va loglarni ko'rish.

| Qism | Birinchi bosqich uchun taklif |
| --- | --- |
| Desktop | Loyiha ro'yxati, loyiha sahifasi, sozlamalar va tray. |
| Import | Papka tanlash, Laravel'ni aniqlash, nom va domen. |
| PHP/Node | Cheklangan katalogdan o'rnatish, loyiha bo'yicha tanlash; dastlab ikkitadan mos versiya. |
| DB | Dastlab PostgreSQL, umumiy serverda loyiha uchun alohida baza va foydalanuvchi. |
| Web | `project.test`, HTTP/HTTPS va brauzerda ochish. |
| Jarayonlar | Start/stop/restart, Vite va bitta queue worker. |
| Diagnostika | Servis chiqishi, Laravel loglari va tushunarli xatolar. |
| Tarqatish | macOS/Windows uchun o'rnatiladigan paketlar. |

Loyiha terminali va DB ulanish sozlamalarini boshqarish shu ish jarayonining
bir qismi sifatida taklif qilingan; aniq `.env` yozish xatti-harakati tanlanmagan.

# Taklif qilingan qabul mezonlari

Ikki loyiha turli PHP/Node versiyalari va alohida bazalar bilan bir vaqtda ishlaydi.
Birini o'zgartirish yoki to'xtatish ikkinchisiga ta'sir qilmaydi. Ilova va
kompyuter qayta ochilganda tanlovlar hamda DB ma'lumotlari saqlanadi. Har ikkala
OS alohida tekshiriladi. Bu mezonlar hali bajarilgan natijalar emas.

# Keyingi bosqichga qoldirish taklifi

MySQL/MariaDB, Redis, mail viewer, SQL/dump kuzatuvi, Git worktree, MCP va
avtomatik yangilash MVP'dan keyin qo'shilishi taklif qilingan. Bularni doimiy
taqiq yoki tasdiqlangan navbat sifatida talqin qilmang.

# Ochiq qarorlar

MVP scope tasdig'i; boshlang'ich DB dvigateli; runtime paket manbalari va aniq
versiyalar; OS/CPU matritsasi; DNS/HTTPS mexanizmi; quit/startup siyosati;
installer signing va release talablari. Tasdiqlanganda shu hujjat va unga
bog'liq qarorlar yangilanadi.

[^conversation]: Foydalanuvchining MVP tavsiyasi haqidagi savoliga assistant javobi; keyingi UI skill topshirig'i MVP scope'ni tasdiqlamaydi.
