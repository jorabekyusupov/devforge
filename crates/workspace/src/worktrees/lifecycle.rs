//! Reviewed archive lifecycle. Only the receipt-owned payload may be purged.
use super::*;
use std::io::Read;
#[derive(Serialize)]
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
) -> Result<()> {
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
            inventory(&entry.path(), h, files, bytes, depth + 1)?;
        }
    } else if m.is_file() {
        *files += 1;
        *bytes += m.len();
        h.update(m.len().to_le_bytes());
        let mut f = fs::File::open(path)?;
        let mut buf = [0u8; 65536];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
    } else {
        return Err(error("worktree_ownership"));
    }
    Ok(())
}
fn copy_payload(
    source: &Path,
    destination: &Path,
    cancel: &Cancellation,
    root: bool,
) -> Result<()> {
    for item in fs::read_dir(source)? {
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
            copy_payload(&from, &to, cancel, false)?;
        } else if m.is_file() {
            fs::copy(from, to)?;
        } else {
            return Err(error("worktree_ownership"));
        }
    }
    Ok(())
}
impl Workspace {
    pub fn worktree_archive_plan(&self, id: &str, operation: &str) -> Result<ArchivePlan> {
        if !["restore", "purge"].contains(&operation) {
            return Err(error("worktree_ownership"));
        }
        let record = self.worktree_record(id)?;
        let dir = self.worktree_dir(id)?;
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
        let mut files = 0;
        let mut bytes = 0;
        let mut h = Sha256::new();
        if blockers.is_empty() && payload.exists() {
            inventory(&payload, &mut h, &mut files, &mut bytes, 0)?;
        }
        if restoring && retained.exists() && dir.join("tree").exists() {
            inventory(&dir.join("tree"), &mut h, &mut files, &mut bytes, 0)?;
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
        let mut plan = ArchivePlan {
            workspace: View {
                files_archived: ordinary(&retained),
                record,
                path: dir.join("tree"),
                retained_path: retained,
            },
            operation: operation.into(),
            files,
            bytes,
            blockers,
            digest: String::new(),
        };
        plan.digest = digest(&(&plan, project, h.finalize().to_vec()));
        Ok(plan)
    }
    pub fn worktree_archive(
        &mut self,
        id: &str,
        operation: &str,
        review: &str,
        consent: bool,
        cancel: &Cancellation,
    ) -> Result<Record> {
        let _lock = self.worktree_lock()?;
        if !consent {
            return Err(error("service_consent_required"));
        }
        let plan = self.worktree_archive_plan(id, operation)?;
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
                    fs::remove_file(&recovery)?;
                }
                git::run(
                    &self.root,
                    &retained,
                    vec![
                        "bundle".into(),
                        "create".into(),
                        recovery.into_os_string(),
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
                                    fs::remove_file(&bundle)?;
                                }
                                git::run(
                                    &self.root,
                                    &p,
                                    vec![
                                        "bundle".into(),
                                        "create".into(),
                                        bundle.into_os_string(),
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
            // Cancellation stops before destruction; once deletion begins it is completed synchronously.
            if cancel.is_cancelled() {
                return Err(error("setup_cancelled"));
            }
            record.stage = "purging".into();
            self.save_worktree(&record)?;
            if retained.exists() {
                fs::remove_dir_all(retained)?;
            }
            record.stage = "purged".into();
            self.save_worktree(&record)?;
            return Ok(record);
        }
        let project = record
            .project_id
            .clone()
            .ok_or_else(|| error("worktree_archive_unavailable"))?;
        let old = self
            .projects
            .archived_worktree(&project, id)
            .or_else(|_| self.projects.get(&project))?;
        record.stage = "restoring".into();
        self.save_worktree(&record)?;
        if record.isolated {
            if !path.exists() {
                fs::rename(&retained, &path)?;
            }
        } else {
            if !path.exists() {
                self.worktree_git(
                    &record.common_dir,
                    vec![
                        "worktree".into(),
                        "add".into(),
                        "--".into(),
                        path.clone().into_os_string(),
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
            copy_payload(&retained, &path, cancel, true)?;
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
            self.projects
                .reactivate_worktree(&project, id, &path, old.revision)?;
        }
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
        // Keep linked-worktree archive as a recovery copy; next cleanup creates a fresh archive.
        if retained.exists() {
            fs::remove_dir_all(retained)?;
        }
        record.stage = "ready".into();
        record.last_error = None;
        self.save_worktree(&record)?;
        Ok(record)
    }
}
