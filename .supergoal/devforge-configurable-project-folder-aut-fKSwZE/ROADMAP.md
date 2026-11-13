# DevForge — loyiha papkasi va avtomatik aniqlash rejasi

Sana: 2026-11-13 (Asia/Tashkent). Holat: IMPLEMENTED / native feature acceptance passed; original planning contract retained below.
Baseline: `9319e82457c7b6dc18c2b7b68b86eac9edda7de0`.

Foydalanuvchi talabi: Settings’da default `~/Devforge`, o‘zgartiriladigan yo‘l,
loyihalarni avtomatik topish, macOS va Windows’da ishlashini tekshirish; avval reja.
Quyidagi original reja keyingi foydalanuvchi topshirig‘i bilan bajarilmoqda; yakuniy mezonlar FINAL_ACCEPTANCE.md da tekshiriladi.

## 1. Natija

Foydalanuvchi loyihani `~/Devforge/shop` ichiga joylaydi. Ilova uni o‘zi topadi,
Projects’da ko‘rsatadi va imkoniga qarab ro‘yxatdan o‘tkazadi. Runtime yetishmasa
“Tayyorlash kerak” ko‘rsatiladi; mavjud Prepare/Start amallari ishlatiladi.
Yangi domain `shop.devforge.test`; mavjud `.localhost`/port fallback saqlanadi.
Avtomatik topilish avtomatik xizmat ishga tushgani yoki loyiha to‘liq tayyorligini anglatmaydi.

## 2. Tasdiqlangan talab va ishchi qarorlar

**Foydalanuvchi tasdiqlagan:** bitta sozlanadigan loyiha papkasi; default aynan
`~/Devforge`; avtomatik discovery; ikkala OS’da real verification; hozir rejalash.

**Ushbu rejadagi ishchi takliflar:** shallow Laravel discovery; Save/Reset/Scan now;
watcher + startup/focus/30s recovery scan; no-runtime candidates; suppression;
saqlanuvchi stable project ID; shu repodagi domen va explicit Start modeli.
Bu tafsilotlar bajarilgan funksiya yoki alohida user-approved arxitektura emas.

- macOS default: `<OS home>/Devforge`, masalan `/Users/name/Devforge`.
- Windows default: `<OS home>\Devforge`, masalan `C:\Users\name\Devforge`.
- `/Devforge` filesystem root sifatida default qilinmaydi.
- Shu versiyada bitta root. Multiple roots va rekursiv discovery keyingi scope.
- Mavjud Laravel import chegarasi saqlanadi. WordPress/static driver platformasi,
  Herd’dan migration, avtomatik dev stack, yangi DNS daemon bu task tarkibiga kirmaydi.

## 3. Mavjud koddan keladigan cheklovlar

- `crates/core/src/lib.rs`: Settings hozir language/theme; serde default va DTO compatibility kerak.
- `crates/platform/src/lib.rs`: JSON preference SQLite v1 ichida; additive change eng kichik yo‘l.
- `crates/projects/src/project.rs`: ProjectStore v5, immutable selected runtime IDs,
  Laravel inspect, Unix dev/inode duplicate key va Windows lowercase canonical path.
- `crates/workspace/src/lib.rs`: Import project + site configure qiladi; snapshot faqat
  registered projects list’dan yig‘iladi; candidate lifecycle yangi kontrakt talab qiladi.
- `apps/desktop/src-tauri/src/workspace.rs`: Workspace Mutex va cached snapshots bor.
  Filesystem scan’ni shu mutex yoki UI event callback ichida bajarish mumkin emas.
- `apps/desktop/src/workspace.ts`: 1500ms polling mavjud. Yangi umumiy event bus shart emas;
  snapshot/revision bilan natijani yetkazish yetarli, scan pollingga bog‘lanmaydi.
- `crates/serving/src/site.rs`: `.devforge.test` va `.localhost` alias; bare `.test`
  uchun serving kontraktini kengaytirish kerak bo‘lardi, bu task uni o‘zgartirmaydi.
