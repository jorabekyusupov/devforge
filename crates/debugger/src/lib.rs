//! Native Xdebug protocol and function profiles. Separate from completed Laravel telemetry.
pub mod profiles;
pub mod protocol;
pub mod provider;
pub mod server;
pub mod session;
pub mod store;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    Unavailable,
    Protocol,
    Unauthorized,
    SourceOutside,
    Busy,
    InvalidBreakpoint,
    ProfileInvalid,
    ProfileLimit,
    RuntimeMissing,
    RuntimeIncompatible,
    BuildFailed,
    Cancelled,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self).unwrap().as_str().unwrap()
        )
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Unavailable
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    Debug,
    Profile,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Breakpoint {
    pub file: String,
    pub line: u32,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub runtime: Option<String>,
    pub mode: Mode,
    pub breakpoints: Vec<Breakpoint>,
}
pub fn source(root: &std::path::Path, file: &str) -> Result<std::path::PathBuf> {
    let rel = std::path::Path::new(file);
    if rel.is_absolute()
        || file.contains(['\0', '\r', '\n'])
        || !rel
            .components()
            .all(|p| matches!(p, std::path::Component::Normal(_)))
    {
        return Err(Error::SourceOutside);
    }
    let root = std::fs::canonicalize(root)?;
    let path = std::fs::canonicalize(root.join(rel))?;
    if !path.starts_with(&root) || !path.is_file() || path.extension().is_none_or(|s| s != "php") {
        return Err(Error::SourceOutside);
    }
    Ok(path)
}
impl Settings {
    pub fn validate(&self, root: &std::path::Path) -> Result<()> {
        if self
            .runtime
            .as_ref()
            .is_some_and(|id| id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(Error::RuntimeMissing);
        }
        let mut unique = std::collections::BTreeSet::new();
        if self.breakpoints.len() > 100 {
            return Err(Error::InvalidBreakpoint);
        }
        for b in &self.breakpoints {
            if b.line == 0 || b.line > 1_000_000 || !unique.insert((&b.file, b.line)) {
                return Err(Error::InvalidBreakpoint);
            }
            if self.mode == Mode::Debug {
                source(root, &b.file)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;
    #[test]
    fn missing_breakpoint_source_cannot_prevent_disabling_debugging() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("work.php");
        std::fs::write(&file, "<?php echo 1;").unwrap();
        let mut settings = Settings {
            mode: Mode::Debug,
            runtime: Some("a".repeat(32)),
            breakpoints: vec![Breakpoint {
                file: "work.php".into(),
                line: 1,
            }],
        };
        settings.validate(root.path()).unwrap();
        std::fs::remove_file(file).unwrap();
        assert!(settings.validate(root.path()).is_err());
        settings.mode = Mode::Off;
        settings.validate(root.path()).unwrap();
        settings.mode = Mode::Profile;
        settings.validate(root.path()).unwrap();
    }
}
