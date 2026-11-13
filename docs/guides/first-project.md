# Birinchi loyihani ishga tushirish

Bu qo‘llanma mavjud DevForge desktop boshqaruvlari uchun vazifa yo‘lidir. U yangi
checklist UI yoki o‘tkazilgan user pilot natijasi emas. Tugma va xato matnlarining
ruscha, o‘zbekcha va inglizcha mosligini [atamalar jadvalidan](terminology.md)
toping. Quyida English katalogidagi aniq nomlar ishlatiladi; loyiha nomi, yo‘l,
URL, buyruq yoki xom log matni tarjima qilinmaydi.

Maqsad: tanlangan loyiha ishlaydi, o‘zingiz kutgan sahifa HTTP javobini beradi va
shu yangi so‘rov Inspector’da ko‘rinadi. DB, queue va mail faqat loyiha talab
qilsa tekshiriladi. Faqat `Running` belgisi sahifa sog‘lomligini isbotlamaydi.

## 1. To‘g‘ri yo‘lni tanlang

| Sizdagi holat | Mavjud amal | Saqlanadigan chegara |
| --- | --- | --- |
| Laravel repository allaqachon diskda | Projects → Add project | Import fayllarni o‘zgartirmaydi, dependency o‘rnatmaydi va script ishlatmaydi |
| Yangi Laravel kerak | Projects → New project → New Laravel project | Yangi, band bo‘lmagan destination; tanlangan runtime va preparation review |
| Repository’ni Git’dan olish kerak | Projects → New project → Clone Git repository | Git va uning mavjud credentials’i; URL ichiga token yozmang; mavjud destination almashtirilmaydi |
| Jamoa `devforge.yaml` bergan | Repository’ni oching; Configuration’da saved plan’ni ko‘ring | Exact pins, mahalliy secret references va reviewed setup; faylni saqlash setup boshlamaydi |

Discovery topgan loyiha hali runtime konfiguratsiyasini kutishi mumkin. Loyiha
nomini/path’ini tekshirib `Configure project` oqimini bajaring; ayni papkani boshqa
loyiha sifatida qayta yaratish bilan xatoni aylanib o‘tmang.

## 2. Loyiha talab qiladigan runtime’larni tayyorlang

Runtimes sahifasida shu kompyuterga mos kerakli PHP va Node paketlarini o‘rnating.
New project wizard faqat installed runtime’ni tanlashga ruxsat beradi. Version
loyihaning `composer.json`/`package.json` talablari bilan mos bo‘lsin; global PHP
va shell profile’ni o‘zgartirish kerak emas. Laravel serving uchun tanlangan PHP’ga
mos web runtime/proxy ham kerak bo‘lishi mumkin; xatoda aynan qaysi paket yetishmagani
va katalogdagi target’ni ko‘ring.

`runtime_unavailable` yoki `manifest_runtime_unavailable` bo‘lsa: missing paketni
aniqlang → platforma/arxitektura va exact pinni tekshiring → Runtimes’da mavjud
mos paketni o‘rnating → plan’ni qayta ko‘ring. Exact pin katalogda yo‘q bo‘lsa,
jim boshqa versiyani tanlamang; jamoa konfiguratsiyasini review qilib tuzatish kerak.

[Windows mosligi](../knowledge/windows-compatibility.md) installer minimum’i,
actual native acceptance va provider edition supportini ajratadi. Server runner
natijasi actual Windows 10/11 desktop natijasi emas.

## 3. Loyiha va preparation rejasini tekshiring

Existing project’ni import qilgach PHP/Node, domen, Vite va queue tanlovini loyiha
kartasida tekshiring. `Prepare project` dependency/env/key tayyorlash uchun alohida
amal. New project oqimida esa preparation variantlarini yaratishdan oldin ko‘rasiz.

Composer/npm o‘rnatilishi, missing `.env` yaratish va missing `APP_KEY` yaratish
variantlarini faqat kerak bo‘lsa tanlang. Existing `APP_KEY` saqlanadi; o‘zgargan
environment fayllari private backup bilan boshqariladi. Scriptlar default o‘chiq;
ularni yoqish repository kodi bajarilishini anglatadi. Composer pluginlari o‘chiq
qoladi. Trust roziligi mazmunini o‘qing; noma’lum repository’ga avtomatik bermang.

