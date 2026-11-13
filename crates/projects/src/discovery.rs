//! Bounded, read-only discovery. No package execution, credentials or environment reads.
use crate::project::inspect;
use devforge_runtime::Cancellation;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub const ENTRY_LIMIT: usize = 10_000;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub identity: String,
    pub path: PathBuf,
    pub name: String,
    pub problem: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scan {
    pub root: PathBuf,
    pub candidates: Vec<Candidate>,
    pub complete: bool,
    pub problem: Option<String>,
    pub skipped: usize,
    #[serde(skip)]
    pub watch_paths: Vec<PathBuf>,
}

/// File identity survives a rename. Path fallback is deliberately marked unstable.
pub fn identity(path: &Path) -> std::io::Result<String> {
    let meta = fs::metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let created = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Ok(format!("file:{}:{}:{created}", meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        use std::{
            mem::MaybeUninit,
            os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        };
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle,
        };
        let _ = meta;
        let file = fs::OpenOptions::new()
            .access_mode(0)
            .share_mode(7)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } != 0 {
            let info = unsafe { info.assume_init() };
            Ok(format!(
                "file:{}:{}:{}:{}:{}",
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
                info.ftCreationTime.dwHighDateTime,
                info.ftCreationTime.dwLowDateTime
            ))
        } else {
            Ok(format!(
                "path:{}",
                fs::canonicalize(path)?.to_string_lossy().to_lowercase()
            ))
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = meta;
        Ok(format!("path:{}", fs::canonicalize(path)?.display()))
    }
}

pub fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

pub fn scan(root: &Path, cancel: &Cancellation) -> Scan {
    scan_with_limit(root, cancel, ENTRY_LIMIT)
}
fn scan_with_limit(root: &Path, cancel: &Cancellation, limit: usize) -> Scan {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut result = Scan {
        root: root.clone(),
        complete: true,
        ..Default::default()
    };
    if cancel.is_cancelled() {
        result.complete = false;
        result.problem = Some("cancelled".into());
        return result;
    }
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(_) => {
            result.complete = false;
            result.problem = Some("unavailable".into());
            return result;
        }
    };
    for (count, entry) in entries.enumerate() {
        if cancel.is_cancelled() {
            result.complete = false;
            result.problem = Some("cancelled".into());
            break;
        }
        if count >= limit {
            result.complete = false;
            result.problem = Some("limit".into());
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                result.complete = false;
                result.problem = Some("partial".into());
                continue;
            }
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.')
            || ["vendor", "node_modules", "target"].contains(&name.to_ascii_lowercase().as_str())
        {
            result.skipped += 1;
            continue;
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                result.complete = false;
                result.problem = Some("partial".into());
                continue;
            }
        };
        if is_link(&meta) {
            result.skipped += 1;
            continue;
        }
        if !meta.is_dir() {
            continue;
        }
        if path.join(".devforge-project-store").exists()
            || path.join("preferences.sqlite3").exists()
        {
            result.skipped += 1;
            continue;
        }
        if fs::read_dir(&path).is_err() {
            result.complete = false;
            result.problem = Some("partial".into());
            continue;
        }
        result.watch_paths.push(path.clone());
        if path.join(".devforge-setup-reservation").exists() {
            result.skipped += 1;
            continue;
        }
        // Do not surface arbitrary non-PHP folders as broken applications.
        if !path.join("composer.json").exists() && !path.join("artisan").exists() {
            continue;
        }
        let identity = match identity(&path) {
            Ok(i) => i,
            Err(_) => {
                result.complete = false;
                result.problem = Some("partial".into());
                continue;
            }
        };
        let problem = if !path.join("artisan").is_file() {
            Some("invalid_project".into())
        } else {
            inspect(&path).err().map(|e| e.to_string())
        };
        result.candidates.push(Candidate {
            identity,
            path,
            name,
            problem,
        });
    }
    result.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn laravel(path: &Path) {
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("artisan"), "<?php // must never execute").unwrap();
        fs::write(path.join("composer.json"),r#"{"require":{"laravel/framework":"^13.0","php":"^8.4"},"scripts":{"post-install-cmd":"false"}}"#).unwrap();
        fs::write(path.join("public/index.php"), "<?php").unwrap();
    }
    #[test]
    fn shallow_incomplete_invalid_and_no_side_effects() {
        let root = tempfile::tempdir().unwrap();
        laravel(&root.path().join("good"));
        laravel(&root.path().join("client/nested"));
        laravel(&root.path().join(".stage"));
        laravel(&root.path().join("vendor"));
        fs::create_dir(root.path().join("incomplete")).unwrap();
        fs::write(root.path().join("incomplete/artisan"), "sentinel").unwrap();
        let before = fs::read(root.path().join("good/composer.json")).unwrap();
        let s = scan(root.path(), &Cancellation::default());
        assert!(s.complete);
        assert_eq!(s.candidates.len(), 2);
        assert!(
            s.candidates
                .iter()
                .find(|c| c.name == "good")
                .unwrap()
                .problem
                .is_none()
        );
        assert!(
            s.candidates
                .iter()
                .find(|c| c.name == "incomplete")
                .unwrap()
                .problem
                .is_some()
        );
        assert_eq!(
            fs::read(root.path().join("good/composer.json")).unwrap(),
            before
        );
        assert!(!root.path().join("good/.env").exists());
        laravel(&root.path().join("incomplete"));
        assert!(
            scan(root.path(), &Cancellation::default())
                .candidates
                .iter()
                .all(|c| c.problem.is_none())
        );
    }
    #[test]
    fn cancellation_cap_missing_and_identity() {
        let root = tempfile::tempdir().unwrap();
        laravel(&root.path().join("a"));
        laravel(&root.path().join("b"));
        let id = identity(&root.path().join("a")).unwrap();
        fs::rename(root.path().join("a"), root.path().join("renamed")).unwrap();
        assert_eq!(identity(&root.path().join("renamed")).unwrap(), id);
        assert!(!scan_with_limit(root.path(), &Cancellation::default(), 1).complete);
        let cancel = Cancellation::default();
        cancel.cancel();
        assert_eq!(
            scan(root.path(), &cancel).problem.as_deref(),
            Some("cancelled")
        );
        assert_eq!(
            scan(&root.path().join("absent"), &Cancellation::default())
                .problem
                .as_deref(),
            Some("unavailable")
        );
    }
    #[cfg(unix)]
    #[test]
    fn symlink_child_is_not_followed() {
        let root = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        laravel(external.path());
        std::os::unix::fs::symlink(external.path(), root.path().join("link")).unwrap();
        let s = scan(root.path(), &Cancellation::default());
        assert!(s.candidates.is_empty());
        assert_eq!(s.skipped, 1);
    }
    #[test]
    fn entry_limit_and_cancelled_empty_root_are_explicit() {
        let root = tempfile::tempdir().unwrap();
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(!scan(root.path(), &cancel).complete);
        for n in 0..=ENTRY_LIMIT {
            fs::write(root.path().join(format!("file-{n}")), "").unwrap();
        }
        let result = scan(root.path(), &Cancellation::default());
        assert!(!result.complete);
        assert_eq!(result.problem.as_deref(), Some("limit"));
    }
    #[cfg(unix)]
    #[test]
    fn unreadable_child_does_not_invent_a_complete_removal_scan() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        laravel(&root.path().join("locked"));
        let path = root.path().join("locked");
        fs::set_permissions(&path, fs::Permissions::from_mode(0)).unwrap();
        let result = scan(root.path(), &Cancellation::default());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(!result.complete);
        assert_eq!(result.problem.as_deref(), Some("partial"));
    }
}

