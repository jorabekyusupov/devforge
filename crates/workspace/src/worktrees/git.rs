//! Read-only local Git metadata. Mutations use M2's cancellable owned process tree.
use super::*;
use std::process::Command as Process;
thread_local! {
    static READ_CANCEL: std::cell::RefCell<Option<Cancellation>> = const { std::cell::RefCell::new(None) };
}
/// A scope-bound cancellation token for Git reads on the current thread.
/// `!Send` is essential: dropping on another thread cannot restore this slot.
pub struct ReadCancellationGuard {
    previous: Option<Cancellation>,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Drop for ReadCancellationGuard {
    fn drop(&mut self) {
        READ_CANCEL.with(|slot| *slot.borrow_mut() = self.previous.take());
    }
}
pub fn read_cancellation_guard(cancel: &Cancellation) -> ReadCancellationGuard {
    let previous = READ_CANCEL.with(|slot| slot.borrow_mut().replace(cancel.clone()));
    ReadCancellationGuard {
        previous,
        _thread_bound: std::marker::PhantomData,
    }
}
/// Restore the prior cancellation token even if the action unwinds.
pub(super) fn with_read_cancellation<T>(
    cancel: &Cancellation,
    action: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _guard = read_cancellation_guard(cancel);
    action()
}

pub(super) fn args(root: &Path, mutating: bool) -> Result<Vec<std::ffi::OsString>> {
    let hooks = root.join("worktree-hooks");
    if mutating {
        fs::create_dir_all(&hooks)?;
    }
    match fs::symlink_metadata(&hooks) {
        Ok(metadata) => {
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || fs::canonicalize(&hooks)? != hooks
                || fs::read_dir(&hooks)?.next().is_some()
            {
                return Err(error("worktree_ownership"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !mutating => {}
        Err(error) => return Err(error.into()),
    }
    Ok(vec![
        "-c".into(),
        format!("core.hooksPath={}", dunce::simplified(&hooks).display()).into(),
        "-c".into(),
        "core.fsmonitor=false".into(),
        "-c".into(),
        "core.longpaths=true".into(),
        "-c".into(),
        "core.quotePath=false".into(),
        "-c".into(),
        "submodule.recurse=false".into(),
    ])
}
pub(super) fn run(
    root: &Path,
    directory: &Path,
    input: Vec<std::ffi::OsString>,
    mutating: bool,
    cancel: &Cancellation,
) -> Result<Vec<u8>> {
    if cancel.is_cancelled() {
        return Err(error("setup_cancelled"));
    }
    let git =
        devforge_projects::execution::find_git().map_err(|_| error("setup_git_unavailable"))?;
    let mut command = Process::new(git);
    command
        .current_dir(dunce::simplified(directory))
        .args(args(root, mutating)?)
        .args(input)
        .env_clear()
        .envs(
            [
                "SystemRoot",
                "WINDIR",
                "TEMP",
                "TMP",
                "HOME",
                "USERPROFILE",
                "APPDATA",
                "LOCALAPPDATA",
                "PATH",
                "SSH_AUTH_SOCK",
            ]
            .iter()
            .filter_map(|name| std::env::var_os(name).map(|v| (*name, v))),
        )
        .env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GCM_INTERACTIVE", "never");
    if !mutating {
        command.env("GIT_OPTIONAL_LOCKS", "0");
    }
    let result = devforge_runtime::process::host::run(
        &mut command,
        Duration::from_secs(if mutating { 300 } else { 15 }),
        2 * 1024 * 1024,
        cancel,
    );
    if cancel.is_cancelled() {
        return Err(error("setup_cancelled"));
    }
    let result = result.map_err(|_| error("worktree_git_failed"))?;
    if result.exit_code != Some(0) {
        return Err(error("worktree_git_failed"));
    }
    Ok(result.stdout)
}
pub(super) fn read(root: &Path, directory: &Path, input: &[&str]) -> Result<Vec<u8>> {
    let cancel = READ_CANCEL
        .with(|slot| slot.borrow().clone())
        .unwrap_or_default();
    run(
        root,
        directory,
        input.iter().map(std::ffi::OsString::from).collect(),
        false,
        &cancel,
    )
}
pub(super) fn text(root: &Path, directory: &Path, input: &[&str]) -> Result<String> {
    String::from_utf8(read(root, directory, input)?)
        .map(|s| s.trim_end_matches(['\r', '\n']).to_string())
        .map_err(|_| error("worktree_git_failed"))
}
pub(super) fn branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value != "HEAD"
        && !value.starts_with(['-', '/', '.'])
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
        && !value.contains("..")
        && !value.contains("//")
        && !value.ends_with(['.', '/'])
}
pub(super) fn common(root: &Path, directory: &Path) -> Result<PathBuf> {
    let path = text(
        root,
        directory,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Ok(fs::canonicalize(path)?)
}
pub(super) fn oid(root: &Path, directory: &Path, reference: &str) -> Result<String> {
    if reference != "HEAD" && !branch(reference) {
        return Err(error("worktree_branch_invalid"));
    }
    let id = text(
        root,
        directory,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )?;
    if ![40, 64].contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error("worktree_git_failed"));
    }
    Ok(id)
}
pub(super) fn status(root: &Path, path: &Path) -> Result<Vec<u8>> {
    // Compare actual normalized content, staged state and untracked files. A
    // porcelain stat result can report an unchanged LF overlay as modified after
    // a fresh CRLF checkout. Never stage data or rewrite a user's index to hide it.
    let mut changed = read(
        root,
        path,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--cached",
            "--name-only",
            "-z",
            "--ignore-submodules=none",
            "HEAD",
            "--",
        ],
    )?;
    changed.extend(read(
        root,
        path,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            "-z",
            "--ignore-submodules=none",
            "--",
        ],
    )?);
    changed.extend(read(
        root,
        path,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?);
    Ok(changed)
}

pub(super) fn submodules(root: &Path, path: &Path, commit: Option<&str>) -> Result<bool> {
    let bytes = if let Some(commit) = commit {
        read(
            root,
            path,
            &["ls-tree", "-r", "--format=%(objectmode)", commit],
        )?
    } else {
        read(root, path, &["ls-files", "--format=%(objectmode)"])?
    };
    Ok(bytes
        .split(|b| *b == b'\n')
        .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == b"160000"))
}

pub(super) fn attached_branch(root: &Path, path: &Path) -> Result<String> {
    let value = text(root, path, &["symbolic-ref", "-q", "HEAD"])?;
    value
        .strip_prefix("refs/heads/")
        .filter(|v| branch(v))
        .map(str::to_string)
        .ok_or_else(|| error("worktree_detached"))
}

#[cfg(test)]
mod read_only_tests {
    use super::*;

    #[test]
    fn cancellation_scopes_restore_previous_token_and_do_not_start_git() {
        let root = tempfile::tempdir().unwrap();
        let cancel = Cancellation::default();
        cancel.cancel();
        let outcome =
            with_read_cancellation(&cancel, || read(root.path(), root.path(), &["status"]));
        assert_eq!(outcome.err().unwrap().code, "setup_cancelled");
        assert!(READ_CANCEL.with(|slot| slot.borrow().is_none()));
        assert!(!root.path().join("worktree-hooks").exists());
    }
    #[test]
    fn read_only_git_arguments_do_not_create_hooks_directory() {
        let root = tempfile::tempdir().unwrap();
        let canonical = fs::canonicalize(root.path()).unwrap();
        let args = args(&canonical, false).unwrap();
        assert!(
            args.iter()
                .any(|v| v.to_string_lossy().starts_with("core.hooksPath="))
        );
        assert!(!canonical.join("worktree-hooks").exists());
    }
}
