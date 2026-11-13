# Sozlanadigan loyiha papkasi — yakuniy qabul

Funksiya scope’i qabul qilindi:7 bosqich,60 mezon. Native macOS ARM64/Intel va
Windows x64 backend/UI sinovlari o‘tdi. Branch `codex/project-discovery-verify`.
Ilova, yadro, runtime katalogi va barcha runner source fayllari bir xil tree SHA256
bilan bog‘langan: `74a7e5f84fcf45c7f7bddd5c4896c8bd1c26689bde92719ed33ca8cc5285331a`.

## Foydalanuvchi uchun natija

Settings default papka OS home ichidagi `Devforge`: macOS’da `~/Devforge`, Windows’da
`<user profile>\Devforge`. Yo‘lni yozish/native picker bilan tanlash, explicit Save,
Create, default draft va Scan now mavjud. Bevosita ichki Laravel papkalari avtomatik
topiladi. Mos installed PHP CLI/web va Node bilan project/site saqlanadi, runtime
bo‘lmasa needs-runtime candidate chiqadi. Prepare/Start explicit qoladi. Root
almashtirilishi yoki yo‘qolishi project/DB fayllarini o‘chirmaydi. Forget suppression
va Restore original ID/profile bilan ishlaydi. New Project parent saved root’dan.

## Actual native matritsa

| Target | Backend / actual GUI | 100 cold scan | 100-project event → UI | Actual fallback | UI / axe cases / violations |
| --- | --- | --- | --- | --- | --- |
| macos-arm64 | PASS | 11ms | 1397ms | 30663ms | 12 / 10 / 0 |
| macos-x64 | PASS | 9ms | 2022ms | 30588ms | 12 / 10 / 0 |
| windows-x64 | PASS | 26ms | 1199ms | 30518ms | 12 / 10 / 0 |

Har targetda real PHP8.5.12/Laravel13.42.0 HTTP, no-runtime, incomplete copy,
rename/domain/ID, Forget/Restore profile, manual metadata, root delete/recreate,
permission-denied preservation, symlink/junction, long Unicode/case paths,50 root
changes va10k-file workload tekshirildi. Resource before/after qiymatlari individual
JSON/provenance’da. Bu o‘lchovlar universal tezlik kafolati emas; vendor’ni nusxalash
bilan copy/register va ready-project event-to-UI alohida workload’lar.

Actual NSOpenPanel AXPress va Win32 Button HWND BM_CLICK select/cancel natijasini
berdi; strict samefile bilan tekshirildi. Native keyboard Tab, focus return, tray/
relaunch, three locales x light/dark va minimum window tekshirildi. Actual viewport
har report’da saqlangan, horizontal overflow0. Native UI real WKWebView/WebView2,
real Tauri IPC va alohida e2e featureli debug acceptance build bilan bajarildi.
Default-feature desktop va CLI build bu control endpoint’ni qo‘shmaydi.

