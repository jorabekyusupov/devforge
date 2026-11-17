//! Merge the bound source HEAD into a receipt-owned child without touching source.
//! Divergent preview runs in a pristine temporary Git repository with verified
//! object alternates; custom drivers, unsafe configuration and conflicts are refused.
use super::*;

#[derive(Clone, Serialize)]
pub struct MergePlan {
    pub workspace_id: String,
    pub source_head: String,
    pub target_head: String,
    pub target_branch: String,
    pub effect: String,
    pub result_tree: Option<String>,
    pub isolated: bool,
    pub target_project_revision: u64,
    pub source_dirty: bool,
    pub blockers: Vec<String>,
    pub digest: String,
}

/// Invoke a fixed Git command with bounded output, no inherited Git environment,
/// an empty hooks directory, and the current operation cancellation token.
fn run_git(
    root: &Path,
    cwd: &Path,
    args: &[OsString],
    extra: &[(&str, OsString)],
    cancel: &Cancellation,
) -> Result<devforge_runtime::process::host::Output> {
    let git =
        devforge_projects::execution::find_git().map_err(|_| error("setup_git_unavailable"))?;
    let mut command = std::process::Command::new(git);
    // Use identical, explicitly empty global attributes for preview and apply;
    // caller separately refuses any configured or implicit attributes file.
    let empty_attrs = tempfile::NamedTempFile::new()?;
    let attr_path = empty_attrs
        .path()
        .to_str()
        .ok_or_else(|| error("worktree_merge_unsupported"))?;
    command
        .current_dir(dunce::simplified(cwd))
        .args(git::args(root, false)?);
    if args.iter().any(|arg| {
        matches!(
            arg.to_str(),
            Some("merge" | "merge-tree" | "check-attr" | "fetch")
        )
    }) {
        command
            .arg("-c")
            .arg(format!("core.attributesFile={attr_path}"));
    }
    command.args(args).env_clear();
    for key in [
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PATH",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1");
    for (key, value) in extra {
        command.env(key, value);
    }
    let mutating = args
        .iter()
        .any(|arg| matches!(arg.to_str(), Some("fetch" | "merge")));
    let timeout = std::time::Duration::from_secs(if mutating { 300 } else { 15 });
    devforge_runtime::process::host::run(&mut command, timeout, 2 * 1024 * 1024, cancel).map_err(
        |_| {
            if cancel.is_cancelled() {
                error("setup_cancelled")
            } else {
                error("worktree_git_failed")
            }
        },
    )
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

/// Git resolves config includes for this query. Any external driver/filter,
/// custom attributes file, signing or non-default merge policy is not supported
/// by the sanitized preview and must be rejected before execution.
fn safe_merge_config(root: &Path, cwd: &Path, cancel: &Cancellation) -> Result<bool> {
    let pattern = r"^(merge\.(renormalize|renames|directoryrenames|conflictstyle|default)$|merge\..*\.(driver|recursive)$|rerere\.|core\.attributesfile$|diff\.renames$|commit\.gpgsign$|uploadpack\.packobjectshook$|url\..*\.insteadof$|branch\..*\.mergeoptions$)";
    let result = run_git(
        root,
        cwd,
        &args(&[
            "config",
            "--includes",
            "--name-only",
            "--get-regexp",
            pattern,
        ]),
        &[],
        cancel,
    )?;
    match result.exit_code {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(error("worktree_git_failed")),
    }
}

/// A read-only `git diff` can run configured clean/process filters. Neutralize
/// every configured filter in the status subprocess; never execute its program.
/// A filtered worktree may conservatively appear dirty and be refused.
fn filter_overrides(root: &Path, cwd: &Path, cancel: &Cancellation) -> Result<Vec<OsString>> {
    let pattern = r"^filter\..*\.(clean|smudge|process|required)$";
    let names = run_git(
        root,
        cwd,
        &args(&[
            "config",
            "--includes",
            "--name-only",
            "--get-regexp",
            pattern,
        ]),
        &[],
        cancel,
    )?;
    if !matches!(names.exit_code, Some(0 | 1)) {
        return Err(error("worktree_git_failed"));
    }
    let names =
        std::str::from_utf8(&names.stdout).map_err(|_| error("worktree_merge_unsupported"))?;
    let mut filters = std::collections::BTreeSet::new();
    for key in names.lines() {
        let Some(tail) = key.strip_prefix("filter.") else {
            return Err(error("worktree_merge_unsupported"));
        };
        let Some((name, _)) = tail.rsplit_once('.') else {
            return Err(error("worktree_merge_unsupported"));
        };
        if name.is_empty()
            || name.len() > 128
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(error("worktree_merge_unsupported"));
        }
        filters.insert(name.to_owned());
        if filters.len() > 128 {
            return Err(error("worktree_merge_unsupported"));
        }
    }
    let mut overrides = Vec::new();
    for name in filters {
        for suffix in ["clean=", "smudge=", "process=", "required=false"] {
            overrides.push(OsString::from("-c"));
            overrides.push(OsString::from(format!("filter.{name}.{suffix}")));
        }
    }
    Ok(overrides)
}

