//! Test-support acceptance fixture only; never a production credential store.
//! FixtureSecrets alone does not prove that a root is disposable or establish a
//! Windows directory DACL. Its constructor is lazy/no-I/O. FixtureRoot adds the
//! explicit new-disposable-root/marker boundary for debug native composition;
//! callers of older helpers still choose their own isolated acceptance roots.
use devforge_core::{ErrorCode, SecretStore};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{MapAccess, Visitor},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

pub const MAX_FILE_BYTES: usize = 1_048_576;
pub const MAX_KEYS: usize = 1024;
pub const MAX_KEY_BYTES: usize = 256;
pub const MAX_VALUE_BYTES: usize = 65_536;
const LOCK_WAIT: Duration = Duration::from_secs(5);
type Result<T> = std::result::Result<T, ErrorCode>;
pub const FIXTURE_MARKER: &str = ".devforge-disposable-e2e.json";
const MAX_MARKER_BYTES: usize = 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureMarker {
    schema: u32,
    kind: String,
    canonical_root_sha256: String,
    filesystem_identity: String,
}
/// Explicit debug acceptance root proof, not a production credential authority.
/// Initialization creates a NEW private leaf only; validation never repairs or
/// adopts an arbitrary existing directory, or changes native OS permissions.
pub struct FixtureRoot {
    root: Root,
}
impl FixtureRoot {
    pub fn create_new(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(ErrorCode::SecretUnavailable);
        }
        let parent = inspect_root(path.parent().ok_or(ErrorCode::SecretUnavailable)?)?;
        let name = path.file_name().ok_or(ErrorCode::SecretUnavailable)?;
        let path = parent.path.join(name);
        create_private_fixture_directory(&path)?;
        let current_parent = inspect_root(&parent.path)?;
        if current_parent.path != parent.path || current_parent.identity != parent.identity {
            return Err(ErrorCode::SecretUnavailable);
        }
        let root = validate_fixture_directory(&path)?;
        if fs::read_dir(&root.path)
            .map_err(unavailable)?
            .next()
            .is_some()
        {
            return Err(ErrorCode::SecretUnavailable);
        }
        let marker = FixtureMarker {
            schema: 1,
            kind: "devforge-disposable-e2e-v1".into(),
            canonical_root_sha256: fixture_path_hash(&root.path),
            filesystem_identity: root.identity,
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        secure_options(&mut options);
        let mut file = options
            .open(root.path.join(FIXTURE_MARKER))
            .map_err(unavailable)?;
        validate_file(&file)?;
        serde_json::to_writer(&mut file, &marker).map_err(unavailable)?;
        file.sync_all().map_err(unavailable)?;
        #[cfg(unix)]
        File::open(&root.path)
            .and_then(|file| file.sync_all())
            .map_err(unavailable)?;
        Self::validate(&root.path)
    }
    pub fn validate(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(ErrorCode::SecretUnavailable);
        }
        let root = validate_fixture_directory(path)?;
        let file =
            open_existing(&root.path.join(FIXTURE_MARKER))?.ok_or(ErrorCode::SecretUnavailable)?;
        if file.metadata().map_err(unavailable)?.len() > MAX_MARKER_BYTES as u64 {
            return Err(ErrorCode::SecretUnavailable);
        }
        let mut bytes = Vec::new();
        file.take((MAX_MARKER_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(unavailable)?;
        if bytes.len() > MAX_MARKER_BYTES {
            return Err(ErrorCode::SecretUnavailable);
        }
        let marker: FixtureMarker = serde_json::from_slice(&bytes).map_err(unavailable)?;
        if marker.schema != 1
            || marker.kind != "devforge-disposable-e2e-v1"
            || marker.canonical_root_sha256 != fixture_path_hash(&root.path)
            || marker.filesystem_identity != root.identity
        {
            return Err(ErrorCode::SecretUnavailable);
        }
        let again = validate_fixture_directory(path)?;
        if again.path != root.path || again.identity != root.identity {
            return Err(ErrorCode::SecretUnavailable);
        }
        Ok(Self { root })
    }
    pub fn root(&self) -> &Path {
        &self.root.path
    }
    pub fn project_store_path(&self) -> PathBuf {
        self.root.path.join("e2e-secrets.json")
    }
    pub fn control_store_path(&self) -> PathBuf {
        self.root.path.join("e2e-control-secrets.json")
    }
}
fn fixture_path_hash(path: &Path) -> String {
    let mut hash = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hash.update(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for unit in path.as_os_str().encode_wide() {
            hash.update(unit.to_le_bytes());
        }
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn validate_fixture_directory(path: &Path) -> Result<Root> {
    let root = inspect_root(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if fs::metadata(&root.path).map_err(unavailable)?.mode() & 0o077 != 0 {
            return Err(ErrorCode::SecretUnavailable);
        }
    }
    #[cfg(windows)]
    fixture_windows::validate_directory(&root.path)?;
    Ok(root)
}
fn create_private_fixture_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(unavailable)
    }
    #[cfg(windows)]
    {
        fixture_windows::create_directory(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(ErrorCode::SecretUnavailable)
    }
}
#[cfg(windows)]
mod fixture_windows {
    use super::*;
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    use windows_sys::Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{SystemServices::ACCESS_ALLOWED_ACE_TYPE, Threading::*},
    };
    struct Memory(*mut core::ffi::c_void);
    impl Drop for Memory {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    LocalFree(self.0);
                }
            }
        }
    }
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }
    fn sid_text(sid: PSID) -> Result<String> {
        let mut text = std::ptr::null_mut();
        if sid.is_null() || unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err(ErrorCode::SecretUnavailable);
        }
        let memory = Memory(text.cast());
        let mut length = 0;
        while unsafe { *text.add(length) } != 0 {
            length += 1;
            if length > 256 {
                return Err(ErrorCode::SecretUnavailable);
            }
        }
        let result = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
            .map_err(unavailable)?;
        drop(memory);
        Ok(result)
    }
    fn current_user() -> Result<String> {
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(ErrorCode::SecretUnavailable);
        }
        let token = Handle(token);
        let mut length = 0;
        unsafe {
            GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut length);
        }
        if length == 0 || length > 65536 {
            return Err(ErrorCode::SecretUnavailable);
        }
        let mut bytes = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                bytes.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(ErrorCode::SecretUnavailable);
        }
        sid_text(unsafe { (*(bytes.as_ptr().cast::<TOKEN_USER>())).User.Sid })
    }
    pub(super) fn create_directory(path: &Path) -> Result<()> {
        let user = current_user()?;
        let sddl = format!("O:{user}D:P(A;OICI;FA;;;{user})")
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut descriptor = std::ptr::null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(ErrorCode::SecretUnavailable);
        }
        let descriptor = Memory(descriptor);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let name = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
            return Err(ErrorCode::SecretUnavailable);
        }
        validate_directory(path)
    }
    pub(super) fn validate_directory(path: &Path) -> Result<()> {
        let meta = fs::symlink_metadata(path).map_err(unavailable)?;
        if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(ErrorCode::SecretUnavailable);
        }
        let name = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut descriptor = std::ptr::null_mut();
        let mut owner = std::ptr::null_mut();
        let mut dacl = std::ptr::null_mut();
        if unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        } != 0
        {
            return Err(ErrorCode::SecretUnavailable);
        }
        let descriptor = Memory(descriptor);
        let user = current_user()?;
        if owner.is_null() || dacl.is_null() || sid_text(owner)? != user {
            return Err(ErrorCode::SecretUnavailable);
        }
        let mut control = 0;
        let mut revision = 0;
        if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0
            || control & SE_DACL_PROTECTED == 0
            || unsafe { (*dacl).AceCount } != 1
        {
            return Err(ErrorCode::SecretUnavailable);
        }
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(dacl, 0, &mut ace) } == 0 || ace.is_null() {
            return Err(ErrorCode::SecretUnavailable);
        }
        let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        if allowed.Header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
            || allowed.Header.AceFlags != (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8
            || allowed.Mask != FILE_ALL_ACCESS
            || sid_text((&allowed.SidStart as *const u32).cast_mut().cast())? != user
        {
            return Err(ErrorCode::SecretUnavailable);
        }
        Ok(())
    }
}
fn unavailable<T>(_: T) -> ErrorCode {
    ErrorCode::SecretUnavailable
}

