//! Disposable fixture adapter only. Production uses the OS SecretStore. This
//! private test file is never placed in repository evidence or printed.
use devforge_core::{ErrorCode, SecretStore};
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf, sync::Mutex};
pub struct Secrets {
    path: PathBuf,
    values: Mutex<BTreeMap<String, String>>,
}
impl Secrets {
    pub fn open(path: PathBuf) -> Self {
        let values = fs::read(&path)
            .ok()
            .map(|v| serde_json::from_slice(&v).expect("fixture secrets"))
            .unwrap_or_default();
        Self {
            path,
            values: Mutex::new(values),
        }
    }
    pub fn values(&self) -> Vec<String> {
        self.values.lock().unwrap().values().cloned().collect()
    }
    fn save(&self, values: &BTreeMap<String, String>) -> Result<(), ErrorCode> {
        let mut file = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())
            .map_err(|_| ErrorCode::SecretUnavailable)?;
        file.write_all(&serde_json::to_vec(values).unwrap())
            .map_err(|_| ErrorCode::SecretUnavailable)?;
        file.persist(&self.path)
            .map_err(|_| ErrorCode::SecretUnavailable)?;
        Ok(())
    }
}
impl SecretStore for Secrets {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode> {
        let mut values = self.values.lock().unwrap();
        values.insert(key.into(), value.into());
        self.save(&values)
    }
    fn delete(&self, key: &str) -> Result<(), ErrorCode> {
        let mut values = self.values.lock().unwrap();
        values.remove(key);
        self.save(&values)
    }
}