- `.github/workflows/worktrees.yml`: macOS ARM64/Intel va Windows x64 native harness pattern’i bor.
- `apps/desktop/src/components/ProjectSetup.tsx`: New Project parent hozir bo‘sh;
  saved root’ni default qilish va create/discovery staging race’ini yopish kerak.

## 4. Settings oqimi

Mavjud Settings dizaynida “Loyiha papkasi” bo‘limi:

1. Label va path input (`~/Devforge` yoki saqlangan custom yo‘l).
2. Native “Papka tanlash” tugmasi; cancel hech narsani o‘zgartirmaydi.
3. “Saqlash”, “Defaultga qaytarish”; draft typing watcher’ni almashtirmaydi.
4. Yo‘l mavjud bo‘lmasa “Papka yaratish”; custom yo‘l jim yaratilmaydi.
5. “Hozir tekshirish”, holat, topilganlar soni, oxirgi tekshiruv va retry.
6. Invalid path/permission/no home/storage failure matnlari va fieldga bog‘langan xato.

Default katalog fresh initialize paytida bir marta yaratiladi yoki oldindan mavjud
bo‘lsa ishlatiladi. Keyinchalik user o‘chirgan root har startup’da qayta yaratilmaydi.
OS home topilmasa root guess qilinmaydi; explicit selection ko‘rsatiladi.
Save oldidan validate; failed persist old root’ni saqlaydi. Persistdan keyingi watch
failure yangi saved root’da degraded scan status beradi, old root natijasini qo‘llamaydi.
Root almashtirilishi hech qanday source file yoki database’ni ko‘chirmaydi.

## 5. Aniqlash va holatlar

Faqat root’ning bevosita child directory’lari tekshiriladi. Dot folders,
DevForge staging/data kataloglari, root child `vendor`/`node_modules`, symlink yoki
Windows junction/reparse child avtomatik follow qilinmaydi. Katalogdan tashqaridagi
loyiha existing Import bilan qo‘shiladi. Aniq sabab/ignored summary kerak bo‘lsa ko‘rsatiladi.

Laravel inspect manifest limitlari va constraints parser’i qayta ishlatiladi.
Filesystem o‘qish script execution yoki secret/.env inspection talab qilmaydi.
Candidate runtime IDs’dan mustaqil bo‘ladi; hozirgi Project invariants buzilmaydi.

| Holat | Ma’nosi va amal |
| --- | --- |
| Scanning | Root tekshirilmoqda; previous snapshot ko‘rinib turadi. |
| Needs runtime | Laravel topildi, mos installed runtime yo‘q; Prepare/tanlash. |
| Ready / stopped | Project va HTTP site saqlangan, explicit Start mumkin. |
| Conflict | Domen yoki identity to‘qnashuvi; old ma’lumot ustidan yozilmaydi. |
| Incomplete / invalid | Ko‘chirish tugamagan yoki manifest yaroqsiz; keyingi scan qayta tekshiradi. |
| Missing | Complete scan oldingi child yo‘qligini ko‘rsatdi; metadata/DB saqlanadi. |
| Unavailable / degraded | Root/permission/watch xatosi; ommaviy missing qilish yo‘q. |
| Relocation pending | Rename/move aniq apply uchun review yoki running session stop kerak. |
| Ignored | Forget suppression; Restore/Import kerak. |
| Detached | Root o‘zgardi, old registry entry saqlanadi. |

Installed runtimes orasidan constraint’ga mos reviewed catalog default, aks holda
deterministic compatible stable tanlov qo‘llanadi; mavjud project tanloviga tegilmaydi.
Mos installed variant bo‘lmasa install avtomatik boshlanmaydi. PHP web runtime ham
Start’dan oldin tekshiriladi; faqat CLI mavjudligi web ready degani emas.

## 6. Ma’lumotlar va identity

Discovery ledger/candidate/suppression ProjectStore’da additive migration bilan
saqlanadi; old registry va credentials saqlanadi. Core preferences faqat root policy’ni
saqlaydi. Domain/runtime lifecycle workspace orqali boshqariladi; Tauri faqat adapter.

