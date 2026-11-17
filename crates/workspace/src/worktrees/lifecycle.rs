//! Reviewed archive lifecycle. Only the receipt-owned payload may be purged.
use super::*;
use std::io::Read;
#[derive(Clone, Serialize)]
pub struct ArchivePlan {
    pub workspace: View,
    pub operation: String,
    pub files: u64,
    pub bytes: u64,
    pub blockers: Vec<String>,
    pub digest: String,
}
// Content hashes detect changes to ignored SQLite/environment files as well as Git files.
fn inventory(
    path: &Path,
    h: &mut Sha256,
    files: &mut u64,
    bytes: &mut u64,
    depth: usize,
    cancel: &Cancellation,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    check()?;
    if cancel.is_cancelled() {
        return Err(error("setup_cancelled"));
    }
    if depth > 100 || *files > 100000 || *bytes > 64 * 1024 * 1024 * 1024 {
        return Err(error("worktree_archive_limit"));
    }
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() {
        h.update(
            fs::read_link(path)?
                .as_os_str()
                .to_string_lossy()
                .as_bytes(),
        );
        return Ok(());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(error("worktree_ownership"));
        }
    }
    if m.is_dir() {
        let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        h.update(b"directory\0");
        for entry in entries {
            h.update(entry.file_name().to_string_lossy().as_bytes());
            h.update(b"\0");
            inventory(&entry.path(), h, files, bytes, depth + 1, cancel, check)?;
        }
    } else if m.is_file() {
        *files += 1;
        *bytes += m.len();
        if *files > 100000 || *bytes > 64 * 1024 * 1024 * 1024 {
            return Err(error("worktree_archive_limit"));
        }
        h.update(m.len().to_le_bytes());
        let mut f = fs::File::open(path)?;
        let mut buf = [0u8; 65536];
        let mut read = 0u64;
        loop {
            check()?;
            if cancel.is_cancelled() {
                return Err(error("setup_cancelled"));
            }
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            read += n as u64;
            if read > m.len() {
                return Err(error("worktree_review_changed"));
            }
            h.update(&buf[..n]);
        }
        if read != m.len() {
            return Err(error("worktree_review_changed"));
        }
    } else {
        return Err(error("worktree_ownership"));
    }
    Ok(())
}
/// Private owned-project review inputs, never returned directly to clients.
pub struct ArchiveReviewSnapshot {
    plan: ArchivePlan,
    project: Option<Project>,
    inventories: Vec<PathBuf>,
}
impl ArchiveReviewSnapshot {
    pub fn fingerprint(&self) -> String {
        digest(&(&self.plan, &self.project, &self.inventories))
    }
    pub fn finish(
        &self,
        cancel: &Cancellation,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<ArchivePlan> {
        let mut files = 0;
        let mut bytes = 0;
        let mut h = Sha256::new();
        check()?;
        let mut last = std::time::Instant::now();
        let periodic = std::cell::RefCell::new(&mut last);
        let bounded_check = || -> Result<()> {
            if cancel.is_cancelled() {
                return Err(error("setup_cancelled"));
            }
            let mut observed = periodic.borrow_mut();
            if observed.elapsed() >= std::time::Duration::from_millis(200) {
                check()?;
                **observed = std::time::Instant::now();
            }
            Ok(())
        };
        for path in &self.inventories {
            inventory(
                path,
                &mut h,
                &mut files,
                &mut bytes,
                0,
                cancel,
                &bounded_check,
            )?;
        }
        check()?;
        let mut plan = self.plan.clone();
        plan.files = files;
        plan.bytes = bytes;
        plan.digest = digest(&(&plan, &self.project, h.finalize().to_vec()));
        Ok(plan)
    }
}
fn copy_payload(
    source: &Path,
    destination: &Path,
    cancel: &Cancellation,
    root: bool,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    for item in fs::read_dir(source)? {
        check()?;
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        let item = item?;
        if root && item.file_name() == ".git" {
            continue;
        }
        let from = item.path();
        let to = destination.join(item.file_name());
        let m = fs::symlink_metadata(&from)?;
        if fs::symlink_metadata(&to).is_ok() {
            if to.is_dir() && !fs::symlink_metadata(&to)?.file_type().is_symlink() {
                fs::remove_dir_all(&to)?;
            } else {
                fs::remove_file(&to)?;
            }
        }
        if m.file_type().is_symlink() {
            let link = fs::read_link(&from)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(link, to)?;
            #[cfg(windows)]
            {
                if from.is_dir() {
                    std::os::windows::fs::symlink_dir(link, to)?;
                } else {
                    std::os::windows::fs::symlink_file(link, to)?;
                }
            }
        } else if m.is_dir() {
            fs::create_dir(&to)?;
            copy_payload(&from, &to, cancel, false, check)?;
        } else if m.is_file() {
            fs::copy(from, to)?;
        } else {
            return Err(error("worktree_ownership"));
        }
    }
    Ok(())
}
impl Workspace {
    /// Bind nested Git metadata subprocesses to the caller's cancellation token
    /// for this owner-thread scope. The previous token is restored on return.
    pub fn external_with_git_cancellation<T>(
        &self,
        cancel: &Cancellation,
        action: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let _safe = git::external_read_guard();
        git::with_read_cancellation(cancel, action)
    }
    pub fn worktree_archive_plan(&self, id: &str, operation: &str) -> Result<ArchivePlan> {
        self.worktree_archive_review(id, operation, &Cancellation::default())
    }
    pub fn worktree_archive_review(
        &self,
        id: &str,
        operation: &str,
        cancel: &Cancellation,
    ) -> Result<ArchivePlan> {
        let record = self.worktree_record(id)?;
        let dir = self.worktree_dir(id)?;
        self.archive_review_snapshot(record, dir, operation)?
            .finish(cancel, &|| Ok(()))
    }
    /// Actor captures scoped private metadata without creating a worktree directory.
    pub fn external_archive_review_snapshot(
        &self,
        source: &str,
        id: &str,
        operation: &str,
    ) -> Result<ArchiveReviewSnapshot> {
        let record = self.external_worktree_record(source, id)?;
        let dir = self.root.join("worktrees").join(id);
        self.archive_review_snapshot(record, dir, operation)
    }
    fn archive_review_snapshot(
        &self,
        record: Record,
        dir: PathBuf,
        operation: &str,
    ) -> Result<ArchiveReviewSnapshot> {
        let id = record.id.as_str();
        if !["restore", "purge"].contains(&operation) {
            return Err(error("worktree_ownership"));
        }
        let retained = dir.join("retained");
        let mut blockers = vec![];
        let restoring = operation == "restore" && record.stage == "restoring";
        let purging = operation == "purge" && record.stage == "purging";
        let payload = if restoring && !retained.exists() {
            dir.join("tree")
        } else {
            retained.clone()
        };
        if !(record.stage == "removed" || restoring || purging)
            || (!purging && (!ordinary(&payload) || fs::canonicalize(&payload)? != payload))
            || (!restoring && dir.join("tree").exists())
        {
            blockers.push("worktree_archive_unavailable".into());
        }
        if self.projects.list()?.iter().any(|p| {
            p.path.starts_with(&dir)
                && (!restoring || Some(p.id.as_str()) != record.project_id.as_deref())
        }) {
            blockers.push("worktree_nested_project".into());
        }
        let project = record
            .project_id
            .as_deref()
            .map(|p| {
                self.projects
                    .archived_worktree(p, id)
                    .or_else(|_| self.projects.get(p))
            })
            .transpose()?;
        if project
            .as_ref()
            .is_some_and(|p| p.worktree.as_deref() != Some(id) || p.path != dir.join("tree"))
        {
            return Err(error("worktree_ownership"));
        }
        if operation == "restore" {
            if project.is_none() {
                blockers.push("worktree_archive_unavailable".into());
            }
            self.validate_site(record.project_id.as_deref().unwrap_or(""), &record.domain)?;
            if !record.isolated {
                if git::oid(&self.root, &record.source_path, &record.input.branch)
                    .ok()
                    .as_deref()
                    != Some(&record.commit)
                {
                    blockers.push("worktree_review_changed".into());
                }
                let listed = git::read(
                    &self.root,
                    &record.source_path,
                    &["worktree", "list", "--porcelain", "-z"],
                )?;
                if !restoring
                    && listed.split(|b| *b == 0).any(|l| {
                        l == format!("branch refs/heads/{}", record.input.branch).as_bytes()
                    })
                {
                    blockers.push("worktree_branch_in_use".into());
                }
            } else {
                self.worktree_identity(&record, &payload)?;
            }
        }
        let inventories = if blockers.is_empty() && payload.exists() {
            vec![payload.clone()]
        } else {
            Vec::new()
        };
        let mut inventories = inventories;
        if restoring && retained.exists() && dir.join("tree").exists() {
            inventories.push(dir.join("tree"));
        }
        let plan = ArchivePlan {
            workspace: View {
                files_archived: ordinary(&retained),
                record,
                path: dir.join("tree"),
                retained_path: retained,
            },
            operation: operation.into(),
            files: 0,
            bytes: 0,
            blockers,
            digest: String::new(),
        };
        Ok(ArchiveReviewSnapshot {
            plan,
            project,
            inventories,
        })
    }
    pub fn worktree_archive(
        &mut self,
        id: &str,
        operation: &str,
        review: &str,
        consent: bool,
        cancel: &Cancellation,
    ) -> Result<Record> {
        self.worktree_archive_guarded(id, operation, review, consent, cancel, &|| Ok(()))
    }
    pub fn worktree_archive_guarded(
        &mut self,
        id: &str,
        operation: &str,
        review: &str,
        consent: bool,
        cancel: &Cancellation,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Record> {
        check()?;
        let _lock = self.worktree_lock()?;
        check()?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        let record = self.worktree_record(id)?;
        let dir = self.worktree_dir(id)?;
        let plan = self
            .archive_review_snapshot(record, dir, operation)?
            .finish(cancel, check)?;
        check()?;
        if plan.digest != review {
            return Err(error("worktree_review_changed"));
        }
        if let Some(code) = plan.blockers.first() {
            return Err(error(code));
        }
        if cancel.is_cancelled() {
            return Err(error("setup_cancelled"));
        }
        let mut record = plan.workspace.record;
        let retained = plan.workspace.retained_path;
        let path = plan.workspace.path;
        if operation == "purge" {
            // Isolated repositories contain branch history. Keep recoverable bundles outside payload.
            if record.isolated && retained.exists() && record.stage != "purging" {
                let recovery = self.worktree_dir(id)?.join("history.bundle");
                if recovery.exists() {
                    check()?;
                    fs::remove_file(&recovery)?;
                }
                check()?;
                git::run(
                    &self.root,
                    &retained,
                    vec![
                        "bundle".into(),
                        "create".into(),
                        dunce::simplified(&recovery).as_os_str().to_owned(),
                        "--all".into(),
                    ],
                    true,
                    cancel,
                )?;
                // Preserve every nested repository's commits, including detached submodule HEADs.
                let mut pending = vec![retained.clone()];
                let mut number = 0;
                while let Some(dir) = pending.pop() {
                    for item in fs::read_dir(&dir)? {
                        check()?;
                        let item = item?;
                        let p = item.path();
                        if item.file_name() == ".git" {
                            continue;
                        }
                        if ordinary(&p) {
                            if fs::symlink_metadata(p.join(".git")).is_ok() {
                                number += 1;
                                let bundle = self
                                    .worktree_dir(id)?
                                    .join(format!("submodule-{number}.bundle"));
                                if bundle.exists() {
                                    check()?;
                                    fs::remove_file(&bundle)?;
                                }
                                check()?;
                                git::run(
                                    &self.root,
                                    &p,
                                    vec![
                                        "bundle".into(),
                                        "create".into(),
                                        dunce::simplified(&bundle).as_os_str().to_owned(),
                                        "--all".into(),
                                        "HEAD".into(),
                                    ],
                                    true,
                                    cancel,
                                )?;
                            }
                            pending.push(p);
                        }
                    }
                }
            }
            if record.stage != "purging"
                && self
                    .archive_review_snapshot(
                        self.worktree_record(id)?,
                        self.worktree_dir(id)?,
                        operation,
                    )?
                    .finish(cancel, check)?
                    .digest
                    != review
            {
                return Err(error("worktree_review_changed"));
            }
            // Cancellation stops before destruction; once deletion begins it is completed synchronously.
            if cancel.is_cancelled() {
                return Err(error("setup_cancelled"));
            }
            check()?;
            record.stage = "purging".into();
            self.save_worktree(&record)?;
            if retained.exists() {
                // Reauthorize after the journal write, immediately before the
                // irreversible owned deletion begins. Once started, finish it.
                check()?;
                if cancel.is_cancelled() {
                    return Err(error("setup_cancelled"));
                }
                erase::payload(&retained)?;
            }
            record.stage = "purged".into();
            self.save_worktree(&record)?;
            return Ok(record);
        }
        let project = record
            .project_id
            .clone()
            .ok_or_else(|| error("worktree_archive_unavailable"))?;
        let _project_guard = self.sites.project_mutation_guard(&project)?;
        let old = self
            .projects
            .archived_worktree(&project, id)
            .or_else(|_| self.projects.get(&project))?;
        check()?;
        record.stage = "restoring".into();
        self.save_worktree(&record)?;
        if record.isolated {
            if !path.exists() {
                check()?;
                fs::rename(&retained, &path)?;
            }
        } else {
            if !path.exists() {
                check()?;
                self.worktree_git(
                    &record.common_dir,
                    vec![
                        "worktree".into(),
                        "add".into(),
                        "--".into(),
                        dunce::simplified(&path).as_os_str().to_owned(),
                        record.input.branch.clone().into(),
                    ],
                    cancel,
                )?;
                record.git_dir = Some(fs::canonicalize(git::text(
                    &self.root,
                    &path,
                    &["rev-parse", "--absolute-git-dir"],
                )?)?);
                self.save_worktree(&record)?;
            }
            self.worktree_identity(&record, &path)?;
            if retained.exists() {
                // Bound content-scan reauthorization without a registry/SQLite
                // transaction for every 64 KiB of a large retained file.
                let last_check = std::cell::Cell::new(std::time::Instant::now());
                let scan_check = || -> Result<()> {
                    if cancel.is_cancelled() {
                        return Err(error("setup_cancelled"));
                    }
                    if last_check.get().elapsed() >= std::time::Duration::from_millis(200) {
                        check()?;
                        last_check.set(std::time::Instant::now());
                    }
                    Ok(())
                };
                check()?;
                let (mut before, mut files, mut bytes) = (Sha256::new(), 0, 0);
                inventory(
                    &retained,
                    &mut before,
                    &mut files,
                    &mut bytes,
                    0,
                    cancel,
                    &scan_check,
                )?;
                check()?;
                copy_payload(&retained, &path, cancel, true, check)?;
                let (mut after, mut files, mut bytes) = (Sha256::new(), 0, 0);
                inventory(
                    &retained,
                    &mut after,
                    &mut files,
                    &mut bytes,
                    0,
                    cancel,
                    &scan_check,
                )?;
                check()?;
                if before.finalize() != after.finalize() {
                    return Err(error("worktree_review_changed"));
                }
            } else if !old.active {
                return Err(error("worktree_ownership"));
            }
        }
        record.git_dir = Some(fs::canonicalize(git::text(
            &self.root,
            &path,
            &["rev-parse", "--absolute-git-dir"],
        )?)?);
        self.worktree_identity(&record, &path)?;
        if git::oid(&self.root, &path, "HEAD")? != record.commit {
            return Err(error("worktree_review_changed"));
        }
        if !old.active {
            check()?;
            self.projects
                .reactivate_worktree(&project, id, &path, old.revision)?;
        }
        check()?;
        self.sites.configure(
            &self.projects,
            Site {
                project_id: project.clone(),
                domain: record.domain.clone(),
                https: record.configuration.project.https,
                vite: record.configuration.project.vite,
            },
        )?;
        self.preferences.development.insert(
            project,
            Development {
                queue: record.configuration.project.queue,
            },
        );
        self.save()?;
        // The archive remains intact until activation succeeds; remove only this duplicate afterwards.
        if retained.exists() {
            check()?;
            erase::payload(&retained)?;
        }
        record.stage = "ready".into();
        record.last_error = None;
        self.save_worktree(&record)?;
        Ok(record)
    }
}

#[cfg(test)]
mod guarded_inventory_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn scan_stops_before_content_when_authority_revoked() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("item");
        fs::write(&file, b"fixture").unwrap();
        let calls = AtomicUsize::new(0);
        let denied = || {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(error("grant_revoked"))
        };
        let result = inventory(
            &file,
            &mut Sha256::new(),
            &mut 0,
            &mut 0,
            0,
            &Cancellation::default(),
            &denied,
        );
        assert_eq!(result.err().unwrap().code, "grant_revoked");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(unix)]
    #[test]
    fn inventory_hashes_symlink_target_without_reading_external_content() {
        use std::os::unix::fs::symlink;
        let owned = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("private");
        fs::write(&target, b"SYNTHETIC_OUTSIDE_CONTENT").unwrap();
        let link = owned.path().join("link");
        symlink(&target, &link).unwrap();
        let (mut hash, mut files, mut bytes) = (Sha256::new(), 0, 0);
        inventory(
            &link,
            &mut hash,
            &mut files,
            &mut bytes,
            0,
            &Cancellation::default(),
            &|| Ok(()),
        )
        .unwrap();
        assert_eq!((files, bytes), (0, 0));
        assert_eq!(
            hash.finalize().to_vec(),
            Sha256::digest(target.as_os_str().to_string_lossy().as_bytes()).to_vec()
        );
    }
}
