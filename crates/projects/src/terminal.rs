use crate::{
    ProjectError as E, Result,
    execution::{Context, TerminalFacts},
    project::{Catalog, ProjectStore},
};
use devforge_core::SecretStore;
use devforge_runtime::package::{PackageStore, VerifiedResource};
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
/// PackageStore::open can create directories and repair root permissions. A
/// coexisting terminal must first prove the existing fixed layout is not a link
/// into another location. This is an ordinary path check, not an openat sandbox.
pub fn existing_packages(root: &Path) -> Result<PackageStore> {
    fn checked(path: &Path, directory: bool) -> Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink()
            || (if directory {
                !metadata.is_dir()
            } else {
                !metadata.is_file()
            })
        {
            return Err(E::StorageUnavailable);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(E::StorageUnavailable);
            }
        }
        Ok(())
    }
    if !root.is_absolute() {
        return Err(E::StorageUnavailable);
    }
    checked(root.parent().ok_or(E::StorageUnavailable)?, true)?;
    checked(root, true)?;
    let canonical = fs::canonicalize(root)?;
    let expected = fs::canonicalize(root.parent().ok_or(E::StorageUnavailable)?)?
        .join(root.file_name().ok_or(E::StorageUnavailable)?);
    if canonical != expected {
        return Err(E::StorageUnavailable);
    }
    for name in ["packages", "staging", "leases"] {
        let path = root.join(name);
        checked(&path, true)?;
        if fs::canonicalize(path)? != canonical.join(name) {
            return Err(E::StorageUnavailable);
        }
    }
    let marker = root.join(".devforge-runtime-store");
    checked(&marker, false)?;
    if fs::metadata(&marker)?.len() != b"devforge-runtime-store-v1\n".len() as u64
        || fs::read(marker)? != b"devforge-runtime-store-v1\n"
    {
        return Err(E::RuntimeUnavailable);
    }
    PackageStore::open(root, devforge_runtime::package::Limits::default()).map_err(Into::into)
}

/// A terminal owns its immutable INI/scan/shims and verified runtime leases until
/// the shell exits. Persistent tool homes stay project-scoped; no auth files are
/// copied into the session. Existing Xdebug INI material stays private and is
/// deleted with this directory, never used as a manager credential handoff.
pub struct Session {
    context: Context,
    _composer: VerifiedResource,
    directory: tempfile::TempDir,
}
impl Session {
    pub fn prepare(
        store: &ProjectStore,
        id: &str,
        catalog: &Catalog,
        packages: &PackageStore,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        // Select the unique private destination before any Context preparation.
        let directory = tempfile::Builder::new()
            .prefix(".terminal-")
            .tempdir_in(store.root())?;
        crate::private_dir(directory.path())?;
        let facts = TerminalFacts::capture(store, id)?;
        let mut context = Context::resolve_isolated(
            &facts,
            store,
            catalog,
            packages,
            secrets,
            directory.path().to_path_buf(),
        )?;
        let composer =
            packages.resource(catalog.composer(&context.project.composer)?, "composer")?;
        prepare(&mut context, catalog, packages)?;
        Ok(Self {
            context,
            _composer: composer,
            directory,
        })
    }
    pub fn project(&self) -> &crate::project::Project {
        self.context.project()
    }
    pub fn ini_path(&self) -> &Path {
        self.context.ini_path()
    }
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
    /// Fixed native probe of this already prepared snapshot. It never rewrites
    /// shims or resolves the project's latest selection.
    pub fn probe(&self) -> Result<String> {
        probe_prepared(&self.context)
    }
    pub fn shell(self) -> Result<i32> {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::System::Console::AllocConsole();
        }
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
            .envs(&self.context.environment)
            .current_dir(dunce::simplified(&self.context.project.path))
            .status()
            .map_err(|_| E::TerminalUnavailable)?;
        Ok(status.code().unwrap_or(1))
    }
}
pub fn shell(
    store: &ProjectStore,
    id: &str,
    catalog: &Catalog,
    packages: &PackageStore,
    secrets: &dyn SecretStore,
) -> Result<i32> {
    Session::prepare(store, id, catalog, packages, secrets)?.shell()
}
/// Fixed diagnostic program, with no project-supplied shell interpolation.
pub fn probe(context: &mut Context, catalog: &Catalog, packages: &PackageStore) -> Result<String> {
    prepare(context, catalog, packages)?;
    probe_prepared(context)
}
fn probe_prepared(context: &Context) -> Result<String> {
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

#[cfg(test)]
mod tests;