- Canonical path va platform file identity dublikatni yo‘qotadi.
- Unix dev/inode qayta ishlatiladi; Windows’da handle-based volume/File ID tekshiriladi.
  Identity imkoniyati yo‘q bo‘lsa conservative relocation review, no guessing.
- Same-filesystem aniq rename ID/bindings’ni saqlaydi. Cross-volume copy yangi identity;
  avtomatik eski DB/secrets bilan birlashtirilmaydi.
- Running project rename’ida eski context’ni yangi path’ga jim almashtirish yo‘q:
  pending holat, session guard va existing stop/restart oqimi.
- Manual import va discovery bir entry’da coexist qiladi; worktree ownership’i saqlanadi.
- Forget suppression persist qilinadi; rediscovery/relaunch qayta qo‘shmaydi.
- Root switch old discovery associations’ni detached qiladi; old manual projects saqlanadi.
- Name rename mavjud custom domain’ni almashtirmaydi. Yangi domen uchun deterministic
  valid ASCII slug; collision bo‘lsa conflict holati, boshqaning domeni egallanmaydi.
- Registry va sites.json cross-file commit emas: pending state/retry/reconciliation
  yarim yozuvni duplicate yoki orphan holatida qoldirmaydi.

## 7. Watcher va recovery

Authoritative scanner bitta; native event faqat uni invalidate qiladi. Native notify
backend o‘zgarishi/overflow/atomic save eventlari to‘liq va tartibli kelishi shart emas.
Shuning uchun startup, app focus, explicit Scan now va 30s periodic shallow recovery bor.
Root parent deletion/recreation uchun kuzatiladi. Direct children nonrecursive
watchlanadi; kerakli marker (`public/index.php`) uchun maqsadli watcher yoki shallow
recovery bor; dependency subtree rekursiv watch qilinmaydi.

- 500ms quiet debounce; 2s maximum dirty scheduling latency; one active scan.
- Bounded/coalesced queue, root generation token va per-entry fingerprint/cache.
- Worker heavy FS scan’ni UI thread/Workspace Mutex tashqarisida qiladi.
- Snapshot complete/partial/cancelled/unavailable’ni alohida ifodalaydi.
- Cancellation, stale generation discard, source revision/operation guard.
- Tray’da ishlaydi; quit’da watch/timer/worker tugaydi. Alohida OS daemon qo‘shilmaydi.
- Network/removable root event bermasa polling fallback; access failure ko‘rinadi.
  Bunday mount’lar native local-filesystem pass sifatida da’vo qilinmaydi.

