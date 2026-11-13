//! Retained function profiles. Per-file reads and export are bounded and refuse links.
use crate::{
    Error, Result,
    profiles::{self, Profile},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub id: String,
    pub created_ms: u64,
    pub bytes: u64,
    pub oversized: bool,
}
fn valid(name: &str) -> bool {
    name.starts_with("cachegrind.")
        && name.ends_with(".out")
        && name.len() <= 180
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn id(name: &str) -> String {
    Sha256::digest(name.as_bytes())
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn list(root: &Path) -> Result<Vec<Item>> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let directory = fs::symlink_metadata(root)?;
    if !directory.is_dir() || directory.file_type().is_symlink() {
        return Err(Error::Unauthorized);
    }
    let mut out = vec![];
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !valid(&name) {
            continue;
        }
        let m = fs::symlink_metadata(entry.path())?;
        if !m.is_file() || m.file_type().is_symlink() {
            return Err(Error::Unauthorized);
        }
        let created = m
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::ProfileInvalid)?
            .as_millis() as u64;
        out.push(Item {
            id: id(&name),
            created_ms: created,
            bytes: m.len(),
            oversized: m.len() > 32 * 1024 * 1024,
        });
        if out.len() > 1000 {
            return Err(Error::ProfileLimit);
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.created_ms));
    Ok(out)
}
fn path(root: &Path, key: &str) -> Result<PathBuf> {
    if key.len() != 32 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::ProfileInvalid);
    }
    for entry in fs::read_dir(root)? {
        let e = entry?;
        let name = e.file_name().to_string_lossy().to_string();
        if valid(&name) && id(&name) == key {
            let m = fs::symlink_metadata(e.path())?;
            if !m.is_file() || m.file_type().is_symlink() {
                return Err(Error::Unauthorized);
            }
            return Ok(e.path());
        }
    }
    Err(Error::Unavailable)
}
pub fn load(root: &Path, key: &str, project: &Path, secrets: &[String]) -> Result<Profile> {
    let p = path(root, key)?;
    let before = fs::metadata(&p)?;
    if before.len() > 32 * 1024 * 1024 {
        return Err(Error::ProfileLimit);
    }
    let mut bytes = vec![];
    fs::File::open(&p)?
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let after = fs::metadata(&p)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(Error::Busy);
    }
    let mut profile = profiles::parse(&bytes)?;
    let root = fs::canonicalize(project)?;
    for f in &mut profile.functions {
        let file = Path::new(&f.file);
        f.file = if file.is_absolute() {
            let normalized = fs::canonicalize(file).unwrap_or_else(|_| file.to_owned());
            normalized
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| {
                    file.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                })
        } else {
            f.file.clone()
        };
        for s in secrets.iter().filter(|s| !s.is_empty()) {
            f.file = f.file.replace(s, "[REDACTED]");
            f.name = f.name.replace(s, "[REDACTED]");
        }
        f.file = f.file.chars().take(512).collect();
        f.name = f.name.chars().take(512).collect();
    }
    Ok(profile)
}
pub fn trim(root: &Path) -> Result<usize> {
    let files = list(root)?;
    let mut bytes = 0;
    let mut removed = 0;
    let now = SystemTime::now();
    for (index, f) in files.iter().enumerate() {
        bytes += f.bytes;
        let path = path(root, &f.id)?;
        if (index >= 20 || bytes > 128 * 1024 * 1024 || f.oversized)
            && now
                .duration_since(fs::metadata(&path)?.modified()?)
                .unwrap_or(Duration::ZERO)
                > Duration::from_secs(2)
        {
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}
pub fn delete(root: &Path, key: &str) -> Result<()> {
    fs::remove_file(path(root, key)?)?;
    Ok(())
}
pub fn export(root: &Path, key: &str, destination: &Path) -> Result<()> {
    let source = path(root, key)?;
    if fs::metadata(&source)?.len() > 32 * 1024 * 1024 {
        return Err(Error::ProfileLimit);
    }
    let parent = destination.parent().ok_or(Error::Unauthorized)?;
    let mut output = tempfile::NamedTempFile::new_in(parent)?;
    let copied = std::io::copy(
        &mut fs::File::open(source)?.take(32 * 1024 * 1024 + 1),
        output.as_file_mut(),
    )?;
    if copied > 32 * 1024 * 1024 {
        return Err(Error::ProfileLimit);
    }
    output.flush()?;
    output.as_file().sync_all()?;
    output
        .persist_noclobber(destination)
        .map_err(|_| Error::Unavailable)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_profiles_refuse_links_and_exports_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cachegrind.1.2.out");
        fs::write(
            &p,
            b"events: Time_(10ns) Memory_(bytes)\nfl=php:internal\nfn=php::strlen\n1 100000 10\n",
        )
        .unwrap();
        let files = list(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        let parsed = load(dir.path(), &files[0].id, dir.path(), &[]).unwrap();
        assert_eq!(parsed.total_ms, 1.0);
        let out = dir.path().join("saved");
        export(dir.path(), &files[0].id, &out).unwrap();
        assert!(export(dir.path(), &files[0].id, &out).is_err());
        assert!(delete(dir.path(), "../outside").is_err());
    }
}
