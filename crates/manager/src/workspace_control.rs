//! Scoped A10 worktree preparation. Public responses never include paths or
//! serialized worktree records; only the manager retains these private plans.
use devforge_control::{ErrorCode, Id};
use devforge_runtime::Cancellation;
use devforge_workspace::{
    Workspace,
    worktrees::{Input, MergePlan, Record},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
};

#[derive(Clone)]
pub(crate) enum Action {
    Create(Input),
    Archive(Id),
    Restore(Id),
    Purge(Id),
    Merge(Id),
}
impl Action {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Create(_) => "create",
            Self::Archive(_) => "archive",
            Self::Restore(_) => "restore",
            Self::Purge(_) => "purge",
            Self::Merge(_) => "merge",
        }
    }
    pub(crate) fn clones_database(&self) -> bool {
        matches!(self, Self::Create(input) if input.database == devforge_workspace::worktrees::DatabaseMode::Clone)
    }
    pub(crate) fn sensitive(&self) -> bool {
        self.copies_secrets() || self.clones_database() || matches!(self, Self::Merge(_))
    }
    pub(crate) fn copies_secrets(&self) -> bool {
        matches!(self, Self::Create(input) if input.copy_environment)
    }
    pub(crate) fn id(&self) -> Option<&Id> {
        match self {
            Self::Create(_) => None,
            Self::Archive(id) | Self::Restore(id) | Self::Purge(id) | Self::Merge(id) => Some(id),
        }
    }
}
#[derive(Clone)]
pub(crate) struct CreateReview {
    pub source_commit: String,
    pub source_dirty: Option<bool>,
    pub isolated: bool,
}
#[derive(Clone)]
pub(crate) struct Prepared {
    pub action: Action,
    pub core_digest: String,
    pub blockers: Vec<String>,
    pub record: Option<Record>,
    pub merge: Option<MergePlan>,
    pub create: Option<CreateReview>,
}
impl Prepared {
    pub(crate) fn read(
        workspace: &Workspace,
        project: &Id,
        action: Action,
        cancel: &Cancellation,
    ) -> devforge_workspace::Result<Self> {
        match &action {
            Action::Create(input) => {
                if input.source_id != project.as_str() {
                    return Err(crate::service_control::ws_error(ErrorCode::ProjectMismatch));
                }
                let plan = workspace.worktree_plan(input)?;
                let source_dirty = if plan.source_dirty {
                    Some(true)
                } else if plan.source_dirty_unknown {
                    None
                } else {
                    Some(false)
                };
                Ok(Self {
                    action,
                    create: Some(CreateReview {
                        source_commit: plan.commit,
                        source_dirty,
                        isolated: plan.isolated,
                    }),
                    core_digest: plan.digest,
                    blockers: plan.blockers,
                    record: None,
                    merge: None,
                })
            }
            Action::Archive(id) => {
                let record = workspace.external_worktree_record(project.as_str(), id.as_str())?;
                let plan = workspace.worktree_cleanup_plan(id.as_str())?;
                Ok(Self {
                    action,
                    core_digest: plan.digest,
                    blockers: plan.blockers,
                    record: Some(record),
                    merge: None,
                    create: None,
                })
            }
            Action::Restore(id) | Action::Purge(id) => {
                let record = workspace.external_worktree_record(project.as_str(), id.as_str())?;
                let plan = workspace.worktree_archive_review(id.as_str(), action.name(), cancel)?;
                Ok(Self {
                    action,
                    core_digest: plan.digest,
                    blockers: plan.blockers,
                    record: Some(record),
                    merge: None,
                    create: None,
                })
            }
            Action::Merge(id) => {
                let record = workspace.external_worktree_record(project.as_str(), id.as_str())?;
                let plan = workspace.worktree_merge_plan(project.as_str(), id.as_str())?;
                Ok(Self {
                    action,
                    core_digest: plan.digest.clone(),
                    blockers: plan.blockers.clone(),
                    record: Some(record),
                    merge: Some(plan),
                    create: None,
                })
            }
        }
    }
}
pub(crate) enum Captured {
    Ready(Box<Prepared>),
    Archive {
        action: Action,
        record: Box<Record>,
        snapshot: Box<devforge_workspace::worktrees::ArchiveReviewSnapshot>,
    },
}
type Check = Box<dyn Fn(&Workspace) -> devforge_workspace::Result<()> + Send>;
pub(crate) struct WorkspaceRead {
    pub project: Id,
    pub action: Action,
    pub check: Check,
    pub cancelled: Arc<AtomicBool>,
    pub cancel: Cancellation,
    pub reply: SyncSender<devforge_workspace::Result<Captured>>,
}
impl WorkspaceRead {
    pub(crate) fn execute(self, workspace: &Workspace) {
        let result = (|| {
            let check = || {
                if self.cancelled.load(Ordering::Acquire) || self.cancel.is_cancelled() {
                    return Err(crate::service_control::ws_error(ErrorCode::Interrupted));
                }
                (self.check)(workspace)?;
                if self.cancelled.load(Ordering::Acquire) || self.cancel.is_cancelled() {
                    return Err(crate::service_control::ws_error(ErrorCode::Interrupted));
                }
                Ok(())
            };
            check()?;
            let value = workspace.external_with_git_cancellation(&self.cancel, || {
                Ok(match &self.action {
                    Action::Restore(id) | Action::Purge(id) => {
                        let record = workspace
                            .external_worktree_record(self.project.as_str(), id.as_str())?;
                        let snapshot = workspace.external_archive_review_snapshot(
                            self.project.as_str(),
                            id.as_str(),
                            self.action.name(),
                        )?;
                        Captured::Archive {
                            action: self.action.clone(),
                            record: Box::new(record),
                            snapshot: Box::new(snapshot),
                        }
                    }
                    _ => Captured::Ready(Box::new(Prepared::read(
                        workspace,
                        &self.project,
                        self.action.clone(),
                        &self.cancel,
                    )?)),
                })
            })?;
            check()?;
            Ok(value)
        })();
        let _ = self.reply.send(result);
    }
}

