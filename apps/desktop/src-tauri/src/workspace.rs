use crate::native_bridge::{BridgeError, NativeBridge, StoreCredentialProvider};
use devforge_control::{Id, OperationState};
use devforge_manager::{
    desktop::{AccessRequest, OpenTarget, ReadRequest},
    native_client::{FailureDetail, FailureScope, NativeClient, ReadProgress, Receipt},
};
use devforge_workspace::{Command, Error};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Manager;

#[derive(Clone)]
pub struct State {
    root: PathBuf,
    bridge: NativeBridge,
    cache: Arc<Mutex<Option<View>>>,
    busy: Arc<AtomicBool>,
    connections: Arc<AtomicUsize>,
    pending_discovery: Arc<Mutex<Option<PathBuf>>>,
    pub quitting: Arc<AtomicBool>,
    pub exited: Arc<AtomicBool>,
}
#[derive(Clone, serde::Serialize)]
pub struct View {
    pub host_pid: u32,
    pub snapshot: serde_json::Value,
    pub busy: bool,
    pub operation: Option<serde_json::Value>,
}
impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View")
            .field("host_pid", &self.host_pid)
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}
struct ConnectionGuard(Arc<AtomicUsize>);
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl State {
    pub(crate) fn agent_bridge(&self) -> NativeBridge {
        self.bridge.clone()
    }
    pub(crate) fn agent_snapshot(&self) -> Result<View, Error> {
        self.view()
    }
    pub(crate) fn agent_connect(&self) -> Result<(), Error> {
        self.connect_explicit()
    }
    pub fn new(root: PathBuf) -> Result<Self, Error> {
        #[cfg(feature = "e2e")]
        let provider = {
            let fixture = crate::fixture_root::from_environment()
                .map_err(|_| Self::failure("storage_unavailable"))?;
            Arc::new(StoreCredentialProvider::new(Arc::new(
                devforge_workspace::testing::FixtureSecrets::open(fixture.control_store_path()),
            )))
        };
        #[cfg(not(feature = "e2e"))]
        let provider = Arc::new(StoreCredentialProvider::native());
        let bridge =
            NativeBridge::from_native_configuration(provider).map_err(Self::bridge_error)?;
        Ok(Self {
            root,
            bridge,
            cache: Arc::new(Mutex::new(None)),
            busy: Arc::new(AtomicBool::new(false)),
            connections: Arc::new(AtomicUsize::new(0)),
            pending_discovery: Arc::new(Mutex::new(None)),
            quitting: Arc::new(AtomicBool::new(false)),
            exited: Arc::new(AtomicBool::new(false)),
        })
    }
    fn failure(code: &str) -> Error {
        Error {
            scope: "workspace",
            code: code.into(),
            project_id: None,
        }
    }
    fn bridge_error(error: BridgeError) -> Error {
        Self::failure(error.code())
    }
    fn id(value: &str) -> Result<Id, Error> {
        Id::new(value).map_err(|_| Self::failure("unexpected"))
    }
    fn retry<T>(
        &self,
        mut call: impl FnMut() -> crate::native_bridge::Result<T>,
    ) -> Result<T, Error> {
        let start = Instant::now();
        loop {
            match call() {
                Err(BridgeError::Busy) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                other => return other.map_err(Self::bridge_error),
            }
        }
    }
    pub fn set_discovery_root(&self, root: PathBuf) {
        if let Ok(mut pending) = self.pending_discovery.lock() {
            *pending = Some(root);
        }
    }
    pub fn request_discovery(&self) {
        let state = self.clone();
        std::thread::spawn(move || {
            let _ = state.bridge.discovery_request();
        });
    }
    fn connection_attempt(&self) -> Result<ConnectionGuard, Error> {
        if self.quitting.load(Ordering::Acquire) {
            return Err(Self::failure("shutdown_failed"));
        }
        self.connections.fetch_add(1, Ordering::AcqRel);
        if self.quitting.load(Ordering::Acquire) {
            self.connections.fetch_sub(1, Ordering::AcqRel);
            return Err(Self::failure("shutdown_failed"));
        }
        Ok(ConnectionGuard(self.connections.clone()))
    }
    pub fn start_discovery_coordinator(&self) {
        let Ok(connection) = self.connection_attempt() else {
            return;
        };
        let state = self.clone();
        std::thread::spawn(move || {
            if !state.quitting.load(Ordering::Acquire) {
                let _ = state.bridge.connect_cold_user_launch();
                let _ = state.apply_discovery();
            }
            drop(connection);
            while !state.quitting.load(Ordering::Acquire) {
                // Separate native lifecycle maintenance, never a credential-provider fallback.
                let _ = state.bridge.maintain_connected_context();
                let _ = state.apply_discovery();
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    }
    fn connect_explicit(&self) -> Result<(), Error> {
        let _connection = self.connection_attempt()?;
        match self.bridge.connect_user_action() {
            Ok(()) => {}
            Err(BridgeError::OwnerUnavailable) => self
                .bridge
                .connect_cold_user_launch()
                .map_err(Self::bridge_error)?,
            Err(error) => return Err(Self::bridge_error(error)),
        }
        if self
            .bridge
            .operation_view()
            .is_some_and(|operation| operation.unconfirmed)
        {
            return Err(Self::failure("native_client_admission_unconfirmed"));
        }
        self.apply_discovery()
    }
    fn apply_discovery(&self) -> Result<(), Error> {
        let path = self
            .pending_discovery
            .lock()
            .map_err(|_| Self::failure("storage_unavailable"))?
            .take();
        if let Some(path) = path {
            let request = AccessRequest::DiscoveryRoot {
                path: path.to_string_lossy().into_owned(),
            };
            let key = NativeClient::new_intent_id().map_err(|_| Self::failure("unexpected"))?;
            let outcome = self
                .retry(|| self.bridge.access_user_action(&key, &request))
                .and_then(|receipt| self.wait_value(&receipt, false).map(|_| ()));
            if let Err(error) = outcome {
                Self::restore_pending(&self.pending_discovery, path);
                return Err(error);
            }
        }
        Ok(())
    }
    fn restore_pending(pending: &Mutex<Option<PathBuf>>, path: PathBuf) {
        if let Ok(mut pending) = pending.lock()
            && pending.is_none()
        {
            *pending = Some(path);
        }
    }
    fn view(&self) -> Result<View, Error> {
        let mut read = self.retry(|| self.bridge.begin_snapshot())?;
        loop {
            match self.retry(|| self.bridge.snapshot_step(&mut read)) {
                Ok(ReadProgress::Complete) => break,
                Ok(_) => {}
                Err(error) => {
                    let _ = self.bridge.release_snapshot(read);
                    return Err(error);
                }
            }
        }
        let snapshot = self
            .bridge
            .finish_snapshot(read)
            .map_err(Self::bridge_error)?;
        if let Some(error) = snapshot.error {
            return Err(Self::failure(&error));
        }
        let view = View {
            host_pid: std::process::id(),
            snapshot: snapshot.snapshot,
            busy: snapshot.busy || self.busy.load(Ordering::Acquire),
            operation: snapshot.progress,
        };
        *self
            .cache
            .lock()
            .map_err(|_| Self::failure("storage_unavailable"))? = Some(view.clone());
        Ok(view)
    }
    fn domain_error(detail: FailureDetail) -> Error {
        let scope = match detail.scope {
            FailureScope::Project => "project",
            FailureScope::Runtime => "runtime",
            FailureScope::Serving => "serving",
            FailureScope::Workspace => "workspace",
            FailureScope::Debugger => "debugger",
        };
        let project = detail.recovery_project_id.or_else(|| {
            if detail.project_id.as_str() == devforge_manager::actor::ROOT_SUBJECT {
                None
            } else {
                Some(detail.project_id)
            }
        });
        Error {
            scope,
            code: detail.code,
            project_id: project.map(|id| id.as_str().to_owned()),
        }
    }
    fn image_value(&self, receipt: &Receipt, pure_read: bool) -> Result<serde_json::Value, Error> {
        let mut read = self.retry(|| {
            if pure_read {
                self.bridge
                    .begin_read_result(&receipt.project_id, &receipt.operation_id)
            } else {
                self.bridge
                    .begin_result_image(&receipt.project_id, &receipt.operation_id)
            }
        })?;
        loop {
            match self.retry(|| self.bridge.read_step(&mut read)) {
                Ok(ReadProgress::Complete) => break,
                Ok(_) => {}
                Err(error) => {
                    let _ = self.bridge.release_read(read);
                    return Err(error);
                }
            }
        }
        self.bridge.finish_read(read).map_err(Self::bridge_error)
    }
    fn wait_value(&self, receipt: &Receipt, pure_read: bool) -> Result<serde_json::Value, Error> {
        loop {
            let current = self.retry(|| {
                self.bridge
                    .status(&receipt.project_id, &receipt.operation_id)
            })?;
            match current.state {
                OperationState::Succeeded => {
                    let value = if pure_read {
                        self.image_value(&current, true)
                    } else {
                        match self
                            .bridge
                            .result(&current.project_id, &current.operation_id)
                        {
                            Ok(value) => Ok(value),
                            Err(BridgeError::RequestCode(code))
                                if code == "actor_result_unavailable" =>
                            {
                                self.image_value(&current, false)
                            }
                            Err(error) => Err(Self::bridge_error(error)),
                        }
                    };
                    if !pure_read {
                        let _ = self.bridge.finish_gui_operation(&current);
                    }
                    return value;
                }
                OperationState::Failed | OperationState::Cancelled => {
                    let error = self
                        .bridge
                        .failure_detail(&current.project_id, &current.operation_id)
                        .map(Self::domain_error)
                        .unwrap_or_else(|_| {
                            Self::failure(if current.state == OperationState::Cancelled {
                                "cancelled"
                            } else {
                                "unexpected"
                            })
                        });
                    if !pure_read {
                        let _ = self.bridge.finish_gui_operation(&current);
                    }
                    return Err(error);
                }
                OperationState::Interrupted | OperationState::Unknown => {
                    if !pure_read {
                        let _ = self.bridge.finish_gui_operation(&current);
                    }
                    return Err(Self::failure("actor_result_unavailable"));
                }
                _ => std::thread::sleep(Duration::from_millis(40)),
            }
        }
    }
    fn read(&self, request: &ReadRequest) -> Result<serde_json::Value, Error> {
        let key = NativeClient::new_intent_id().map_err(|_| Self::failure("unexpected"))?;
        let receipt = self.retry(|| self.bridge.begin_read(&key, request))?;
        self.wait_value(&receipt, true)
    }
    fn execute(&self, command: &Command) -> Result<serde_json::Value, Error> {
        if self.quitting.load(Ordering::Acquire) {
            return Err(Self::failure("shutdown_failed"));
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            return Err(Self::failure("busy"));
        }
        struct Reset(Arc<AtomicBool>);
        impl Drop for Reset {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(self.busy.clone());
        let intent = self.retry(|| self.bridge.new_gui_intent(command))?;
        match self.retry(|| self.bridge.admit_gui_command(&intent, command)) {
            Ok(receipt) => self.wait_value(&receipt, false),
            Err(error) => {
                let _ = self.bridge.discard_rejected_gui_intent(&intent);
                Err(error)
            }
        }
    }
    fn shutdown_owner(&self) -> Result<(), Error> {
        let starting = Instant::now();
        while self.connections.load(Ordering::Acquire) > 0 {
            if starting.elapsed() > Duration::from_secs(35) {
                return Err(Self::failure("shutdown_failed"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.bridge.cancel_gui_operation();
        if self.bridge.quit_owner_user_action().is_err() {
            // An unprepared GUI that never connected has no managed owner to
            // stop. Keep the no-owner lease proof until this return; a known
            // connection never turns missing cleanup evidence into success.
            if self
                .cache
                .lock()
                .map_err(|_| Self::failure("shutdown_failed"))?
                .is_none()
                && devforge_platform::distribution::AppLease::acquire(&self.root).is_ok()
            {
                return Ok(());
            }
            self.bridge
                .connect_user_action()
                .map_err(Self::bridge_error)?;
            self.bridge
                .quit_owner_user_action()
                .map_err(Self::bridge_error)?;
        }
        let started = Instant::now();
        loop {
            match self.bridge.confirm_owner_shutdown() {
                Ok(()) => return Ok(()),
                Err(BridgeError::Busy) => {}
                Err(BridgeError::RequestCode(code)) if code == "controller_busy" => {}
                Err(_) => return Err(Self::failure("shutdown_failed")),
            }
            if started.elapsed() >= Duration::from_secs(30) {
                return Err(Self::failure("shutdown_failed"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn access(&self, request: &AccessRequest) -> Result<serde_json::Value, Error> {
        let intent = self.retry(|| self.bridge.new_gui_access_intent(request))?;
        match self.retry(|| self.bridge.admit_gui_access(&intent, request)) {
            Ok(receipt) => self.wait_value(&receipt, false),
            Err(error) => {
                let _ = self.bridge.discard_rejected_gui_intent(&intent);
                Err(error)
            }
        }
    }
}
pub fn secrets(root: &std::path::Path) -> Arc<dyn devforge_core::SecretStore + Send + Sync> {
    #[cfg(feature = "e2e")]
    {
        let fixture = devforge_workspace::testing::FixtureRoot::validate(root)
            .expect("validated isolated fixture root");
        Arc::new(devforge_workspace::testing::FixtureSecrets::open(
            fixture.project_store_path(),
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
#[tauri::command]
pub async fn connect_manager(state: tauri::State<'_, State>) -> Result<(), Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.connect_explicit())
        .await
        .map_err(|_| State::failure("unexpected"))?
}
#[tauri::command]
pub async fn reconcile_manager_operation(
    state: tauri::State<'_, State>,
) -> Result<serde_json::Value, Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let receipt = state
            .bridge
            .reconcile_gui_operation()
            .map_err(State::bridge_error)?
            .ok_or_else(|| State::failure("native_client_admission_unconfirmed"))?;
        state.wait_value(&receipt, false)
    })
    .await
    .map_err(|_| State::failure("unexpected"))?
}
#[tauri::command]
pub async fn discovery_scan(state: tauri::State<'_, State>) -> Result<(), Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .bridge
            .discovery_request()
            .map_err(State::bridge_error)
    })
    .await
    .map_err(|_| State::failure("unexpected"))?
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
        state.read(&ReadRequest::Inspector {
            project_id: State::id(&id)?,
            context: context.as_deref().map(State::id).transpose()?,
        })
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
    tauri::async_runtime::spawn_blocking(move || state.execute(&command))
        .await
        .map_err(|_| State::failure("unexpected"))?
}
#[tauri::command]
pub async fn workspace_cancel(state: tauri::State<'_, State>) -> Result<(), Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .bridge
            .cancel_gui_operation()
            .map_err(State::bridge_error)
    })
    .await
    .map_err(|_| State::failure("unexpected"))?
}
#[tauri::command]
pub async fn open_project(
    state: tauri::State<'_, State>,
    id: String,
    target: String,
) -> Result<(), Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let target = match target.as_str() {
            "browser" => OpenTarget::Browser,
            "editor" => OpenTarget::Editor,
            "folder" => OpenTarget::Folder,
            "terminal" => OpenTarget::Terminal,
            _ => return Err(State::failure("open_failed")),
        };
        state
            .access(&AccessRequest::OpenTarget {
                project_id: State::id(&id)?,
                target,
            })
            .map(|_| ())
    })
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
    if state.quitting.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let worker = state.clone();
        let result = tauri::async_runtime::spawn_blocking(move || worker.shutdown_owner()).await;
        if matches!(result, Ok(Ok(()))) {
            state.exited.store(true, Ordering::SeqCst);
            app.exit(0);
        } else {
            state.quitting.store(false, Ordering::SeqCst);
            use tauri::Emitter;
            let _ = app.emit("workspace-exit-error", "shutdown_failed");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
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
        let project_id = State::id(&id)?;
        let request = match kind.as_str() {
            "inbox" => ReadRequest::Inbox {
                project_id,
                query,
                page: u32::try_from(page).map_err(|_| State::failure("mail_unavailable"))?,
            },
            "message" => ReadRequest::MailMessage {
                project_id,
                message: State::id(
                    message
                        .as_deref()
                        .ok_or_else(|| State::failure("mail_unavailable"))?,
                )?,
            },
            "part" => ReadRequest::MailPart {
                project_id,
                message: State::id(
                    message
                        .as_deref()
                        .ok_or_else(|| State::failure("mail_unavailable"))?,
                )?,
                index: u32::try_from(index.ok_or_else(|| State::failure("mail_unavailable"))?)
                    .map_err(|_| State::failure("mail_unavailable"))?,
            },
            "logs" => ReadRequest::Logs {
                project_id,
                filter: filter.unwrap_or_default(),
            },
            _ => return Err(State::failure("mail_unavailable")),
        };
        state.read(&request)
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

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebugRequest {
    id: String,
    kind: String,
    profile: Option<String>,
    file: Option<String>,
}
#[tauri::command]
pub async fn debug_read(
    state: tauri::State<'_, State>,
    request: DebugRequest,
) -> Result<serde_json::Value, Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let project_id = State::id(&request.id)?;
        let request = match request.kind.as_str() {
            "view" => ReadRequest::DebugView { project_id },
            "profile" => ReadRequest::DebugProfile {
                project_id,
                profile: State::id(
                    request
                        .profile
                        .as_deref()
                        .ok_or_else(|| State::failure("unexpected"))?,
                )?,
            },
            "source" => ReadRequest::DebugSource {
                project_id,
                file: request.file.ok_or_else(|| State::failure("unexpected"))?,
            },
            _ => return Err(State::failure("unexpected")),
        };
        state.read(&request)
    })
    .await
    .map_err(|_| State::failure("unexpected"))?
}
#[tauri::command]
pub async fn choose_debug_profile(app: tauri::AppHandle) -> Result<Option<PathBuf>, Error> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Xdebug Cachegrind")
            .set_file_name("profile.cachegrind")
            .blocking_save_file()
            .map(|p| {
                p.into_path()
                    .map_err(|_| State::failure("storage_unavailable"))
            })
            .transpose()
    })
    .await
    .map_err(|_| State::failure("storage_unavailable"))?
}

#[tauri::command]
pub async fn configuration_read(
    state: tauri::State<'_, State>,
    id: String,
    plan: bool,
    write_env: bool,
) -> Result<serde_json::Value, Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let project_id = State::id(&id)?;
        state.read(&if plan {
            ReadRequest::ConfigurationPlan {
                project_id,
                write_env,
            }
        } else {
            ReadRequest::ConfigurationDocument { project_id }
        })
    })
    .await
    .map_err(|_| State::failure("manifest_invalid"))?
}

