//! Editor adapter around the existing portable manifest planner and setup transaction.
use super::*;
use devforge_projects::manifest::{self, Manifest};
use sha2::{Digest, Sha256};
use std::io::Write;
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl Workspace {
    pub fn configuration_document(&self, id: &str) -> Result<serde_json::Value> {
        let project = self.projects.get(id)?;
        let path = project.path.join("devforge.yaml");
        let existing = if fs::symlink_metadata(&path).is_ok() {
            Some(manifest::read_file(&path, 65536)?)
        } else {
            None
        };
        let text = match &existing {
            Some(bytes) => {
                String::from_utf8(bytes.clone()).map_err(|_| error("manifest_invalid"))?
            }
            None => self.manifest_definition(id)?.yaml()?,
        };
        Ok(
            serde_json::json!({"text":text,"expected":existing.as_ref().map(|b|hash(b)),"exists":existing.is_some(),"journal":self.manifest_status(&project.path).ok()}),
        )
    }
    pub fn configuration_save(
        &mut self,
        id: &str,
        expected: Option<&str>,
        text: &str,
        consent: bool,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        if !consent {
            return Err(error("manifest_review_required"));
        }
        if text.len() > 65536 {
            return Err(error("manifest_invalid"));
        }
        let project = self.projects.get(id)?;
        let parsed = Manifest::parse(text.as_bytes())?;
        self.validate_site(id, &parsed.project.domain)?;
        let _lock = self.repro_lock(&project.path)?;
        let path = project.path.join("devforge.yaml");
        let current = if fs::symlink_metadata(&path).is_ok() {
            Some(manifest::read_file(&path, 65536)?)
        } else {
            None
        };
        if current.as_ref().map(|b| hash(b)).as_deref() != expected {
            return Err(error("manifest_review_changed"));
        }
        if let Some(bytes) = &current {
            let history = self.repro_dir(&project.path)?.join("configuration-history");
            fs::create_dir_all(&history)?;
            if fs::symlink_metadata(&history)?.file_type().is_symlink() {
                return Err(error("manifest_invalid"));
            }
            let backup = history.join(format!("{}.yaml", hash(bytes)));
            if !backup.exists() {
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(backup)?;
                f.write_all(bytes)?;
                f.sync_all()?;
            }
        }
        let mut temporary = tempfile::NamedTempFile::new_in(&project.path)?;
        temporary.write_all(text.as_bytes())?;
        temporary.as_file().sync_all()?;
        let again = if path.exists() {
            Some(manifest::read_file(&path, 65536)?)
        } else {
            None
        };
        if again.as_ref().map(|b| hash(b)).as_deref() != expected {
            return Err(error("manifest_review_changed"));
        }
        if expected.is_none() {
            temporary
                .persist_noclobber(path)
                .map_err(|_| error("manifest_review_changed"))?;
        } else {
            temporary
                .persist(path)
                .map_err(|_| error("storage_unavailable"))?;
        }
        self.configuration_document(id)
    }
    pub fn configuration_plan(&self, id: &str, write_env: bool) -> Result<serde_json::Value> {
        let p = self.projects.get(id)?;
        Ok(serde_json::to_value(self.manifest_plan(&p.path, write_env)?).unwrap())
    }
    pub fn configuration_apply(
        &mut self,
        id: &str,
        review: &str,
        trusted: bool,
        write_env: bool,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        self.require_stopped(id)?;
        let p = self.projects.get(id)?;
        Ok(
            serde_json::to_value(self.manifest_setup(&p.path, review, trusted, write_env, cancel)?)
                .unwrap(),
        )
    }
}
