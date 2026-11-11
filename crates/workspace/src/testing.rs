//! Isolated acceptance fixture only; never selected by a production desktop build.
use devforge_core::{ErrorCode, SecretStore};
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf, sync::Mutex};
pub struct FixtureSecrets {
    path: PathBuf,
    values: Mutex<BTreeMap<String, String>>,
}
impl FixtureSecrets {
    pub fn open(path: PathBuf) -> Self {
        let values = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).expect("fixture JSON"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => panic!("fixture storage: {e}"),
        };
        Self {
            path,
            values: Mutex::new(values),
        }
    }
    fn save(&self, v: &BTreeMap<String, String>) -> Result<(), ErrorCode> {
        let mut f = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())
            .map_err(|_| ErrorCode::SecretUnavailable)?;
        f.write_all(&serde_json::to_vec(v).unwrap())
            .map_err(|_| ErrorCode::SecretUnavailable)?;
        f.persist(&self.path)
            .map_err(|_| ErrorCode::SecretUnavailable)?;
        Ok(())
    }
}
impl SecretStore for FixtureSecrets {
    fn get(&self, k: &str) -> Result<Option<String>, ErrorCode> {
        Ok(self.values.lock().unwrap().get(k).cloned())
    }
    fn set(&self, k: &str, v: &str) -> Result<(), ErrorCode> {
        let mut data = self.values.lock().unwrap();
        data.insert(k.into(), v.into());
        self.save(&data)
    }
    fn delete(&self, k: &str) -> Result<(), ErrorCode> {
        let mut data = self.values.lock().unwrap();
        data.remove(k);
        self.save(&data)
    }
}