pub(super) fn safe_status(root: &Path, cwd: &Path, cancel: &Cancellation) -> Result<Vec<u8>> {
    let overrides = filter_overrides(root, cwd, cancel)?;
    let mut combined = Vec::new();
    for command in [
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--cached",
            "--name-only",
            "-z",
            "--ignore-submodules=all",
            "HEAD",
            "--",
        ][..],
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            "-z",
            "--ignore-submodules=all",
            "--",
        ][..],
        &["ls-files", "--others", "--exclude-standard", "-z"][..],
    ] {
        let mut arguments = overrides.clone();
        arguments.extend(args(command));
        let output = run_git(root, cwd, &arguments, &[], cancel)?;
        if output.exit_code != Some(0) {
            return Err(error("worktree_merge_unsupported"));
        }
        if combined.len().saturating_add(output.stdout.len()) > 2 * 1024 * 1024 {
            return Err(error("worktree_merge_unsupported"));
        }
        combined.extend(output.stdout);
    }
    Ok(combined)
}

fn info_attributes_absent(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path.join("info").join("attributes")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Ok(_) => Ok(false),
        Err(_) => Err(error("worktree_ownership")),
    }
}

/// Git implicitly reads this file even without core.attributesFile configured.
/// Refuse rather than silently discard a user's global attribute semantics.
/// Temporary preview does not inherit Git's replace refs, grafts or shallow
/// history. Refuse such repositories rather than silently changing graph semantics.
fn graph_is_unmodified(
    root: &Path,
    cwd: &Path,
    common: &Path,
    admin: &Path,
    check_refs: bool,
    cancel: &Cancellation,
) -> Result<bool> {
    for base in [common, admin] {
        for path in [base.join("shallow"), base.join("info").join("grafts")] {
            match fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Ok(false),
                Err(_) => return Err(error("worktree_ownership")),
            }
        }
    }
    if !check_refs {
        // Linked worktrees share refs with their verified common Git directory.
        return Ok(true);
    }
    let refs = run_git(
        root,
        cwd,
        &args(&["for-each-ref", "--format=%(refname)", "refs/replace/"]),
        &[],
        cancel,
    )?;
    if refs.exit_code != Some(0) {
        return Err(error("worktree_git_failed"));
    }
    Ok(refs.stdout.is_empty())
}

fn implicit_attributes_absent() -> Result<bool> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    let Some(home) = home else {
        return Err(error("worktree_merge_unsupported"));
    };
    match fs::symlink_metadata(PathBuf::from(home).join(".config/git/attributes")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Ok(_) => Ok(false),
        Err(_) => Err(error("worktree_ownership")),
    }
}

fn ignored_attributes_absent(root: &Path, target: &Path, cancel: &Cancellation) -> Result<bool> {
    let out = run_git(
        root,
        target,
        &args(&[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ]),
        &[],
        cancel,
    )?;
    if out.exit_code != Some(0) {
        return Err(error("worktree_git_failed"));
    }
    Ok(!out
        .stdout
        .split(|b| *b == 0)
        .any(|p| p == b".gitattributes" || p.ends_with(b"/.gitattributes")))
}

fn oid_line(data: &[u8]) -> Result<String> {
    let value = std::str::from_utf8(data)
        .map_err(|_| error("worktree_git_failed"))?
        .lines()
        .next()
        .ok_or_else(|| error("worktree_git_failed"))?;
    if ![40, 64].contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error("worktree_git_failed"));
    }
    Ok(value.to_ascii_lowercase())
}

