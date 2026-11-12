---
okf_version: "0.2"
---

# DevForge bilim bazasi

Ushbu OKF bundle 2026-11-11 kungi DevForge suhbatidan tuzildi. Avval kerakli
mavzuni tanlang, so'ng faqat tegishli hujjatlar va ularning manbalarini o'qing.

## Qarorlar va yo'nalishlar

- [Mahsulot chegarasi](product-scope.md) - macOS va Windows uchun desktop dastur, loyiha nomi va repozitoriy.
- [Arxitektura yo'nalishi](architecture.md) - Rust/Tauri stack va yadro, interfeys, CLI hamda OS adapterlari chegaralari.
- [Loyiha muhitlari](project-environments.md) - loyiha bo'yicha PHP, Node.js va DB tanlovi, umumiy paketlar va izolyatsiya.
- [Muhandislik siyosati](engineering-policy.md) - AI bilan ishlab chiqish, vaziyatga mos SOLID/OOP/pattern tanlovi va versiya siyosati.
- [UI/UX siyosati](ui-ux-policy.md) - to'rtta UI skill'ning vazifasi, ishga qarab ketma-ketligi va desktop tekshiruvi.
- [Ko'p tillilik](localization.md) - rus, o'zbek va ingliz tili talabi hamda yangi tillar uchun kengayadigan i18n kontrakti.

## Taklif va ochiq masalalar

- [M6 dan keyingi roadmap](post-mvp-roadmap.md) - yangi M7 debugging/profiling bilan boshlanadi; yakuniy beta/tarqatish M15'ga ko'chirilgan, eski-yangi raqamlar mosligi berilgan.

- [MVP birinchi bosqich rejasi](mvp-phase-1-plan.md) - macOS va Windows hamda rus, o'zbek va ingliz tillari uchun bosqichlar, natijalar va qabul mezonlari.
- [Birinchi MVP taklifi](mvp-proposal.md) - tavsiya etilgan birinchi ish jarayoni, funksiyalar va hali tasdiqlanmagan chegaralar.

## Manba va yuritish

- [M8 loyihani tayyorlash](m8-project-setup-results.md) - Laravel/Git wizard, project setup, PHP va HTTPS review hamda native verification holati.

- [M7 Inspector](m7-inspector-results.md) - M7.1 Laravel capture/profiling uch native targetda o'tdi; M7.2 Xdebug boshqaruvi hali ochiq.

- [M6 distributiv va qabul](m6-distribution-results.md) - ichki paketlash, data preservation, Windows tester HTML qo'llanmasi va ochiq release gate holati.
- [M5 kundalik boshqaruv](m5-daily-workspace-results.md) - real desktop controls, project lifecycle va macOS/Windows native GUI dalillari.
- [M4 domen va serving](m4-serving-results.md) - domain/HTTPS/hosts helper, Caddy/PHP/Vite orchestration va macOS Chrome/Windows Edge native dalillari.
- [M3 loyiha muhitlari](m3-project-environments-results.md) - import, scoped PHP/Node/Composer, PostgreSQL DB/role, secret references va explicit env patch dalillari.
- [M2 runtime yadrosi](m2-runtime-core-results.md) - paket store, cancellation, process ownership/readiness/restart/loglar, CLI va platforma sinovlari.
- [M1 desktop natijalari](m1-foundation-results.md) - Rust/Tauri poydevori, uch tilli UI, SQLite/keychain, bajarilgan test va native macOS dalillari hamda ochiq platforma gate'lari.
- [M0 runtime natijalari](m0-runtime-findings.md) - macOS va Windows paketlari, haqiqiy bajarish dalillari, cheklovlar va M0 ochiq gate'lari.
- [Bilim bazasini yuritish](knowledge-policy.md) - OKF tuzilishi, qaror holatlari, manba, yangilash va tekshirish qoidalari.
- [OKF validator](validation-tooling.md) - read-only tekshirish, strict rejim, o'rnatish, testlar va qamrov cheklovlari.
- [Suhbat manbasi](references/conversation-decisions.md) - foydalanuvchi ko'rsatmalari va assistant takliflarining tanlangan qaydi.
- [O'zgarishlar tarixi](log.md) - mazmunli bilim yangilanishlari.

## Holatni to'g'ri talqin qilish

`decision_status` loyiha kengaytmasi:

| Qiymat | Ma'nosi |
| --- | --- |
| `confirmed` | Foydalanuvchining aniq ko'rsatmasi yoki bevosita tasdiqlangan ish qoidasi. |
| `working-direction` | Suhbat va amaldagi skill'dagi ishchi tavsiya; alohida yakuniy tasdiq deb talqin qilinmaydi. |
| `proposed` | Taklif; bajarish yoki mahsulot hajmini kengaytirishga ruxsat bermaydi. |

OKF `status: draft` dastlabki hujjat inson tomonidan ko'rib chiqilmaganini
bildiradi. Bu `decision_status` bilan bir xil emas. `verified` metama'lumoti
kiritilmadi: yozuvlar inson tekshirgan deb ko'rsatilmaydi.

Bundle yaratilgan paytda repozitoriyda AGENTS.md va skill fayllari bor edi;
ilova manifestlari, runtime boshqaruvi yoki tayyor installer mavjudligi
aniqlanmadi. Quyidagi hujjatlar mahsulotning ishlayotganini isbotlamaydi.
