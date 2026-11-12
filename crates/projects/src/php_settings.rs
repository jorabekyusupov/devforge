use crate::{ProjectError as E, Result, project::ProjectStore};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct PhpSettings {
    pub memory_limit: Option<String>,
    pub upload_mb: Option<u32>,
    pub post_mb: Option<u32>,
    pub execution_seconds: Option<u32>,
    pub timezone: Option<String>,
}
impl PhpSettings {
    pub fn validate(&self) -> Result<()> {
        if let Some(value) = &self.memory_limit {
            let valid = value == "-1"
                || value
                    .strip_suffix('M')
                    .and_then(|n| n.parse::<u32>().ok())
                    .is_some_and(|n| (16..=65536).contains(&n));
            if !valid {
                return Err(E::InvalidEnvironment);
            }
        }
        match (self.upload_mb, self.post_mb) {
            (None, None) => {}
            (Some(u), Some(p)) if u > 0 && p >= u && p <= 4096 => {}
            _ => return Err(E::InvalidEnvironment),
        }
        if self.execution_seconds.is_some_and(|n| n > 3600) {
            return Err(E::InvalidEnvironment);
        }
        if let Some(value) = &self.timezone
            && (value.is_empty()
                || value.len() > 80
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/_+-".contains(&c)))
        {
            return Err(E::InvalidEnvironment);
        }
        Ok(())
    }
    pub(crate) fn ini(&self) -> Result<String> {
        self.validate()?;
        let mut text = String::new();
        if let Some(v) = &self.memory_limit {
            text.push_str(&format!("memory_limit={v}\n"));
        }
        if let (Some(u), Some(p)) = (self.upload_mb, self.post_mb) {
            text.push_str(&format!("upload_max_filesize={u}M\npost_max_size={p}M\n"));
        }
        if let Some(v) = self.execution_seconds {
            text.push_str(&format!("max_execution_time={v}\n"));
        }
        if let Some(v) = &self.timezone {
            text.push_str(&format!("date.timezone=\"{v}\"\n"));
        }
        Ok(text)
    }
}
pub fn save(store: &ProjectStore, id: &str, revision: u64, settings: PhpSettings) -> Result<()> {
    settings.validate()?;
    let _lock = store.operation_lock()?;
    let mut p = store.get(id)?;
    p.php_settings = settings;
    store.save(&mut p, revision)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_ini_injection_and_inconsistent_upload_limits() {
        let mut p = PhpSettings {
            memory_limit: Some("256M\nextension=evil".into()),
            ..Default::default()
        };
        assert!(p.validate().is_err());
        p.memory_limit = Some("512M".into());
        p.upload_mb = Some(32);
        p.post_mb = Some(16);
        assert!(p.validate().is_err());
        p.post_mb = Some(40);
        p.timezone = Some("Asia/Tashkent".into());
        assert!(p.ini().unwrap().contains("post_max_size=40M"));
        assert_eq!(PhpSettings::default().ini().unwrap(), "");
    }
}
