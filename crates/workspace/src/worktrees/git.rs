//! Read-only local Git metadata. Mutations use M2's cancellable owned process tree.
use super::*;
use std::process::Command as Process;
pub(super) fn args(root: &Path) -> Result<Vec<std::ffi::OsString>> {
    let hooks = root.join("worktree-hooks");
    fs::create_dir_all(&hooks)?;
    if fs::symlink_metadata(&hooks)?.file_type().is_symlink()
        || fs::canonicalize(&hooks)? != hooks
        || fs::read_dir(&hooks)?.next().is_some()
    {
        return Err(error("worktree_ownership"));
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
    let git =
        devforge_projects::execution::find_git().map_err(|_| error("setup_git_unavailable"))?;
    let mut command = Process::new(git);
    command
        .current_dir(dunce::simplified(directory))
        .args(args(root)?)
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
    run(
        root,
        directory,
        input.iter().map(std::ffi::OsString::from).collect(),
        false,
        &Default::default(),
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
    read(
        root,
        path,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
    )
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
