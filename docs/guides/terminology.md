# DevForge task and error terminology / Термины / Atamalar

This glossary copies current labels from the shared catalogs:
[English](../../shared/locales/en.json), [Русский](../../shared/locales/ru.json),
[O‘zbekcha](../../shared/locales/uz-Latn.json). The registry uses `en`, `ru` and
`uz-Latn` (`uz` is an alias). This is a documentation snapshot, not a second
translation catalog. A catalog change can require updating the affected row.
Native-language review and observed RU/UZ/EN task completion remain unverified
by this document; key parity does not establish linguistic or usability quality.

## Find a control / Найти действие / Amalni topish

Use the labels with the [first-project runbook](first-project.md). The stable key
identifies the same control/state when the language changes.

| Catalog key | English | Русский | O‘zbekcha |
| --- | --- | --- | --- |
| `nav.projects` | Projects | Проекты | Loyihalar |
| `nav.runtimes` | Runtimes | Runtime | Runtime’lar |
| `nav.services` | Services & logs | Сервисы и логи | Xizmatlar va loglar |
| `nav.inspector` | Inspector | Инспектор | Inspektor |
| `work.addProject` | Add project | Добавить проект | Loyiha qo‘shish |
| `setup.newProject` | New project | Новый проект | Yangi loyiha |
| `setup.prepare` | Prepare project | Подготовить проект | Loyihani tayyorlash |
| `setup.phpSettings` | PHP settings | Настройки PHP | PHP sozlamalari |
| `setup.systemHttps` | HTTPS setup | Настройка HTTPS | HTTPS sozlash |
| `work.install` | Install | Установить | O‘rnatish |
| `work.start` | Start | Запустить | Ishga tushirish |
| `work.stop` | Stop | Остановить | To‘xtatish |
| `work.restart` | Restart | Перезапустить | Qayta ishga tushirish |
| `work.openBrowser` | Open browser | Открыть в браузере | Brauzerda ochish |
| `work.openTerminal` | Terminal | Терминал | Terminal |
| `setup.cancelOperation` | Cancel operation | Отменить операцию | Amalni bekor qilish |
| `work.states.running` | Running | Работает | Ishlamoqda |
| `work.states.ready` | Ready | Готов | Tayyor |
| `work.states.stopped` | Stopped | Остановлен | To‘xtatilgan |
| `work.states.missing` | Not installed | Не установлен | O‘rnatilmagan |
| `inspector.install` | Install companion | Установить пакет | Laravel paketini o‘rnatish |
| `inspector.enable` | Enable capture | Включить сбор | Yig‘ishni yoqish |
| `inspector.pause` | Pause capture | Остановить сбор | Yig‘ishni to‘xtatish |
| `configuration.title` | Project configuration | Конфигурация проекта | Loyiha konfiguratsiyasi |
| `configuration.reviewAction` | Review saved configuration | Проверить сохранённую конфигурацию | Saqlangan konfiguratsiyani tekshirish |
| `configuration.apply` | Apply reviewed plan | Применить проверенный план | Tekshirilgan rejani qo‘llash |
| `configuration.localSecret` | Local secret store | Локальное хранилище секретов | Mahalliy maxfiy saqlash joyi |
| `configuration.storeSecret` | Store locally | Сохранить локально | Mahalliy saqlash |
| `configuration.writeEnv` | Include reviewed .env changes | Включить проверенные изменения .env | .env o‘zgarishlarini rejaga qo‘shish |

## Recognize a failure / Распознать ошибку / Xatoni aniqlash

Keep the stable error code when reporting a problem; the visible explanation
changes with the selected language. A missing secret and an unavailable
credential store need different recovery actions.