Yangi DevForge Laravel loyihasida owned SQLite initialization taklif qilinishi
mumkin. Existing Git loyiha bazasi yashirin migrate qilinmaydi. Production DB’ga
ulanib qolmaganini tekshirmasdan migration yoki queue boshlamang.

PHP muammosi bo‘lsa `PHP settings` orqali shu loyihaning INI/extension/Composer
platform diagnostikasini ko‘ring. Saqlash running loyihani restart qilishi mumkin;
boshqa loyihaning PHP tanlovini o‘zgartirish bu oqimning maqsadi emas.

## 4. YAML va secret kerak bo‘lsa

Configuration’da avval loyihani to‘xtating, saqlangan konfiguratsiyani tekshiring.
`Review saved configuration` natijasida downloads, DB amali, blockers va local
secret availability’ni ko‘ring. `Include reviewed .env changes` alohida opt-in.
`Apply reviewed plan` faqat tekshirilgan va trusted plan uchun ishlatiladi.

Missing local secret bo‘lsa Configuration’dagi `Local secret store` bo‘limida
manifest kutayotgan reference va qiymatni kiriting, `Store locally` bosing. Qiymat
mahalliy credential store’da qoladi; manifest’ga secret qiymatini yozmang. Keyin
yangi plan oling: secret rotation yoki konfiguratsiya o‘zgarishi eski review’ni
bekor qilishi mumkin. CLI kerak bo‘lsa [README’dagi config oqimi](../../README.md)
va uning stdin orqali secret berish qoidalaridan foydalaning.

`secret_unavailable`/`manifest_secret_unavailable` har doim “qiymatni yana yozing”
degani emas: OS credential store’ga kirish yo‘q bo‘lishi ham mumkin. Credential
service/access tiklanmaguncha mavjud binding/key’ni o‘chirib qayta yaratmang.
Secret, password-bearing URL, private key yoki to‘liq `.env`ni yordam hisobotiga
qo‘shmang. `manifest_review_changed` bo‘lsa faylni reload qilib qayta review qiling.

## 5. HTTP natijani oling

Loyiha kartasida `Start` bosing. Status/readiness va logdagi xatoni ko‘ring, keyin
`Open browser` orqali DevForge ko‘rsatgan actual URL’ni oching. O‘zingiz bilgan
read-only sahifani tanlang; HTTP status va ko‘rinadigan natijani alohida qayd qiling.
Jarayon ishlayotgan bo‘lsa ham Laravel dependency/config/DB xatosi bilan 500
qaytarishi mumkin. Bunda start tugmasini takror bosishdan oldin tegishli logni o‘qing.

Domen, HTTP, HTTPS va system trust turli holatlardir. System trust tayyor bo‘lmasa
ko‘rsatilgan HTTP localhost fallback’dan foydalanish mumkin. HTTPS kerak bo‘lsa
`HTTPS setup` orqali exact domen/CA fingerprint va system change review’ni ko‘ring.
Preview o‘zi trust o‘rnatmaydi; ruxsatni bekor qilish HTTP yo‘lni tekshirishga to‘sqinlik
qilmasligi mumkin. Boshqa dasturning DNS/hosts/certificate sozlamasini o‘chirmang.

Port band bo‘lsa DevForge actual boshqa loopback URL/port qaytarishi mumkin;
oldindan taxmin qilingan portni emas, kartadagi URL’ni ishlating. `port_unavailable`
qolsa qaysi xizmat va port ekani, actual log va shu loyihaning holatini tekshiring.
O‘zingiz boshqarayotgan boshqa dev server ayni loyihaga tegishli bo‘lsa uning
normal stop amalidan foydalaning; begona processni kill qilmang. `hot_file_conflict`
holatida avval shu loyihaning oldingi development serverini to‘xtating; Vite hot
faylini yoki lockni sababini bilmay o‘chirish recovery emas.

## 6. Shu yangi so‘rovni Inspector’da toping

Inspector’da aynan shu loyihani tanlang. Companion hali yo‘q bo‘lsa loyihani
avval to‘xtating, `Install companion` review’ni o‘qing. Bu trusted loyihada
`.devforge/laravel-inspector`, Composer fayllari/vendor va package discovery bilan
bog‘liq o‘zgarishlar qiladi; oddiy read-only import emas.

