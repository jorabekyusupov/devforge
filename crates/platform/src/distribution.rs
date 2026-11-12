//! Installer-facing lifecycle and read-only prerequisite checks. No system writes.
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
/// An OS-released lock, not a stale PID. The file is intentionally retained.
pub struct AppLease {
    _file: File,
}
impl AppLease {
    pub fn acquire(root: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(root)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("desktop.lock"))?;
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
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(e) => Err(e),
        }
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
        let vc = registry(
            "HKLM\\SOFTWARE\\Microsoft\\VisualStudio\\14.0\\VC\\Runtimes\\x64",
            "Installed",
        )?;
        Ok(Prerequisites {
            target: format!("windows-{}", std::env::consts::ARCH),
            os_supported: std::env::consts::ARCH == "x86_64"
                && build.parse::<u32>().is_ok_and(|b| b >= 22000),
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
    fn minimum_macos_is_explicit() {
        for v in ["13.5", "13.5.1", "14.0", "26.1"] {
            assert!(mac_supported(v));
        }
        for v in ["13.4", "12.9", "broken"] {
            assert!(!mac_supported(v));
        }
    }
}