#[tauri::command]
pub async fn configuration_secret(
    state: tauri::State<'_, State>,
    id: String,
    reference: String,
    value: String,
) -> Result<(), Error> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let value = devforge_manager::desktop::SecretValue::new(&value)
            .ok_or_else(|| State::failure("manifest_secret_unavailable"))?;
        state
            .access(&AccessRequest::ConfigurationSecret {
                project_id: State::id(&id)?,
                reference,
                value,
            })
            .map(|_| ())
    })
    .await
    .map_err(|_| State::failure("manifest_secret_unavailable"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_bridge::fixture_root;
    use devforge_core::{ErrorCode, SecretStore};
    use devforge_manager::controller::{Controller, StartIntent, Stores};
    use std::collections::BTreeMap;
    #[derive(Default)]
    struct MemoryStore(Mutex<BTreeMap<String, String>>);
    impl SecretStore for MemoryStore {
        fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), ErrorCode> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    #[test]
    fn failed_discovery_update_never_overwrites_a_newer_pending_choice() {
        let pending = Mutex::new(None);
        State::restore_pending(&pending, PathBuf::from("A"));
        assert_eq!(
            pending.lock().unwrap().as_deref(),
            Some(std::path::Path::new("A"))
        );
        *pending.lock().unwrap() = Some(PathBuf::from("B"));
        State::restore_pending(&pending, PathBuf::from("A"));
        assert_eq!(
            pending.lock().unwrap().as_deref(),
            Some(std::path::Path::new("B"))
        );
    }
    #[test]
    fn immediate_quit_settles_initial_owner_publication_before_no_owner_success() {
        let root = fixture_root::root("df-quit-start-");
        let desktop = Arc::new(MemoryStore::default());
        let bridge = NativeBridge::from_fixture(
            root.path().to_owned(),
            Arc::new(StoreCredentialProvider::new(desktop.clone())),
        )
        .unwrap();
        let state = State {
            root: root.path().to_owned(),
            bridge,
            cache: Arc::new(Mutex::new(None)),
            busy: Arc::new(AtomicBool::new(false)),
            connections: Arc::new(AtomicUsize::new(1)),
            pending_discovery: Arc::new(Mutex::new(None)),
            quitting: Arc::new(AtomicBool::new(true)),
            exited: Arc::new(AtomicBool::new(false)),
        };
        let startup = state.clone();
        let path = root.path().to_owned();
        let launcher = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(60));
            let controller = Controller::start(
                &path,
                StartIntent::ExplicitUserStart,
                Stores {
                    workspace: Arc::new(MemoryStore::default()),
                    desktop,
                },
            )
            .unwrap();
            let endpoint = controller.endpoint().clone();
            let worker = std::thread::spawn(move || controller.serve(None));
            let started = Instant::now();
            while NativeClient::observe_owner(&endpoint).is_err() {
                assert!(started.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(5));
            }
            startup.bridge.connect_user_action().unwrap();
            startup.connections.store(0, Ordering::Release);
            worker
        });
        state.shutdown_owner().unwrap();
        launcher.join().unwrap().join().unwrap().unwrap();
        assert_eq!(
            state.connect_explicit().unwrap_err().code,
            "shutdown_failed"
        );
        assert_eq!(state.connections.load(Ordering::Acquire), 0);
        assert!(devforge_platform::distribution::AppLease::acquire(root.path()).is_ok());
    }
    #[test]
    fn thin_consumer_preserves_actual_results_large_reads_errors_host_pid_and_quit_cleanup() {
        let root = fixture_root::root("df-consumer-");
        let desktop = Arc::new(MemoryStore::default());
        let projects = Arc::new(MemoryStore::default());
        let controller = Controller::start(
            root.path(),
            StartIntent::ExplicitUserStart,
            Stores {
                workspace: projects,
                desktop: desktop.clone(),
            },
        )
        .unwrap();
        let endpoint = controller.endpoint().clone();
        let worker = std::thread::spawn(move || controller.serve(None));
        let started = Instant::now();
        while NativeClient::observe_owner(&endpoint).is_err() {
            assert!(started.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        let bridge = NativeBridge::from_fixture(
            root.path().to_owned(),
            Arc::new(StoreCredentialProvider::new(desktop)),
        )
        .unwrap();
        let state = State {
            root: root.path().to_owned(),
            bridge,
            cache: Arc::new(Mutex::new(None)),
            busy: Arc::new(AtomicBool::new(false)),
            connections: Arc::new(AtomicUsize::new(0)),
            pending_discovery: Arc::new(Mutex::new(None)),
            quitting: Arc::new(AtomicBool::new(false)),
            exited: Arc::new(AtomicBool::new(false)),
        };
        assert_eq!(
            state.view().unwrap_err().code,
            "native_bridge_authentication_required"
        );
        state.connect_explicit().unwrap();
        let app = root.path().join("app");
        std::fs::create_dir_all(app.join("public")).unwrap();
        std::fs::write(app.join("public/index.php"), "<?php").unwrap();
        std::fs::write(
            app.join("composer.json"),
            r#"{"require":{"laravel/framework":"^13.0","php":">=8.0"}}"#,
        )
        .unwrap();
        let catalog = devforge_projects::project::Catalog::bundled();
        let target = devforge_runtime::package::Target::current().unwrap();
        let php = catalog
            .packages
            .iter()
            .find(|(id, p)| {
                p.target == target
                    && p.kind == "php"
                    && p.executables.contains_key("php")
                    && !catalog.superseded.contains_key(*id)
            })
            .unwrap()
            .0
            .clone();
        let node = catalog
            .packages
            .iter()
            .find(|(id, p)| {
                p.target == target && p.kind == "node" && !catalog.superseded.contains_key(*id)
            })
            .unwrap()
            .0
            .clone();
        let result = state
            .execute(&Command::Import {
                path: app.clone(),
                name: "Consumer".into(),
                php,
                node,
                domain: "consumer.devforge.test".into(),
                https: false,
                vite: false,
                queue: false,
            })
            .unwrap();
        assert!(
            result.get("operation_id").is_none(),
            "legacy command must return actual result, not admission receipt"
        );
        let id = result["project_id"].as_str().unwrap().to_owned();
        let view = state.view().unwrap();
        assert_eq!(view.host_pid, std::process::id());
        assert_eq!(view.snapshot["projects"].as_array().unwrap().len(), 1);
        let source = format!("<?php\n{}", "// \"🦀\" \"\\path\"\n".repeat(13_000));
        std::fs::write(app.join("source.php"), &source).unwrap();
        let read = state
            .read(&ReadRequest::DebugSource {
                project_id: State::id(&id).unwrap(),
                file: "source.php".into(),
            })
            .unwrap();
        assert_eq!(read["code"], source);
        let error = state
            .execute(&Command::ConfigurationSave {
                id: id.clone(),
                expected: None,
                text: "invalid fixture".into(),
                consent: true,
            })
            .unwrap_err();
        assert_eq!(error.scope, "project");
        assert_eq!(error.code, "invalid_manifest");
        assert_eq!(error.project_id.as_deref(), Some(id.as_str()));
        state.shutdown_owner().unwrap();
        worker.join().unwrap().unwrap();
        assert!(devforge_platform::distribution::AppLease::acquire(root.path()).is_ok());
    }
}
