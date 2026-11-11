//! Recover only an unchanged hot file whose private owner receipt outlived a crash.
use crate::{Result, ServingError as E, site::SiteStore};
use serde::{Deserialize, Serialize};
use std::{fs, net::TcpListener, path::Path, time::UNIX_EPOCH};
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Stamp {
    length: u64,
    modified: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u32,
    owner: String,
    project: String,
    origin: String,
    port: u16,
    stamp: Stamp,
}
fn stamp(path: &Path) -> Result<Stamp> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(E::HotFileConflict);
    }
    Ok(Stamp {
        length: m.len(),
        modified: m
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| E::HotFileConflict)?
            .as_nanos()
            .to_string(),
    })
}
fn receipt(store: &SiteStore, id: &str) -> std::path::PathBuf {
    store.root().join("hot-owners").join(format!("{id}.json"))
}
fn matches(path: &Path, origin: &str) -> bool {
    fs::read_to_string(path).is_ok_and(|s|matches!((reqwest::Url::parse(s.trim()),reqwest::Url::parse(origin)),(Ok(a),Ok(b)) if a==b))
}
pub(crate) fn record(
    store: &SiteStore,
    id: &str,
    path: &Path,
    origin: &str,
    port: u16,
) -> Result<()> {
    if !matches(path, origin) {
        return Err(E::HotFileConflict);
    }
    let first = stamp(path)?;
    std::thread::sleep(std::time::Duration::from_millis(75));
    let final_stamp = stamp(path)?;
    if first != final_stamp || !matches(path, origin) {
        return Err(E::HotFileConflict);
    }
    crate::atomic(
        &receipt(store, id),
        &serde_json::to_vec(&Receipt {
            schema: 1,
            owner: store.owner()?,
            project: id.into(),
            origin: origin.into(),
            port,
            stamp: final_stamp,
        })
        .unwrap(),
    )
}
pub(crate) fn recover(store: &SiteStore, id: &str, path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let bytes = fs::read(receipt(store, id)).map_err(|_| E::HotFileConflict)?;
    if bytes.len() > 8192 {
        return Err(E::HotFileConflict);
    }
    let r: Receipt = serde_json::from_slice(&bytes).map_err(|_| E::HotFileConflict)?;
    if r.schema != 1
        || r.owner != store.owner()?
        || r.project != id
        || r.port == 0
        || stamp(path)? != r.stamp
        || !matches(path, &r.origin)
    {
        return Err(E::HotFileConflict);
    }
    let _port = TcpListener::bind(("127.0.0.1", r.port)).map_err(|_| E::HotFileConflict)?;
    if stamp(path)? != r.stamp || !matches(path, &r.origin) {
        return Err(E::HotFileConflict);
    }
    fs::remove_file(path)?;
    fs::remove_file(receipt(store, id))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_owned_hot_recovers_but_foreign_or_live_files_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let store = SiteStore::open(root.path().join("store")).unwrap();
        let path = root.path().join("hot");
        let origin = "http://vite-a.localhost:8080";
        fs::write(&path, origin).unwrap();
        assert!(recover(&store, "a", &path).is_err());
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        record(&store, "a", &path, origin, port).unwrap();
        assert!(recover(&store, "a", &path).is_err());
        drop(listener);
        recover(&store, "a", &path).unwrap();
        assert!(!path.exists());
        fs::write(&path, origin).unwrap();
        record(&store, "a", &path, origin, port).unwrap();
        fs::write(&path, "http://foreign.localhost:5173").unwrap();
        assert!(recover(&store, "a", &path).is_err());
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "http://foreign.localhost:5173"
        );
    }
}