/// Evaluate only paths changed by this merge in the pinned trees. This avoids
/// rejecting a globally configured but unused filter such as Git LFS, while
/// refusing any affected custom filter or external merge driver.
fn affected_attributes(
    root: &Path,
    cwd: &Path,
    env: &[(&str, OsString)],
    tree: &str,
    paths: &[String],
    cancel: &Cancellation,
) -> Result<BTreeMap<String, (String, String)>> {
    let mut result = BTreeMap::new();
    let mut start = 0;
    while start < paths.len() {
        let mut end = start;
        let mut bytes = 0usize;
        while end < paths.len() && end - start < 50 && bytes + paths[end].len() < 8000 {
            bytes += paths[end].len() + 1;
            end += 1;
        }
        if end == start {
            return Err(error("worktree_merge_unsupported"));
        }
        let mut command = vec![
            "check-attr".into(),
            format!("--source={tree}").into(),
            "-z".into(),
            "filter".into(),
            "merge".into(),
            "--".into(),
        ];
        command.extend(paths[start..end].iter().map(OsString::from));
        let out = run_git(root, cwd, &command, env, cancel)?;
        if out.exit_code != Some(0) {
            return Err(error("worktree_merge_unsupported"));
        }
        let fields = out.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
        if fields.len() != (end - start) * 6 + 1 {
            return Err(error("worktree_merge_unsupported"));
        }
        for pair in fields[..fields.len() - 1].as_chunks::<6>().0.iter() {
            let path =
                std::str::from_utf8(pair[0]).map_err(|_| error("worktree_merge_unsupported"))?;
            let name1 =
                std::str::from_utf8(pair[1]).map_err(|_| error("worktree_merge_unsupported"))?;
            let value1 =
                std::str::from_utf8(pair[2]).map_err(|_| error("worktree_merge_unsupported"))?;
            let path2 =
                std::str::from_utf8(pair[3]).map_err(|_| error("worktree_merge_unsupported"))?;
            let name2 =
                std::str::from_utf8(pair[4]).map_err(|_| error("worktree_merge_unsupported"))?;
            let value2 =
                std::str::from_utf8(pair[5]).map_err(|_| error("worktree_merge_unsupported"))?;
            if path != path2
                || name1 != "filter"
                || name2 != "merge"
                || result
                    .insert(path.to_owned(), (value1.to_owned(), value2.to_owned()))
                    .is_some()
            {
                return Err(error("worktree_merge_unsupported"));
            }
        }
        start = end;
    }
    Ok(result)
}

