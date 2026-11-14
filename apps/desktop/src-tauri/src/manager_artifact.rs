//! Compiled packaging authority for the fixed adjacent manager CLI.
//! This resolver does not authorize launch: the SDK must verify `expected_sha256` before
//! execution. It never derives an expected digest from the candidate executable.
use std::path::{Path, PathBuf};
struct Descriptor {
    target: &'static str,
    filename: &'static str,
    expected_sha256: [u8; 32],
}
include!(concat!(env!("OUT_DIR"), "/manager_artifact.rs"));
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledManager {
    pub executable: PathBuf,
    pub target: &'static str,
    pub expected_sha256: [u8; 32],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactError {
    ExecutableLocationUnavailable,
}
impl std::fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bundled_manager_location_unavailable")
    }
}
impl std::error::Error for ArtifactError {}
fn adjacent(executable: &Path, descriptor: &Descriptor) -> Result<BundledManager, ArtifactError> {
    let parent = executable
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or(ArtifactError::ExecutableLocationUnavailable)?;
    Ok(BundledManager {
        executable: parent.join(descriptor.filename),
        target: descriptor.target,
        expected_sha256: descriptor.expected_sha256,
    })
}
/// Ordinary development builds return None even if a stale generated packaging
/// manifest exists. No PATH, working-directory or runtime environment fallback.
pub fn bundled_launch() -> Result<Option<BundledManager>, ArtifactError> {
    let Some(descriptor) = PACKAGED_MANAGER.as_ref() else {
        return Ok(None);
    };
    let executable =
        std::env::current_exe().map_err(|_| ArtifactError::ExecutableLocationUnavailable)?;
    adjacent(&executable, descriptor).map(Some)
}
#[cfg(test)]
#[path = "manager_artifact_manifest.rs"]
mod manifest;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_adjacent_candidate_keeps_compiled_authority() {
        let descriptor = Descriptor {
            target: "x86_64-pc-windows-msvc",
            filename: "devforge.exe",
            expected_sha256: [0xab; 32],
        };
        let resolved = adjacent(Path::new("/owned/app/devforge-desktop"), &descriptor).unwrap();
        assert_eq!(resolved.executable, Path::new("/owned/app/devforge.exe"));
        assert_eq!(resolved.expected_sha256, [0xab; 32]);
        assert!(adjacent(Path::new("devforge-desktop"), &descriptor).is_err());
    }
    #[test]
    fn ordinary_build_has_no_compiled_manager_authority() {
        if option_env!("DEVFORGE_PACKAGE_MANAGER").is_none() {
            assert!(PACKAGED_MANAGER.is_none());
            assert_eq!(bundled_launch().unwrap(), None);
        } else {
            assert_eq!(option_env!("DEVFORGE_PACKAGE_MANAGER"), Some("1"));
            let bundled = bundled_launch().unwrap().expect("packaged authority");
            assert_eq!(bundled.expected_sha256.len(), 32);
            assert_eq!(
                bundled.executable.file_name().unwrap(),
                if cfg!(windows) {
                    "devforge.exe"
                } else {
                    "devforge"
                }
            );
        }
    }
}
