---
type: Implementation Plan
title: DevForge M6 dan keyingi rivojlanish tartibi
description: Debugging birinchi va yakuniy beta oxirgi bo'lgan M7-M15 ketma-ketligi.
status: draft
decision_status: proposed
tags: [roadmap, post-mvp, prioritization, debugging, beta]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M12 ni birinchi va M7 ni oxirgi qilish topshirig'i
  - id: foundation
    resource: /mvp-phase-1-plan.md
    title: M0-M6 poydevori va tarixiy MVP chegarasi
  - id: m6
    resource: /m6-distribution-results.md
    title: Ichki paketlar va ochiq release tekshiruvlari
---

# Holat va raqamlash

Foydalanuvchi avval taklif qilingan M12 debugging/profiling bosqichini birinchi,
M7 barqaror beta bosqichini oxirgi o'ringa ko'chirishni topshirdi.[^request]
Ushbu tartib ko'rsatmasi tasdiqlangan; quyidagi funksional tafsilotlar hali reja
va implementatsiya dalili emas. Hujjatning `decision_status: proposed` qiymati shu
feature tafsilotlariga tegishli. Qayta tartiblash topshirig'i kodni boshlamagan;
keyingi «m7dan boshla» ko'rsatmasi M7 implementatsiyasini vakolatladi. Joriy
holat [M7 natijalarida](m7-inspector-results.md); M8 keyingi user topshirig'i bilan boshlandi; M9-M15 tafsilotlari taklif bo'lib qoladi.

M7.1 uchun Laravel companion, opt-in Inspector va request/query profiling kodi
yaratildi. Native platforma dalillari va cheklovlari o'sha hisobotda yuritiladi.
M7.2 Xdebug extension/debugger/function-profiler boshqaruvi hali amalga
oshirilmagan; M7 bosqichi to'liq yakunlangan deb hisoblanmaydi.

M8 Laravel/Git wizard, preparation, PHP settings/diagnostics, recent/favorites
va HTTPS review bilan functional native qabuldan o'tdi; actual source/job
matritsasi [M8 natijalarida](m8-project-setup-results.md). System authorization
dialoglari va release/minimum-OS/profile gate'lari M15'da qoladi. Keyingi reja
M9 bo'lib, bu yozuv uning implementatsiyasini avtomatik boshlamaydi.

M0-M6 tarixiy raqamlari saqlanadi. Keyingi ishlar bajarish ketma-ketligi bo'yicha
qayta raqamlandi: oldingi M12 endi M7, oldingi M7 endi M15. Eski raqamlar faqat
quyidagi moslik jadvali va suhbat tarixiga murojaat qilish uchun ishlatiladi.

# Yangi ketma-ketlik

