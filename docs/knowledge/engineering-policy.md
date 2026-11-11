---
type: Engineering Policy
title: AI bilan ishlab chiqish va kod siyosati
description: AI bilan ishlab chiqish, vaziyatga mos SOLID/OOP/pattern tanlovi va versiya siyosati.
status: draft
decision_status: confirmed
tags: [engineering, ai, solid, oop, patterns, versions]
sources:
  - id: conversation
    resource: /references/conversation-decisions.md
    title: Foydalanuvchining skill va ishlab chiqish ko'rsatmalari
---

# Kelishilgan ish qoidalari

- Loyiha foydalanuvchi bilan AI yordamidan to'liq foydalanib ishlab chiqiladi.
- Texnologiya tanlovi dasturchining taxminiy tajribasi yoki moslashish vaqtiga
  qarab hal qilinmaydi. Mahsulotning tezligi, qulayligi, moslashuvchanligi va
  kelajakdagi o'sishi baholanadi.
- SOLID, OOP va design pattern'lar uchun agent vaziyatga qarab o'zi tanlaydigan
  qoida qo'shilgan. Har vazifa uchun majburiy pattern yoki class soni yo'q.
- Skill texnologiyalarning doimiy major/minor versiyasiga bog'lanmaydi.[^conversation]

# Kod va tekshirish

Rust'da encapsulation, `struct`, `enum`, `trait`, composition va tegishli
polymorphism ishlatiladi. Klassik inheritance iyerarxiyasi majburan ko'chirilmaydi.
Oddiy funksiya yoki konkret tur kontraktni yaxshi ifodalasa, shu yetarli.
Muhim qaror sababi qisqa tushuntiriladi; kundalik design tanlovi alohida tasdiq
bosqichiga aylanmaydi.

Pattern nomini emas, xatti-harakat va kontraktni tekshirish kerak. AI yozgan kod,
kompilyatsiya yoki hujjatning mavjudligi runtime tekshiruvi o'rnini bosmaydi.
Platformaga bog'liq o'zgarishlar uchun macOS va Windows dalillari ajratiladi.
Performance raqamlari o'lchovga tayanadi, til nomiga emas.

Amaliy mezonlar va pattern misollari
[kod dizayni yo'riqnomasida](../../.agents/skills/devforge/references/code-design.md);
tekshirish tartibi
[verification yo'riqnomasida](../../.agents/skills/devforge/references/verification.md).

# Versiya siyosati

Dependency talablari manifestlarda, resolved versiyalar lockfile'larda, build
vositalari toolchain konfiguratsiyasida saqlanadi. Agent yangi dependency uchun
rasmiy manbadan barqaror, qo'llab-quvvatlanadigan va loyiha/platformaga mos versiyani
tanlaydi. Oddiy feature yoki bug fix aloqasiz upgrade'ga aylanmaydi.

Yangilanish xavfsizlik, support, zarur imkoniyat va moslik bo'yicha baholanadi;
breaking change uchun migratsiya, recovery va tegishli tekshirish ko'zda tutiladi.
DevForge dependency'lari va u boshqaradigan PHP/Node/DB paketlari alohida
versiyalanadi. Aniq ish qoidalari
[asosiy skill'da](../../.agents/skills/devforge/SKILL.md).

[^conversation]: AI bilan to'liq ishlash, agentning vaziyatga mos pattern tanlovi va tavsiya asosida versiya siyosatini yangilash haqidagi bevosita foydalanuvchi ko'rsatmalari.