/// Persistent association and suppression, independent from selected runtimes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub identity: String,
    pub root: PathBuf,
    pub path: PathBuf,
    pub name: String,
    pub project_id: Option<String>,
    pub state: String,
    pub problem: Option<String>,
    pub ignored: bool,
}
impl crate::project::ProjectStore {
    pub fn discovery_entries(&self) -> crate::Result<Vec<Entry>> {
        let mut q = self
            .connection
            .prepare("SELECT record FROM discovery_entries ORDER BY identity")?;
        q.query_map([], |r| r.get::<_, String>(0))?
            .map(|row| {
                serde_json::from_str(&row?).map_err(|_| crate::ProjectError::StorageUnavailable)
            })
            .collect()
    }
    pub fn save_discovery(&self, entry: &Entry) -> crate::Result<()> {
        self.connection.execute("INSERT INTO discovery_entries(identity,record) VALUES(?1,?2) ON CONFLICT(identity) DO UPDATE SET record=excluded.record WHERE record<>excluded.record",rusqlite::params![entry.identity,serde_json::to_string(entry).unwrap()])?;
        Ok(())
    }
    pub fn suppress_discovery(&self, id: &str) -> crate::Result<()> {
        for mut e in self
            .discovery_entries()?
            .into_iter()
            .filter(|e| e.project_id.as_deref() == Some(id))
        {
            e.ignored = true;
            e.state = "ignored".into();
            self.save_discovery(&e)?;
        }
        Ok(())
    }
    pub fn restore_discovery(&self, identity: &str) -> crate::Result<()> {
        let mut e = self
            .discovery_entries()?
            .into_iter()
            .find(|e| e.identity == identity)
            .ok_or(crate::ProjectError::NotFound)?;
        e.ignored = false;
        e.project_id = None;
        e.state = "detected".into();
        self.save_discovery(&e)
    }
    pub fn clear_discovery_suppression(&self, path: &Path) -> crate::Result<()> {
        if let Ok(id) = identity(path) {
            for mut e in self
                .discovery_entries()?
                .into_iter()
                .filter(|e| e.identity == id)
            {
                e.ignored = false;
                self.save_discovery(&e)?;
            }
        }
        Ok(())
    }
}