[Core CI](https://github.com/jorabekyusupov/devforge/actions/runs/43419610528) ham o‘tdi: macOS110/Windows108 Rust, har birida1 opt-in
OS credential fixture ignored;37 frontend,6 packaging,3 locale testi,3 semantic
catalog, typecheck/fmt/clippy va default desktop build. OS credential fixture
discovery uchun kerak bo‘lmaganligi sabab ishga tushirilmadi.

## Hash va source dalili

- [macos-arm64 native job](https://github.com/jorabekyusupov/devforge/actions/runs/43416651687/job/131856134944) — source `0fb0eca7d7bf28dfae1039d584503275cadb2b79`; ZIP SHA256 `6db7b7037b194a074ef43adcf0200be5f9d3334bac7a28ee470d35c2915531b9`.
- [windows-x64 native job](https://github.com/jorabekyusupov/devforge/actions/runs/43416651687/job/131856135395) — source `0fb0eca7d7bf28dfae1039d584503275cadb2b79`; ZIP SHA256 `671e9cb46f80cca88e764b29591b5d16b101d5bd1ee3f42b94470b84386e63a5`.
- [macos-x64 native job](https://github.com/jorabekyusupov/devforge/actions/runs/43419610647/job/131866006740) — source `c33eadc4486ebf12c9ad50a95dab81dbdd5d4e7e`; ZIP SHA256 `f5fa9eae014be50e9bf50c5a46c15cb052b53b5c31bf7fa0be525ef5e940a3dc`.

ARM64/Windows `0fb0eca`, Intel `c33eadc` checkout’dan: ikkinchisida faqat CI target
selector va original CLI cache entry qo‘shilgan. [Tree equivalence](product-source-equivalence.json)
ilova/yadro/katalog/runner fayllarining hashlarini aynan teng deb tekshiradi. Har
job o‘z commit/archive/ZIP SHA256 bilan qayd etiladi; eski source’dagi pass yangi
source o‘rniga ko‘rsatilmaydi. [Core](core-checks.json), [provenance](provenance.json)
va [60 mezon](../../../.supergoal/devforge-configurable-project-folder-aut-fKSwZE/acceptance-audit.json)
individual tekshiruvni beradi. Qo‘shimcha actual local Apple M4/macOS27.2.1 dalili
[alohida](local-macos-provenance.json); source/binary/report/runner hashlari saqlangan.

## Tuzatilgan xatolar va o‘zgargan dizayn

Windows descendant watch handles stopped Laravel rename’ini to‘sdi: collection
root recursive va parent nonrecursive watch ishlatiladi. Callback deep dependency/
cache events’ni lexical filter qiladi; scanner shallow qoladi. macOS shallow child/
public watch’larni ishlatadi. Windows normal/verbatim/case yo‘llari moslashtirildi;
100-project actual UI failure bu xatoni ochdi. Pathless overflow/rescan flag ham
recovery’ni boshlaydi; unit flag testi va actual30s fallback dalili alohida.

Schema6 additive ledger, physical replacement protection, transactional CLI/first-
scan Forget, original-profile Restore, stale Settings patch, generation cancellation,
picker focus after re-enable va unsaved draft guard tuzatildi. UI runner route
mount’ni kutadi; assertions va performance deadline’lari o‘zgarmagan.

## Runtime arxivlari va qamrov chegarasi

PHP tarqatuvchisi bir xil versiyali macOS arxivlarini qayta chiqardi. Joriy live-URL
hashlari katalog pinlaridan farq qiladi; [qayd](upstream-runtime-checksums.json).
Katalog/versiyalar o‘zgartirilmadi va mismatched arxivlar ishga tushirilmadi. Native
CI original reviewed public arxivlarni SHA256 bilan tekshirilgan vaqtinchalik
[cache metadata](../../../.github/fixtures/discovery-runtime-blobs.json) orqali oldi.
Git blob bytes streaming’dan keyin original katalog hashiga tekshiriladi, PackageStore
install oldidan yana tekshiradi. Cache binary fayllari branch tree yoki release’ga
qo‘shilmadi; bu fresh public-URL/catalog maintenance gate’ini yopmaydi.

OS security permissions so‘ralmadi/o‘zgartirilmadi. Denied-root testi faqat owned
disposable fixture papkasi ruxsatini vaqtincha o‘zgartirib, qaytardi. Public artifacts
faqat report JSON va screenshot’lardan iborat; raw preferences/.env/secret store yo‘q.

Delivered scope:one shallow Laravel root. Multiple roots, arbitrary framework
drivers, network/removable FS certification, Windows ARM, signed public release,
fresh installer va minimum OS certification ushbu feature scope’idan tashqari.
Existing M15 release gate’lari saqlanadi. Bu vazifa M7/M13 ishlarini o‘zgartirmadi.


## Branch integration chegarasi

Feature9319e82 baseline’dan ajratilgan branch’da tekshirildi. Tekshiruv davomida
remote main alohida M7 vazifasi orqali65948e0 ga ko‘chdi; bu task main’ni merge yoki
deploy qilmadi. Qabul dalillari yuqoridagi feature/source hashlariga tegishli.
M7 bilan birlashtirilgan main checkout uchun alohida integration tekshiruvi kerak.
