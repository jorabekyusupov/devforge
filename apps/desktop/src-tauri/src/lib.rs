use devforge_core::{ErrorCode, LocaleRegistry, Settings, SettingsRepository, Theme};
use devforge_platform::{Diagnostics, SqliteSettings, app_data_dir, diagnostics};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
};

const BUNDLE: &str = include_str!(concat!(env!("OUT_DIR"), "/locales.json"));

#[derive(Clone)]
struct AppState {
    database: Arc<Mutex<Option<SqliteSettings>>>,
    directory: PathBuf,
    registry: Arc<LocaleRegistry>,
    system_locale: Option<String>,
    native_synced: Arc<AtomicBool>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bootstrap {
    settings: Settings,
    resolved_locale: String,
    direction: String,
    system_locale: Option<String>,
    unsupported_preference: bool,
    diagnostics: Diagnostics,
    native_ui_synced: bool,
}
impl AppState {
    fn snapshot(&self, update: Option<Settings>) -> Result<Bootstrap, ErrorCode> {
        if let Some(ref settings) = update {
            self.registry.validate(settings)?;
        }
        let mut lock = self
            .database
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        if lock.is_none() {
            *lock = Some(SqliteSettings::open(&self.directory)?);
        }
        let database = lock.as_mut().ok_or(ErrorCode::StorageUnavailable)?;
        if let Some(settings) = update {
            database.save(&settings)?;
        }
        let settings = database.load()?;
        let resolved = self
            .registry
            .resolve(&settings.language, self.system_locale.as_deref());
        Ok(Bootstrap {
            unsupported_preference: !self.registry.supports_choice(&settings.language),
            resolved_locale: resolved.id.clone(),
            direction: resolved.direction.clone(),
            settings,
            diagnostics: diagnostics(database),
            system_locale: self.system_locale.clone(),
            native_ui_synced: self.native_synced.load(Ordering::Relaxed),
        })
    }
}

#[tauri::command]
async fn bootstrap(state: tauri::State<'_, AppState>) -> Result<Bootstrap, ErrorCode> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.snapshot(None))
        .await
        .map_err(|_| ErrorCode::StorageUnavailable)?
}
#[tauri::command]
async fn save_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    settings: Settings,
) -> Result<Bootstrap, ErrorCode> {
    let worker = state.inner().clone();
    let mut snapshot =
        tauri::async_runtime::spawn_blocking(move || worker.snapshot(Some(settings)))
            .await
            .map_err(|_| ErrorCode::StorageUnavailable)??;
    snapshot.native_ui_synced =
        apply_native(&app, &snapshot.resolved_locale, snapshot.settings.theme).is_ok();
    state
        .native_synced
        .store(snapshot.native_ui_synced, Ordering::Relaxed);
    Ok(snapshot)
}

fn label(locale: &str, key: &str) -> String {
    let bundle: serde_json::Value = serde_json::from_str(BUNDLE).expect("validated bundled JSON");
    let fallback = bundle["registry"]["fallback"].as_str().expect("fallback");
    bundle["catalogs"][locale]["native"][key]
        .as_str()
        .or_else(|| bundle["catalogs"][fallback]["native"][key].as_str())
        .expect("validated native label")
        .into()
}
fn tray_menu(app: &tauri::AppHandle, locale: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let show = MenuItem::with_id(app, "show", label(locale, "show"), true, None::<&str>)?;
    let settings = MenuItem::with_id(
        app,
        "settings",
        label(locale, "settings"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", label(locale, "quit"), true, None::<&str>)?;
    Menu::with_items(
        app,
        &[
            &show,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}
fn apply_native(app: &tauri::AppHandle, locale: &str, theme: Theme) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", label(locale, "show"), true, None::<&str>)?;
    let settings = MenuItem::with_id(
        app,
        "settings",
        label(locale, "settings"),
        true,
        Some("CmdOrCtrl+,"),
    )?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        label(locale, "quit"),
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    let app_menu = Submenu::with_items(
        app,
        "DevForge",
        true,
        &[
            &show,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        label(locale, "edit"),
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(&label(locale, "undo")))?,
            &PredefinedMenuItem::redo(app, Some(&label(locale, "redo")))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(&label(locale, "cut")))?,
            &PredefinedMenuItem::copy(app, Some(&label(locale, "copy")))?,
            &PredefinedMenuItem::paste(app, Some(&label(locale, "paste")))?,
            &PredefinedMenuItem::select_all(app, Some(&label(locale, "selectAll")))?,
        ],
    )?;
    app.set_menu(Menu::with_items(app, &[&app_menu, &edit])?)?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(tray_menu(app, locale)?))?;
    }
    if let Some(window) = app.get_webview_window("main") {
        window.set_theme(match theme {
            Theme::System => None,
            Theme::Light => Some(tauri::Theme::Light),
            Theme::Dark => Some(tauri::Theme::Dark),
        })?;
    }
    Ok(())
}
fn menu_action(app: &tauri::AppHandle, id: &str) {
    match id {
        "quit" => app.exit(0),
        "show" | "settings" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            if id == "settings" {
                let _ = app.emit("native-navigate", "settings");
            }
        }
        _ => {}
    }
}
fn tray_icon() -> tauri::image::Image<'static> {
    let mut pixels = vec![0; 16 * 16 * 4];
    for y in 2..14 {
        for x in 2..14 {
            if !(5..=10).contains(&x) || !(5..=10).contains(&y) {
                let index = (y * 16 + x) * 4;
                pixels[index..index + 4].copy_from_slice(&[15, 118, 110, 255]);
            }
        }
    }
    tauri::image::Image::new_owned(pixels, 16, 16)
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![bootstrap, save_settings])
        .on_menu_event(|app, event| menu_action(app, event.id.as_ref()))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.app_handle().tray_by_id("main").is_some()
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let bundle: serde_json::Value = serde_json::from_str(BUNDLE)?;
            let state = AppState {
                database: Arc::new(Mutex::new(None)),
                directory: app_data_dir()?,
                registry: Arc::new(LocaleRegistry::from_json(&bundle["registry"].to_string())?),
                system_locale: sys_locale::get_locale(),
                native_synced: Arc::new(AtomicBool::new(true)),
            };
            let snapshot = state.snapshot(None).ok();
            let locale = snapshot
                .as_ref()
                .map(|s| s.resolved_locale.as_str())
                .unwrap_or(state.registry.fallback());
            let theme = snapshot
                .as_ref()
                .map(|s| s.settings.theme)
                .unwrap_or_default();
            let tray = TrayIconBuilder::with_id("main")
                .icon(tray_icon())
                .tooltip("DevForge")
                .menu(&tray_menu(app.handle(), locale)?)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| menu_action(app, event.id.as_ref()))
                .build(app);
            let menus_synced = apply_native(app.handle(), locale, theme).is_ok();
            state
                .native_synced
                .store(tray.is_ok() && menus_synced, Ordering::Relaxed);
            app.manage(state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("DevForge desktop runtime failed");
}