Primary source: [notify documentation](https://docs.rs/notify/8.2.2/notify/),
[watcher configuration](https://docs.rs/notify/8.2.2/notify/struct.Config.html).
8.2.2 research snapshot; implementation oldidan compiler/target compatibility
tekshiriladi va exact tanlov Cargo manifest/lock’da belgilanadi. Shu task boshqa
mavjud dependency’larni upgrade qilmaydi. Herd kodi yoki proprietary asset nusxalanmaydi.

## 8. O‘lchanadigan maqsadlar

Quyidagilar taklif qilingan acceptance budget, hozirgi o‘lchangan tezlik emas:

- 100 local sibling fixture cold shallow scan: 3s ichida.
- Native filesystem o‘zgarishi UI’da: 5s ichida, existing polling bilan birga.
- Missed event/fallback convergence: 35s ichida.
- 10,000 child cap: aniq partial/truncated holat; full remove reconciliation yo‘q.
- 50 root switch va 10,000 event burst: bounded queue, no leaked active watcher/thread.
- Release artifact, OS/architecture/hardware va workload hisobotda qayd qilinadi.
  Budget o‘zgarsa o‘lchov/sabab alohida yoziladi, fail natija jim pass qilinmaydi.

## 9. Bajarish bosqichlari

| № | Bosqich | Bog‘liqlik | Mezonlar |
| --- | --- | --- | --- |
| 1 | Sozlama kontraktini kengaytirish | none | 7 |
| 2 | Chegaralangan skanerni yaratish | 1 | 8 |
| 3 | Registrni xavfsiz birlashtirish | 1, 2 | 10 |
| 4 | Kuzatuvchi hayotini ulash | 1, 2, 3 | 8 |
| 5 | Settings oqimini yetkazish | 1, 2, 3, 4 | 9 |
| 6 | Native platformalarni tekshirish | 1, 2, 3, 4, 5 | 9 |
| 7 | Polish & Harden | 1, 2, 3, 4, 5, 6 | 9 |

### 1. Sozlama kontraktini kengaytirish

Loyiha papkasining yagona, saqlanadigan va eski sozlamalarga mos kontraktini yaratish.

**Deliverables:**
- `crates/core/src/lib.rs`
- `crates/platform/src/lib.rs`
- `apps/desktop/src/bridge.ts`
- `apps/desktop/src-tauri/src/lib.rs`

**Acceptance criteria:**
- P1.1: Eski language/theme JSON yangi maydonlarsiz yuklanadi va ikkala qiymat saqlanadi.
- P1.2: Default policy macOS va Windows uchun foydalanuvchining home katalogiga Devforge segmentini qo‘shadi; disk ildizidagi /Devforge yaratilmaydi.
- P1.3: Custom absolute yo‘l yoki boshlang‘ich ~ qo‘llab-quvvatlanadi; relative, bo‘sh, NUL/control character va faylga ishora qiladigan yo‘l actionable error beradi.
- P1.4: Til/theme o‘zgarishi discovery yo‘lini, discovery o‘zgarishi til/theme qiymatini yo‘qotmaydi; stale full-settings yozuvlari uchun regression testi bor.
- P1.5: Default katalogni yaratish bir martalik initialization yoki explicit create bilan bajariladi; mavjud katalog va uning tarkibi o‘zgarmaydi.
- P1.6: Custom path mavjud bo‘lmasa Save uni jim yaratmaydi: Create folder amali yoki boshqa yo‘l tanlash talab etiladi.
- P1.7: Saqlash/validation xatosida oldingi saqlangan yo‘l saqlanadi; app startup settings xatosini muvaffaqiyat deb yashirmaydi.

Batafsil work-spec va buyruqlar: [phase-1.md](phases/phase-1.md).

### 2. Chegaralangan skanerni yaratish

Runtime tanlanmagan bo‘lsa ham Laravel loyihasini xavfsiz aniqlash.

**Deliverables:**
- `crates/projects/src/discovery.rs (new)`
- `crates/projects/src/project.rs`
- `crates/projects/src/lib.rs`

**Acceptance criteria:**
- P2.1: Laravel fixture artisan/public/index.php/composer.json belgilariga ko‘ra topiladi; mos kelmagan katalog successful Laravel deb ko‘rsatilmaydi.
- P2.2: Bir daraja skanerlanadi: nested app, vendor va node_modules tarkibi rekursiv o‘qilmaydi; dot-directory va DevForge staging/data kataloglari kandidat bo‘lmaydi.
- P2.3: PHP/Node paketlari o‘rnatilmagan fixture ham needs_runtime candidate sifatida chiqadi; hech qanday download boshlanmaydi.
- P2.4: Qisman ko‘chirilgan loyiha keyingi skanda to‘liq bo‘lgach topiladi; invalid yoki oversize manifest boshqa loyihalarning natijasini to‘sib qo‘ymaydi.
- P2.5: Scan script, composer/npm, artisan, Git hook va .env amallarini bajarmaydi; byte/sentinel tekshiruvi project fayllari o‘zgarmaganini isbotlaydi.
- P2.6: Symlink/junction/reparse child boshlang‘ich scope’da avtomatik follow qilinmaydi; sabab ko‘rsatiladi va explicit Import saqlanadi.
- P2.7: Root unavailable yoki entry cap holatida partial/unavailable qaytadi; bu holat barcha eski loyihalar missing degan natijaga aylantirilmaydi.
- P2.8: Tekshiruv uchun fake clock/temporary tree ishlatilib cancellation va 10,000 direct-child cap aniq tasdiqlanadi; cap natijasida truncation ko‘rsatiladi.

Batafsil work-spec va buyruqlar: [phase-2.md](phases/phase-2.md).

### 3. Registrni xavfsiz birlashtirish

Topilgan loyihalarni mavjud ma’lumotlar bilan dublikatsiyasiz va tiklanadigan tarzda birlashtirish.

**Deliverables:**
- `crates/projects/src/project.rs`
- `crates/projects/src/discovery.rs`
- `crates/workspace/src/discovery.rs (new)`
- `crates/workspace/src/lib.rs`
- `crates/serving/src/site.rs (only if scoped reuse is necessary)`

**Acceptance criteria:**
- P3.1: Bir xil canonical/fizik loyiha manual import, create wizard va discovery orqali uchrasa bitta active Project ID bilan qoladi.
- P3.2: Compatible installed runtime mavjud bo‘lganda har qanday qayta skan registrda aynan bitta project/site yaratadi; yarim yozuv keyingi reconcile’da tuzatiladi.
- P3.3: Runtime yetishmasa candidate UI contract’da qoladi; register qilish uchun soxta package ID ishlatilmaydi va xizmat ishga tushmaydi.
- P3.4: Mavjud PHP/Node selections, custom domain, favorite, DB/cache/mail bindings, secrets, worktree ownership va project revisions skaner tomonidan almashtirilmaydi.
- P3.5: Forget discovery suppression yozadi; qayta skan/restart loyihani qaytarmaydi; Restore/Import suppression’ni explicit olib tashlaydi.
- P3.6: Complete skan yo‘qolgan papkani missing deb belgilaydi; failed/partial scan hech qanday ommaviy missing holatini qo‘llamaydi va DB/project fayllari o‘chmaydi.
- P3.7: Same-filesystem stable identity bilan ishonchli rename project ID va bog‘lanishlarni saqlaydi; running session bo‘lsa relocation pending bo‘ladi, guarded stop/apply yo‘li talab etiladi.
- P3.8: Windows File ID/volume identity mavjud bo‘lmasa yoki identity noaniq bo‘lsa avtomatik birlashtirish qilinmaydi: relocation review candidate, old record saqlanadi.
- P3.9: Root A dan B ga o‘tganda old association detached bo‘ladi, manual entries qoladi va A generation natijalari B ni o‘zgartira olmaydi.
- P3.10: Yangi domen shop.devforge.test formatida; Unicode/invalid nomga stable valid slug taklif qilinadi; domain conflict mavjud saytni egallamaydi va rename custom domenni o‘zgartirmaydi.

Batafsil work-spec va buyruqlar: [phase-3.md](phases/phase-3.md).

### 4. Kuzatuvchi hayotini ulash

Ilova ochiq paytda fayl tizimi o‘zgarishlarini yo‘qotmasdan va UI’ni bloklamasdan aniqlash.

**Deliverables:**
- `crates/workspace/src/discovery.rs`
- `crates/workspace/Cargo.toml`
- `Cargo.lock (if watcher dependency is added)`
- `apps/desktop/src-tauri/src/workspace.rs`
- `apps/desktop/src-tauri/src/lib.rs`

**Acceptance criteria:**
- P4.1: Ilova startida UI sahifasi ochilishini kutmasdan saved root scan boshlanadi; close-to-tray holatida davom etadi, Quit’da to‘liq tugaydi.
- P4.2: Root ichida mkdir/copy/rename/delete va direct-child manifest atomic replace natijalari restart yoki Refresh bosmasdan snapshot’da chiqadi.
- P4.3: Watcher event’lari 500ms quiet debounce va 2s max-latency invalidation bilan birlashtiriladi; bitta active scan va bounded dirty flag/queue mavjud.
- P4.4: Root/child watch error yoki overflow 30s periodic shallow fallback va ko‘rinadigan degraded status beradi; app focus va manual rescan recovery qiladi.
- P4.5: 100-child local fixture event’i UI pollingni qo‘shib 5s ichida ko‘rinadi; fallback fixture 35s ichida konvergensiya qiladi; bular proposed acceptance budget, o‘lchov qayd etiladi.
- P4.6: Root switch vaqtida eski watch’lar va pending timer bekor qilinadi; late generation natija persist qilinmaydi; 50 switch stress’da handle/thread soni baseline’ga qaytadi.
- P4.7: Root delete/recreate yoki disconnect/reconnect parent watch/fallback orqali tiklanadi; permission-denied degraded holati eski rows’ni o‘chirmaydi.
- P4.8: 10,000 ta event burst bounded xotira bilan bitta final authoritative natijaga konvergensiya qiladi; user project scripts/caches bo‘yicha recursive event storm kuzatilmaydi.

Batafsil work-spec va buyruqlar: [phase-4.md](phases/phase-4.md).

### 5. Settings oqimini yetkazish

Foydalanuvchi papkani tanlashi va aniqlash natijasini boshqarishi uchun mavjud UI’ni kengaytirish.

**Deliverables:**
- `apps/desktop/src/pages/SettingsPage.tsx`
- `apps/desktop/src/components/ProjectDiscoverySettings.tsx (new, if extracted)`
- `apps/desktop/src/pages/WorkspacePage.tsx`
- `apps/desktop/src/components/ProjectSetup.tsx`
- `apps/desktop/src/bridge.ts`
- `apps/desktop/src/workspace.ts`
- `apps/desktop/src/App.tsx`
- `apps/desktop/src/styles.css`
- `apps/desktop/src/discovery.test.tsx (new)`
- `shared/locales/{en,ru,uz-Latn}.json`

**Acceptance criteria:**
- P5.1: Text input va native folder picker bir xil draft’ni yangilaydi; Cancel picker current/saved qiymatni o‘zgartirmaydi; Save keyin relaunch’da yo‘l qaytadi.
- P5.2: Save explicit: har keystroke active watcher’ni o‘zgartirmaydi; invalid input inline xato, pending holatda double-submit bloklanadi; failed Save eski active root’ni saqlaydi.
- P5.3: Default qiymat ~/Devforge, Windows’da resolved native path ko‘rsatiladi; Reset default oldingi user folder contentini ko‘chirmaydi yoki o‘chirmaydi.
- P5.4: Scan now single-flight ishlaydi; scanning/empty/success/degraded/error/unavailable holatlar ko‘rinadi, error’ning retry amali bor.
- P5.5: Candidate needs-runtime/missing/conflict/ignored/relocation-pending holatlari oddiy stopped/ready bilan aralashtirilmaydi; Start faqat haqiqiy registered ready loyiha uchun ishlaydi.
- P5.6: Forget qilingan entry uchun Restore oqimi va root o‘zgargandagi detached loyihalar tushuntiriladi; existing manual Import tashqaridagi loyihalarni qo‘shadi.
- P5.7: New Project default parent saved root’dan olinadi; publish/setup staging katalogi tayyor bo‘lguncha discovery uni register qilmaydi; create/discovery race bitta project yaratadi.
- P5.8: Russian, Uzbek, English catalogs label/error/status/a11y matnlar bilan teng; dark/light/system va minimum desktop window’da long Unicode path horizontal overflow bermaydi.
- P5.9: Keyboard Tab, visible focus, picker cancel/return focus va aria-live status ishlaydi; loyiha nomlari/yo‘llari tarjima qilinmaydi.

Batafsil work-spec va buyruqlar: [phase-5.md](phases/phase-5.md).

### 6. Native platformalarni tekshirish

OS watcher, path identity, Tauri IPC va real serving’ni har targetda bajarib isbotlash.

**Deliverables:**
- `crates/workspace/examples/support/discovery.rs (new)`
- `crates/workspace/examples/native_workspace.rs (existing dispatcher path verify)`
- `scripts/discovery_desktop_smoke.py (new)`
- `scripts/discovery_native_smoke.py (if needed)`
- `.github/workflows/discovery.yml (new)`
- `docs/evidence/project-discovery/ (public reports only)`

**Acceptance criteria:**
- P6.1: macOS ARM64’da built Tauri app, watcher, saved path, startup/relaunch va discovered Laravel HTTP fixture actual ishlaydi; OS/arch/source hash hisobotda bor.
- P6.2: macOS Intel’da ayni final source hash bilan native backend va WebView checks bajariladi; ARM64 natijasi Intel o‘rniga yozilmaydi.
- P6.3: Windows x64’da ayni final source hash bilan WebView2, watcher, backslash/drive case, case-only rename, Cyrillic/spaces/non-ASCII parent va long path tests bajariladi.
- P6.4: Har targetda real folder picker selection/cancel tekshiriladi yoki alohida NOT VERIFIED gate yoziladi; native dialogga xizmat qiladigan IPC mock bilan pass belgilanmaydi.
- P6.5: No-runtime candidate, installed-runtime register, duplicate/manual import, Forget+Restore, corrupt manifest recovery, root permission/missing va switch-during-scan stsenariylari native hisobotda bor.
- P6.6: Actual watcher event/overflow fallback va tray/quit lifecycle tekshiriladi; App e2e feature production HTTP control endpointini ochmaydi.
- P6.7: Fixture HTTP selected PHP orqali 200 beradi; existing .devforge.test/.localhost-port serving modeli saqlanadi; hosts/CA native fixture faqat owned isolated muhitda.
- P6.8: Reports/screenshots tested commit + uncommitted artifact digest/source archive bilan bog‘lanadi; zip report provenance tekshiriladi va secrets/raw .env yuklanmaydi.
- P6.9: Target yoki dialog automation mavjud bo‘lmasa no green completion: bajarilgan mustaqil checks va ochiq native gates aniq saqlanadi; cross-compile runtime deb ko‘rsatilmaydi.

Batafsil work-spec va buyruqlar: [phase-6.md](phases/phase-6.md).

### 7. Polish & Harden

Oxirgi source barcha kontrakt, foydalanish va platforma mezonlariga mosligini yakuniy tekshirish.

**Deliverables:**
- `affected implementation/tests from phases 1-6`
- `docs/knowledge/project-discovery.md`
- `docs/knowledge/references/conversation-decisions.md`
- `docs/knowledge/index.md`
- `docs/knowledge/log.md`
- `docs/evidence/project-discovery/`

**Acceptance criteria:**
- P7.1: Final workspace fmt/clippy/tests va frontend typecheck/test/build/locale checks exit 0; relevant baseline failures bo‘lsa ular aniq dalil bilan ochiq va complete claim berilmaydi.
- P7.2: 100 local sibling candidate fixture cold scan 3s ichida, UI confirmation native event’dan 5s ichida, fallback 35s ichida; release build/hardware/workload bilan o‘lchanadi yoki asosli revised budget review’da qayd etiladi.
- P7.3: Normal local idle’da periodic root reconciliation 30s dan tez emas, unchanged candidates reimport qilinmaydi, child dependency subtree o‘qilmaydi; resource trend 50 root switch/10k event test’da bound ichida.
- P7.4: Minimum supported desktop size va 3 locale x light/dark (system representative) states screenshot/interaction bilan tekshiriladi; scoped axe’da yangi finding 0, native picker focus alohida tekshiriladi.
- P7.5: Existing manual import/Create/Prepare, custom runtime/domain, worktree cleanup, service binding va persistent DB ma’lumotlari uchun tegishli regression stsenariylari o‘tadi.
- P7.6: Discovery avtomatik process/download/DB/.env/system DNS/CA mutation boshlamasligi sentinel/owned-state checks bilan tasdiqlanadi; Start/Prepare mavjud explicit oqimida qoladi.
- P7.7: Yakuniy knowledge’da confirmed requirement, proposed/changed design, actual implementation va har platformaning verified/unverified chegarasi ajratiladi; strict validator 0 error/0 warning.
- P7.8: Final diff’da task debug prints, session TODO/FIXME, dead imports yoki aloqasiz source o‘zgarishi yo‘q; publish/deploy/commit/push userning yangi ko‘rsatmasisiz bajarilmaydi.
- P7.9: Original ROADMAP criteria yakuniy source/evidence bilan qayta audit qilinadi; native yoki system-dialog gate ochiq bo‘lsa feature fully verified deb yopilmaydi.

Batafsil work-spec va buyruqlar: [phase-7.md](phases/phase-7.md).

## 10. Native qabul matritsasi

| Tekshiruv | macOS ARM64 | macOS Intel | Windows x64 |
| --- | --- | --- | --- |
| Default/custom path, fresh + legacy settings | Majburiy | Majburiy | Majburiy |
| Actual native watcher + fallback | Majburiy | Majburiy | Majburiy |
| Add/incomplete/rename/delete/root reconnect | Majburiy | Majburiy | Majburiy |
| Duplicate/manual import/Forget/Restore | Majburiy | Majburiy | Majburiy |
| Unicode/spaces/long path/case behavior | Majburiy | Majburiy | Majburiy |
| Real Tauri save/relaunch + picker select/cancel | Majburiy | Majburiy | Majburiy |
| Selected PHP HTTP + no-runtime state | Majburiy | Majburiy | Majburiy |
| 3 locale, light/dark, minimum window, keyboard | Majburiy | Majburiy | Majburiy |
| Tray/quit cleanup, source/report hash | Majburiy | Majburiy | Majburiy |

Har katak actual run/report bilan yopiladi. Current planning’da barchasi NOT RUN.
Windows ARM, signed release, fresh installer va minimum OS certification shu feature
qabulidan tashqarida; existing release gates yopilgan deb ko‘rsatilmaydi.
System dialog bo‘lmasa text-input save path natijasi alohida report qilinadi; dialog
sinovi ochiq gate. CI dispatch/push uchun hozir authorization yo‘q, shu sabab native
run kelajak implementatsiya davomida mavjud authorized yo‘l orqali bajariladi.

## 11. Uch asosiy xavf

1. **Identity/adashgan merge (o‘rta ehtimol, yuqori zarar):** path-only identity yoki
   cross-volume rename DB/secrets’ni boshqa kodga ulashi mumkin. Stable platform identity,
   conservative review, revision/session guard, source associations va preservation tests.
2. **Watcher/race/data loss (yuqori ehtimol, o‘rta-yuqori zarar):** incomplete copy,
   lost events, denied root yoki late old-root natija. Single-flight/generation, full-vs-partial
   scan, bounded queue, periodic reconciliation, no automatic destructive cleanup.
3. **Platform/native verification gap (o‘rta ehtimol, yuqori yashirin risk):** Windows
   File ID/junction/case/path/dialog behavior macOS’dan farq qiladi. Uch actual native
   target, test source hash, system dialog alohida gate; unavailable target green emas.

## 12. Self-critique

Plan audit uch muammoni tuzatdi:

- “Rename’da sozlamalar saqlanadi” umumiy va’da o‘rniga aniq stable identity, running
  session guard va ambiguous/cross-volume review kontrakti berildi.
- Watcher root-only bo‘lsa yarim ko‘chirilgan loyiha markerlarini o‘tkazib yuborishi mumkin;
  direct-child/targeted marker invalidation va periodic rescan qabul mezoniga qo‘shildi.
- “Native GUI o‘tdi” system picker ishlaganini anglatmaydi; select/cancel haqiqiy dialog
  gate’i va typed-input test’i alohida ajratildi.

Weakest dependency: phase 3 reconciliation. U pass bo‘lmasdan watcher registration
faollashtirilmaydi. Har criterion phase spec’da individual ID bilan tekshiriladi.

## 13. Yakuniy audit va topshirish

Har phase code/evidence/knowledge’ni solishtiradi. Final audit original mezonlar,
actual final diff, mandatory checks va exact tested native source’ni qayta tekshiradi.
Har platformaning bajarilgan/skipped qamrovi alohida report qilinadi. Global Codex
memory user alohida so‘ramasa yozilmaydi. Old M7/M13 yoki boshqa roadmap ishlarini
shu task bilan boshlash yo‘q. Commit/push/deploy uchun alohida user ko‘rsatmasi kerak.

To‘liq tugaganlik: Settings default/custom persistence, unattended discovery,
metadata preservation, no duplicates/suppression, lifecycle/failure recovery,
uch native target va actual picker gate, 3 locale UI, mandatory checks,
strict knowledge validator va original acceptance audit’ining barchasi dalil bilan yopilgan.