/// All merge-tree-created blobs and trees live in this disposable bare repo;
/// it reads verified existing source/target object stores as alternates, with
/// empty global/system config, no custom drivers and no original index/refs.
fn isolated_analysis(
    root: &Path,
    source_objects: &Path,
    target_objects: &Path,
    target: &str,
    source: &str,
    cancel: &Cancellation,
) -> Result<(bool, bool, Option<String>, bool, bool)> {
    for path in [source_objects, target_objects] {
        let meta = fs::symlink_metadata(path).map_err(|_| error("worktree_ownership"))?;
        if !meta.is_dir()
            || devforge_projects::discovery::is_link(&meta)
            || fs::canonicalize(path)? != path
        {
            return Err(error("worktree_ownership"));
        }
        // Alternates are themselves untrusted path references, not an owned object store.
        if fs::symlink_metadata(path.join("info").join("alternates")).is_ok() {
            return Err(error("worktree_merge_unsupported"));
        }
    }
    let temp = tempfile::tempdir()?;
    let bare = temp.path().join("preview.git");
    let empty_config = temp.path().join("empty-config");
    fs::write(&empty_config, b"")?;
    let isolated = [
        ("GIT_CONFIG_GLOBAL", empty_config.clone().into_os_string()),
        ("GIT_CONFIG_SYSTEM", empty_config.into_os_string()),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
    ];
    let object_format = if source.len() == 64 {
        "--object-format=sha256"
    } else {
        "--object-format=sha1"
    };
    let init = run_git(
        root,
        temp.path(),
        &[
            "init".into(),
            "--bare".into(),
            "--quiet".into(),
            "--template=".into(),
            object_format.into(),
            bare.as_os_str().to_os_string(),
        ],
        &isolated,
        cancel,
    )?;
    if init.exit_code != Some(0) {
        return Err(error("worktree_git_failed"));
    }
    // Git for Windows does not consistently accept verbatim `\\?\C:\...`
    // paths. Keep canonical paths for the ownership checks above, but hand Git
    // equivalent simplified local paths.
    let alternates = std::env::join_paths([
        dunce::simplified(source_objects),
        dunce::simplified(target_objects),
    ])
    .map_err(|_| error("worktree_merge_unsupported"))?;
    let mut env = isolated.to_vec();
    env.push(("GIT_DIR", bare.as_os_str().to_os_string()));
    env.push(("GIT_ALTERNATE_OBJECT_DIRECTORIES", alternates));
    let ancestor = |a: &str, b: &str| -> Result<bool> {
        let out = run_git(
            root,
            temp.path(),
            &args(&["merge-base", "--is-ancestor", a, b]),
            &env,
            cancel,
        )?;
        match out.exit_code {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(error("worktree_merge_unsupported")),
        }
    };
    let source_in_target = ancestor(source, target)?;
    let target_in_source = ancestor(target, source)?;
    let (result_tree, conflict) = if source_in_target || target_in_source {
        (None, false)
    } else {
        let out = run_git(
            root,
            temp.path(),
            &args(&["merge-tree", "--write-tree", target, source]),
            &env,
            cancel,
        )?;
        match out.exit_code {
            Some(0) => (Some(oid_line(&out.stdout)?), false),
            Some(1) => (None, true),
            _ => return Err(error("worktree_merge_unsupported")),
        }
    };
    if source_in_target {
        return Ok((
            source_in_target,
            target_in_source,
            result_tree,
            conflict,
            true,
        ));
    }
    let diff = run_git(
        root,
        temp.path(),
        &args(&[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--name-only",
            "-z",
            target,
            source,
            "--",
        ]),
        &env,
        cancel,
    )?;
    if diff.exit_code != Some(0) {
        return Err(error("worktree_merge_unsupported"));
    }
    let paths = diff
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            std::str::from_utf8(p)
                .map(str::to_owned)
                .map_err(|_| error("worktree_merge_unsupported"))
        })
        .collect::<Result<Vec<_>>>()?;
    if paths.len() > 1000 {
        return Err(error("worktree_merge_unsupported"));
    }
    let a = affected_attributes(root, temp.path(), &env, target, &paths, cancel)?;
    let b = affected_attributes(root, temp.path(), &env, source, &paths, cancel)?;
    let c = if let Some(tree) = &result_tree {
        affected_attributes(root, temp.path(), &env, tree, &paths, cancel)?
    } else {
        b.clone()
    };
    let safe = a == b
        && b == c
        && a.values().all(|(filter, merge)| {
            matches!(filter.as_str(), "unspecified" | "unset")
                && matches!(
                    merge.as_str(),
                    "unspecified" | "unset" | "set" | "text" | "binary" | "union"
                )
        });
    Ok((
        source_in_target,
        target_in_source,
        result_tree,
        conflict,
        safe,
    ))
}

impl Workspace {
    /// Noncreating lookup: an absent receipt is never created by a preview.
    fn merge_owned_record(&self, source: &str, id: &str) -> Result<(Record, PathBuf)> {
        if !valid_id(id) {
            return Err(error("worktree_ownership"));
        }
        let base = self.root.join("worktrees");
        let dir = base.join(id);
        for path in [&base, &dir] {
            let meta = fs::symlink_metadata(path).map_err(|_| error("worktree_ownership"))?;
            if !meta.is_dir()
                || devforge_projects::discovery::is_link(&meta)
                || fs::canonicalize(path).map_err(|_| error("worktree_ownership"))? != *path
            {
                return Err(error("worktree_ownership"));
            }
        }
        let path = dir.join("receipt.json");
        let record: Record =
            serde_json::from_slice(&devforge_projects::manifest::read_file(&path, 262144)?)
                .map_err(|_| error("worktree_ownership"))?;
        if ![1, 2].contains(&record.schema)
            || record.id != id
            || record.input.source_id != source
            || !git::branch(&record.input.branch)
            || record.stage != "ready"
        {
            return Err(error("worktree_ownership"));
        }
        Ok((record, dir.join("tree")))
    }