| Catalog key | English | Русский | O‘zbekcha |
| --- | --- | --- | --- |
| `project.errors.invalid_project` | This folder is not a supported Laravel project. | Эта папка не является поддерживаемым проектом Laravel. | Bu papka qo‘llanadigan Laravel loyihasi emas. |
| `project.errors.incompatible_php` | The selected PHP version does not match this project. | Выбранная версия PHP не подходит для проекта. | Tanlangan PHP versiyasi loyihaga mos emas. |
| `project.errors.runtime_unavailable` | A required runtime is missing or does not match this computer. | Нужная среда выполнения не установлена или несовместима с компьютером. | Kerakli runtime o‘rnatilmagan yoki kompyuterga mos emas. |
| `project.errors.secret_unavailable` | The native credential store is unavailable or a required secret is missing. | Хранилище учётных данных недоступно или нужный секрет отсутствует. | Maxfiy ma’lumotlar saqlagichi mavjud emas yoki kerakli qiymat yo‘q. |
| `project.errors.stale_revision` | The configuration changed. Reload it and try again. | Настройки изменились. Обновите их и повторите попытку. | Sozlamalar o‘zgargan. Yangilab, qayta urinib ko‘ring. |
| `runtime.errors.busy` | This runtime or process is currently in use. | Среда выполнения или процесс сейчас используется. | Bu runtime yoki jarayon hozir ishlatilmoqda. |
| `serving.errors.domain_conflict` | This domain or its development alias belongs to another project. | Домен или его локальный псевдоним принадлежит другому проекту. | Domen yoki uning lokal muqobili boshqa loyihaga biriktirilgan. |
| `serving.errors.port_unavailable` | The serving session or a required loopback port is unavailable. | Сессия сервера или нужный локальный порт недоступны. | Serving sessiyasi yoki kerakli lokal port band. |
| `serving.errors.permission_required` | Administrator permission is required for this system change. The HTTP fallback remains available. | Для изменения системы нужны права администратора. Резервный HTTP-адрес остаётся доступен. | Bu tizim o'zgarishi uchun administrator ruxsati kerak. Muqobil HTTP manzil ishlashda davom etadi. |
| `workspace.errors.setup_cancelled` | Setup was cancelled. Completed project files are preserved. | Подготовка отменена. Созданные файлы проекта сохранены. | Tayyorlash bekor qilindi. Yaratilgan loyiha fayllari saqlandi. |
| `workspace.errors.manifest_secret_missing` | Provide the missing local secret with config secret, then create a new plan. | Задайте недостающий локальный секрет через config secret и создайте новый план. | Yetishmayotgan lokal secretni config secret orqali kiriting va yangi reja oling. |
| `workspace.errors.manifest_secret_unavailable` | The local credential store is unavailable. Retry after restoring access. | Локальное хранилище учётных данных недоступно. Восстановите доступ и повторите. | Lokal credential store ochilmadi. Kirishni tiklab qayta urinib ko‘ring. |
| `workspace.errors.manifest_runtime_unavailable` | An exact runtime or package pin is unavailable for this platform. Review the bundled catalog. | Точная версия или pin пакета недоступны для платформы. Проверьте каталог. | Ushbu platforma uchun aniq runtime yoki paket pini mavjud emas. Katalogni tekshiring. |
| `workspace.errors.manifest_review_changed` | The file changed. Reload before saving. | Файл изменился. Перезагрузите перед сохранением. | Fayl o‘zgargan. Saqlashdan oldin qayta yuklang. |

## Preserve literal values / Не переводить значения / Qiymatlarni tarjima qilmang

- **EN:** Translate owned UI labels, not project names, raw paths, URLs, runtime
  IDs, commands, environment-variable names, error codes or child output.
- **RU:** Переводятся подписи интерфейса. Имена проектов, пути, URL, идентификаторы
  runtime, команды, имена переменных, коды ошибок и исходный вывод не переводятся.
- **UZ:** Interfeys atamalari tarjima qilinadi. Loyiha nomi, fayl yo‘li, URL,
  runtime ID, buyruq, env kaliti, xato kodi va jarayonning xom chiqishi o‘zgartirilmaydi.

`APP_KEY`, `.env`, `devforge.yaml`, `manifest_secret_missing` and `/path/to/project`
are literal identifiers. Examples use placeholders; never copy a real credential,
password-bearing URL or `.env` into a report.

**Running / Работает / Ishlamoqda** describes process state; **Ready / Готов /
Tayyor** is a readiness state. Neither alone proves that the application's chosen
page returned the expected HTTP response. Record that result separately.
**Cancelled / Отменено / Bekor qilindi** does not mean all completed setup work
was rolled back. Review retained files and the operation result before retrying.

## Maintenance boundary

Sources for reachable controls: [WorkspacePage](../../apps/desktop/src/pages/WorkspacePage.tsx),
[ProjectSetup](../../apps/desktop/src/components/ProjectSetup.tsx),
[ConfigurationPage](../../apps/desktop/src/pages/ConfigurationPage.tsx) and
[InspectorPage](../../apps/desktop/src/pages/InspectorPage.tsx). A glossary update
needs changed-key/label and local-link checks; it does not require rerunning the
product suite. Observed user-language review is a separate pilot task.
