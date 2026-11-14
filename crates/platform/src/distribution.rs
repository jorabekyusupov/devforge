//! Installer-facing lifecycle and read-only prerequisite checks. No system writes.
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
// Keep in sync with the NSIS floor (scripts/test_distribution.py checks parity).
#[cfg(any(windows, test))]
const MIN_WINDOWS_BUILD: u32 = 19045;
/// An OS-released lock, not a stale PID. The file is intentionally retained.
pub struct AppLease {
    _file: File,
}
impl Drop for AppLease {
    fn drop(&mut self) {
        // Closing only this handle need not release a lock retained by a
        // duplicated/fork-inherited descriptor. End the logical lease now.
        let _ = self._file.unlock();
    }
}
impl AppLease {
    pub fn acquire(root: &Path) -> std::io::Result<Self> {
        let file = lease_file(root, "desktop.lock")?;
        file.try_lock().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => {
                std::io::Error::from(std::io::ErrorKind::WouldBlock)
            }
            std::fs::TryLockError::Error(e) => e,
        })?;
        Ok(Self { _file: file })
    }
    pub fn stopped(root: &Path) -> std::io::Result<bool> {
        if !root.exists() {
            return Ok(true);
        }
        match Self::acquire(root) {
            Ok(_desktop) => {
                let file = lease_file(root, "operations.lock")?;
                match file.try_lock() {
                    Ok(()) => {
                        // Reuse explicit-release semantics for this brief probe.
                        let _probe = OperationLease { _file: file };
                        Ok(true)
                    }
                    Err(std::fs::TryLockError::WouldBlock) => Ok(false),
                    Err(std::fs::TryLockError::Error(e)) => Err(e),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(e) => Err(e),
        }
    }
}
/// Shared by GUI and CLI sessions. Installer checks need an exclusive lock.
/// CLI/terminal activity can coexist with the GUI without being orphaned by removal.
pub struct OperationLease {
    _file: File,
}
fn unsafe_lease() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, "unsafe lifecycle lease")
}
fn lease_file(root: &Path, name: &str) -> std::io::Result<File> {
    std::fs::create_dir_all(root)?;
    let path = root.join(name);
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => return Err(unsafe_lease()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(unsafe_lease());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(unsafe_lease());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            GetFileInformationByHandle,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
            || info.nNumberOfLinks != 1
        {
            return Err(unsafe_lease());
        }
    }
    Ok(file)
}