pub(crate) fn parse(
    project: &Id,
    input: devforge_control::tools::WorkspacePrepareInput,
) -> Result<Action, ErrorCode> {
    use devforge_control::tools::{WorkspaceDatabase, WorkspacePrepareAction as Kind};
    let has_create = input.name.is_some()
        || input.branch.is_some()
        || input.base.is_some()
        || input.new_branch.is_some()
        || input.database.is_some()
        || input.dependencies.is_some()
        || input.copy_environment.is_some();
    Ok(match input.action {
        Kind::Create => {
            if input.workspace_id.is_some() {
                return Err(ErrorCode::InvalidInput);
            }
            Action::Create(Input {
                source_id: project.as_str().into(),
                name: input.name.ok_or(ErrorCode::InvalidInput)?.as_str().into(),
                branch: input.branch.ok_or(ErrorCode::InvalidInput)?.as_str().into(),
                base: input.base.ok_or(ErrorCode::InvalidInput)?.as_str().into(),
                new_branch: input.new_branch.ok_or(ErrorCode::InvalidInput)?,
                database: match input.database.ok_or(ErrorCode::InvalidInput)? {
                    WorkspaceDatabase::Empty => devforge_workspace::worktrees::DatabaseMode::Empty,
                    WorkspaceDatabase::Clone => devforge_workspace::worktrees::DatabaseMode::Clone,
                },
                dependencies: input.dependencies.ok_or(ErrorCode::InvalidInput)?,
                copy_environment: input.copy_environment.ok_or(ErrorCode::InvalidInput)?,
            })
        }
        action => {
            if has_create {
                return Err(ErrorCode::InvalidInput);
            }
            let id = input.workspace_id.ok_or(ErrorCode::InvalidInput)?;
            match action {
                Kind::Archive => Action::Archive(id),
                Kind::Restore => Action::Restore(id),
                Kind::Purge => Action::Purge(id),
                Kind::Merge => Action::Merge(id),
                _ => return Err(ErrorCode::InvalidInput),
            }
        }
    })
}
pub(crate) fn reference(
    input: &devforge_control::mutations::MutationInput,
) -> Result<(&devforge_control::Digest, Option<&Id>, &'static str), ErrorCode> {
    use devforge_control::mutations::MutationInput;
    match input {
        MutationInput::WorkspaceCreate(v) => Ok((&v.plan, None, "create")),
        MutationInput::WorkspaceArchive(v) => Ok((&v.plan, Some(&v.workspace_id), "archive")),
        MutationInput::WorkspaceRestore(v) => Ok((&v.plan, Some(&v.workspace_id), "restore")),
        MutationInput::WorkspacePurge(v) => Ok((&v.plan, Some(&v.workspace_id), "purge")),
        MutationInput::WorkspaceMerge(v) => Ok((&v.plan, Some(&v.workspace_id), "merge")),
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}

/// Stops and joins the cooperative authorization/deadline monitor on every exit.
pub(crate) struct Monitor {
    done: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Monitor {
    pub(crate) fn new(done: Arc<AtomicBool>, worker: std::thread::JoinHandle<()>) -> Self {
        Self {
            done,
            worker: Some(worker),
        }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
