//! Test harness only. Hosted runners are elevated; PostgreSQL deliberately
//! refuses an enabled Admin/Power User token. Reduce the existing token for the
//! child fixture without accounts, filesystem ACL changes, UAC changes, or
//! bypass flags. Its new token's default DACL retains access for its own user.
use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::OsStrExt,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::*,
    System::{Console::*, Threading::*},
};
struct Owned(HANDLE);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn relaunch() -> Result<Option<i32>, Box<dyn std::error::Error>> {
    if !devforge_projects::database::administrator_enabled()? {
        return Ok(None);
    }
    if std::env::args().any(|a| a == "--restricted-fixture-child") {
        return Err("Fixture administrative groups were not disabled".into());
    }
    let mut original = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut original) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let original = Owned(original);
    let mut admins = [0u32; 17];
    let mut power = [0u32; 17];
    for (kind, buffer) in [
        (WinBuiltinAdministratorsSid, &mut admins),
        (WinBuiltinPowerUsersSid, &mut power),
    ] {
        let mut bytes = std::mem::size_of_val(buffer) as u32;
        if unsafe {
            CreateWellKnownSid(
                kind,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    let disabled = [
        SID_AND_ATTRIBUTES {
            Sid: admins.as_mut_ptr().cast(),
            Attributes: 0,
        },
        SID_AND_ATTRIBUTES {
            Sid: power.as_mut_ptr().cast(),
            Attributes: 0,
        },
    ];
    let mut token = std::ptr::null_mut();
    if unsafe {
        CreateRestrictedToken(
            original.0,
            DISABLE_MAX_PRIVILEGE,
            disabled.len() as u32,
            disabled.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            &mut token,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let token = Owned(token);
    owner_default_dacl(token.0)?;
    let program = std::env::current_exe()?;
    let mut application: Vec<u16> = program.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut args = vec![program.into_os_string()];
    args.extend(std::env::args_os().skip(1));
    args.push(OsString::from("--restricted-fixture-child"));
    let mut command = Vec::new();
    for arg in args {
        if !command.is_empty() {
            command.push(32);
        }
        command.extend(quote(&arg));
    }
    command.push(0);
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of_val(&startup) as u32;
    startup.dwFlags = STARTF_USESTDHANDLES;
    unsafe {
        startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
        startup.hStdOutput = GetStdHandle(STD_OUTPUT_HANDLE);
        startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    }
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        CreateProcessAsUserW(
            token.0,
            application.as_mut_ptr(),
            command.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
            CREATE_UNICODE_ENVIRONMENT,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut process,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let handle = Owned(process.hProcess);
    let _thread = Owned(process.hThread);
    eprintln!("Running Windows fixture with administrative groups and privileges disabled");
    if unsafe { WaitForSingleObject(handle.0, INFINITE) } != WAIT_OBJECT_0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut code = 1;
    if unsafe { GetExitCodeProcess(handle.0, &mut code) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(Some(code as i32))
}
fn token_info(token: HANDLE, kind: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>, std::io::Error> {
    let mut needed = 0;
    unsafe {
        GetTokenInformation(token, kind, std::ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token, kind, buffer.as_mut_ptr().cast(), needed, &mut needed) }
        == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(buffer)
}
fn owner_default_dacl(token: HANDLE) -> Result<(), std::io::Error> {
    use windows_sys::Win32::Security::Authorization::*;
    let user = token_info(token, TokenUser)?;
    let default = token_info(token, TokenDefaultDacl)?;
    let sid = unsafe { (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    let old = unsafe { (*(default.as_ptr().cast::<TOKEN_DEFAULT_DACL>())).DefaultDacl };
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_ALL,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: OBJECT_INHERIT_ACE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
        },
    };
    let mut acl = std::ptr::null_mut();
    let result = unsafe { SetEntriesInAclW(1, &access, old, &mut acl) };
    if result != 0 {
        return Err(std::io::Error::from_raw_os_error(result as i32));
    }
    let dacl = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
    let success = unsafe {
        SetTokenInformation(
            token,
            TokenDefaultDacl,
            &dacl as *const _ as *const _,
            std::mem::size_of_val(&dacl) as u32,
        )
    };
    let error = if success == 0 {
        Some(std::io::Error::last_os_error())
    } else {
        None
    };
    unsafe {
        LocalFree(acl.cast());
    }
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
fn quote(arg: &OsStr) -> Vec<u16> {
    let mut out = vec![34];
    let mut backslashes = 0;
    for unit in arg.encode_wide() {
        if unit == 92 {
            backslashes += 1;
            continue;
        }
        if unit == 34 {
            out.extend(std::iter::repeat_n(92, backslashes * 2 + 1));
            out.push(34);
        } else {
            out.extend(std::iter::repeat_n(92, backslashes));
            out.push(unit);
        }
        backslashes = 0;
    }
    out.extend(std::iter::repeat_n(92, backslashes * 2));
    out.push(34);
    out
}
