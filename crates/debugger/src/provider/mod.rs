//! Reviewed Windows DLLs and app-attested macOS source builds. Normal PHP pins stay unchanged.
use crate::{Error, Result, protocol::constant_time};
use devforge_core::SecretStore;
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, PackageStore, Target, VerifiedExecutable, VerifiedResource},
};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
pub const RECIPE: &str = include_str!("recipe.json");
pub const BUILDER: &str = include_str!("build_runtime.py");
pub const XDEBUG: &str = "3.5.3";
const SIGNING: &str = "debug-runtime/source-build-attestation-v1";
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn recipe_hash() -> String {
    digest(format!("{RECIPE}\n{BUILDER}").as_bytes())
}
fn file_hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub php: String,
    pub xdebug: String,
    pub target: Target,
    pub source_build: bool,
    pub recipe: String,
    pub package: Package,
    pub signature: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub id: String,
    pub php: String,
    pub xdebug: String,
    pub target: Target,
    pub source_build: bool,
    pub installed: bool,
}
impl Entry {
    pub fn info(&self, packages: &PackageStore) -> Info {
        Info {
            id: self.id.clone(),
            php: self.php.clone(),
            xdebug: self.xdebug.clone(),
            target: self.target,
            source_build: self.source_build,
            installed: packages.is_present(&self.package),
        }
    }
}
pub struct Runtime {
    pub entry: Entry,
    pub php: Option<VerifiedExecutable>,
    pub web: Option<VerifiedExecutable>,
    pub extension: VerifiedResource,
}
fn root(store: &Path) -> Result<PathBuf> {
    let root = store.join("debug-runtimes");
    if !root.exists() {
        fs::create_dir(&root)?;
    }
    if fs::symlink_metadata(&root)?.file_type().is_symlink() || fs::canonicalize(&root)? != root {
        return Err(Error::Unauthorized);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    }
    Ok(root)
}
fn unsigned(entry: &Entry) -> Vec<u8> {
    let mut e = entry.clone();
    e.signature.clear();
    serde_json::to_vec(&e).unwrap()
}
fn key(secrets: &dyn SecretStore, create: bool) -> Result<zeroize::Zeroizing<String>> {
    let value = secrets.get(SIGNING).map_err(|_| Error::Unavailable)?;
    match value {
        Some(v) => Ok(zeroize::Zeroizing::new(v)),
        None if create => {
            let v = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            secrets.set(SIGNING, &v).map_err(|_| Error::Unavailable)?;
            Ok(zeroize::Zeroizing::new(v))
        }
        None => Err(Error::Unauthorized),
    }
}
fn signature(entry: &Entry, key: &str) -> Result<String> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(key.as_bytes())
        .map_err(|_| Error::Unauthorized)?;
    mac.update(&unsigned(entry));
    Ok(mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
pub fn entries(store: &Path, secrets: &dyn SecretStore) -> Result<Vec<Entry>> {
    let root = root(store)?;
    let mut out = vec![];
    for file in fs::read_dir(&root)? {
        let file = file?;
        if file.path().extension().is_none_or(|s| s != "json") {
            continue;
        }
        let meta = fs::symlink_metadata(file.path())?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16384 {
            return Err(Error::Unauthorized);
        }
        let e: Entry =
            serde_json::from_slice(&fs::read(file.path())?).map_err(|_| Error::Unauthorized)?;
        let token = key(secrets, false)?;
        if e.id.len() != 32
            || !e.id.bytes().all(|b| b.is_ascii_hexdigit())
            || (e.recipe.len() != 64 || !e.recipe.bytes().all(|b| b.is_ascii_hexdigit()))
            || e.xdebug.is_empty()
            || e.xdebug.len() > 40
            || !constant_time(&e.signature, &signature(&e, &token)?)
        {
            return Err(Error::Unauthorized);
        }
        e.package.key().map_err(|_| Error::Unauthorized)?;
        out.push(e);
        if out.len() > 100 {
            return Err(Error::Unavailable);
        }
    }
    Ok(out)
}
pub fn resolve(
    store: &Path,
    id: &str,
    php: &str,
    packages: &PackageStore,
    secrets: &dyn SecretStore,
) -> Result<Runtime> {
    let entry = entries(store, secrets)?
        .into_iter()
        .find(|e| e.id == id && e.php == php && Some(e.target) == Target::current().ok())
        .ok_or(Error::RuntimeMissing)?;
    let extension = packages
        .resource(&entry.package, "xdebug")
        .map_err(|_| Error::RuntimeMissing)?;
    extension.check().map_err(|_| Error::RuntimeIncompatible)?;
    let (php, web) = if entry.source_build {
        (
            Some(
                packages
                    .executable(&entry.package, "php")
                    .map_err(|_| Error::RuntimeMissing)?,
            ),
            Some(
                packages
                    .executable(&entry.package, "php-fpm")
                    .map_err(|_| Error::RuntimeMissing)?,
            ),
        )
    } else {
        (None, None)
    };
    Ok(Runtime {
        entry,
        php,
        web,
        extension,
    })
}
fn register(store: &Path, mut entry: Entry, secrets: &dyn SecretStore) -> Result<Entry> {
    let token = key(secrets, true)?;
    entry.signature = signature(&entry, &token)?;
    let path = root(store)?.join(format!("{}.json", entry.id));
    let mut file = tempfile::NamedTempFile::new_in(root(store)?)?;
    file.write_all(&serde_json::to_vec_pretty(&entry).unwrap())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|_| Error::Unavailable)?;
    Ok(entry)
}
fn entry(php: &str, package: Package, source_build: bool) -> Entry {
    let id = digest(&serde_json::to_vec(&package).unwrap())[..32].to_string();
    Entry {
        id,
        php: php.into(),
        xdebug: XDEBUG.into(),
        target: package.target,
        source_build,
        recipe: recipe_hash(),
        package,
        signature: String::new(),
    }
}
pub fn windows(php: &str) -> Result<Package> {
    let minor = php.rsplit_once('.').ok_or(Error::RuntimeIncompatible)?.0;
    let hash = match minor {
        "8.4" => "a94d4fcfe4086a300b2decaf7c04e59f4981aa5e56992d48b55c5e49dfd9b9ee",
        "8.5" => "0a0d52bf276dad7e5f90ac30a5110b3119d291b20c118be0cb7f5752064f1811",
        _ => return Err(Error::RuntimeIncompatible),
    };
    Ok(Package {
        kind: "xdebug".into(),
        version: XDEBUG.into(),
        variant: format!("php{minor}-nts-vs17"),
        target: Target::WindowsX64,
        url: format!("https://xdebug.org/files/php_xdebug-{XDEBUG}-{minor}-nts-vs17-x86_64.dll"),
        sha256: hash.into(),
        archive: Archive::File {
            path: "xdebug.dll".into(),
        },
        executables: BTreeMap::new(),
        resources: BTreeMap::from([("xdebug".into(), "xdebug.dll".into())]),
    })
}
pub fn install(
    store: &Path,
    php: &str,
    packages: &PackageStore,
    secrets: &dyn SecretStore,
    trusted: bool,
    cancel: &Cancellation,
) -> Result<Entry> {
    if !trusted {
        return Err(Error::Unauthorized);
    }
    if let Some(e) = entries(store, secrets)?.into_iter().find(|e| {
        e.php == php
            && e.xdebug == XDEBUG
            && e.recipe == recipe_hash()
            && Some(e.target) == Target::current().ok()
            && e.info(packages).installed
    }) {
        return Ok(e);
    }
    let target = Target::current().map_err(|_| Error::Unavailable)?;
    if target == Target::WindowsX64 {
        let package = windows(php)?;
        packages.download(&package, cancel, |_| {}).map_err(|_| {
            if cancel.is_cancelled() {
                Error::Cancelled
            } else {
                Error::BuildFailed
            }
        })?;
        return register(store, entry(php, package, false), secrets);
    }
    let recipe: serde_json::Value = serde_json::from_str(RECIPE).unwrap();
    if recipe["php"].get(php).is_none() {
        return Err(Error::RuntimeIncompatible);
    }
    if !Path::new("/Library/Developer/CommandLineTools/usr/bin/clang").is_file() && !Path::new("/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/clang").is_file(){return Err(Error::BuildFailed);}
    let directory = root(store)?;
    let _lock = {
        if fs::symlink_metadata(directory.join("build.lock"))
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(Error::Unauthorized);
        }
        let f = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("build.lock"))?;
        f.try_lock().map_err(|_| Error::Busy)?;
        f
    };
    let stage = tempfile::tempdir_in(&directory)?;
    // Own the separate short compiler prefix too; Rust removes it on cancellation.
    let work = tempfile::Builder::new()
        .prefix("devforge-xdebug-build-")
        .tempdir_in("/private/tmp")?;
    let source = stage.path().join("builder.py");
    let metadata = stage.path().join("recipe.json");
    let archive = stage.path().join("runtime.zip");
    fs::write(&source, BUILDER)?;
    fs::write(&metadata, RECIPE)?;
    let name = serde_json::to_value(target)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    let mut command = Command::new("/usr/bin/python3");
    command
        .args([
            source.as_os_str(),
            metadata.as_os_str(),
            std::ffi::OsStr::new(php),
            std::ffi::OsStr::new(&name),
            archive.as_os_str(),
        ])
        .arg("--cache")
        .arg(directory.join("build-inputs"))
        .arg("--work-dir")
        .arg(work.path())
        .env_clear();
    for name in ["HOME", "TMPDIR", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"] {
        if let Some(v) = std::env::var_os(name) {
            command.env(name, v);
        }
    }
    command.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    let result = devforge_runtime::process::host::run(
        &mut command,
        Duration::from_secs(3600),
        16 * 1024 * 1024,
        cancel,
    );
    for suffix in ["build.log", "progress.json"] {
        let source = archive.with_extension(suffix);
        if source.is_file() && fs::metadata(&source)?.len() <= 32 * 1024 * 1024 {
            fs::copy(source, directory.join(format!("last-{suffix}")))?;
        }
    }
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    if result.map_err(|_| Error::BuildFailed)?.exit_code != Some(0) {
        return Err(Error::BuildFailed);
    }
    install_archive(store, php, target, &archive, packages, secrets, cancel)
}
fn install_archive(
    store: &Path,
    php: &str,
    target: Target,
    archive: &Path,
    packages: &PackageStore,
    secrets: &dyn SecretStore,
    cancel: &Cancellation,
) -> Result<Entry> {
    let hash = file_hash(archive)?;
    let package = Package {
        kind: "php-debug".into(),
        version: php.into(),
        variant: format!("xdebug-{XDEBUG}-{}-{}", &recipe_hash()[..12], &hash[..12]),
        target,
        url: format!(
            "https://www.php.net/distributions/php-{php}.tar.xz#devforge-local-source-build"
        ),
        sha256: hash,
        archive: Archive::Zip,
        executables: BTreeMap::from([
            ("php".into(), "php".into()),
            ("php-fpm".into(), "php-fpm".into()),
        ]),
        resources: BTreeMap::from([
            ("xdebug".into(), "xdebug.so".into()),
            ("provenance".into(), "provenance.json".into()),
        ]),
    };
    packages
        .install_cached(&package, archive, cancel, |_| {})
        .map_err(|_| Error::BuildFailed)?;
    register(store, entry(php, package, true), secrets)
}
#[cfg(feature = "test-support")]
pub fn install_fixture_archive(
    store: &Path,
    php: &str,
    archive: &Path,
    packages: &PackageStore,
    secrets: &dyn SecretStore,
    cancel: &Cancellation,
) -> Result<Entry> {
    install_archive(
        store,
        php,
        Target::current().map_err(|_| Error::Unavailable)?,
        archive,
        packages,
        secrets,
        cancel,
    )
}
