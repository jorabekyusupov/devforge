use crate::{Cancellation, Result, RuntimeError, relative_path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    MacosArm64,
    MacosX64,
    WindowsX64,
}
impl Target {
    pub fn current() -> Result<Self> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => Ok(Self::MacosArm64),
            ("macos", "x86_64") => Ok(Self::MacosX64),
            ("windows", "x86_64") => Ok(Self::WindowsX64),
            _ => Err(RuntimeError::UnsupportedTarget),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Archive {
    TarGz,
    TarXz,
    Zip,
    ZipTarXz { member: String },
}

/// Caller supplies a reviewed/pinned catalog. A digest supplied by an untrusted
/// download itself is not trusted provenance. Catalog updates never select a
/// newer version for a project; callers resolve exact IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub kind: String,
    pub version: String,
    pub variant: String,
    pub target: Target,
    pub url: String,
    pub sha256: String,
    pub archive: Archive,
    pub executables: BTreeMap<String, String>,
}
impl Package {
    pub fn key(&self) -> Result<String> {
        fn component(s: &str) -> bool {
            !s.is_empty()
                && s.len() <= 80
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                && s != "."
                && s != ".."
        }
        if !component(&self.kind)
            || !component(&self.version)
            || !component(&self.variant)
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.executables.is_empty()
        {
            return Err(RuntimeError::InvalidPackage);
        }
        for (name, path) in &self.executables {
            if !component(name) {
                return Err(RuntimeError::InvalidPackage);
            }
            relative_path(path)?;
        }
        if let Archive::ZipTarXz { member } = &self.archive {
            relative_path(member)?;
        }
        // Length-prefixed JSON avoids ambiguous hyphen joins and path collisions.
        let id =
            serde_json::to_vec(&(&self.kind, &self.version, &self.variant, self.target)).unwrap();
        Ok(format!("{}-{}", self.kind, hex(&Sha256::digest(id))))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub download_bytes: u64,
    pub unpacked_bytes: u64,
    pub entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            download_bytes: 2 * 1024 * 1024 * 1024,
            unpacked_bytes: 4 * 1024 * 1024 * 1024,
            entries: 100_000,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallStage {
    Downloading,
    Verifying,
    Extracting,
    Activating,
    Installed,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Serialize)]
pub struct InstallProgress {
    pub stage: InstallStage,
    pub bytes: u64,
    pub total: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    schema: u32,
    package: Package,
    hashes: BTreeMap<String, String>,
}

pub struct PackageStore {
    root: PathBuf,
    limits: Limits,
}
pub struct Installed {
    pub key: String,
    pub directory: PathBuf,
    pub reused: bool,
}
#[derive(Clone)]
pub struct VerifiedExecutable {
    path: PathBuf,
    sha256: String,
    // Shared OS lease lasts as long as any process/handle uses this package.
    _lease: Arc<File>,
}
impl VerifiedExecutable {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn check(&self) -> Result<()> {
        if hash_file(&self.path)? != self.sha256 {
            return Err(RuntimeError::ExecutableChanged);
        }
        Ok(())
    }
}
impl PackageStore {
    pub fn open(root: impl AsRef<Path>, limits: Limits) -> Result<Self> {
        let root = root.as_ref();
        if limits.entries == 0 || limits.download_bytes == 0 || limits.unpacked_bytes == 0 {
            return Err(RuntimeError::InvalidPackage);
        }
        fs::create_dir_all(root)?;
        let root = fs::canonicalize(root)?;
        let marker = root.join(".devforge-runtime-store");
        if !marker.exists() {
            if fs::read_dir(&root)?.next().is_some() {
                return Err(RuntimeError::PackageConflict);
            }
            write_new(&marker, b"devforge-runtime-store-v1\n")?;
        }
        if fs::read(&marker)? != b"devforge-runtime-store-v1\n" {
            return Err(RuntimeError::PackageConflict);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        for dir in ["packages", "staging", "leases"] {
            let path = root.join(dir);
            if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(RuntimeError::UnsafeArchive);
            }
            fs::create_dir_all(path)?;
        }
        Ok(Self { root, limits })
    }
    fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("store.lock"))?;
        file.try_lock().map_err(|_| RuntimeError::Busy)?;
        Ok(file)
    }
    fn lease(&self, key: &str, exclusive: bool) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join("leases").join(key))?;
        if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        }
        .map_err(|_| RuntimeError::Busy)?;
        Ok(file)
    }
    fn recover(&self) -> Result<()> {
        // Only this marked store's abandoned task staging; never projects/data.
        for entry in fs::read_dir(self.root.join("staging"))? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("install-") {
                if entry.file_type()?.is_dir() {
                    fs::remove_dir_all(entry.path())?;
                } else {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        Ok(())
    }
    pub fn install_cached(
        &self,
        package: &Package,
        archive: &Path,
        cancel: &Cancellation,
        progress: impl FnMut(InstallProgress),
    ) -> Result<Installed> {
        self.install(package, cancel, progress, |out, emit| {
            let mut source = File::open(archive)?;
            copy_checked(&mut source, out, self.limits.download_bytes, cancel, |n| {
                emit(InstallProgress {
                    stage: InstallStage::Downloading,
                    bytes: n,
                    total: None,
                })
            })?;
            Ok(())
        })
    }
    pub fn download(
        &self,
        package: &Package,
        cancel: &Cancellation,
        progress: impl FnMut(InstallProgress),
    ) -> Result<Installed> {
        self.install(package, cancel, progress, |out, emit| {
            let url =
                reqwest::Url::parse(&package.url).map_err(|_| RuntimeError::InvalidPackage)?;
            if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
                return Err(RuntimeError::InvalidPackage);
            }
            let client = reqwest::Client::builder()
                .https_only(true)
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(300))
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()
                .map_err(|_| RuntimeError::DownloadFailed)?;
            cancel.check()?;
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let mut response = cancellable(client.get(url).send(), cancel)
                    .await?
                    .and_then(|r| r.error_for_status())
                    .map_err(|_| RuntimeError::DownloadFailed)?;
                let total = response.content_length();
                if total.is_some_and(|n| n > self.limits.download_bytes) {
                    return Err(RuntimeError::LimitExceeded);
                }
                let mut count = 0u64;
                while let Some(chunk) = cancellable(response.chunk(), cancel)
                    .await?
                    .map_err(|_| RuntimeError::DownloadFailed)?
                {
                    count = count
                        .checked_add(chunk.len() as u64)
                        .ok_or(RuntimeError::LimitExceeded)?;
                    if count > self.limits.download_bytes {
                        return Err(RuntimeError::LimitExceeded);
                    }
                    out.write_all(&chunk)?;
                    emit(InstallProgress {
                        stage: InstallStage::Downloading,
                        bytes: count,
                        total,
                    });
                }
                Ok(())
            })
        })
    }
    fn install(
        &self,
        package: &Package,
        cancel: &Cancellation,
        mut progress: impl FnMut(InstallProgress),
        fetch: impl FnOnce(&mut File, &mut dyn FnMut(InstallProgress)) -> Result<()>,
    ) -> Result<Installed> {
        let result = (|| {
            let key = package.key()?;
            if package.target != Target::current()? {
                return Err(RuntimeError::UnsupportedTarget);
            }
            cancel.check()?;
            let _lock = self.lock()?;
            self.recover()?;
            let destination = self.root.join("packages").join(&key);
            if destination.exists() {
                self.receipt(package)?;
                for name in package.executables.keys() {
                    self.executable(package, name)?.check()?;
                }
                progress(InstallProgress {
                    stage: InstallStage::Installed,
                    bytes: 0,
                    total: None,
                });
                return Ok(Installed {
                    key,
                    directory: destination,
                    reused: true,
                });
            }
            let stage = tempfile::Builder::new()
                .prefix("install-")
                .tempdir_in(self.root.join("staging"))?;
            let source_path = stage.path().join("download");
            let mut source = File::create(&source_path)?;
            fetch(&mut source, &mut progress)?;
            source.sync_all()?;
            progress(InstallProgress {
                stage: InstallStage::Verifying,
                bytes: source.metadata()?.len(),
                total: None,
            });
            cancel.check()?;
            if hash_file(&source_path)? != package.sha256.to_ascii_lowercase() {
                return Err(RuntimeError::IntegrityMismatch);
            }
            let payload = stage.path().join("payload");
            fs::create_dir(&payload)?;
            progress(InstallProgress {
                stage: InstallStage::Extracting,
                bytes: 0,
                total: None,
            });
            crate::archive::extract(
                &source_path,
                &package.archive,
                &payload,
                self.limits,
                cancel,
            )?;
            let mut hashes = BTreeMap::new();
            for (name, path) in &package.executables {
                cancel.check()?;
                let executable = fs::canonicalize(payload.join(relative_path(path)?))?;
                if !executable.starts_with(&payload) || !executable.is_file() {
                    return Err(RuntimeError::UnsafeArchive);
                }
                verify_architecture(&executable, package.target)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
                }
                hashes.insert(name.clone(), hash_file(&executable)?);
            }
            let receipt = Receipt {
                schema: 1,
                package: package.clone(),
                hashes,
            };
            write_new(
                &payload.join(".devforge-receipt.json"),
                &serde_json::to_vec(&receipt).unwrap(),
            )?;
            progress(InstallProgress {
                stage: InstallStage::Activating,
                bytes: 0,
                total: None,
            });
            cancel.check()?;
            // Same-filesystem rename is the commit point. Cancellation after this
            // point does not roll back a successfully installed immutable version.
            fs::rename(&payload, &destination)?;
            progress(InstallProgress {
                stage: InstallStage::Installed,
                bytes: 0,
                total: None,
            });
            Ok(Installed {
                key,
                directory: destination,
                reused: false,
            })
        })();
        if let Err(error) = &result {
            progress(InstallProgress {
                stage: if *error == RuntimeError::Cancelled {
                    InstallStage::Cancelled
                } else {
                    InstallStage::Failed
                },
                bytes: 0,
                total: None,
            });
        }
        result
    }
    fn receipt(&self, package: &Package) -> Result<(PathBuf, Receipt)> {
        let path = self.root.join("packages").join(package.key()?);
        let bytes = fs::read(path.join(".devforge-receipt.json"))
            .map_err(|_| RuntimeError::NotInstalled)?;
        let receipt: Receipt =
            serde_json::from_slice(&bytes).map_err(|_| RuntimeError::IntegrityMismatch)?;
        if receipt.schema != 1 || receipt.package != *package {
            return Err(RuntimeError::PackageConflict);
        }
        Ok((path, receipt))
    }
    pub fn executable(&self, package: &Package, name: &str) -> Result<VerifiedExecutable> {
        if package.target != Target::current()? {
            return Err(RuntimeError::UnsupportedTarget);
        }
        let lease = self.lease(&package.key()?, false)?;
        let (root, receipt) = self.receipt(package)?;
        let relative = package
            .executables
            .get(name)
            .ok_or(RuntimeError::InvalidPackage)?;
        let path = fs::canonicalize(root.join(relative_path(relative)?))?;
        if !path.starts_with(root) {
            return Err(RuntimeError::UnsafeArchive);
        }
        let executable = VerifiedExecutable {
            // Ownership was checked with canonical paths above. External tools
            // (notably Node's script loader) can reject Windows verbatim paths.
            // Simplify only where the path retains identical filesystem meaning.
            path: dunce::simplified(&path).to_owned(),
            sha256: receipt
                .hashes
                .get(name)
                .ok_or(RuntimeError::IntegrityMismatch)?
                .clone(),
            _lease: Arc::new(lease),
        };
        executable.check()?;
        Ok(executable)
    }
    pub fn remove(&self, package: &Package) -> Result<()> {
        let _lock = self.lock()?;
        let _lease = self.lease(&package.key()?, true)?;
        let (path, _) = self.receipt(package)?;
        fs::remove_dir_all(path)?;
        Ok(())
    }
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        digest.update(&bytes[..n]);
    }
    Ok(hex(&digest.finalize()))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
