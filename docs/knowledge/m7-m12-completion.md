---
type: Development Milestone
title: M7–M12 qolgan imkoniyatlarni yakunlash
description: Tayyor funksiyalarni saqlagan holda qolgan debugging, konfiguratsiya, Intel MySQL va workspace lifecycle qismlari.
status: draft
decision_status: confirmed
tags: [m7, m9, m10, m12, completion]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Qolgan ishlarni tugatish va push topshirigi
  - id: previous
    resource: /m7-xdebug-results.md
    title: Oldingi functional acceptance va ochiq cheklovlar
  - id: roadmap
    resource: /post-mvp-roadmap.md
    title: M7–M15 scope chegaralari
---

# Vakolat va saqlanadigan ishlar

User M7–M12 qolgan ishlarini bajarish, test, OKF va yakunda pushni topshirdi;
bajarilgan funksiyalar saqlanadi. Asos65948e0 main, yangi isolated checkout.
Parallel project-discovery branchidagi dirty fayllar bu scope'ga kirmaydi.[^request]

# Bajarish ketma-ketligi

1. M7: GET bilan cheklangan trigger'ga methods/headers/body request builder;
   paused primitive locals'ni explicit o'zgartirish, existing stepping/profiler'ni saqlash.
2. M9: Intel uchun verified portable MySQL provider; existing DB'ni auto-upgrade qilmaslik.
3. M10: existing portable schema/plan/setup ustiga localized GUI YAML read/edit,
   validation, reviewed save va setup/recovery.
4. M12: reviewed retained archive reactivation va explicit purge; source/DB/branch
   saqlanishi. Reviewed recursive submodule lifecycle va external PG/MySQL source
   dump'ni yangi managed child DB'ga staged copy qilish.
5. macOS ARM64/Intel va Windows x64 native acceptance; RU/UZ/EN, regression,
   source/evidence reconciliation, strict OKF va final push.

M8/M11'ning accepted funksiyalari qayta yozilmaydi. M13/M14 integrations va M15
release/signing/clean-OS credential/system-dialog gate'lari alohida bosqichlarda.
M7 request/variable changes faqat explicit developer actions; arbitrary shell yoki
remote administration API qo'shilmaydi.[^previous][^roadmap]

# Joriy holat

Audit va implementatsiya boshlandi. Oldingi passing reports yangi code uchun
verification deb ko'rsatilmaydi. Har slice natijasi va native source/job chegarasi
ushbu yozuvda keyin yangilanadi.

[^request]: Joriy user topshirigi: qolgan ishlar to'liq, test, OKF, final push; bajarilganlar saqlansin.
[^previous]: Oldingi accepted M7.1/M7.2 normal operation scope saqlanadi.
[^roadmap]: Qolgan functional scope ushbu topshiriq bilan vakolatlandi; public release emas.
