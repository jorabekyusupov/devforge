use devforge_runtime::Cancellation;
use devforge_workspace::{Command, Error, Snapshot, Workspace};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::Manager;
#[derive(Clone)]
pub struct State {
    root: PathBuf,
    workspace: Arc<Mutex<Option<Workspace>>>,
    cache: Arc<Mutex<Snapshot>>,
    progress: Arc<Mutex<Option<devforge_workspace::setup::Progress>>>,
    cancel: Arc<Mutex<Option<Cancellation>>>,
    discovery: Arc<Mutex<Option<devforge_workspace::discovery::Service>>>,
    pub quitting: Arc<AtomicBool>,
    pub exited: Arc<AtomicBool>,
}
impl State {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            workspace: Arc::new(Mutex::new(None)),
            cache: Arc::new(Mutex::new(Snapshot::default())),
            progress: Arc::new(Mutex::new(None)),
            cancel: Arc::new(Mutex::new(None)),
            discovery: Arc::new(Mutex::new(Some(
                devforge_workspace::discovery::Service::new(),
            ))),
            quitting: Arc::new(AtomicBool::new(false)),
            exited: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn set_discovery_root(&self, root: PathBuf) {
        if let Some(service) = self.discovery.lock().unwrap().as_ref() {
            service.set_root(root);
        }
    }
    pub fn request_discovery(&self) {
        if let Some(service) = self.discovery.lock().unwrap().as_ref() {
            service.request();
        }
    }
    pub fn start_discovery_coordinator(&self) {
        let state = self.clone();
        std::thread::spawn(move || {
            while !state.quitting.load(Ordering::Relaxed) {
                if let Ok(mut lock) = state.workspace.try_lock() {
                    if lock.is_none() {
                        *lock = Workspace::open(&state.root, secrets(&state.root)).ok();
                    }
                    if let Some(w) = lock.as_mut() {
                        let _ = state.sync_discovery(w);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        });
    }
    fn sync_discovery(&self, w: &mut Workspace) -> Result<(), Error> {
        if let Some(service) = self.discovery.lock().unwrap().as_ref() {
            if let Some(scan) = service.take() {
                w.reconcile_discovery(scan)?;
            }
            let status = service.status();
            w.discovery_status(status.root, status.scanning, status.degraded);
        }
        Ok(())
    }
    fn failure(code: &str) -> Error {
        Error {
            scope: "workspace",
            code: code.into(),
            project_id: None,
        }
    }
    fn with<T>(&self, f: impl FnOnce(&mut Workspace) -> Result<T, Error>) -> Result<T, Error> {
        let mut lock = self
            .workspace
            .lock()
            .map_err(|_| Self::failure("storage_unavailable"))?;
        if lock.is_none() {
            *lock = Some(Workspace::open(&self.root, secrets(&self.root))?);
        }
        let w = lock.as_mut().unwrap();
        self.sync_discovery(w)?;
        w.progress = self.progress.clone();
        let result = f(w);
        if let Ok(snapshot) = w.snapshot() {
            *self.cache.lock().unwrap() = snapshot;
        }
        result
    }
    fn view(&self) -> Result<View, Error> {
        let mut available = false;
        if let Ok(mut lock) = self.workspace.try_lock() {
            available = true;
            if lock.is_none() {
                *lock = Some(Workspace::open(&self.root, secrets(&self.root))?);
            }
            self.sync_discovery(lock.as_mut().unwrap())?;
            let value = lock.as_ref().unwrap().snapshot()?;
            *self.cache.lock().unwrap() = value;
        }
        Ok(View {
            host_pid: std::process::id(),
            snapshot: self.cache.lock().unwrap().clone(),
            busy: !available
                || self.cancel.lock().unwrap().is_some()
                || self.quitting.load(Ordering::Relaxed),
            operation: self.progress.lock().unwrap().clone(),
        })
    }
}
#[tauri::command]
pub fn discovery_scan(state: tauri::State<'_, State>) -> Result<(), Error> {
    if let Some(service) = state.discovery.lock().unwrap().as_ref() {
        service.request();
    }
    Ok(())
}
pub fn secrets(root: &std::path::Path) -> Arc<dyn devforge_core::SecretStore + Send + Sync> {
    #[cfg(feature = "e2e")]
    {
        Arc::new(devforge_workspace::testing::FixtureSecrets::open(
            root.join("e2e-secrets.json"),
        ))
    }
    #[cfg(not(feature = "e2e"))]
    {
        let _ = root;
        Arc::new(devforge_platform::NativeSecretStore::new(format!(
            "{}.projects",
            devforge_platform::APP_IDENTIFIER
        )))
    }
}
#[derive(serde::Serialize)]
pub struct View {
    pub host_pid: u32,
    pub snapshot: Snapshot,
    pub busy: bool,
    pub operation: Option<devforge_workspace::setup::Progress>,
}
#[tauri::command]
pub async fn workspace_snapshot(state: tauri::State<'_, State>) -> Result<View, Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.view())
        .await
        .map_err(|_| State::failure("storage_unavailable"))?
}
#[tauri::command]
pub async fn inspector_snapshot(
    state: tauri::State<'_, State>,
    id: String,
    context: Option<String>,
) -> Result<serde_json::Value, Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.with(|w| w.inspector_context(&id, context.as_deref()))
    })
    .await
    .map_err(|_| State::failure("inspection_unavailable"))?
}
#[tauri::command]
pub async fn workspace_command(
    state: tauri::State<'_, State>,
    command: Command,
) -> Result<serde_json::Value, Error> {
    let state = state.inner().clone();
    let cancel = Cancellation::default();
    {
        let mut slot = state.cancel.lock().unwrap();
        if slot.is_some() || state.quitting.load(Ordering::Relaxed) {
            return Err(State::failure("busy"));
        }
        *slot = Some(cancel.clone());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let result = state.with(|w| w.execute(command, &cancel));
        if let Some(service) = state.discovery.lock().unwrap().as_ref() {
            service.request();
        }
        state.cancel.lock().unwrap().take();
        result
    })
    .await
    .map_err(|_| State::failure("storage_unavailable"))?
}
#[tauri::command]
pub fn workspace_cancel(state: tauri::State<'_, State>) {
    if let Some(cancel) = state.cancel.lock().unwrap().as_ref() {
        cancel.cancel();
    }
}
#[tauri::command]
pub async fn open_project(
    state: tauri::State<'_, State>,
    id: String,
    target: String,
) -> Result<(), Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.with(|w| w.open_target(&id, &target)))
        .await
        .map_err(|_| State::failure("open_failed"))?
}
#[tauri::command]
pub async fn choose_project_directory(
    app: tauri::AppHandle,
    preferences: tauri::State<'_, crate::AppState>,
) -> Result<Option<PathBuf>, Error> {
    use tauri_plugin_dialog::DialogExt;
    let preferences = preferences.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = preferences
            .snapshot(None)
            .map_err(|_| State::failure("storage_unavailable"))?;
        let bundle: serde_json::Value =
            serde_json::from_str(crate::BUNDLE).expect("validated bundle");
        let fallback = bundle["registry"]["fallback"].as_str().unwrap();
        let title = bundle["catalogs"][snapshot.resolved_locale]["work"]["path"]
            .as_str()
            .or_else(|| bundle["catalogs"][fallback]["work"]["path"].as_str())
            .unwrap();
        let mut dialog = app.dialog().file().set_title(title);
        if let Some(folder) = snapshot
            .settings
            .project_folder
            .as_ref()
            .filter(|p| std::path::Path::new(p).is_dir())
        {
            dialog = dialog.set_directory(folder);
        }
        dialog
            .blocking_pick_folder()
            .map(|p| p.into_path().map_err(|_| State::failure("open_failed")))
            .transpose()
    })
    .await
    .map_err(|_| State::failure("open_failed"))?
}
#[tauri::command]
pub async fn choose_backup_file(
    app: tauri::AppHandle,
    preferences: tauri::State<'_, crate::AppState>,
    save: bool,
) -> Result<Option<PathBuf>, Error> {
    use tauri_plugin_dialog::DialogExt;
    let preferences = preferences.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = preferences
            .snapshot(None)
            .map_err(|_| State::failure("storage_unavailable"))?;
        let bundle: serde_json::Value = serde_json::from_str(crate::BUNDLE).unwrap();
        let key = if save { "export" } else { "import" };
        let title = bundle["catalogs"][snapshot.resolved_locale]["services"][key]
            .as_str()
            .unwrap_or("DevForge");
        let dialog = app
            .dialog()
            .file()
            .set_title(title)
            .add_filter("DevForge", &["dfbackup"]);
        let selected = if save {
            dialog.set_file_name("backup.dfbackup").blocking_save_file()
        } else {
            dialog.blocking_pick_file()
        };
        selected
            .map(|p| p.into_path().map_err(|_| State::failure("open_failed")))
            .transpose()
    })
    .await
    .map_err(|_| State::failure("open_failed"))?
}
pub fn quit(app: &tauri::AppHandle) {
    let state = app.state::<State>().inner().clone();
    if state.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(cancel) = state.cancel.lock().unwrap().as_ref() {
        cancel.cancel();
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let worker = state.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            worker.discovery.lock().unwrap().take();
            worker
                .workspace
                .lock()
                .map_err(|_| State::failure("shutdown_failed"))
                .and_then(|mut w| w.as_mut().map(|w| w.shutdown()).unwrap_or(Ok(())))
        })
        .await;
        if matches!(result, Ok(Ok(()))) {
            state.exited.store(true, Ordering::SeqCst);
            app.exit(0);
        } else {
            state.quitting.store(false, Ordering::SeqCst);
            use tauri::Emitter;
            let _ = app.emit("workspace-exit-error", "shutdown_failed");
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
        }
    });
}