#[derive(Clone)]
struct Root {
    path: PathBuf,
    identity: String,
}
pub struct FixtureSecrets {
    path: PathBuf,
    root: Mutex<Option<Root>>,
}
struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        // Release the logical transaction even with fork/duplicate descriptors.
        let _ = self.0.unlock();
    }
}
impl FixtureSecrets {
    pub fn open(path: PathBuf) -> Self {
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map(|root| root.join(path))
                .unwrap_or_default()
        };
        Self {
            path,
            root: Mutex::new(None),
        }
    }
    fn binding(&self) -> Result<Root> {
        let current = inspect_root(self.path.parent().ok_or(ErrorCode::SecretUnavailable)?)?;
        let mut root = self.root.lock().map_err(unavailable)?;
        match root.as_ref() {
            Some(previous)
                if previous.path != current.path || previous.identity != current.identity =>
            {
                Err(ErrorCode::SecretUnavailable)
            }
            Some(previous) => Ok(previous.clone()),
            None => {
                *root = Some(current.clone());
                Ok(current)
            }
        }
    }
    fn locked<T>(
        &self,
        operation: impl FnOnce(&Root, &Path, &mut BTreeMap<String, String>) -> Result<T>,
    ) -> Result<T> {
        let root = self.binding()?;
        let name = self
            .path
            .file_name()
            .filter(|name| !name.is_empty() && name.len() <= 128)
            .ok_or(ErrorCode::SecretUnavailable)?;
        let path = root.path.join(name);
        let mut lock_name = name.to_os_string();
        lock_name.push(".lock");
        let lock_path = root.path.join(lock_name);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        secure_options(&mut options);
        reject_link(&lock_path)?;
        let file = options.open(&lock_path).map_err(unavailable)?;
        validate_file(&file)?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(std::fs::TryLockError::WouldBlock) if started.elapsed() < LOCK_WAIT => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return Err(ErrorCode::SecretUnavailable),
            }
        }
        let _lock = Lock(file);
        let current = inspect_root(self.path.parent().ok_or(ErrorCode::SecretUnavailable)?)?;
        if current.path != root.path || current.identity != root.identity {
            return Err(ErrorCode::SecretUnavailable);
        }
        let mut values = load(&path)?;
        operation(&root, &path, &mut values)
    }
    fn save(root: &Root, path: &Path, values: &BTreeMap<String, String>) -> Result<()> {
        validate_values(values)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&root.path).map_err(unavailable)?;
        {
            let mut writer = BoundedWriter {
                inner: temporary.as_file_mut(),
                bytes: 0,
            };
            serde_json::to_writer(&mut writer, values).map_err(unavailable)?;
        }
        temporary.as_file().sync_all().map_err(unavailable)?;
        // No existing target attribute/permission repair or malformed-file reset.
        drop(open_existing(path)?);
        let current = inspect_root(&root.path)?;
        if current.path != root.path || current.identity != root.identity {
            return Err(ErrorCode::SecretUnavailable);
        }
        temporary.persist(path).map_err(unavailable)?;
        #[cfg(unix)]
        File::open(&root.path)
            .and_then(|directory| directory.sync_all())
            .map_err(unavailable)?;
        Ok(())
    }
}
impl SecretStore for FixtureSecrets {
    fn get(&self, key: &str) -> Result<Option<String>> {
        validate_key(key)?;
        self.locked(|_, _, values| Ok(values.get(key).cloned()))
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        validate_key(key)?;
        if value.len() > MAX_VALUE_BYTES {
            return Err(ErrorCode::SecretUnavailable);
        }
        self.locked(|root, path, values| {
            values.insert(key.to_owned(), value.to_owned());
            Self::save(root, path, values)
        })
    }
    fn delete(&self, key: &str) -> Result<()> {
        validate_key(key)?;
        self.locked(|root, path, values| {
            values.remove(key);
            Self::save(root, path, values)
        })
    }
}
fn inspect_root(path: &Path) -> Result<Root> {
    let metadata = fs::symlink_metadata(path).map_err(unavailable)?;
    if !metadata.is_dir() || devforge_projects::discovery::is_link(&metadata) {
        return Err(ErrorCode::SecretUnavailable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            return Err(ErrorCode::SecretUnavailable);
        }
    }
    let path = fs::canonicalize(path).map_err(unavailable)?;
    let identity = devforge_projects::discovery::identity(&path).map_err(unavailable)?;
    if !identity.starts_with("file:") {
        return Err(ErrorCode::SecretUnavailable);
    }
    Ok(Root { path, identity })
}
fn secure_options(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
}
fn reject_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || devforge_projects::discovery::is_link(&metadata) => {
            Err(ErrorCode::SecretUnavailable)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ErrorCode::SecretUnavailable),
    }
}
fn validate_file(file: &File) -> Result<()> {
    let metadata = file.metadata().map_err(unavailable)?;
    if !metadata.is_file() {
        return Err(ErrorCode::SecretUnavailable);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(ErrorCode::SecretUnavailable);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            GetFileInformationByHandle,
        };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
            || info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT)
                != 0
            || info.nNumberOfLinks != 1
        {
            return Err(ErrorCode::SecretUnavailable);
        }
    }
    Ok(())
}
fn open_existing(path: &Path) -> Result<Option<File>> {
    reject_link(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    secure_options(&mut options);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ErrorCode::SecretUnavailable),
    };
    validate_file(&file)?;
    Ok(Some(file))
}
fn load(path: &Path) -> Result<BTreeMap<String, String>> {
    let Some(file) = open_existing(path)? else {
        return Ok(BTreeMap::new());
    };
    if file.metadata().map_err(unavailable)?.len() > MAX_FILE_BYTES as u64 {
        return Err(ErrorCode::SecretUnavailable);
    }
    let mut bytes = Vec::new();
    file.take((MAX_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(unavailable)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(ErrorCode::SecretUnavailable);
    }
    let Values(values) = serde_json::from_slice(&bytes).map_err(unavailable)?;
    Ok(values)
}
fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES {
        Err(ErrorCode::SecretUnavailable)
    } else {
        Ok(())
    }
}
fn validate_values(values: &BTreeMap<String, String>) -> Result<()> {
    if values.len() > MAX_KEYS {
        return Err(ErrorCode::SecretUnavailable);
    }
    for (key, value) in values {
        validate_key(key)?;
        if value.len() > MAX_VALUE_BYTES {
            return Err(ErrorCode::SecretUnavailable);
        }
    }
    Ok(())
}
struct Values(BTreeMap<String, String>);
impl<'de> Deserialize<'de> for Values {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Map;
        impl<'de> Visitor<'de> for Map {
            type Value = Values;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("bounded fixture key/value map")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut access: A,
            ) -> std::result::Result<Values, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = access.next_entry::<String, String>()? {
                    if values.len() >= MAX_KEYS
                        || validate_key(&key).is_err()
                        || value.len() > MAX_VALUE_BYTES
                        || values.insert(key, value).is_some()
                    {
                        return Err(serde::de::Error::custom("invalid fixture map"));
                    }
                }
                Ok(Values(values))
            }
        }
        deserializer.deserialize_map(Map)
    }
}
struct BoundedWriter<'a> {
    inner: &'a mut File,
    bytes: usize,
}
impl Write for BoundedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FILE_BYTES.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("fixture file limit"));
        }
        let written = self.inner.write(bytes)?;
        self.bytes += written;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
#[cfg(test)]
mod lock_tests {
    use super::*;
    #[test]
    fn fixture_lock_logically_unlocks_while_duplicate_descriptor_survives() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("fixture.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        secure_options(&mut options);
        let file = options.open(&path).unwrap();
        file.try_lock().unwrap();
        let lock = Lock(file);
        let duplicate = lock.0.try_clone().unwrap();
        drop(lock);
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        secure_options(&mut options);
        let next = options.open(&path).unwrap();
        next.try_lock().unwrap();
        let next = Lock(next);
        drop(duplicate);
        let contender = options.open(&path).unwrap();
        assert!(matches!(
            contender.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(next);
        contender.try_lock().unwrap();
        drop(Lock(contender));
    }
}
