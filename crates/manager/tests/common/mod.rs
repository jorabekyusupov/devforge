//! Disposable manager fixture roots and sanitized native security diagnostics.
//! Only a newly allocated Windows TempDir's owner may be normalized. Production
//! paths, ACLs, security policy and process tokens are never changed.
pub fn root(prefix: &str) -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    // Darwin's sockaddr_un payload is limited to 103 bytes.
    #[cfg(unix)]
    let root = builder.tempdir_in("/tmp").unwrap();
    #[cfg(not(unix))]
    let root = builder.tempdir().unwrap();
    #[cfg(windows)]
    {
        diagnose("new_root_before_owner_normalization", root.path());
        let normalized = windows::normalize_new_root_owner(&root);
        diagnose("new_root_after_owner_normalization", root.path());
        assert!(normalized.is_ok(), "MANAGER_FIXTURE_OWNER_NORMALIZED=false");
    }
    root
}

/// New disposable fixture files only. Never adopts a file, repairs an ACL or
/// prints its credential bytes. Windows hosted runners may use an elevated
/// default owner; set the current TokenUser explicitly at exclusive creation.
#[allow(dead_code)]
pub fn credential_file(path: &std::path::Path, bytes: &[u8]) {
    use std::io::Write;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap()
    };
    #[cfg(windows)]
    let mut file = windows::new_credential_file(path);
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

/// Preserve acquisition semantics and emit sanitized context before a fixture's
/// expected-success unwrap. Other test binaries may only need `root`.
#[allow(dead_code)]
pub fn acquire(path: &std::path::Path) -> devforge_manager::Result<devforge_manager::Owner> {
    let result = devforge_manager::Owner::acquire(path);
    #[cfg(windows)]
    if result.is_err() {
        diagnose("failed_acquire_root", path);
        let control = path.join("control");
        if control.exists() {
            diagnose("failed_acquire_control", &control);
        } else {
            eprintln!("MANAGER_FIXTURE_SECURITY failed_acquire_control: exists=false");
        }
    }
    result
}

#[cfg(windows)]
pub fn diagnose(label: &str, path: &std::path::Path) {
    // Labels are static fixture stages, never paths, SIDs or request contents.
    match windows::inspect(path) {
        Ok(state) => eprintln!(
            "MANAGER_FIXTURE_SECURITY {label}: directory={} reparse={} owner_matches_user={} dacl_present={} dacl_protected={} ace_count={} first_ace_allowed={} first_ace_inherits_files_and_dirs={} first_ace_full_access={} first_ace_matches_user={}",
            state.directory,
            state.reparse,
            state.owner_matches_user,
            state.dacl_present,
            state.dacl_protected,
            state.ace_count,
            state.first_ace_allowed,
            state.first_ace_inherits_files_and_dirs,
            state.first_ace_full_access,
            state.first_ace_matches_user,
        ),
        Err(()) => eprintln!("MANAGER_FIXTURE_SECURITY {label}: inspection_ok=false"),
    }
}