    pub fn worktree_merge_plan(&self, source_id: &str, id: &str) -> Result<MergePlan> {
        let source = self.available_project(source_id)?;
        let (record, target_path) = self.merge_owned_record(source_id, id)?;
        if record.source_path != source.path {
            return Err(error("worktree_ownership"));
        }
        let target_id = record
            .project_id
            .as_deref()
            .ok_or_else(|| error("worktree_ownership"))?;
        let target = self.projects.get(target_id)?;
        if target.id == source.id
            || target.path != target_path
            || target.worktree.as_deref() != Some(id)
            || git::common(&self.root, &source.path)? != record.common_dir
        {
            return Err(error("worktree_ownership"));
        }
        self.worktree_identity(&record, &target_path)?;
        let source_identity = owned_directory_identity(&source.path)?;
        let target_identity = owned_directory_identity(&target_path)?;
        if source.filesystem_identity.as_ref() != Some(&source_identity)
            || target.filesystem_identity.as_ref() != Some(&target_identity)
        {
            return Err(error("worktree_ownership"));
        }
        let cancel = git::current_read_cancellation();
        // `worktree_identity` above already verified the target common dir and
        // (when recorded) Git admin directory against these exact paths. Avoid
        // repeating two Git subprocesses on slow Windows hosts.
        let target_common = if record.isolated {
            target_path.join(".git")
        } else {
            record.common_dir.clone()
        };
        let source_dotgit = source.path.join(".git");
        let source_admin =
            if ordinary(&source_dotgit) && fs::canonicalize(&source_dotgit)? == record.common_dir {
                // Normal main checkout: the verified common dir is its admin dir.
                record.common_dir.clone()
            } else {
                // Linked source checkouts still need Git's exact per-worktree admin.
                fs::canonicalize(git::text(
                    &self.root,
                    &source.path,
                    &["rev-parse", "--absolute-git-dir"],
                )?)?
            };
        let target_admin = if record.isolated {
            target_common.clone()
        } else if let Some(admin) = &record.git_dir {
            admin.clone()
        } else {
            fs::canonicalize(git::text(
                &self.root,
                &target_path,
                &["rev-parse", "--absolute-git-dir"],
            )?)?
        };
        if !graph_is_unmodified(
            &self.root,
            &source.path,
            &record.common_dir,
            &source_admin,
            true,
            &cancel,
        )? || !graph_is_unmodified(
            &self.root,
            &target_path,
            &target_common,
            &target_admin,
            target_common != record.common_dir,
            &cancel,
        )? {
            return Err(error("worktree_merge_graph_unsupported"));
        }
        let source_head = git::oid(&self.root, &source.path, "HEAD")?;
        let target_head = git::oid(&self.root, &target_path, "HEAD")?;
        let target_branch = git::attached_branch(&self.root, &target_path)?;
        if target_branch != record.input.branch
            || (!record.isolated
                && git::attached_branch(&self.root, &source.path)
                    .ok()
                    .as_deref()
                    == Some(target_branch.as_str()))
        {
            return Err(error("worktree_ownership"));
        }
        // The parent `git diff` can execute a child repository's clean filter.
        // Refuse tracked or staged gitlinks before any status/diff operation.
        if git::submodules(&self.root, &source.path, Some(&source_head))?
            || git::submodules(&self.root, &target_path, Some(&target_head))?
            || git::submodules(&self.root, &source.path, None)?
            || git::submodules(&self.root, &target_path, None)?
        {
            return Err(error("worktree_submodules_unsupported"));
        }
        let source_dirty = !safe_status(&self.root, &source.path, &cancel)?.is_empty();
        let mut blockers = Vec::new();
        if self.sessions.contains_key(target_id) {
            blockers.push("worktree_running".into());
        }
        if !safe_status(&self.root, &target_path, &cancel)?.is_empty() {
            blockers.push("worktree_dirty".into());
        }
        if source_head.len() != target_head.len() {
            return Err(error("worktree_merge_unsupported"));
        }
        // The pristine preview never reads project/global Git configuration. The
        // approved target execution is permitted only with equivalent safe config.
        let config_safe = safe_merge_config(&self.root, &target_path, &cancel)?
            && safe_merge_config(&self.root, &source.path, &cancel)?
            && info_attributes_absent(&target_common)?
            && info_attributes_absent(&target_admin)?
            && info_attributes_absent(&record.common_dir)?
            && implicit_attributes_absent()?
            && ignored_attributes_absent(&self.root, &target_path, &cancel)?;
        if !config_safe {
            blockers.push("worktree_merge_custom_config_unsupported".into());
        }
        let (source_in_target, target_in_source, result_tree, conflict, attributes_safe) =
            isolated_analysis(
                &self.root,
                &record.common_dir.join("objects"),
                &target_common.join("objects"),
                &target_head,
                &source_head,
                &cancel,
            )?;
        let effect = if source_in_target {
            "already_included"
        } else if target_in_source {
            "fast_forward"
        } else {
            "merge_commit"
        };
        if conflict {
            blockers.push("worktree_merge_conflict".into());
        }
        if !attributes_safe {
            blockers.push("worktree_merge_attributes_unsupported".into());
        }
        let author = if effect == "merge_commit" && !conflict {
            // Freeze only the timestamp while determining identity. Git's normal
            // `var` output otherwise changes every second and invalidates review.
            let frozen_dates = [
                ("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z".into()),
                ("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z".into()),
            ];
            let author = run_git(
                &self.root,
                &target_path,
                &args(&["var", "GIT_AUTHOR_IDENT"]),
                &frozen_dates,
                &cancel,
            )?;
            let committer = run_git(
                &self.root,
                &target_path,
                &args(&["var", "GIT_COMMITTER_IDENT"]),
                &frozen_dates,
                &cancel,
            )?;
            if author.exit_code != Some(0) || committer.exit_code != Some(0) {
                blockers.push("worktree_merge_identity_required".into());
            }
            (author.stdout, committer.stdout)
        } else {
            (Vec::new(), Vec::new())
        };
        let mut plan = MergePlan {
            workspace_id: id.to_string(),
            source_head,
            target_head,
            target_branch,
            effect: effect.into(),
            result_tree,
            isolated: record.isolated,
            target_project_revision: target.revision,
            source_dirty,
            blockers,
            digest: String::new(),
        };
        plan.digest = digest(&(
            &plan,
            &record,
            source_identity,
            target_identity,
            source.revision,
            author,
        ));
        Ok(plan)
    }

