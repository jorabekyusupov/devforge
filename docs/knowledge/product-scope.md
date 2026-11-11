---
type: Product Requirement
title: DevForge mahsulot chegarasi
description: macOS va Windows uchun desktop dastur, loyiha nomi va repozitoriy.
status: draft
decision_status: confirmed
tags: [product, desktop, macos, windows]
sources:
  - id: conversation
    resource: /references/conversation-decisions.md
    title: DevForge rejalashtirish suhbati
---

# Tasdiqlangan talablar

- Herd'ga o'xshash lokal development muhitini boshqaradigan dastur yaratiladi.
- Mahsulot macOS va Windows uchun o'rnatiladigan desktop dastur bo'ladi.
- Foydalanuvchi ikkala platformani aniq ko'rsatgan; faqat macOS bilan cheklanmaydi.
- Dastur dastlab rus, o'zbek va ingliz tillarida ishlashi, keyinchalik boshqa
  tillarni qo'shish imkoniyati bo'lishi kerak. [Til talabi va i18n kontrakti](localization.md).
- Joriy nom: DevForge. Foydalanuvchi keyingi topshiriqlarda shu nomdan foydalangan.
- Ishchi papka: `/Users/yusupov/Herd/devforge`.
- Belgilangan GitHub repozitoriy: [jorabekyusupov/devforge](https://github.com/jorabekyusupov/devforge).[^conversation]

# Muhim chegara

Papka `Herd` ichida joylashgani uni Laravel web ilovasiga aylantirmaydi.
Desktop mahsulot o'z navbatida Laravel/PHP loyihalarini boshqarishi rejalashtirilgan.
Tauri WebView interfeysidan foydalanish [arxitektura ishchi yo'nalishi](architecture.md),
alohida brauzerda xizmat qiladigan web mahsulot talabi emas.

Git remote'ga bog'lash ko'rsatmasi commit, push yoki release qilishga umumiy
ruxsat bermaydi. Ushbu bilim bazasini yaratish ham tashqi nashrga ruxsat emas.

# Hali belgilanmagan

Minimal OS versiyalari, Intel/ARM arxitekturalari bo'yicha qo'llab-quvvatlash
matritsasi va ommaviy tarqatish shartlari hali tanlanmagan. Ular tanlangach,
tegishli build va real platforma dalillari bilan qayd etiladi.

[^conversation]: Suhbat manbasidagi platforma, nom/papka va GitHub bo'limlari; bu talab qaydi, runtime tekshiruv natijasi emas.