impl Drop for OperationLease {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}
impl OperationLease {
    /// Reproducible setup owns configuration across GUI/CLI lifetimes.
    pub fn exclusive(root: &Path) -> std::io::Result<Self> {
        let file = lease_file(root, "operations.lock")?;
        file.try_lock()
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::WouldBlock))?;
        Ok(Self { _file: file })
    }

    pub fn acquire(root: &Path) -> std::io::Result<Self> {
        let file = lease_file(root, "operations.lock")?;
        file.try_lock_shared().map_err(|e| match e {
            std::fs::TryLockError::WouldBlock => {
                std::io::Error::from(std::io::ErrorKind::WouldBlock)
            }
            std::fs::TryLockError::Error(e) => e,
        })?;
        Ok(Self { _file: file })
    }
}
#[derive(Serialize)]
pub struct Prerequisites {
    pub target: String,
    pub os_version: String,
    pub os_supported: bool,
    pub webview: &'static str,
    pub vc_runtime: Option<bool>,
    pub vc_runtime_help: Option<&'static str>,
}
pub fn prerequisites() -> std::io::Result<Prerequisites> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()?;
        if !out.status.success() {
            return Err(std::io::Error::other("os_version_unavailable"));
        }
        let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(Prerequisites {
            target: format!("macos-{}", std::env::consts::ARCH),
            os_supported: mac_supported(&version),
            os_version: version,
            webview: "system_wkwebview",
            vc_runtime: None,
            vc_runtime_help: None,
        })
    }
    #[cfg(windows)]
    {
        let build = registry(
            "HKLM\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
            "CurrentBuildNumber",
        )?;
        let build = build.split_whitespace().last().unwrap_or("").to_string();
        // The compiled x64 binary may run under ARM emulation. Match the
        // installer's native AMD64 requirement rather than the process target.
        let native_arch = registry(
            "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment",
            "PROCESSOR_ARCHITECTURE",
        )?;
        let native_arch = native_arch.split_whitespace().last().unwrap_or("");
        let vc = registry(
            "HKLM\\SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64",
            "Installed",
        )?;
        Ok(Prerequisites {
            target: format!("windows-{}", std::env::consts::ARCH),
            os_supported: windows_supported(&build, native_arch),
            os_version: build,
            webview: "installer_detects_evergreen_webview2",
            vc_runtime: Some(vc.split_whitespace().last() == Some("0x1")),
            vc_runtime_help: Some(
                "https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist",
            ),
        })
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Err(std::io::Error::other("unsupported_target"))
    }
}
#[cfg(any(windows, test))]
fn windows_supported(build: &str, native_arch: &str) -> bool {
    native_arch == "AMD64" && build.parse::<u32>().is_ok_and(|b| b >= MIN_WINDOWS_BUILD)
}
#[cfg(windows)]
fn registry(key: &str, value: &str) -> std::io::Result<String> {
    let root = std::env::var_os("SystemRoot")
        .ok_or_else(|| std::io::Error::other("system_root_unavailable"))?;
    let out = std::process::Command::new(std::path::PathBuf::from(root).join("System32/reg.exe"))
        .args(["query", key, "/v", value, "/reg:64"])
        .output()?;
    // Missing prerequisite is a negative result; no installation is attempted.
    Ok(if out.status.success() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        String::new()
    })
}
#[cfg(any(target_os = "macos", test))]
fn mac_supported(version: &str) -> bool {
    let parts: Vec<_> = version
        .split('.')
        .filter_map(|v| v.parse::<u32>().ok())
        .collect();
    matches!(parts.as_slice(), [major, minor, ..] if (*major,*minor) >= (13,5))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_accepts_22h2_and_later_native_x64_but_fails_closed() {
        for build in ["19045", "22000", "22631", "26100"] {
            assert!(windows_supported(build, "AMD64"), "{build}");
        }
        for build in ["19044", "19041", "17763", "9600", "", "unknown", "19045.1"] {
            assert!(!windows_supported(build, "AMD64"), "{build}");
        }
        for arch in ["ARM64", "x86", "", "unknown"] {
            assert!(!windows_supported("26100", arch), "{arch}");
        }
    }
    #[test]
    fn installer_lock_detects_running_then_releases_without_deleting_data() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("database-sentinel"), b"preserve").unwrap();
        let lease = AppLease::acquire(root.path()).unwrap();
        assert!(!AppLease::stopped(root.path()).unwrap());
        assert!(AppLease::acquire(root.path()).is_err());
        drop(lease);
        assert!(AppLease::stopped(root.path()).unwrap());
        assert_eq!(
            std::fs::read(root.path().join("database-sentinel")).unwrap(),
            b"preserve"
        );
    }
    #[test]
    fn setup_excludes_gui_and_other_cli_sessions() {
        let root = tempfile::tempdir().unwrap();
        let shared = OperationLease::acquire(root.path()).unwrap();
        assert!(OperationLease::exclusive(root.path()).is_err());
        drop(shared);
        let exclusive = OperationLease::exclusive(root.path()).unwrap();
        assert!(OperationLease::acquire(root.path()).is_err());
        drop(exclusive);
        assert!(OperationLease::acquire(root.path()).is_ok());
    }
    #[test]
    fn cli_sessions_coexist_and_block_removal_until_every_session_exits() {
        let root = tempfile::tempdir().unwrap();
        let first = OperationLease::acquire(root.path()).unwrap();
        let second = OperationLease::acquire(root.path()).unwrap();
        assert!(!AppLease::stopped(root.path()).unwrap());
        drop(first);
        assert!(!AppLease::stopped(root.path()).unwrap());
        drop(second);
        assert!(AppLease::stopped(root.path()).unwrap());
    }
    #[cfg(unix)]
    #[test]
    fn lifecycle_leases_unlock_with_duplicate_descriptors_alive() {
        let root = tempfile::tempdir().unwrap();
        let app = AppLease::acquire(root.path()).unwrap();
        let inherited_app = app._file.try_clone().unwrap();
        drop(app);
        assert!(AppLease::acquire(root.path()).is_ok());
        for exclusive in [true, false] {
            let operation = if exclusive {
                OperationLease::exclusive(root.path()).unwrap()
            } else {
                OperationLease::acquire(root.path()).unwrap()
            };
            let inherited = operation._file.try_clone().unwrap();
            drop(operation);
            assert!(OperationLease::exclusive(root.path()).is_ok());
            drop(inherited);
        }
        drop(inherited_app);
    }
    #[test]
    fn minimum_macos_is_explicit() {
        for v in ["13.5", "13.5.1", "14.0", "26.1"] {
            assert!(mac_supported(v));
        }
        for v in ["13.4", "12.9", "broken"] {
            assert!(!mac_supported(v));
        }
    }
}

#[cfg(test)]
mod lease_path_regressions {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn dangling_operation_symlink_never_creates_external_target() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("must-not-create");
        std::os::unix::fs::symlink(&target, root.path().join("operations.lock")).unwrap();
        assert!(OperationLease::acquire(root.path()).is_err());
        assert!(!target.exists());
    }
    #[test]
    fn lock_hardlinks_and_directories_are_refused_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("owned-fixture");
        std::fs::write(&target, b"unchanged").unwrap();
        std::fs::hard_link(&target, root.path().join("operations.lock")).unwrap();
        assert!(OperationLease::acquire(root.path()).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"unchanged");
        std::fs::create_dir(root.path().join("desktop.lock")).unwrap();
        assert!(AppLease::acquire(root.path()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn desktop_symlink_cannot_create_or_lock_external_file() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("must-not-create");
        std::os::unix::fs::symlink(&target, root.path().join("desktop.lock")).unwrap();
        assert!(AppLease::acquire(root.path()).is_err());
        assert!(!target.exists());
    }
}
