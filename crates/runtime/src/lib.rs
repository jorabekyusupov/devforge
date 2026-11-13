//! Blocking runtime infrastructure. Call from a worker, never the UI thread.
//! No Tauri, project .env edits, global PATH mutations or arbitrary shell IPC.
mod archive;
pub mod package;
pub mod process;

use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeError {
    InvalidPackage,
    UnsupportedTarget,
    IntegrityMismatch,
    UnsafeArchive,
    LimitExceeded,
    Cancelled,
    DownloadFailed,
    Io,
    DiskFull,
    Busy,
    PackageConflict,
    NotInstalled,
    ExecutableChanged,
    SpawnFailed,
    ReadinessTimeout,
    ProcessExited,
    InvalidProcess,
    LogFailed,
    FileBudget,
    PackageSuperseded,
}
impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Stable machine code; localized presentation belongs at the boundary.
        write!(
            f,
            "{}",
            serde_json::to_value(self).unwrap().as_str().unwrap()
        )
    }
}
impl std::error::Error for RuntimeError {}
impl From<std::io::Error> for RuntimeError {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::StorageFull {
            Self::DiskFull
        } else {
            Self::Io
        }
    }
}
pub type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(RuntimeError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Portable paths are also validated on Unix with Windows rules. A catalog cannot
/// mean a safe path on one target and a drive/ADS/device path on another.
pub(crate) fn relative_path(value: &str) -> Result<std::path::PathBuf> {
    if value.is_empty()
        || value.contains(['\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
        || value.starts_with('/')
    {
        return Err(RuntimeError::UnsafeArchive);
    }
    let mut path = std::path::PathBuf::new();
    for part in value.split('/') {
        if part == "." || part.is_empty() {
            continue;
        }
        let stem = part.split('.').next().unwrap().to_ascii_uppercase();
        if part == ".."
            || part.ends_with(['.', ' '])
            || part.chars().any(|c| c.is_control())
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(RuntimeError::UnsafeArchive);
        }
        path.push(part);
    }
    if path.as_os_str().is_empty() {
        return Err(RuntimeError::UnsafeArchive);
    }
    Ok(path)
}
