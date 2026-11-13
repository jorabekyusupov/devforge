//! Snapshot only the conventional project-local SQLite database, never a linked path.
use crate::{ProjectError as E, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub fn location(project: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(project)?;
    let dir = root.join("database");
    if fs::symlink_metadata(&dir)?.file_type().is_symlink() || fs::canonicalize(&dir)? != dir {
        return Err(E::InvalidProject);
    }
    let file = dir.join("database.sqlite");
    if fs::symlink_metadata(&file).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err(E::InvalidProject);
    }
    Ok(file)
}
pub fn conventional_profile(project: &Path) -> Result<bool> {
    let expected = location(project)?;
    let bytes = zeroize::Zeroizing::new(crate::manifest::read_file(
        &project.join(".env"),
        1024 * 1024,
    )?);
    let text = std::str::from_utf8(&bytes).map_err(|_| E::InvalidEnvFile)?;
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some((name, value)) = line.split_once('=') {
            let name = name.trim();
            if ["DB_CONNECTION", "DB_DATABASE", "DB_URL", "DATABASE_URL"].contains(&name)
                && values
                    .insert(name, value.trim().trim_matches(['\'', '"']))
                    .is_some()
            {
                return Ok(false);
            }
        }
    }
    if values.get("DB_CONNECTION") != Some(&"sqlite") {
        return Ok(false);
    }
    for key in ["DB_URL", "DATABASE_URL"] {
        if values
            .get(key)
            .is_some_and(|v| !["", "null", "(null)"].contains(v))
        {
            return Ok(false);
        }
    }
    if let Some(value) = values.get("DB_DATABASE").filter(|v| !v.is_empty()) {
        let path = std::path::Path::new(value);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            project.join(path)
        };
        if fs::canonicalize(path)? != fs::canonicalize(expected)? {
            return Ok(false);
        }
    }
    Ok(true)
}
pub fn snapshot(source: &Path, destination: &Path) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(E::DatabaseConflict);
    }
    let db =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.backup("main", destination, rusqlite::backup::NO_PROGRESS)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conventional_profile_rejects_external_paths_urls_and_links() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        fs::create_dir(root.join("database")).unwrap();
        fs::write(root.join("database/database.sqlite"), b"").unwrap();
        fs::write(root.join(".env"), "DB_CONNECTION=sqlite\n").unwrap();
        assert!(conventional_profile(&root).unwrap());
        fs::write(root.join(".env"), "DB_CONNECTION=pgsql\n").unwrap();
        assert!(!conventional_profile(&root).unwrap());
        fs::write(
            root.join(".env"),
            "DB_CONNECTION=sqlite\nDB_URL=postgres://example.test\n",
        )
        .unwrap();
        assert!(!conventional_profile(&root).unwrap());
        #[cfg(unix)]
        {
            fs::remove_file(root.join("database/database.sqlite")).unwrap();
            std::os::unix::fs::symlink(
                "/tmp/foreign.sqlite",
                root.join("database/database.sqlite"),
            )
            .unwrap();
            assert!(location(&root).is_err());
        }
    }
    #[test]
    fn snapshot_includes_wal_and_refuses_existing_output() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("source");
        let b = dir.path().join("copy");
        let db = rusqlite::Connection::open(&a).unwrap();
        db.execute_batch(
            "PRAGMA journal_mode=WAL;CREATE TABLE test(value);INSERT INTO test VALUES(7)",
        )
        .unwrap();
        snapshot(&a, &b).unwrap();
        let copy = rusqlite::Connection::open(&b).unwrap();
        assert_eq!(
            copy.query_row("SELECT value FROM test", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            7
        );
        assert!(snapshot(&a, &b).is_err());
    }
}
