use crate::{Result, ServingError as E};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Hosts {
        before_sha256: String,
        domains: Vec<String>,
    },
    TrustInstall {
        certificate_der: Vec<u8>,
    },
    TrustRemove {
        certificate_der: Vec<u8>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: u32,
    pub owner: String,
    pub action: Operation,
}
#[derive(Debug, Serialize)]
pub struct Effect {
    pub changed: bool,
    pub preexisting: bool,
    pub digest: String,
}
impl Plan {
    pub fn digest(&self) -> String {
        crate::hash(&serde_json::to_vec(self).expect("bounded plan"))
    }
    pub fn validate(&self, digest: &str) -> Result<()> {
        if self.schema != 1
            || !crate::site::owner(&self.owner)
            || digest.len() != 64
            || self.digest() != digest
        {
            return Err(E::InvalidConfiguration);
        }
        match &self.action {
            Operation::Hosts {
                before_sha256,
                domains,
            } => {
                if before_sha256.len() != 64
                    || !before_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    || domains.len() > 128
                {
                    return Err(E::InvalidConfiguration);
                }
                for d in domains {
                    crate::site::domain(d)?;
                }
            }
            Operation::TrustInstall { certificate_der }
            | Operation::TrustRemove { certificate_der } => {
                crate::authority::validate_root(certificate_der, &self.owner)?
            }
        }
        Ok(())
    }
    pub fn apply(&self, expected_digest: &str) -> Result<Effect> {
        self.validate(expected_digest)?;
        if !elevated()? {
            return Err(E::PermissionRequired);
        }
        let (changed, preexisting) = match &self.action {
            Operation::Hosts {
                before_sha256,
                domains,
            } => {
                let path = hosts_path()?;
                let metadata = fs::symlink_metadata(&path)?;
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(E::HostsConflict);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.nlink() != 1 {
                        return Err(E::HostsConflict);
                    }
                }
                let lock = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .map_err(|_| E::PermissionRequired)?;
                lock.try_lock().map_err(|_| E::HostsChanged)?;
                let before = fs::read(&path)?;
                if crate::hash(&before) != *before_sha256 {
                    return Err(E::HostsChanged);
                }
                let after = crate::hosts::patch(&before, &self.owner, domains)?;
                let changed = after != before;
                if changed {
                    let mut staged = tempfile::NamedTempFile::new_in(
                        path.parent().ok_or(E::StorageUnavailable)?,
                    )?;
                    #[cfg(target_os = "macos")]
                    {
                        let copied = std::process::Command::new("/bin/cp")
                            .arg("-p")
                            .arg(&path)
                            .arg(staged.path())
                            .status()?;
                        if !copied.success() {
                            return Err(E::PermissionRequired);
                        }
                    }
                    staged.as_file_mut().set_len(0)?;
                    staged.write_all(&after)?;
                    staged.as_file().sync_all()?;
                    if crate::hash(&fs::read(&path)?) != *before_sha256 {
                        return Err(E::HostsChanged);
                    }
                    #[cfg(unix)]
                    staged.persist(&path).map_err(|_| E::PermissionRequired)?;
                    #[cfg(windows)]
                    {
                        use std::os::windows::ffi::OsStrExt;
                        use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
                        let target: Vec<u16> =
                            path.as_os_str().encode_wide().chain(Some(0)).collect();
                        let source: Vec<u16> = staged
                            .path()
                            .as_os_str()
                            .encode_wide()
                            .chain(Some(0))
                            .collect();
                        if unsafe {
                            ReplaceFileW(
                                target.as_ptr(),
                                source.as_ptr(),
                                std::ptr::null(),
                                0,
                                std::ptr::null(),
                                std::ptr::null(),
                            )
                        } == 0
                        {
                            return Err(E::PermissionRequired);
                        }
                    }
                }
                drop(lock);
                (changed, !changed)
            }
            Operation::TrustInstall { certificate_der } => {
                if certificate_present(certificate_der)? {
                    (false, true)
                } else {
                    install(certificate_der)?;
                    (true, false)
                }
            }
            Operation::TrustRemove { certificate_der } => {
                if certificate_present(certificate_der)? {
                    remove(certificate_der)?;
                    (true, false)
                } else {
                    (false, false)
                }
            }
        };
        Ok(Effect {
            changed,
            preexisting,
            digest: self.digest(),
        })
    }
}
#[allow(clippy::needless_return)]
pub fn hosts_path() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    return Ok(PathBuf::from("/private/etc/hosts"));
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
        let mut path = [0u16; 32768];
        let len = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
        if len == 0 || len >= path.len() {
            return Err(E::StorageUnavailable);
        }
        return Ok(PathBuf::from(
            String::from_utf16(&path[..len]).map_err(|_| E::StorageUnavailable)?,
        )
        .join("drivers")
        .join("etc")
        .join("hosts"));
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    Err(E::InvalidConfiguration)
}
pub fn hosts_plan(owner: &str, domains: Vec<String>) -> Result<Plan> {
    let before = fs::read(hosts_path()?)?;
    crate::hosts::patch(&before, owner, &domains)?;
    Ok(Plan {
        schema: 1,
        owner: owner.into(),
        action: Operation::Hosts {
            before_sha256: crate::hash(&before),
            domains,
        },
    })
}
#[allow(clippy::needless_return)]
pub fn elevated() -> Result<bool> {
    #[cfg(unix)]
    return Ok(unsafe { libc::geteuid() } == 0);
    #[cfg(windows)]
    return devforge_projects::database::administrator_enabled().map_err(|_| E::PermissionRequired);
    #[cfg(not(any(unix, windows)))]
    Ok(false)
}
#[allow(clippy::needless_return)]
pub fn trusted(der: &[u8]) -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(pem::encode(&pem::Pem::new("CERTIFICATE", der.to_vec())).as_bytes())?;
        let result = std::process::Command::new("/usr/bin/security")
            .args(["verify-cert", "-p", "basic", "-c"])
            .arg(file.path())
            .output()?;
        return Ok(result.status.success());
    }
    #[cfg(windows)]
    return certificate_present(der);
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = der;
        Ok(false)
    }
}
#[cfg(target_os = "macos")]
fn certificate_present(der: &[u8]) -> Result<bool> {
    let result = std::process::Command::new("/usr/bin/security")
        .args([
            "find-certificate",
            "-a",
            "-p",
            "/Library/Keychains/System.keychain",
        ])
        .output()?;
    if !result.status.success() {
        return Err(E::TrustFailed);
    }
    let list = pem::parse_many(result.stdout).map_err(|_| E::TrustFailed)?;
    Ok(list.iter().any(|p| p.contents() == der))
}
#[cfg(target_os = "macos")]
fn install(der: &[u8]) -> Result<()> {
    // The privileged process stages its own private copy. security never re-reads
    // a mutable caller-owned certificate path after validation.
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(pem::encode(&pem::Pem::new("CERTIFICATE", der.to_vec())).as_bytes())?;
    file.as_file().sync_all()?;
    let status = std::process::Command::new("/usr/bin/security")
        .args([
            "add-trusted-cert",
            "-d",
            "-r",
            "trustRoot",
            "-k",
            "/Library/Keychains/System.keychain",
        ])
        .arg(file.path())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(E::TrustFailed)
    }
}
#[cfg(target_os = "macos")]
fn remove(der: &[u8]) -> Result<()> {
    use sha1::{Digest, Sha1};
    let fp: String = Sha1::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    let status = std::process::Command::new("/usr/bin/security")
        .args([
            "delete-certificate",
            "-Z",
            &fp,
            "/Library/Keychains/System.keychain",
        ])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(E::TrustFailed)
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::Win32::Security::Cryptography::*;
    pub struct Store(HCERTSTORE);
    impl Drop for Store {
        fn drop(&mut self) {
            unsafe {
                CertCloseStore(self.0, 0);
            }
        }
    }
    fn open(write: bool) -> Result<Store> {
        let name = [b'R' as u16, b'o' as u16, b'o' as u16, b't' as u16, 0];
        let flags = CERT_SYSTEM_STORE_LOCAL_MACHINE
            | CERT_STORE_OPEN_EXISTING_FLAG
            | if write { 0 } else { CERT_STORE_READONLY_FLAG };
        let handle =
            unsafe { CertOpenStore(CERT_STORE_PROV_SYSTEM_W, 0, 0, flags, name.as_ptr().cast()) };
        if handle.is_null() {
            return Err(E::TrustFailed);
        }
        Ok(Store(handle))
    }
    pub fn present(der: &[u8]) -> Result<bool> {
        let store = open(false)?;
        let mut context = std::ptr::null();
        loop {
            let current = unsafe { CertEnumCertificatesInStore(store.0, context) };
            if current.is_null() {
                return Ok(false);
            }
            context = current;
            let cert = unsafe { &*current };
            let bytes = unsafe {
                std::slice::from_raw_parts(cert.pbCertEncoded, cert.cbCertEncoded as usize)
            };
            if bytes == der {
                unsafe {
                    CertFreeCertificateContext(current);
                }
                return Ok(true);
            }
        }
    }
    pub fn add(der: &[u8]) -> Result<()> {
        let store = open(true)?;
        if unsafe {
            CertAddEncodedCertificateToStore(
                store.0,
                X509_ASN_ENCODING,
                der.as_ptr(),
                der.len() as u32,
                CERT_STORE_ADD_NEW,
                std::ptr::null_mut(),
            )
        } == 0
        {
            Err(E::TrustFailed)
        } else {
            Ok(())
        }
    }
    pub fn delete(der: &[u8]) -> Result<()> {
        let store = open(true)?;
        let mut context = std::ptr::null();
        loop {
            let current = unsafe { CertEnumCertificatesInStore(store.0, context) };
            if current.is_null() {
                return Err(E::TrustFailed);
            }
            context = current;
            let cert = unsafe { &*current };
            if unsafe {
                std::slice::from_raw_parts(cert.pbCertEncoded, cert.cbCertEncoded as usize)
            } == der
            {
                return if unsafe { CertDeleteCertificateFromStore(current) } == 0 {
                    Err(E::TrustFailed)
                } else {
                    Ok(())
                };
            }
        }
    }
}
#[cfg(windows)]
fn certificate_present(der: &[u8]) -> Result<bool> {
    windows::present(der)
}
#[cfg(windows)]
fn install(der: &[u8]) -> Result<()> {
    windows::add(der)
}
#[cfg(windows)]
fn remove(der: &[u8]) -> Result<()> {
    windows::delete(der)
}
#[cfg(not(any(target_os = "macos", windows)))]
fn certificate_present(_: &[u8]) -> Result<bool> {
    Err(E::TrustFailed)
}
#[cfg(not(any(target_os = "macos", windows)))]
fn install(_: &[u8]) -> Result<()> {
    Err(E::TrustFailed)
}
#[cfg(not(any(target_os = "macos", windows)))]
fn remove(_: &[u8]) -> Result<()> {
    Err(E::TrustFailed)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digest_binds_every_reviewed_operation_and_rejects_extra_fields() {
        let p = Plan {
            schema: 1,
            owner: "a".repeat(32),
            action: Operation::Hosts {
                before_sha256: "0".repeat(64),
                domains: vec!["a.devforge.test".into()],
            },
        };
        p.validate(&p.digest()).unwrap();
        assert!(p.validate(&"0".repeat(64)).is_err());
        let mut q = serde_json::to_value(&p).unwrap();
        q["target"] = "/etc/passwd".into();
        assert!(serde_json::from_value::<Plan>(q).is_err());
    }
}