#[cfg(windows)]
mod windows {
    use std::{
        fs,
        os::windows::{ffi::OsStrExt, fs::MetadataExt},
        path::Path,
    };
    use windows_sys::Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{SystemServices::ACCESS_ALLOWED_ACE_TYPE, Threading::*},
    };

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct LocalMemory(PSECURITY_DESCRIPTOR);
    impl Drop for LocalMemory {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    pub fn new_credential_file(path: &Path) -> fs::File {
        use std::os::windows::io::FromRawHandle;
        let user = current_user_buffer().expect("fixture current user");
        let sid = unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        let mut sid_text = std::ptr::null_mut();
        assert_ne!(
            unsafe { ConvertSidToStringSidW(sid, &mut sid_text) },
            0,
            "fixture SID encoding"
        );
        let _sid_memory = LocalMemory(sid_text.cast());
        let mut length = 0;
        while unsafe { *sid_text.add(length) } != 0 {
            length += 1;
            assert!(length <= 256);
        }
        let user =
            String::from_utf16(unsafe { std::slice::from_raw_parts(sid_text, length) }).unwrap();
        let sddl = format!("O:{user}D:P(A;;FA;;;{user})")
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut sd = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut sd,
                    std::ptr::null_mut(),
                )
            },
            0,
            "fixture private descriptor"
        );
        let sd = LocalMemory(sd);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let name = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_WRITE | GENERIC_READ,
                FILE_SHARE_READ,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(
            handle, INVALID_HANDLE_VALUE,
            "fixture exclusive file creation"
        );
        unsafe { fs::File::from_raw_handle(handle) }
    }
    #[derive(Default)]
    pub struct State {
        pub directory: bool,
        pub reparse: bool,
        pub owner_matches_user: bool,
        pub dacl_present: bool,
        pub dacl_protected: bool,
        pub ace_count: u16,
        pub first_ace_allowed: bool,
        pub first_ace_inherits_files_and_dirs: bool,
        pub first_ace_full_access: bool,
        pub first_ace_matches_user: bool,
    }
    fn current_user_buffer() -> Result<Vec<usize>, ()> {
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(());
        }
        let token = Handle(token);
        let mut length = 0;
        unsafe {
            GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut length);
        }
        if length == 0 || length > 65536 {
            return Err(());
        }
        let mut user = vec![0usize; (length as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                user.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(());
        }
        Ok(user)
    }

    /// Takes the fresh TempDir itself, not an arbitrary caller-supplied path.
    /// Hosted runners can create a default owner different from TokenUser;
    /// normalize only this disposable fixture to the ordinary app-root owner.
    pub(super) fn normalize_new_root_owner(root: &tempfile::TempDir) -> Result<(), ()> {
        let path = root.path();
        let before = inspect(path)?;
        if !before.directory
            || before.reparse
            || fs::read_dir(path).map_err(|_| ())?.next().is_some()
        {
            return Err(());
        }
        if !before.owner_matches_user {
            // Negative regression uses the observed original foreign owner, not
            // an invented permissive fixture. Production must reject before
            // writing its private control directory or any installer lease.
            assert!(
                matches!(
                    devforge_manager::Owner::acquire(path),
                    Err(devforge_manager::Error::UnsafeEndpoint)
                ),
                "MANAGER_FIXTURE_FOREIGN_OWNER_REJECTED=false"
            );
            assert!(
                fs::read_dir(path).map_err(|_| ())?.next().is_none(),
                "MANAGER_FIXTURE_FOREIGN_OWNER_LEFT_ROOT_EMPTY=false"
            );
            eprintln!(
                "MANAGER_FIXTURE_SECURITY original_foreign_owner: negative_case_exercised=true rejected=true root_still_empty=true"
            );
            // Recheck the disposable directory immediately before the narrow
            // owner-only API. Never follow a reparse point or change any DACL.
            let meta = fs::symlink_metadata(path).map_err(|_| ())?;
            if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(());
            }
            let user = current_user_buffer()?;
            let sid = unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
            let name = path
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            if unsafe {
                SetNamedSecurityInfoW(
                    name.as_ptr(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION,
                    sid,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } != 0
            {
                return Err(());
            }
        } else {
            eprintln!(
                "MANAGER_FIXTURE_SECURITY original_foreign_owner: negative_case_exercised=false owner_already_matches=true"
            );
        }
        let after = inspect(path)?;
        if !after.directory || after.reparse || !after.owner_matches_user {
            return Err(());
        }
        Ok(())
    }

    pub fn inspect(path: &Path) -> Result<State, ()> {
        let meta = fs::symlink_metadata(path).map_err(|_| ())?;
        let user = current_user_buffer()?;
        let sid = unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
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
            return Err(());
        }
        let descriptor = LocalMemory(descriptor);
        let mut control = 0;
        let mut revision = 0;
        if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
            return Err(());
        }
        let mut state = State {
            directory: meta.is_dir(),
            reparse: meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0,
            owner_matches_user: !owner.is_null() && unsafe { EqualSid(owner, sid) } != 0,
            dacl_present: !dacl.is_null(),
            dacl_protected: control & SE_DACL_PROTECTED != 0,
            ace_count: if dacl.is_null() {
                0
            } else {
                unsafe { (*dacl).AceCount }
            },
            ..State::default()
        };
        if state.ace_count > 0 {
            let mut ace = std::ptr::null_mut();
            if unsafe { GetAce(dacl, 0, &mut ace) } == 0 || ace.is_null() {
                return Err(());
            }
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            state.first_ace_allowed = header.AceType == ACCESS_ALLOWED_ACE_TYPE as u8;
            state.first_ace_inherits_files_and_dirs =
                header.AceFlags == (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8;
            if state.first_ace_allowed {
                let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
                state.first_ace_full_access = allowed.Mask == FILE_ALL_ACCESS;
                state.first_ace_matches_user =
                    unsafe { EqualSid((&allowed.SidStart as *const u32).cast_mut().cast(), sid) }
                        != 0;
            }
        }
        Ok(state)
    }
}