    pub fn worktree_merge_guarded(
        &mut self,
        source_id: &str,
        id: &str,
        expected: &str,
        cancel: &Cancellation,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<serde_json::Value> {
        check()?;
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        let _lock = self.worktree_lock()?;
        let plan = self.worktree_merge_plan(source_id, id)?;
        if plan.digest != expected {
            return Err(error("worktree_review_changed"));
        }
        if !plan.blockers.is_empty() {
            return Err(error("worktree_merge_blocked"));
        }
        let (record, path) = self.merge_owned_record(source_id, id)?;
        let target_id = record
            .project_id
            .as_deref()
            .ok_or_else(|| error("worktree_ownership"))?;
        self.require_stopped(target_id)?;
        let _guard = self.sites.project_mutation_guard(target_id)?;
        check()?;
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        if self.worktree_merge_plan(source_id, id)?.digest != expected {
            return Err(error("worktree_review_changed"));
        }
        if plan.effect != "already_included" {
            // An isolated child has its own object database. Transfer only the
            // already-reviewed source commit from its bound local project; no
            // remote refs, FETCH_HEAD, tags or maintenance are touched.
            if plan.isolated {
                let source = self.available_project(source_id)?;
                if source.path != record.source_path {
                    return Err(error("worktree_review_changed"));
                }
                let mut fetch = args(&[
                    "-c",
                    "protocol.file.allow=always",
                    "fetch",
                    "--no-write-fetch-head",
                    "--no-tags",
                    "--no-recurse-submodules",
                    "--no-auto-maintenance",
                    "--no-write-commit-graph",
                    "--",
                ]);
                fetch.push(dunce::simplified(&source.path).as_os_str().to_os_string());
                fetch.push(plan.source_head.clone().into());
                let local_only = [("GIT_ALLOW_PROTOCOL", "file".into())];
                let result = run_git(&self.root, &path, &fetch, &local_only, cancel)
                    .map_err(|_| error("worktree_merge_incomplete"))?;
                if result.exit_code != Some(0) {
                    return Err(error("worktree_merge_incomplete"));
                }
                check()?;
                if cancel.is_cancelled() {
                    return Err(error("worktree_merge_incomplete"));
                }
                if self.worktree_merge_plan(source_id, id)?.digest != expected {
                    return Err(error("worktree_review_changed"));
                }
            }
            // Git can fail after touching the target checkout. Do not claim
            // rollback: the reviewed target_head is the recovery reference.
            // Even pre-merge status can invoke filters on unrelated tracked
            // files. Keep all configured filter programs inert during Git apply.
            let mut merge = filter_overrides(&self.root, &path, cancel)?;
            if plan.effect == "fast_forward" {
                merge.extend(args(&["merge", "--ff-only", "--no-overwrite-ignore", "--"]));
            } else if plan.effect == "merge_commit" {
                merge.extend(args(&[
                    "merge",
                    "--no-ff",
                    "--no-edit",
                    "--no-overwrite-ignore",
                    "--",
                ]));
            } else {
                return Err(error("worktree_merge_unsupported"));
            }
            merge.push(plan.source_head.clone().into());
            let result = run_git(&self.root, &path, &merge, &[], cancel)
                .map_err(|_| error("worktree_merge_incomplete"))?;
            if result.exit_code != Some(0) {
                return Err(error("worktree_merge_incomplete"));
            }
            let head = git::oid(&self.root, &path, "HEAD")
                .map_err(|_| error("worktree_merge_incomplete"))?;
            if plan.effect == "fast_forward" && head != plan.source_head {
                return Err(error("worktree_merge_incomplete"));
            }
            if plan.effect == "merge_commit" {
                let tree = git::text(&self.root, &path, &["rev-parse", "HEAD^{tree}"])
                    .map_err(|_| error("worktree_merge_incomplete"))?;
                let parents = git::text(
                    &self.root,
                    &path,
                    &["rev-list", "--parents", "-n", "1", "HEAD"],
                )
                .map_err(|_| error("worktree_merge_incomplete"))?;
                let values = parents.split_whitespace().collect::<Vec<_>>();
                if plan.result_tree.as_deref() != Some(tree.as_str())
                    || values.as_slice()
                        != [
                            head.as_str(),
                            plan.target_head.as_str(),
                            plan.source_head.as_str(),
                        ]
                {
                    return Err(error("worktree_merge_incomplete"));
                }
            }
        }
        Ok(serde_json::json!({"merged":true,"effect":plan.effect,"source_head":plan.source_head}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "fixture command failed");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn implicit_global_attributes_are_not_silently_used_or_ignored() {
        const CHILD: &str = "DEVFORGE_A10_ATTR_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let home = tempfile::tempdir().unwrap();
            let status = Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg("worktrees::merge::tests::implicit_global_attributes_are_not_silently_used_or_ignored")
                .env(CHILD, "1")
                .env("HOME", home.path())
                .env("USERPROFILE", home.path())
                .env("XDG_CONFIG_HOME", home.path().join(".config"))
                .status().unwrap();
            assert!(status.success());
            return;
        }
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        let attrs = home.join(".config/git/attributes");
        fs::create_dir_all(attrs.parent().unwrap()).unwrap();
        fs::write(&attrs, "tracked.txt merge=union\n").unwrap();
        assert!(!implicit_attributes_absent().unwrap());
        let repo = home.join("project");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        fs::write(repo.join("tracked.txt"), "fixture").unwrap();
        let ambient = Command::new("git")
            .current_dir(&repo)
            .args(["check-attr", "merge", "--", "tracked.txt"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&ambient.stdout).contains("union"));
        let sanitized = run_git(
            &home,
            &repo,
            &args(&["check-attr", "merge", "--", "tracked.txt"]),
            &[],
            &Cancellation::default(),
        )
        .unwrap();
        assert_eq!(sanitized.exit_code, Some(0));
        assert!(String::from_utf8_lossy(&sanitized.stdout).contains("unspecified"));
        fs::remove_file(&attrs).unwrap();
        assert!(implicit_attributes_absent().unwrap());
    }

    #[test]
    fn preview_never_executes_clean_filter_merge_driver_or_fsmonitor() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let repo = root.join("project");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        fs::write(
            repo.join(".gitattributes"),
            "tracked.txt filter=evil\nconflict.txt merge=evil\n",
        )
        .unwrap();
        fs::write(repo.join("tracked.txt"), "AAAA").unwrap();
        fs::write(repo.join("conflict.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        let base_branch = git(&repo, &["branch", "--show-current"]);
        let base_head = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "-qb", "other"]);
        fs::write(repo.join("conflict.txt"), "theirs\n").unwrap();
        fs::write(repo.join("tracked.txt"), "CCCC").unwrap();
        git(&repo, &["commit", "-qam", "other"]);
        let source = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["checkout", "-q", &base_branch]);
        fs::write(repo.join("conflict.txt"), "ours\n").unwrap();
        git(&repo, &["commit", "-qam", "ours"]);
        let target = git(&repo, &["rev-parse", "HEAD"]);
        let canary = root.join("executed-canary");
        // Git interprets driver values as shell commands; if executed this
        // writes a harmless synthetic marker in the disposable fixture.
        let marker = format!("sh -c 'echo ran > \"{}\"; cat'", canary.display());
        git(&repo, &["config", "filter.evil.clean", &marker]);
        git(&repo, &["config", "filter.evil.process", &marker]);
        git(&repo, &["config", "merge.evil.driver", &marker]);
        git(&repo, &["config", "core.fsmonitor", &marker]);
        fs::write(repo.join("tracked.txt"), "BBBB").unwrap();
        let _ = safe_status(&root, &repo, &Cancellation::default());
        {
            let _external = git::external_read_guard();
            let _ = git::status(&root, &repo);
        }
        assert!(
            !canary.exists(),
            "status must not execute project-defined programs"
        );
        let objects = fs::canonicalize(repo.join(".git/objects")).unwrap();
        let (_, _, _, conflict, attrs_safe) = isolated_analysis(
            &root,
            &objects,
            &objects,
            &target,
            &source,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(conflict);
        assert!(
            !attrs_safe,
            "affected custom merge attribute must block application"
        );
        let (included, ff, _, _, attrs_safe) = isolated_analysis(
            &root,
            &objects,
            &objects,
            &base_head,
            &source,
            &Cancellation::default(),
        )
        .unwrap();
        assert!(
            !included && ff && !attrs_safe,
            "fast-forward affected custom merge attribute must fail closed"
        );
        assert!(
            !canary.exists(),
            "merge preview must not execute project-defined programs"
        );
        // Git permits overriding even the builtin-named `union` merge driver.
        // It must be denied before apply just like any custom driver.
        git(&repo, &["config", "--unset", "merge.evil.driver"]);
        git(&repo, &["config", "merge.union.driver", &marker]);
        assert!(!safe_merge_config(&root, &repo, &Cancellation::default()).unwrap());
        git(&repo, &["config", "--unset", "merge.union.driver"]);
        git(
            &repo,
            &[
                "config",
                "url.ssh://invalid.example/.insteadOf",
                "/fixture/source",
            ],
        );
        assert!(
            !safe_merge_config(&root, &repo, &Cancellation::default()).unwrap(),
            "URL rewrites must block pinned local fetch"
        );
        git(
            &repo,
            &["config", "--unset", "url.ssh://invalid.example/.insteadOf"],
        );
        assert!(!canary.exists());

        // A child-only filter is absent from the parent config. A staged
        // gitlink must be detected before status and status must not recurse.
        git(&repo, &["config", "--unset", "core.fsmonitor"]);
        let nested = repo.join("nested");
        fs::create_dir(&nested).unwrap();
        git(&nested, &["init", "-q"]);
        git(&nested, &["config", "user.name", "Fixture"]);
        git(
            &nested,
            &["config", "user.email", "fixture@example.invalid"],
        );
        fs::write(nested.join(".gitattributes"), "nested.txt filter=evil\n").unwrap();
        fs::write(nested.join("nested.txt"), "AAAA").unwrap();
        git(&nested, &["add", "."]);
        git(&nested, &["commit", "-qm", "nested"]);
        let nested_head = git(&nested, &["rev-parse", "HEAD"]);
        let nested_canary = root.join("nested-executed-canary");
        let nested_marker = format!("sh -c 'echo ran > \"{}\"; cat'", nested_canary.display());
        git(&nested, &["config", "filter.evil.clean", &nested_marker]);
        fs::write(nested.join("nested.txt"), "BBBB").unwrap();
        git(
            &repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{nested_head},nested"),
            ],
        );
        assert!(git::submodules(&root, &repo, None).unwrap());
        assert!(!nested_canary.exists());
        {
            let _external = git::external_read_guard();
            let (_, nested_unknown) = git::source_status(&root, &repo).unwrap();
            assert!(
                nested_unknown,
                "source dirty metadata must report nested state unknown"
            );
            assert_eq!(
                git::status(&root, &repo).err().unwrap().code,
                "worktree_submodules_unsupported"
            );
        }
        let _ = safe_status(&root, &repo, &Cancellation::default());
        assert!(
            !nested_canary.exists(),
            "parent status must not run a child-only filter"
        );
    }
}
