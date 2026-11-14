//! Acceptance-build root validation. No directory or credential is created here.
use devforge_core::ErrorCode;
use devforge_workspace::testing::FixtureRoot;
use std::ffi::OsStr;

pub(crate) fn resolve(input: Option<&OsStr>) -> Result<FixtureRoot, ErrorCode> {
    let path = input.ok_or(ErrorCode::StorageUnavailable)?;
    FixtureRoot::validate(std::path::Path::new(path)).map_err(|_| ErrorCode::StorageUnavailable)
}

pub(crate) fn from_environment() -> Result<FixtureRoot, ErrorCode> {
    resolve(std::env::var_os("DEVFORGE_DATA_DIR").as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn acceptance_root_is_explicit_and_validated_without_adoption_or_writes() {
        assert!(resolve(None).is_err());
        let parent = tempfile::tempdir().unwrap();
        let absent = parent.path().join("absent");
        assert!(resolve(Some(absent.as_os_str())).is_err());
        assert!(!absent.exists());

        let existing = parent.path().join("existing");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("canary"), b"unchanged").unwrap();
        assert!(resolve(Some(existing.as_os_str())).is_err());
        assert_eq!(fs::read(existing.join("canary")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(&existing).unwrap().count(), 1);

        let valid = parent.path().join("fixture");
        let fixture = FixtureRoot::create_new(&valid).unwrap();
        let before = fs::read_dir(&valid).unwrap().count();
        let resolved = resolve(Some(valid.as_os_str())).unwrap();
        assert_eq!(resolved.root(), fixture.root());
        assert_eq!(fs::read_dir(&valid).unwrap().count(), before);
    }
}