async fn cancellable<F: std::future::Future>(
    future: F,
    cancel: &Cancellation,
) -> Result<F::Output> {
    tokio::pin!(future);
    loop {
        tokio::select! {
            result=&mut future=>return Ok(result),
            _=tokio::time::sleep(Duration::from_millis(20))=>cancel.check()?,
        }
    }
}
pub(crate) fn copy_checked(
    source: &mut impl Read,
    output: &mut impl Write,
    limit: u64,
    cancel: &Cancellation,
    mut progress: impl FnMut(u64),
) -> Result<u64> {
    let mut bytes = [0; 65536];
    let mut count = 0u64;
    loop {
        cancel.check()?;
        let n = source.read(&mut bytes)?;
        if n == 0 {
            return Ok(count);
        }
        count = count
            .checked_add(n as u64)
            .ok_or(RuntimeError::LimitExceeded)?;
        if count > limit {
            return Err(RuntimeError::LimitExceeded);
        }
        output.write_all(&bytes[..n])?;
        progress(count);
    }
}
fn verify_architecture(path: &Path, target: Target) -> Result<()> {
    let mut file = File::open(path)?;
    let mut head = [0; 64];
    file.read_exact(&mut head)
        .map_err(|_| RuntimeError::UnsupportedTarget)?;
    if matches!(target, Target::MacosArm64 | Target::MacosX64)
        && head[..4] == [0xca, 0xfe, 0xba, 0xbe]
    {
        use std::io::{Seek, SeekFrom};
        let count = u32::from_be_bytes(head[4..8].try_into().unwrap());
        if !(1..=16).contains(&count) {
            return Err(RuntimeError::UnsupportedTarget);
        }
        let wanted = if target == Target::MacosArm64 {
            0x0100000c
        } else {
            0x01000007
        };
        for index in 0..count {
            file.seek(SeekFrom::Start(8 + index as u64 * 20))?;
            let mut cpu = [0; 4];
            file.read_exact(&mut cpu)?;
            if u32::from_be_bytes(cpu) == wanted {
                return Ok(());
            }
        }
        return Err(RuntimeError::UnsupportedTarget);
    }
    let matched = match target {
        Target::MacosArm64 => head[..4] == [0xcf, 0xfa, 0xed, 0xfe] && head[4..8] == [12, 0, 0, 1],
        Target::MacosX64 => head[..4] == [0xcf, 0xfa, 0xed, 0xfe] && head[4..8] == [7, 0, 0, 1],
        Target::WindowsX64 => {
            use std::io::{Seek, SeekFrom};
            let offset = u32::from_le_bytes(head[60..64].try_into().unwrap());
            if head[..2] != *b"MZ" || offset > 16 * 1024 * 1024 {
                false
            } else {
                file.seek(SeekFrom::Start(offset as u64))?;
                let mut pe = [0; 6];
                file.read_exact(&mut pe)
                    .map_err(|_| RuntimeError::UnsupportedTarget)?;
                pe == [b'P', b'E', 0, 0, 0x64, 0x86]
            }
        }
    };
    if matched {
        Ok(())
    } else {
        Err(RuntimeError::UnsupportedTarget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_interrupts_a_pending_network_future() {
        let cancel = Cancellation::default();
        let worker = cancel.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            worker.cancel();
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            runtime.block_on(cancellable(std::future::pending::<()>(), &cancel)),
            Err(RuntimeError::Cancelled)
        );
        thread.join().unwrap();
    }
    #[test]
    fn disk_full_write_is_a_stable_error() {
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::StorageFull))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(
            copy_checked(
                &mut &b"data"[..],
                &mut Full,
                100,
                &Cancellation::default(),
                |_| {}
            ),
            Err(RuntimeError::DiskFull)
        );
    }
    #[test]
    fn every_runtime_error_has_three_catalog_translations() {
        let errors = [
            RuntimeError::InvalidPackage,
            RuntimeError::UnsupportedTarget,
            RuntimeError::IntegrityMismatch,
            RuntimeError::UnsafeArchive,
            RuntimeError::LimitExceeded,
            RuntimeError::Cancelled,
            RuntimeError::DownloadFailed,
            RuntimeError::Io,
            RuntimeError::DiskFull,
            RuntimeError::Busy,
            RuntimeError::PackageConflict,
            RuntimeError::NotInstalled,
            RuntimeError::ExecutableChanged,
            RuntimeError::SpawnFailed,
            RuntimeError::ReadinessTimeout,
            RuntimeError::ProcessExited,
            RuntimeError::InvalidProcess,
            RuntimeError::LogFailed,
        ];
        for json in [
            include_str!("../../../shared/locales/en.json"),
            include_str!("../../../shared/locales/ru.json"),
            include_str!("../../../shared/locales/uz-Latn.json"),
        ] {
            let catalog: serde_json::Value = serde_json::from_str(json).unwrap();
            for error in errors {
                assert!(
                    catalog["runtime"]["errors"][error.to_string()]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())
                );
            }
        }
    }
}
