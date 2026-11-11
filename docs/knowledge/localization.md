---
type: Product Requirement
title: DevForge ko'p tillilik kontrakti
description: Rus, o'zbek va ingliz tili talabi hamda yangi tillar uchun kengayadigan i18n kontrakti.
status: draft
decision_status: confirmed
tags: [i18n, localization, languages, extensibility, desktop]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Dastur tillari haqidagi foydalanuvchi talabi
  - id: language-tags
    resource: https://www.rfc-editor.org/rfc/rfc5646
    title: BCP 47 language tags
  - id: plural-rules
    resource: https://cldr.unicode.org/index/cldr-spec/plural-rules
    title: Unicode CLDR plural rules
  - id: intl
    resource: https://tc39.es/ecma402/
    title: ECMAScript internationalization API
---

# Tasdiqlangan talab

DevForge macOS va Windows'da dastlab **rus, o'zbek va ingliz** tillarida ishlashi
kerak. Arxitektura keyinchalik til ro'yxatini kengaytirishga mos bo'lishi kerak.
Bu foydalanuvchining aniq talabi.[^request] `decision_status: confirmed` aynan
shu talabga tegishli; quyidagi default va implementatsiya takliflari alohida.

M1'da registry va tarjima kataloglari yaratildi; macOS ARM64 native interfeys va
owned menyular uch tilda tekshirildi. Windows va qolgan qabul qamrovi
[M1 natijasida](m1-foundation-results.md) alohida qayd etilgan.

# Ishchi yechim

## Locale registry va kataloglar

Bitta umumiy registry til identifikatori, o'z nomi, yozuv yo'nalishi, resurs
kataloglari va zarur alias/fallback ma'lumotini saqlaydi. Til tanlash ro'yxati
ham shu registry'dan olinadi. UI/Rust mantiqida uch tilga yopiq enum yoki har
ekranda `if language == ...` bloklari yaratilmaydi.

Yangi til qo'shish normal holatda tarjima kataloglarini yaratish, registry'ga
yozuv qo'shish va tekshiruvdan o'tkazishdan iborat bo'lsin. Domain yoki har bir
ekran kodini tahrirlash talab qilinmasin. Resurslar build/packaging jarayoniga
ham registry orqali bog'lansin. Bu boshlang'ich offline/bundled katalog modeli;
internetdan til paketi yuklash yoki foydalanuvchi plugin tizimi hali talab emas.

Matnlar semantik kalitlar va parametrlar bilan saqlanadi, masalan
`projects.actions.start` va `errors.port_in_use`. Kataloglar feature/namespace
bo'yicha ajratilishi mumkin. Matn qismlarini ulab gap yasash o'rniga to'liq
tarjima xabari va plural/interpolation mexanizmi ishlatiladi.

## Til identifikatorlari va yozuv varianti

BCP 47 til/yozuv/hudud identifikatorlaridan foydalanish rejalashtiriladi.[^language-tags]
Boshlang'ich ishchi variant: `ru`, `en`, o'zbek lotin uchun `uz-Latn` va unga
`uz` alias'i. Lotin yozuvi shu suhbatdagi yozuv asosida taklif etilgan;
foydalanuvchi hali yozuv variantini alohida tasdiqlamagan. Keyinchalik `uz-Cyrl`
alohida katalog sifatida qo'shilishi mumkin.

Locale resolver hudud va yozuvni hisobga oladi. Har bir identifikatorni shunchaki
birinchi `-`gacha kesish orqali `uz-Cyrl`ni lotin katalogiga tenglashtirmang.
Kelajakdagi RTL til uchun registry yo'nalishi va logical layout xususiyatlari
ko'zda tutilsin; haqiqiy RTL qo'llab-quvvatlashi qo'shilganda alohida tekshiriladi.

## Tanlash, saqlash va fallback

Ishchi taklif: foydalanuvchining saqlangan aniq tanlovi ustun. `System` rejimida
yoki birinchi ochilishda mos OS tili tanlanadi; mos tarjima bo'lmasa registry'dagi
fallback ishlatiladi. Boshlang'ich fallback uchun ingliz tili taklif etiladi;
bu foydalanuvchining alohida tanlovi yoki doimiy hardcode talabi emas.

Foydalanuvchi tilni Settings'dan o'zgartira oladi. Tanlov keyingi ochilishda
saqlansin; ishlayotgan UI va DevForge'ga tegishli menu/tray/notification matnlari
mos yangilansin. UI tilini almashtirish PHP, Node yoki DB'ni qayta ishga tushirmasin
va boshqarilayotgan loyihaning `.env`, ma'lumotlari yoki o'z UI tilini o'zgartirmasin.