`Enable capture`ni yoqing, kerak bo‘lsa loyihani qayta boshlang va sahifaga yana
bitta so‘rov yuboring. Inspector’da `Request` eventini uning vaqti, path/name va
context’i bilan toping. Oldingi eventni yangi sinov deb qabul qilmang; qidirish,
type/context filtri va event tafsilotlaridan foydalaning. Capture bo‘sh bo‘lsa:
loyiha to‘g‘rimi → companion bormi → capture yoqilganmi → yangi so‘rov shu managed
serving orqali ketdimi, shuni tekshiring.

Request/query profiling uchun Xdebug o‘rnatish majburiy emas. Xdebug interactive
debugger va function profiling alohida oqim. Repeated query belgisi N+1 ehtimolini
ko‘rsatadi, yakuniy isbot emas. Capture private data saqlashi mumkin; mahalliy
test data’dan foydalaning va keraksiz yig‘ishni `Pause capture` bilan to‘xtating.

## 7. Faqat loyiha talab qiladigan xizmatlarni tekshiring

- **DB:** Services & logs’da mos owned instance tayyorlang va stopped loyihaga
  assign qiling. Connection test va actual ilova natijasini ajrating. Eski binding
  yoki data’ni yo‘qotmasdan review qiling; migration alohida developer amali.
- **Queue:** loyiha queue backend’i tayyor bo‘lsagina worker’ni yoqing. Pending
  job mavjud bo‘lsa actual bajarilganini va logni tekshiring; queue ishlashi uchun
  DB/cache va loyiha migratsiyalari kerak bo‘lishi mumkin.
- **Mail:** Mail & logs’da stopped loyiha uchun capture’ni yoqing; mahalliy
  test xat yuborib ayni loyiha inbox’ida tekshiring. SMTP sink tashqariga relay
  qilmaydi; real yuborilgan xat deb belgilamang.
- **Vite:** kerakli npm dependency’lari va loyiha script’i mavjud bo‘lsa yoqing.
  Frontend kerak bo‘lmagan fixture uchun Vite majburiy acceptance emas.

## 8. Cancel, qaytish va to‘g‘ri handoff

Uzoq preparation vaqtida dialogdagi `Cancel operation` yoki umumiy progressdagi
`Cancel` amalidan foydalaning va terminal natijani kuting.
Cancellation bajarilgan har ishni ortga qaytarmaydi. Yaratilgan fayllar, yuklangan
paketlar yoki registered partial loyiha qolishi mumkin; mavjud destination’ni
ustidan qayta yaratish o‘rniga natijani tekshirib, tegishli preparation’ni davom
ettiring. YAML setup journal’i bo‘lsa uni va yangi review’ni ko‘ring.

Existing APP_KEY/data’ni “toza boshlash” uchun o‘chirmang. Loyiha yoki xizmat hali
busy bo‘lsa takror mutation yuborishdan oldin holat natijasini aniqlang. Window’ni
yopish tray’ga yashiradi; owned xizmatlarni to‘xtatish uchun explicit Quit bor.

Pilot uchun minimal lokal qayd: source/app versiya, OS/build/arxitektura va til,
source turi, conditional xizmatlar, qadamning natijasi, HTTP/Inspector correlation,
vaqt, retry/yordam soni va stable error code. Path/project nomlarini kerak bo‘lsa
yashiring. Bu qo‘llanma mavjud source asosida yozilgan; real user/native pilot,
minimum OS, credential/dialog va barcha til flow’lari shu yozuv bilan verified
bo‘lib qolmaydi. Product testlarini faqat hujjat uchun qayta yugurtirmang.

Source: [WorkspacePage](../../apps/desktop/src/pages/WorkspacePage.tsx),
[ProjectSetup](../../apps/desktop/src/components/ProjectSetup.tsx),
[ConfigurationPage](../../apps/desktop/src/pages/ConfigurationPage.tsx),
[InspectorPage](../../apps/desktop/src/pages/InspectorPage.tsx),
[M8](../knowledge/m8-project-setup-results.md),
[M10](../knowledge/m10-reproducible-results.md) va
[M11](../knowledge/m11-mail-logs-results.md) natijalari.
