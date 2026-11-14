//! Read-only private credential-file composition for stdio. Tokens never share
//! protocol stdin and are never accepted from argv/environment. No file is made
//! or repaired by the loader; a missing/unsafe file fails closed. Explicit native
//! export helpers create fresh private leaves only, never repair existing paths.
use crate::external_auth::ExternalCredential;
use devforge_control::{ErrorCode, Id};
use serde::Deserialize;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroizing;
pub const MAX_CREDENTIAL_FILE_BYTES: usize = 4096;
/// Native-selected export location only. The explicit action marker is never
/// deserialized from renderer or external protocol input.
pub fn create_new_private_export_dir(
    path: &Path,
    _action: &crate::auth::TrustedDesktopAction,
) -> Result<(), ErrorCode> {
    let parent = path.parent().ok_or(ErrorCode::InvalidInput)?;
    if !path.is_absolute() || fs::canonicalize(parent).ok().as_deref() != Some(parent) {
        return Err(ErrorCode::InvalidInput);
    }
    crate::transport::os::validate_root(parent).map_err(|_| ErrorCode::Unauthorized)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|_| ErrorCode::StorageUnavailable)?;
    }
    #[cfg(windows)]
    crate::transport::os::create_private_export_dir(path)
        .map_err(|_| ErrorCode::StorageUnavailable)?;
    crate::transport::os::validate_private_dir(path).map_err(|_| ErrorCode::Unauthorized)
}
pub fn write_new_private_export_file(
    path: &Path,
    bytes: &[u8],
    _action: &crate::auth::TrustedDesktopAction,
) -> Result<(), ErrorCode> {
    if bytes.len() > 65_536 {
        return Err(ErrorCode::OutputLimitExceeded);
    }
    let parent = path.parent().ok_or(ErrorCode::InvalidInput)?;
    if !path.is_absolute() || fs::canonicalize(parent).ok().as_deref() != Some(parent) {
        return Err(ErrorCode::InvalidInput);
    }
    crate::transport::os::validate_private_dir(parent).map_err(|_| ErrorCode::Unauthorized)?;
    #[cfg(unix)]
    let mut file = {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        crate::transport::os::secure_lock_options(&mut options);
        options
            .open(path)
            .map_err(|_| ErrorCode::StorageUnavailable)?
    };
    #[cfg(windows)]
    let mut file = crate::transport::os::create_private_export_file(path)
        .map_err(|_| ErrorCode::StorageUnavailable)?;
    crate::transport::os::validate_lock(&file).map_err(|_| ErrorCode::Unauthorized)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ErrorCode::StorageUnavailable)
}
pub struct CredentialFile;
pub struct LoadedCredential {
    project_id: Id,
    credential: ExternalCredential,
}
impl std::fmt::Debug for LoadedCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LoadedCredential([REDACTED])")
    }
}
impl LoadedCredential {
    pub fn project_id(&self) -> &Id {
        &self.project_id
    }
    pub fn into_parts(self) -> (Id, ExternalCredential) {
        (self.project_id, self.credential)
    }
}
struct FileToken(Zeroizing<String>);
impl<'de> Deserialize<'de> for FileToken {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let text = Zeroizing::new(String::deserialize(decoder)?);
        if text.len() != 64 {
            return Err(serde::de::Error::custom("invalid_credential"));
        }
        Ok(Self(text))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contents {
    schema: u16,
    project_id: Id,
    credential: FileToken,
}
impl CredentialFile {
    pub fn load(path: &Path) -> Result<LoadedCredential, ErrorCode> {
        if !path.is_absolute() {
            return Err(ErrorCode::InvalidInput);
        }
        let metadata = fs::symlink_metadata(path).map_err(|_| ErrorCode::Unauthorized)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(ErrorCode::Unauthorized);
        }
        let mut options = OpenOptions::new();
        options.read(true);
        crate::transport::os::secure_lock_options(&mut options);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        }
        let file = options.open(path).map_err(|_| ErrorCode::Unauthorized)?;
        crate::transport::os::validate_lock(&file).map_err(|_| ErrorCode::Unauthorized)?;
        #[cfg(windows)]
        crate::transport::os::validate_credential_file(&file)
            .map_err(|_| ErrorCode::Unauthorized)?;
        if file.metadata().map_err(|_| ErrorCode::Unauthorized)?.len()
            > MAX_CREDENTIAL_FILE_BYTES as u64
        {
            return Err(ErrorCode::OutputLimitExceeded);
        }
        let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_CREDENTIAL_FILE_BYTES));
        file.take(MAX_CREDENTIAL_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        if bytes.len() > MAX_CREDENTIAL_FILE_BYTES {
            return Err(ErrorCode::OutputLimitExceeded);
        }
        let contents: Contents =
            serde_json::from_slice(&bytes).map_err(|_| ErrorCode::InvalidInput)?;
        if contents.schema != 1 {
            return Err(ErrorCode::UnsupportedVersion);
        }
        let credential = ExternalCredential::from_wire(&contents.credential.0)
            .map_err(|_| ErrorCode::Unauthorized)?;
        Ok(LoadedCredential {
            project_id: contents.project_id,
            credential,
        })
    }
}
