use crate::{
    ProjectError as E, Result,
    execution::Context,
    project::{Catalog, ProjectStore},
};
use devforge_core::SecretStore;
use devforge_runtime::package::PackageStore;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
/// Regenerate project-owned launchers. They contain paths, never credentials.
/// Programmatic execution always uses verified executable+argv; shims are only
/// for an explicitly opened interactive terminal.
pub fn prepare(
    context: &mut Context,
    catalog: &Catalog,
    packages: &PackageStore,
) -> Result<PathBuf> {
    let bin = context.config.join("bin");
    crate::private_dir(&bin)?;
    let composer = catalog.composer(&context.project.composer)?;
    let resource = packages.resource(composer, "composer")?;
    for (name, extra) in [
        ("php", Vec::new()),
        ("composer", vec![resource.path().to_path_buf()]),
        ("artisan", vec![context.project.path.join("artisan")]),
    ] {
        let mut args = vec![
            context.php.path().to_path_buf(),
            PathBuf::from("-c"),
            context.ini.clone(),
        ];
        args.extend(extra);
        let body = if cfg!(windows) {
            format!(
                "@echo off\r\nsetlocal DisableDelayedExpansion\r\n{} %*\r\n",
                args.iter()
                    .map(|s| batch_path(s))
                    .collect::<Result<Vec<_>>>()?
                    .join(" ")
            )
        } else {
            format!(
                "#!/bin/sh\nexec {} \"$@\"\n",
                args.iter()
                    .map(|s| quote(s))
                    .collect::<Result<Vec<_>>>()?
                    .join(" ")
            )
        };
        let path = bin.join(if cfg!(windows) {
            format!("{name}.cmd")
        } else {
            name.into()
        });
        fs::write(&path, body)?;
        make_executable(&path)?;
    }
    let old = context
        .environment
        .get(std::ffi::OsStr::new("PATH"))
        .ok_or(E::InvalidEnvironment)?
        .clone();
    let paths =
        std::iter::once(dunce::simplified(&bin).to_owned()).chain(std::env::split_paths(&old));
    context.environment.insert(
        "PATH".into(),
        std::env::join_paths(paths).map_err(|_| E::InvalidEnvironment)?,
    );
    Ok(bin)
}
pub fn shell(
    store: &ProjectStore,
    id: &str,
    catalog: &Catalog,
    packages: &PackageStore,
    secrets: &dyn SecretStore,
) -> Result<i32> {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::AllocConsole();
    }
    let mut context = Context::resolve(store, id, catalog, packages, secrets)?;
    prepare(&mut context, catalog, packages)?;
    // Keep both interpreter and Composer leases until the interactive shell exits.
    let composer = catalog.composer(&context.project.composer)?;
    let _composer = packages.resource(composer, "composer")?;
    let mut command = if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").ok_or(E::TerminalUnavailable)?;
        let mut c = Command::new(PathBuf::from(root).join("System32").join("cmd.exe"));
        c.arg("/D");
        c
    } else {
        let mut c = Command::new("/bin/zsh");
        c.args(["-d", "-f"]);
        c
    };
    let status = command
        .env_clear()
        .envs(&context.environment)
        .current_dir(dunce::simplified(&context.project.path))
        .status()
        .map_err(|_| E::TerminalUnavailable)?;
    Ok(status.code().unwrap_or(1))
}
/// Fixed diagnostic program, with no project-supplied shell interpolation.
pub fn probe(context: &mut Context, catalog: &Catalog, packages: &PackageStore) -> Result<String> {
    prepare(context, catalog, packages)?;
    let mut command = if cfg!(windows) {
        windows_probe_command("call php --version && node --version")?
    } else {
        let mut c = Command::new("/bin/zsh");
        c.args(["-d", "-f", "-c", "php --version && node --version"]);
        c
    };
    let output = command
        .env_clear()
        .envs(&context.environment)
        .current_dir(dunce::simplified(&context.project.path))
        .output()
        .map_err(|_| E::TerminalUnavailable)?;
    if !output.status.success() {
        let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        for value in &context.sensitive {
            stdout = stdout.replace(value, "[REDACTED]");
            stderr = stderr.replace(value, "[REDACTED]");
        }
        fs::write(context.config.join("terminal-probe-status.json"),serde_json::to_vec(&serde_json::json!({"exit_code":output.status.code(),"stdout":stdout,"stderr":stderr})).expect("safe diagnostic"))?;
        return Err(E::TerminalUnavailable);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
fn windows_probe_command(script: &'static str) -> Result<Command> {
    let mut command = Command::new(
        PathBuf::from(std::env::var_os("SystemRoot").ok_or(E::TerminalUnavailable)?)
            .join("System32")
            .join("cmd.exe"),
    );
    command.args(["/D", "/V:OFF", "/S", "/C"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(format!("\"{script}\""));
    }
    #[cfg(not(windows))]
    command.arg(script);
    Ok(command)
}
#[cfg(all(test, windows))]
mod windows_tests {
    #[test]
    fn fixed_cmd_probe_preserves_the_native_executable_and_shell_syntax() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("A & ' Unicode Ўзбек");
        std::fs::create_dir(&path).unwrap();
        let output = super::windows_probe_command("call echo DEVFORGE_SHELL_PROBE")
            .unwrap()
            .env_clear()
            .envs(crate::environment::base())
            .current_dir(dunce::simplified(&path))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("DEVFORGE_SHELL_PROBE"));
    }
}
pub fn open(store: &ProjectStore, id: &str, data_dir: &Path) -> Result<()> {
    store.get(id)?;
    let executable = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    {
        let dir = store.root.join("terminal-launchers");
        crate::private_dir(&dir)?;
        let path = dir.join(format!("{id}.command"));
        use std::io::Write;
        let body = format!(
            "#!/bin/sh\nexec /usr/bin/env DEVFORGE_DATA_DIR={} {} project shell {}\n",
            quote(data_dir)?,
            quote(&executable)?,
            quote(Path::new(id))?
        );
        let mut f = fs::File::create(&path)?;
        f.write_all(body.as_bytes())?;
        make_executable(&path)?;
        let status = Command::new("/usr/bin/open")
            .args(["-a", "Terminal"])
            .arg(&path)
            .status()
            .map_err(|_| E::TerminalUnavailable)?;
        if !status.success() {
            return Err(E::TerminalUnavailable);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new(&executable)
            .args(["project", "shell", id])
            .env("DEVFORGE_DATA_DIR", data_dir)
            .creation_flags(0x00000010)
            .spawn()
            .map_err(|_| E::TerminalUnavailable)?;
    }
    Ok(())
}
fn quote(path: &Path) -> Result<String> {
    let s = path.to_str().ok_or(E::TerminalUnavailable)?;
    if s.contains(['\n', '\r', '\0']) {
        return Err(E::TerminalUnavailable);
    }
    Ok(format!("'{}'", s.replace('\'', "'\\''")))
}
fn batch_path(path: &Path) -> Result<String> {
    let s = dunce::simplified(path)
        .to_str()
        .ok_or(E::TerminalUnavailable)?;
    if s.contains(['"', '\n', '\r', '\0']) {
        return Err(E::TerminalUnavailable);
    }
    Ok(format!("\"{}\"", s.replace('%', "%%")))
}
fn make_executable(path: &Path) -> Result<()> {
    #[cfg(not(unix))]
    let _ = path;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