| Yangi bosqich | Avvalgi taklif | Asosiy ishlar va natija |
| --- | --- | --- |
| **M7 — Debugging va profiling** | M12 | Laravel companion package orqali loyiha bo'yicha request/job/log hodisalari, SQL query va slow query/N+1 belgilari, dump/dd viewer. Keyin Xdebug boshqaruvi va profiler. Birinchi natija mavjud PostgreSQL loyihasida real diagnostika; MySQL yoki mail xizmati qo'shilishini kutmaydi. |
| **M8 — Loyihani tez tayyorlash** | M8 | Yangi Laravel wizard'i, Git'dan olish, Composer/npm va APP_KEY tayyorlash oqimi, HTTPS setup oynasi, PHP sozlamalari/extension diagnostikasi, recent/favorite loyihalar. Natija: yangi yoki mavjud loyiha tushunarli oqim bilan ishga tushadi. |
| **M9 — Baza va xizmatlar** | M9 | MySQL, Windows/macOS provider'i tekshirilgan Redis-compatible xizmat, DB/cache/queue tanlash, backup/restore va eksport/import, parallel xizmat versiyalari. Natija: keng tarqalgan Laravel infratuzilmasi boshqariladi. |
| **M10 — Takrorlanadigan muhit** | M10 | devforge.yaml, mavjud CLI orqali setup, aniq runtime versiyalari va lokal secret references, .env o'zgarishlarini oldindan ko'rish, jamoa konfiguratsiyasi. Natija: repository boshqa qurilmada mos muhit bilan ochiladi. |
| **M11 — Mail va kengaytirilgan loglar** | M11 | Lokal SMTP/mail catcher, HTML/plain text va attachment preview; M7'dagi hodisa modeliga tayangan boy Laravel log/exception viewer, qidirish va filtrlar. Natija: mail va app xatolari bitta joyda tekshiriladi. |
| **M12 — Workspaces** | M13 | Branch/worktree uchun alohida loyiha, domen, port va bo'sh yoki clone DB; yaratish va ownership-aware cleanup. M10 konfiguratsiyasi qayta ishlatiladi. Natija: parallel feature/agent ishlari bir-biriga ta'sir qilmaydi. |
| **M13 — AI integratsiyasi** | M14 | Mavjud CLI'ni barqaror JSON bilan kengaytirish, project-scoped MCP, runtime/query/log/mail konteksti; keyin preview/diff va agent interfeysi. Natija: agent haqiqiy loyiha kontekstidan boshqariladigan API orqali foydalanadi. |
| **M14 — Ulashish va integratsiyalar** | M15 | Vaqtinchalik tunnel orqali ulashish va uni tugatish, IDE/DB client integratsiyasi; keyin tanlangan deploy provider'lari. Natija: tester yoki mijoz lokal loyihani o'z qurilmasidan tekshiradi. |
| **M15 — Yakuniy beta va tarqatish** | M7 | Yakuniy tester regressiyalari, toza/minimum OS va native credential/system-dialog acceptance, uzilgan install/download va disk xatolari, diagnostika eksporti, signing/notarization/Authenticode, licensing review, app update va schema/rollback strategiyasi. Natija: tekshirilgan tarqatish nomzodi va aniq qo'llab-quvvatlash matritsasi. |

Ketma-ketlik: **M7 → M8 → M9 → M10 → M11 → M12 → M13 → M14 → M15**.

# Bog'liqliklar va doimiy tekshirish

M7 uchun minimal strukturalangan Laravel log/event yig'ish shu bosqichning o'zida
bajariladi. M11 yangi SMTP/mail imkoniyati va boy log viewer'ni qo'shadi; debugging
M11'ga bog'lanib orqaga surilmaydi. M9 yangi DB provider qo'shganda M7 query
kontekstini kengaytiradi. Workspace M10 konfiguratsiyasi va M9 DB boshqaruvidan,
AI konteksti esa mavjud diagnostika va xizmatlardan foydalanadi.

Har bosqichda tegishli testlar, xatolarni tuzatish, runtime integrity, project/DB
data preservation va framework-independent Rust boundaries saqlanadi. macOS
ARM64/Intel, Windows x64 hamda ru/uz/en qamrovi tegishli native dalillar bilan
tekshiriladi. Yakuniy beta bosqichining oxirga o'tishi shu kundalik tekshirishlarni
kechiktirmaydi. Bosqich nomi uning bajarilgani yoki tekshirilganini bildirmaydi.

M6'dagi signing, minimum OS, native credential/system UI va licensing masalalari
[M6 natijalarida](m6-distribution-results.md) ochiq qoladi; M15 ularni yakuniy qabul
uchun birlashtiradi. Signing identity, loyiha litsenziyasi yoki haqiqiy target
muhiti talab qiladigan masala vaqt kelganda aniq ko'rsatiladi. Auto-updater uchun
OS signing, updater signature va schema/data recovery alohida tekshiriladi.
Public release/deploy o'zining alohida ruxsatli scope'i bilan bajariladi.

[^request]: Suhbatdagi «M7 ohirgi bosqichga qoy ... m12 birinchiga qoy» ko'rsatmasi; qolgan funksiyalar avvalgi assistant taklifidan tartiblandi.
[^foundation]: M0-M6 scope va dalillar qayta raqamlanmadi.
[^m6]: Ichki acceptance ommaviy signed release tayyorligini bildirmaydi.
