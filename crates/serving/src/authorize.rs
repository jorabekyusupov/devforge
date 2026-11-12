//! Native authorization for the fixed-purpose installed helper, never a root app.
use crate::{Result, ServingError as E};
use std::path::{Path, PathBuf};
pub fn helper_path() -> Result<PathBuf> {
    let path = std::env::current_exe()?
        .parent()
        .ok_or(E::RuntimeUnavailable)?
        .join(if cfg!(windows) {
            "devforge-helper.exe"
        } else {
            "devforge-helper"
        });
    if !path.is_file() {
        return Err(E::RuntimeUnavailable);
    }
    Ok(path)
}
pub fn apply(helper: &Path, plan: &Path, digest: &str) -> Result<()> {
    if digest.len() != 64
        || !digest.bytes().all(|c| c.is_ascii_hexdigit())
        || !helper.is_absolute()
        || !plan.is_absolute()
    {
        return Err(E::InvalidConfiguration);
    }
    launch(helper, plan, digest)
}
#[cfg(target_os = "macos")]
fn launch(helper: &Path, plan: &Path, digest: &str) -> Result<()> {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let script = "on run argv\nset commandText to quoted form of (item 1 of argv) & \" apply \" & quoted form of (item 2 of argv) & \" \" & quoted form of (item 3 of argv)\ndo shell script commandText with administrator privileges\nend run";
    let mut child = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .arg(helper)
        .arg(plan)
        .arg(digest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| E::PermissionRequired)?;
    let until = Instant::now() + Duration::from_secs(120);
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(E::PermissionRequired)
            };
        }
        if Instant::now() > until {
            let _ = child.kill();
            let _ = child.wait();
            return Err(E::PermissionRequired);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
#[cfg(windows)]
fn launch(helper: &Path, plan: &Path, digest: &str) -> Result<()> {
    use std::{ffi::OsStr, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::{
            Com::{
                COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
            },
            Threading::{GetExitCodeProcess, WaitForSingleObject},
        },
        UI::{
            Shell::{
                SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
            },
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };
    let wide = |s: &OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let file = wide(dunce::simplified(helper).as_os_str());
    let verb = wide(OsStr::new("runas"));
    let text = format!(
        "apply {} {}",
        windows_quote(&dunce::simplified(plan).to_string_lossy()),
        windows_quote(digest)
    );
    let parameters = wide(OsStr::new(&text));
    unsafe {
        let initialized = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        ) >= 0;
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        let success = ShellExecuteExW(&mut info) != 0;
        if initialized {
            CoUninitialize();
        }
        if !success || info.hProcess.is_null() {
            return Err(E::PermissionRequired);
        }
        let waited = WaitForSingleObject(info.hProcess, 120_000);
        let mut code = 1;
        let read = GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if waited == WAIT_OBJECT_0 && read != 0 && code == 0 {
            Ok(())
        } else {
            Err(E::PermissionRequired)
        }
    }
}
#[cfg(any(windows, test))]
fn windows_quote(value: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            out.push_str(&"\\".repeat(slashes * 2 + 1));
        } else {
            out.push_str(&"\\".repeat(slashes));
        }
        slashes = 0;
        out.push(c);
    }
    out.push_str(&"\\".repeat(slashes * 2));
    out.push('"');
    out
}
#[cfg(not(any(target_os = "macos", windows)))]
fn launch(_: &Path, _: &Path, _: &str) -> Result<()> {
    Err(E::PermissionRequired)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authorization_arguments_preserve_windows_paths() {
        assert_eq!(
            windows_quote("C:\\A Ўзбек & ' app\\"),
            "\"C:\\A Ўзбек & ' app\\\\\""
        );
        assert_eq!(windows_quote("a\"b"), "\"a\\\"b\"");
        assert!(apply(Path::new("relative"), Path::new("relative"), "invalid").is_err());
    }
}