#[tauri::command]
pub fn quit_application(app: tauri::AppHandle) {
    quit(&app);
}
#[tauri::command]
pub fn main_window_action(app: tauri::AppHandle, action: String) -> Result<bool, Error> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| State::failure("open_failed"))?;
    match action.as_str() {
        "show" => {
            window.show().map_err(|_| State::failure("open_failed"))?;
            window
                .set_focus()
                .map_err(|_| State::failure("open_failed"))?;
        }
        "close" => window.close().map_err(|_| State::failure("open_failed"))?,
        "status" => {}
        _ => return Err(State::failure("open_failed")),
    };
    window
        .is_visible()
        .map_err(|_| State::failure("open_failed"))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityRequest {
    id: String,
    kind: String,
    query: String,
    page: usize,
    message: Option<String>,
    index: Option<usize>,
    filter: Option<devforge_workspace::LogFilter>,
}
#[tauri::command]
pub async fn activity_read(
    state: tauri::State<'_, State>,
    request: ActivityRequest,
) -> Result<serde_json::Value, Error> {
    let ActivityRequest {
        id,
        kind,
        query,
        page,
        message,
        index,
        filter,
    } = request;
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.with(|w| match kind.as_str() {
            "inbox" => w.mail_view(&id, &query, page),
            "message" => w.mail_message(
                &id,
                message
                    .as_deref()
                    .ok_or_else(|| State::failure("mail_unavailable"))?,
            ),
            "part" => w.mail_part(
                &id,
                message
                    .as_deref()
                    .ok_or_else(|| State::failure("mail_unavailable"))?,
                index.ok_or_else(|| State::failure("mail_unavailable"))?,
            ),
            "logs" => w.logs_view(&id, &filter.unwrap_or_default()),
            _ => Err(State::failure("mail_unavailable")),
        })
    })
    .await
    .map_err(|_| State::failure("mail_unavailable"))?
}
#[tauri::command]
pub async fn choose_mail_file(
    app: tauri::AppHandle,
    preferences: tauri::State<'_, crate::AppState>,
    name: String,
) -> Result<Option<PathBuf>, Error> {
    use tauri_plugin_dialog::DialogExt;
    let preferences = preferences.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = preferences
            .snapshot(None)
            .map_err(|_| State::failure("storage_unavailable"))?;
        let bundle: serde_json::Value = serde_json::from_str(crate::BUNDLE).unwrap();
        let title = bundle["catalogs"][snapshot.resolved_locale]["activity"]["saveAttachment"]
            .as_str()
            .unwrap_or("DevForge");
        let name = name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("attachment")
            .chars()
            .filter(|c| !c.is_control() && !"<>:\"|?*".contains(*c))
            .take(120)
            .collect::<String>();
        let name = if name.trim_matches(['.', ' ']).is_empty() {
            "attachment.bin"
        } else {
            name.trim_matches(['.', ' '])
        };
        app.dialog()
            .file()
            .set_title(title)
            .set_file_name(name)
            .blocking_save_file()
            .map(|p| {
                p.into_path()
                    .map_err(|_| State::failure("mail_export_failed"))
            })
            .transpose()
    })
    .await
    .map_err(|_| State::failure("mail_export_failed"))?
}
