//! Bounded host-tool invocation using the same process-group/Job adapter as runtimes.
//! Callers select an absolute trusted host executable; this does not claim package verification.
use super::tree::Tree;
use crate::{Cancellation, Result, RuntimeError};
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[derive(Debug)]
pub struct Output {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
}
pub fn run(
    command: &mut Command,
    timeout: Duration,
    limit: usize,
    cancel: &Cancellation,
) -> Result<Output> {
    if !Path::new(command.get_program()).is_absolute()
        || timeout.is_zero()
        || timeout > Duration::from_secs(3600)
        || !(1..=16 * 1024 * 1024).contains(&limit)
    {
        return Err(RuntimeError::InvalidProcess);
    }
    if cancel.is_cancelled() {
        return Err(RuntimeError::Cancelled);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut tree = Tree::spawn(command)?;
    let stdout = tree.child.stdout.take().ok_or(RuntimeError::Io)?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let until = Instant::now() + timeout;
    let mut expired = false;
    loop {
        if cancel.is_cancelled() {
            break;
        }
        if Instant::now() >= until {
            expired = true;
            break;
        }
        if tree.exited()? {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    let exit_code = tree.finish(Duration::from_millis(250))?;
    let bytes = reader
        .join()
        .map_err(|_| RuntimeError::Io)?
        .map_err(|_| RuntimeError::Io)?;
    if cancel.is_cancelled() {
        return Err(RuntimeError::Cancelled);
    }
    if expired {
        return Err(RuntimeError::ReadinessTimeout);
    }
    if bytes.len() > limit {
        return Err(RuntimeError::Io);
    }
    Ok(Output {
        exit_code,
        stdout: bytes,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_tools_bound_output_timeout_and_cancel_without_runtime_packages() {
        let exe = std::env::current_exe().unwrap();
        let out = run(
            Command::new(&exe).arg("--list"),
            Duration::from_secs(10),
            65536,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(out.exit_code, Some(0));
        assert!(!out.stdout.is_empty());
        assert!(
            run(
                Command::new(&exe).arg("--list"),
                Duration::from_secs(10),
                1,
                &Default::default()
            )
            .is_err()
        );
        let cancel = Cancellation::default();
        cancel.cancel();
        assert_eq!(
            run(&mut Command::new(&exe), Duration::from_secs(1), 32, &cancel).unwrap_err(),
            RuntimeError::Cancelled
        );
        #[cfg(unix)]
        let mut sleep = {
            let mut c = Command::new("/bin/sleep");
            c.arg("30");
            c
        };
        #[cfg(windows)]
        let mut sleep = {
            let path = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let mut c = Command::new(path);
            c.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ]);
            c
        };
        assert_eq!(
            run(
                &mut sleep,
                Duration::from_millis(50),
                32,
                &Default::default()
            )
            .unwrap_err(),
            RuntimeError::ReadinessTimeout
        );
    }
}
