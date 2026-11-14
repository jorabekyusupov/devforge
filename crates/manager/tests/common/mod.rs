//! Disposable manager fixture roots and sanitized native security diagnostics.
//! This helper never changes production paths, ownership, ACLs or process tokens.
pub fn root(prefix: &str) -> tempfile::TempDir {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    // Darwin's sockaddr_un payload is limited to 103 bytes.
    #[cfg(unix)]
    let root = builder.tempdir_in("/tmp").unwrap();
    #[cfg(not(unix))]
    let root = builder.tempdir().unwrap();
    #[cfg(windows)]
    diagnose("new_root", root.path());
    root
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
    pub fn inspect(path: &Path) -> Result<State, ()> {
        let meta = fs::symlink_metadata(path).map_err(|_| ())?;
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
