---
type: Reference
title: DevForge suhbatidan qaror manbalari
description: Foydalanuvchi ko'rsatmalari va assistant takliflarining tanlangan qaydi.
status: draft
tags: [conversation, provenance, decisions]
---

# Manba chegarasi

Bu hujjat 2026-11-11 kungi joriy DevForge suhbatidan AI yordamida tuzilgan
tanlangan qayd. To'liq transcript yoki mustaqil inson tekshiruvi emas.
Foydalanuvchi matnlari qisqa iqtiboslar, assistant javoblari esa xulosa bilan
ajratilgan. Chat uchun ommaviy URL yoki har xabarning aniq vaqti berilmagan;
ular o'ylab topilmadi. Quyidagi ketma-ketlik suhbat tartibiga mos.

## Mahsulot va platformalar

Foydalanuvchi Herd analogini yaratishni so'ragan, so'ng aniqlashtirgan:

> biz web loyiha emas macos va Windows uchun loyiha chiqarishimiz kerak

Natija: ikkala OS uchun desktop mahsulot talabi confirmed.

## Texnologiya tanlovi va AI

Go haqida savoldan keyin foydalanuvchi Go majburiy emasligini, kelajakdagi
o'sish, tezlik, qulaylik va moslashuvchanlikni hisobga olishni so'ragan.
Keyin quyidagini belgilagan:

> loyihani toliq birga yozamiz yani AI dan toliq foydalanamiz shunning uchun rust tajribasi yoki dasturchi moslashish olib tashlab javob ber

Assistantning yakuniy tavsiyasi Rust + Tauri + React/TypeScript bo'lgan;
SQLite metama'lumotlar uchun qo'shilgan. Keyingi skill shu yo'nalishni
boshlang'ich tavsiya sifatida qayd etgan. Alohida formal stack tasdig'i yo'q.

## Loyiha nomi, papka va GitHub

Foydalanuvchi `/Herd/` ichida papka yaratishni so'ragan. Assistant
`/Users/yusupov/Herd/devforge` papkasini yaratgan. Foydalanuvchi keyingi
topshiriqlarda DevForge nomini ishlatgan va shu papkani quyidagi repozitoriyga
bog'lashni topshirgan:

[https://github.com/jorabekyusupov/devforge](https://github.com/jorabekyusupov/devforge).

Assistant Git'ni `main` bilan boshlagan va `origin`ni o'rnatganini bildirgan;
joriy ishda remote qayta tekshirildi. Bu qayd commit/push yoki release dalili emas.

## Kod dizayni

Foydalanuvchi SOLID, OOP va design pattern bo'yicha qoida so'rab, quyidagini
ta'kidlagan:

> AGENT vaziyatga qarab ozi tanlasin

Assistant vaziyatga mos tanlov, Rust composition/traits/enums, oddiy yechimni
saqlash va muhim tradeoff'larni tushuntirish qoidalarini skill'ga qo'shgan.

## Versiya siyosati

Versiyalarga qattiq bog'lanish haqidagi savolga skill'da umumiy siyosat,
manifest/lockfile/toolchain'da aniq versiyalar taklif qilingan. Foydalanuvchi:

> tavsiyalarin asosida skillni yangila

Natija: siyosatni qo'shish confirmed; biror dependency'ning aniq versiyasi
tasdiqlanmagan. `Tauri 2` boshlang'ich matni versiyasiz `Tauri`ga almashtirilgan.

## Loyiha bo'yicha DB, PHP va Node.js

Foydalanuvchi:

> har bir loyihaga alohida db,php, node js biriktiriladi shundaymi

Assistant har loyiha uchun tanlov/config mustaqilligini, bir xil paketlar va
mos DB server instance'i umumiy bo'lishi mumkinligini tushuntirgan. Bu ishchi
model; mavjud runtime yoki alohida izolyatsiya mexanizmi tekshirilmagan.

## Birinchi MVP

Foydalanuvchi birinchi MVP bosqichi uchun tavsiyalar so'ragan. Assistant loyiha
importi, PHP/Node tanlovi, PostgreSQL, HTTP/HTTPS, Vite/queue, loglar va
installer'larni taklif qilgan. Keyingi foydalanuvchi topshirig'i UI skill'lari
haqida bo'lgan; MVP ro'yxatini tasdiqlash yoki yozishni boshlash topshirig'i yo'q.

## UI/UX skill'lari

Foydalanuvchi `ui-ux-pro-max`, `ui-taste`, `uizze-ui` va
`web-design-guidelines`ni sanab, ularni o'rganish va quyidagini so'ragan:

> qachon qaysi birinchi ishlatish kerakligi boyicha skillga qoida qosh shu skillardan foydalansin

Assistant manbalarni o'qib, ish turiga mos routingni lokal skill'ga qo'shgan.
Bu real ilova UI'si yaratilgani yoki vizual auditdan o'tganini anglatmaydi.

## OKF bilim bazasi

Foydalanuvchining joriy topshirig'i:

> DevForge uchun docs/knowledge ichida OKF bilim bazasini yarat, shu suhbatdagi kelishilgan qarorlarni yoz va skillga undan foydalanish qoidasini qo'sh

Natija: ushbu bundle'ni yaratish va undan foydalanish qoidasini qo'shish confirmed.

## Validator va doimiy yuritish

OKF'ni doimiy to'ldirish haqidagi savoldan keyin assistant validator skripti,
muhim vazifa yakunida bilimga ta'sirni tekshirish va bosqich oxirida kod bilan
solishtirishni tavsiya qilgan. Foydalanuvchi quyidagini topshirgan:

> tavsiyalaring asosida OKF validator skriptini yarat va har muhim vazifa yakunida bilim bazasini yangilash hamda bosqich oxirida kod bilan solishtirish qoidasini skillga qo'sh

Natija: skript va yuritish qoidalarini qo'shish confirmed. Alohida fon watcher'i,
avtomatik semantik tekshiruvchi yoki CI ulanishi topshirilmagan.

## Dastur tillari

Foydalanuvchi quyidagi mahsulot talabini belgilagan:

> dastur rus o'zbek va ingliz tilarida bolishi kerak va kelajakda bu royhatni kengaytiradigon yechim qilish kerak

Natija: boshlang'ich uch til va keyin yangi tillarni qo'shish imkoniyati confirmed.
O'zbek yozuv varianti, default/fallback tili va i18n kutubxonasini foydalanuvchi
alohida tanlamagan; tegishli texnik takliflar talabdan ajratib qayd etiladi.

## Aniq MVP reja

Foydalanuvchi:

> Endi MVP birinchi bosqichi uchun aniq reja tuz, macOS va Windows hamda rus, o'zbek va ingliz tillarini boshidan hisobga ol.

Natija: platforma va tillarni boshidan hisobga olgan aniq reja yaratish confirmed.
Rejalashtirilgan bosqichlar yoki yangi texnik tanlovlar ilova implementation'ini
boshlash, tizimni o'zgartirish yoki publish qilishga alohida ruxsat emas.

## M0 bajarish topshirig'i

Foydalanuvchi:

> Reja asosida M0 bosqichini bajarishni boshla, macOS va Windows uchun runtime paketlarini tekshir, natijalarni OKFga yoz.

Bu M0 runtime tekshiruvi, task-local diagnostika/fixture'lar va natijalarni
repozitoriyga yozishni vakolatlaydi. M1 yoki ommaviy publish topshirig'i emas.

Windows VM/test kompyuteri/runner mavjudligi so'ralganda foydalanuvchi:

> Hozir mavjud emas

Shuning uchun Windows artifact tekshiruvi davom etadi, native Windows bajarilishi
esa ochiq blocker sifatida qayd etiladi.

## M1 va Git push topshirig'i

Foydalanuvchi:

> Endi M1 bosqichini boshlaylik, Rust/Tauri poydevori va rus, o'zbek, ingliz tillarida ishlaydigan boshlang'ich desktop interfeysni yarat, natijalarni test qilib OKFga yoz.

Bu M1 implementation, tegishli build/test va OKF yangilashni vakolatlaydi.
Keyingi xabar:

> barcha ishlar yakunlangandan song push qil testlarni qayta ishga tushurmasdan

Natija: M1 yakunidagi commit/push vakolatlandi; oldin bajarilgan test suite'lari
qayta ishga tushirilmaydi. Push ham CI testlarini qayta boshlamasligi uchun
ushbu commit'da `[skip ci]` ishlatiladi. OKF strict strukturaviy validatsiyasi
yangi hujjatlar uchun alohida tekshirish bo'lib, product test suite emas.

## M2 topshirig'i

Foydalanuvchi:

> Endi reja asosida M2 bosqichini boshlaylik, runtime paketlari va jarayonlarni boshqarish yadrosini yarat, test qil, natijalarni OKFga yoz va yakunda push qil.

Bu M2 kodi, tegishli test/smoke, OKF va commit/push'ni vakolatlaydi. Oldingi
M1 push'dagi qayta test qilmaslik ko'rsatmasi o'sha yetkazishga tegishli edi;
ushbu yangi topshiriq M2 uchun testni aniq so'raydi. GitHub Actions read-only
tekshiruvida yoqilganligi aniqlandi; M2 push'dan keyingi CI natijasi real dalilga
qarab yoziladi. Mahalliy Windows VM va GUI tekshiruvi bundan kelib chiqmaydi.

## M3 bosqichiga o'tish

M2 yakunidagi kod, test, OKF va push topshirilganidan keyin foydalanuvchi:

> m3ga o't

Bu mavjud MVP rejasidagi M3 implementation'ini davom ettirish sifatida olindi.
Loyiha importi, runtime binding, PostgreSQL va scoped CLI doirasida kod/test/OKF
ishlari bajariladi; oldingi yetkazish oqimi bo'yicha commit/push va native CI
davom ettiriladi. M4 serving yoki M5 desktop UI hajmiga avtomatik kengaytirilmaydi.

## M4 serving topshirig'i

Foydalanuvchi:

> Endi M4 bosqichiga o't, reja asosida domen, HTTPS va Laravel serving qismini yarat, macOS va Windows’da test qil, natijalarni OKFga yoz va yakunda push qil.

Bu serving kodi, scoped hosts/CA helper, native runtime/browser testlar, OKF va
commit/push'ni vakolatlaydi. Native GitHub Actions uchun source avval push qilinib,
actual natijalar yakuniy evidence commit'ida yoziladi. Foydalanuvchining Mac
hosts/trust sozlamalari avtomatik o'zgartirilmaydi; elevated integratsiya testi
disposable CI VM'da bajariladi.

## M5 kundalik boshqaruv

Foydalanuvchi M5 bosqichini bajarishni topshirdi. Mavjud reja bo'yicha Projects,
runtimes, services/logs, per-project lifecycle, tray/Quit, uch til va real
verification ishlari davom ettiriladi. Oldingi code/test/OKF/push yetkazish
ko'rsatmasi saqlanadi; M6 installer/release scope'iga o'tilmaydi.