Yetishmayotgan xabar supported locale'ning sozlangan fallback zanjiridan olinadi;
cycle bo'lmasin. Asosiy katalogda ham yo'q kalit development/testda aniqlansin,
foydalanuvchiga crash yoki tushunarsiz kalit chiqarilmasin. Runtime fallback
rus/o'zbek tarjimalarini to'liq qilish talabini almashtirmaydi.

## Frontend va Rust chegarasi

UI matnlari, validation, accessible nomlar, tooltip, bo'sh holat, loading,
notification va DevForge'ga tegishli desktop menu/tray xabarlari lokalizatsiya
qatlamidan o'tadi. Rust yadro barqaror error/event kodi va parametrlarini beradi;
foydalanuvchiga ko'rsatiladigan matn presentation/shell chegarasida yechiladi.
UI va native shell uchun mustaqil, bir-biridan uzilib ketadigan tarjima nusxalari
yaratilmasin. OS'ning o'zi chizadigan dialog matni OS siyosatiga bog'liq bo'lishi
mumkin; uni DevForge tarjima qilgan deb hisoblamang.

Loyiha nomlari, yo'llar, command/flag, versiya identifikatorlari va xom servis
loglari tarjima qilinmaydi. Ularga berilgan tushuntirish lokalizatsiya qilinadi.
Interpolation foydalanuvchi/servis qiymatlarini HTML sifatida bajarmasin.

Sana, son va vaqt birliklari tegishli locale-aware formatter orqali ko'rsatiladi;
ichki storage va mashina kontraktlari locale'dan mustaqil qoladi.[^intl]
Plural tanlovi CLDR/ICU'ga mos vosita bilan bajarilsin; faqat inglizchadagi
`count == 1` shartini barcha tillarga qo'llamang.[^plural-rules]

# Qabul mezonlari

- Rus, o'zbek va ingliz kataloglari foydalanuvchiga tegishli kalitlar,
  placeholder nomlari va plural/message sintaksisi bo'yicha tekshiriladi.
- Til almashtirish, saqlash, OS locale moslashtirish, noma'lum locale va yo'q
  xabar fallback'i test qilinadi; unsupported holat foydalanuvchi tanlovini buzmaydi.
- Har uch tilda oynani kichraytirish, uzun matn/yo'l, accessibility va asosiy
  interaction'lar macOS/Windows'da tegishli dalil bilan tekshiriladi.
- Qo'shimcha locale fixture'i katalog+registry orqali qo'shilishi tekshiriladi;
  real yangi til uchun tarjima va tegishli script/RTL/font qamrovi talab qilinadi.
- Translation lint va UI testlari OKF validator'dan alohida: OKF'dagi yozuvning
  to'g'riligi ilovada uch til ishlayotganini isbotlamaydi.

# M1'da tanlangan yechim

M3'da20 ta stable project error code uchun `project.errors` uch katalogga qo'shildi.
Rust mapping va katalog lint ularni tekshiradi. CLI machine JSON code beradi;
Project GUI error flow'lari hali M5 acceptance'iga tegishli.

`shared/locales/registry.json` canonical manba: `en`, `ru`, `uz-Latn`, `uz` alias'i
va `en` fallback. Shu katalogdagi locale JSON'lari i18next/react-i18next orqali
frontend'ga, Tauri build script orqali native shell'ga kiritiladi. Til ro'yxati
registry'dan chiziladi. Rust resolver explicit script'ni hurmat qiladi;
`uz-Cyrl` Latin'ga noto'g'ri tenglashtirilmaydi. Yo'qolgan saqlangan locale
fallback bilan ochiladi, asl preference ustidan yozilmaydi.

Boshlang'ich preference `system`; tanlov SQLite'da saqlanadi. Native macOS'da
system til ruscha bo'lib aniqlandi, qo'lda en/ru/uz-Latn switch va uz/ru uchun
quit/re-launch persistence tekshirildi. Qo'shimcha nemis fixture'i domain va
frontend testlarida data orqali tekshirildi; bu nemis tilini mahsulotga qo'shmaydi.
Kalit/placeholder/plural tekshirish `scripts/check_locales.mjs`da; test dalillari
[M1 hisobotida](m1-foundation-results.md). RTL, Windows UI, minimum oynadagi
to'liq keyboard/accessibility matritsasi hali tekshirilmagan.

[^request]: Suhbat manbasining dastur tillari bo'limi.
[^language-tags]: Til, yozuv va hudud variantlarini ifodalovchi standart identifikatorlar.
[^plural-rules]: CLDR turli tillarning plural kategoriyalarini farqlaydi.
[^intl]: ECMAScript Intl locale-aware formatlash imkoniyatlarini belgilaydi; target WebView mosligi amalda tekshiriladi.
