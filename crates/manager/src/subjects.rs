//! Trusted metadata attribution for resource commands and partial project creation.
//!
//! Callers supply manager-owned snapshots, never renderer JSON projections. The
//! worker resolves again immediately before execution because admission's cache
//! can be older than queued mutations. This module reads no credentials, executes
//! no commands and writes no filesystem or registry state.
use crate::actor::ROOT_SUBJECT;
use devforge_control::Id;
use devforge_workspace::{Command, Snapshot, setup::CreateInput};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const PROJECT_LIMIT: usize = 2048;
const WORKTREE_LIMIT: usize = 1000;
const CACHE_LIMIT: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubjectError {
    MissingResource,
    AmbiguousResource,
    InvalidSubject,
    MetadataLimit,
}

fn project_id(value: &str) -> Result<Id, SubjectError> {
    if value == ROOT_SUBJECT {
        return Err(SubjectError::InvalidSubject);
    }
    Id::new(value).map_err(|_| SubjectError::InvalidSubject)
}

/// None leaves ordinary direct-project/global command attribution to the actor.
/// Worktree namespace is ALWAYS its persisted source project, including after
/// child registration/archive. This matches control-adapter's workspace binding.
pub(crate) fn resource_subject(
    command: &Command,
    snapshot: &Snapshot,
) -> Result<Option<Id>, SubjectError> {
    let worktree = match command {
        Command::WorktreePlan { input } | Command::WorktreeCreate { input, .. } => {
            return project_id(&input.source_id).map(Some);
        }
        Command::WorktreeArchivePlan { id, .. }
        | Command::WorktreeArchive { id, .. }
        | Command::WorktreeRemotePlan { id, .. }
        | Command::WorktreeRemoteCopy { id, .. }
        | Command::WorktreeOpen { id }
        | Command::WorktreeCleanupPlan { id }
        | Command::WorktreeCleanup { id, .. } => Some(id),
        _ => None,
    };
    if let Some(id) = worktree {
        if snapshot.worktrees.len() > WORKTREE_LIMIT {
            return Err(SubjectError::MetadataLimit);
        }
        let mut matching = snapshot
            .worktrees
            .iter()
            .filter(|view| view.record.id == *id);
        let record = &matching.next().ok_or(SubjectError::MissingResource)?.record;
        if matching.next().is_some() {
            return Err(SubjectError::AmbiguousResource);
        }
        if ![1, 2].contains(&record.schema) {
            return Err(SubjectError::InvalidSubject);
        }
        return project_id(&record.input.source_id).map(Some);
    }
    if let Command::StartCache { instance } | Command::StopCache { instance } = command {
        if snapshot.caches.len() > CACHE_LIMIT {
            return Err(SubjectError::MetadataLimit);
        }
        let mut matching = snapshot
            .caches
            .iter()
            .filter(|cache| cache.instance.id == *instance);
        let cache = matching.next().ok_or(SubjectError::MissingResource)?;
        if matching.next().is_some() {
            return Err(SubjectError::AmbiguousResource);
        }
        // Dedicated cache ownership survives deselection or project archival;
        // ordinary domain readiness/in-use checks still run during execution.
        return project_id(&cache.instance.project_id).map(Some);
    }
    Ok(None)
}

/// Transient proof of one worker's CreateProject attempt. Never serialized into
/// an operation receipt or accepted from the caller. A domain error's project ID
/// is merely a candidate until recover_project verifies this complete context.
pub(crate) struct CreateAttempt {
    root: PathBuf,
    operation: Id,
    parent: PathBuf,
    destination: PathBuf,
    before: BTreeSet<String>,
    name: String,
    php: String,
    node: String,
}

impl CreateAttempt {
    /// Invalid/incomplete baseline disables recovery attribution, not the domain
    /// operation itself: its normal validation error remains reportable.
    pub(crate) fn capture(
        root: &Path,
        operation: &Id,
        input: &CreateInput,
        before: &Snapshot,
    ) -> Option<Self> {
        if before.projects.len() > PROJECT_LIMIT
            || devforge_workspace::setup::valid_folder(&input.folder).is_err()
        {
            return None;
        }
        let root = fs::canonicalize(root).ok()?;
        let parent = fs::canonicalize(&input.parent).ok()?;
        if !root.is_dir() || !parent.is_dir() {
            return None;
        }
        let destination = parent.join(&input.folder);
        match fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return None,
        }
        if before
            .projects
            .iter()
            .any(|row| row.project.path == destination)
        {
            return None;
        }
        let ids = before
            .projects
            .iter()
            .map(|row| row.project.id.clone())
            .collect::<BTreeSet<_>>();
        if ids.len() != before.projects.len() {
            return None;
        }
        Some(Self {
            root,
            operation: operation.clone(),
            parent,
            destination,
            before: ids,
            name: input.name.clone(),
            php: input.php.clone(),
            node: input.node.clone(),
        })
    }

    /// Return a separate recovery association, never a replacement for the
    /// ROOT_SUBJECT receipt namespace. actual_identity must be the trusted OS
    /// adapter (projects::discovery::identity), not a caller-reported identity.
    pub(crate) fn recover_project(
        &self,
        root: &Path,
        operation: &Id,
        after: &Snapshot,
        error_project: &str,
        actual_identity: impl FnOnce(&Path) -> Option<String>,
    ) -> Option<Id> {
        if operation != &self.operation
            || fs::canonicalize(root).ok()? != self.root
            || fs::canonicalize(&self.parent).ok()? != self.parent
            || after.projects.len() > PROJECT_LIMIT
            || self.before.contains(error_project)
        {
            return None;
        }
        let id = project_id(error_project).ok()?;
        let mut candidates = after
            .projects
            .iter()
            .filter(|row| row.project.path == self.destination);
        let project = &candidates.next()?.project;
        if candidates.next().is_some()
            || project.id != error_project
            || !project.active
            || project.worktree.is_some()
            || project.name != self.name
            || project.php != self.php
            || project.node != self.node
            || after
                .projects
                .iter()
                .filter(|row| row.project.id == error_project)
                .count()
                != 1
            || fs::canonicalize(&project.path).ok()? != self.destination
        {
            return None;
        }
        let metadata = fs::symlink_metadata(&project.path).ok()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return None;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return None;
            }
        }
        let stored = project.filesystem_identity.as_deref()?;
        if !stored.starts_with("file:") || actual_identity(&project.path).as_deref() != Some(stored)
        {
            return None;
        }
        Some(id)
    }
}

/// Proof of one WorktreeCreate's source and previously existing registry/receipt
/// IDs. Child recovery is separate metadata; the operation keeps its source ID.
pub(crate) struct WorktreeAttempt {
    root: PathBuf,
    operation: Id,
    input: Vec<u8>,
    review: String,
    before_projects: BTreeSet<String>,
    before_worktrees: BTreeSet<String>,
    source: SourceProof,
}

struct SourceProof {
    id: Id,
    path: PathBuf,
    identity: String,
    revision: u64,
    php: String,
    node: String,
    composer: String,
    php_version: String,
    node_version: String,
    composer_version: String,
}

fn ordinary_directory(path: &Path) -> Option<()> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || fs::canonicalize(path).ok()? != path
    {
        return None;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return None;
        }
    }
    Some(())
}

fn runtime_version(snapshot: &Snapshot, id: &str, kind: &str) -> Option<String> {
    if snapshot.runtimes.len() > PROJECT_LIMIT {
        return None;
    }
    let mut matching = snapshot.runtimes.iter().filter(|runtime| runtime.id == id);
    let runtime = matching.next()?;
    if matching.next().is_some() || runtime.kind != kind || runtime.version.len() > 128 {
        return None;
    }
    Some(runtime.version.clone())
}

impl WorktreeAttempt {
    pub(crate) fn capture(
        root: &Path,
        operation: &Id,
        input: &devforge_workspace::worktrees::Input,
        review: &str,
        before: &Snapshot,
    ) -> Option<Self> {
        if before.projects.len() > PROJECT_LIMIT
            || before.worktrees.len() > WORKTREE_LIMIT
            || input.source_id.len() > 128
            || input.name.len() > 200
            || input.branch.len() > 200
            || input.base.len() > 200
            || review.len() != 64
            || !review.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }
        let root = fs::canonicalize(root).ok()?;
        ordinary_directory(&root.join("worktrees"))?;
        let source_id = project_id(&input.source_id).ok()?;
        let mut source_rows = before
            .projects
            .iter()
            .filter(|row| row.project.id == input.source_id);
        let source = &source_rows.next()?.project;
        if source_rows.next().is_some() || !source.active {
            return None;
        }
        ordinary_directory(&source.path)?;
        let identity = source.filesystem_identity.as_deref()?;
        if !identity.starts_with("file:") {
            return None;
        }
        let before_projects = before
            .projects
            .iter()
            .map(|row| row.project.id.clone())
            .collect::<BTreeSet<_>>();
        let before_worktrees = before
            .worktrees
            .iter()
            .map(|view| view.record.id.clone())
            .collect::<BTreeSet<_>>();
        if before_projects.len() != before.projects.len()
            || before_worktrees.len() != before.worktrees.len()
        {
            return None;
        }
        Some(Self {
            root,
            operation: operation.clone(),
            input: serde_json::to_vec(input).ok()?,
            review: review.into(),
            before_projects,
            before_worktrees,
            source: SourceProof {
                id: source_id,
                path: source.path.clone(),
                identity: identity.into(),
                revision: source.revision,
                php: source.php.clone(),
                node: source.node.clone(),
                composer: source.composer.clone(),
                php_version: runtime_version(before, &source.php, "php")?
                    .split('-')
                    .next()?
                    .into(),
                node_version: runtime_version(before, &source.node, "node")?
                    .trim_start_matches('v')
                    .into(),
                composer_version: runtime_version(before, &source.composer, "composer")?,
            },
        })
    }

    pub(crate) fn recover_project(
        &self,
        root: &Path,
        operation: &Id,
        after: &Snapshot,
        error_project: &str,
        error_code: &str,
        actual_identity: impl Fn(&Path) -> Option<String>,
    ) -> Option<Id> {
        if operation != &self.operation
            || fs::canonicalize(root).ok()? != self.root
            || after.projects.len() > PROJECT_LIMIT
            || after.worktrees.len() > WORKTREE_LIMIT
            || self.before_projects.contains(error_project)
        {
            return None;
        }
        let id = project_id(error_project).ok()?;
        let mut source_rows = after
            .projects
            .iter()
            .filter(|row| row.project.id == self.source.id.as_str());
        let source = &source_rows.next()?.project;
        if source_rows.next().is_some()
            || !source.active
            || source.path != self.source.path
            || source.revision != self.source.revision
            || source.php != self.source.php
            || source.node != self.source.node
            || source.composer != self.source.composer
            || source.filesystem_identity.as_deref() != Some(self.source.identity.as_str())
        {
            return None;
        }
        ordinary_directory(&source.path)?;
        if actual_identity(&source.path).as_deref() != Some(self.source.identity.as_str()) {
            return None;
        }
        let mut receipts = after
            .worktrees
            .iter()
            .filter(|view| view.record.project_id.as_deref() == Some(error_project));
        let view = receipts.next()?;
        let record = &view.record;
        if receipts.next().is_some()
            || record.schema != 2
            || record.id.len() != 32
            || !record.id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.before_worktrees.contains(&record.id)
            || record.input.source_id != self.source.id.as_str()
            || record.source_path != self.source.path
            || serde_json::to_vec(&record.input).ok()? != self.input
            || record.review_digest != self.review
            || record.stage != "failed"
            || record.last_error.as_deref() != Some(error_code)
            || record.configuration.project.name != record.input.name
            || record.configuration.runtimes.php != self.source.php_version
            || record.configuration.runtimes.node != self.source.node_version
            || record.configuration.runtimes.composer != self.source.composer_version
        {
            return None;
        }
        let worktrees = self.root.join("worktrees");
        ordinary_directory(&worktrees)?;
        let directory = worktrees.join(&record.id);
        ordinary_directory(&directory)?;
        let destination = directory.join("tree");
        if view.path != destination || view.retained_path != directory.join("retained") {
            return None;
        }
        ordinary_directory(&destination)?;
        let mut children = after
            .projects
            .iter()
            .filter(|row| row.project.path == destination);
        let child = &children.next()?.project;
        if children.next().is_some()
            || child.id != error_project
            || !child.active
            || child.name != record.input.name
            || child.php != self.source.php
            || child.node != self.source.node
            || child.composer != self.source.composer
            || child
                .worktree
                .as_deref()
                .is_some_and(|worktree| worktree != record.id)
            || after
                .projects
                .iter()
                .filter(|row| row.project.id == error_project)
                .count()
                != 1
        {
            return None;
        }
        let stored = child.filesystem_identity.as_deref()?;
        if !stored.starts_with("file:") || actual_identity(&child.path).as_deref() != Some(stored) {
            return None;
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devforge_core::{ErrorCode, SecretStore};
    use devforge_projects::{cache::CacheInstance, project::Catalog};
    use devforge_workspace::{
        Workspace,
        worktrees::{DatabaseMode, Input, Record, View},
    };
    use serde_json::json;
    use std::sync::Arc;

    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
            panic!("subject metadata must not read credentials")
        }
        fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
            panic!("subject metadata must not write credentials")
        }
        fn delete(&self, _: &str) -> Result<(), ErrorCode> {
            panic!("subject metadata must not delete credentials")
        }
    }
    fn id(value: &str) -> Id {
        Id::new(value).unwrap()
    }
    fn input(source: &str) -> Input {
        Input {
            source_id: source.into(),
            name: "Child".into(),
            branch: "feature".into(),
            base: "HEAD".into(),
            new_branch: true,
            database: DatabaseMode::Empty,
            dependencies: false,
            copy_environment: false,
        }
    }
    fn worktree(source: &str, child: Option<&str>, stage: &str) -> View {
        let record = Record {
            isolated: false,
            schema: 2,
            id: "a".repeat(32),
            input: input(source),
            source_path: PathBuf::from("/fixture/source"),
            common_dir: PathBuf::from("/fixture/source/.git"),
            git_dir: None,
            commit: "b".repeat(40),
            domain: "child.devforge.test".into(),
            review_digest: "review".into(),
            project_id: child.map(Into::into),
            stage: stage.into(),
            last_error: None,
            configuration: serde_json::from_value(
                json!({"schema":1,"project":{"name":"Child","domain":"child.devforge.test"},
                "runtimes":{"php":"8.4","node":"24","composer":"2.11.2"},"pins":{}}),
            )
            .unwrap(),
        };
        View {
            files_archived: stage == "removed",
            record,
            path: PathBuf::from("/fixture/worktree/tree"),
            retained_path: PathBuf::from("/fixture/worktree/retained"),
        }
    }
    #[test]
    fn every_worktree_lifecycle_uses_source_even_when_child_registered_or_archived() {
        for stage in [
            "reserved",
            "failed",
            "ready",
            "removed",
            "restoring",
            "purging",
            "purged",
        ] {
            let mut snapshot = Snapshot::default();
            snapshot
                .worktrees
                .push(worktree("source-a", Some("child-b"), stage));
            let commands = [
                Command::WorktreePlan {
                    input: input("source-a"),
                },
                Command::WorktreeCreate {
                    input: input("source-a"),
                    review: "review".into(),
                    trusted: true,
                },
                Command::WorktreeOpen { id: "a".repeat(32) },
                Command::WorktreeArchivePlan {
                    id: "a".repeat(32),
                    operation: "restore".into(),
                },
                Command::WorktreeArchive {
                    id: "a".repeat(32),
                    operation: "purge".into(),
                    review: "review".into(),
                    consent: true,
                },
                Command::WorktreeRemotePlan {
                    id: "a".repeat(32),
                    source: remote_source(),
                },
                Command::WorktreeRemoteCopy {
                    id: "a".repeat(32),
                    source: remote_source(),
                    review: "review".into(),
                    consent: true,
                },
                Command::WorktreeCleanupPlan { id: "a".repeat(32) },
                Command::WorktreeCleanup {
                    id: "a".repeat(32),
                    review: "review".into(),
                    consent: true,
                },
            ];
            for command in commands {
                assert_eq!(
                    resource_subject(&command, &snapshot),
                    Ok(Some(id("source-a")))
                );
            }
        }
    }
    fn remote_source() -> devforge_projects::backup::remote::Source {
        devforge_projects::backup::remote::Source {
            host: "127.0.0.1".into(),
            port: 5432,
            database: "fixture".into(),
            user: "fixture".into(),
            password_ref: "opaque-only".into(),
            tls_ca: None,
            loopback_unencrypted: true,
        }
    }
    #[test]
    fn malformed_missing_ambiguous_and_oversized_resource_metadata_fail_closed() {
        let command = Command::WorktreeOpen { id: "a".repeat(32) };
        let mut snapshot = Snapshot::default();
        assert_eq!(
            resource_subject(&command, &snapshot),
            Err(SubjectError::MissingResource)
        );
        snapshot
            .worktrees
            .push(worktree(ROOT_SUBJECT, None, "reserved"));
        assert_eq!(
            resource_subject(&command, &snapshot),
            Err(SubjectError::InvalidSubject)
        );
        snapshot
            .worktrees
            .push(worktree("source-a", None, "reserved"));
        assert_eq!(
            resource_subject(&command, &snapshot),
            Err(SubjectError::AmbiguousResource)
        );
        snapshot.worktrees = (0..=WORKTREE_LIMIT)
            .map(|_| worktree("source-a", None, "reserved"))
            .collect();
        assert_eq!(
            resource_subject(&command, &snapshot),
            Err(SubjectError::MetadataLimit)
        );
        assert_eq!(
            resource_subject(
                &Command::Visit {
                    id: "project-a".into()
                },
                &Snapshot::default()
            ),
            Ok(None)
        );
    }
    #[test]
    fn cache_owner_does_not_depend_on_current_selection_or_active_project() {
        let root = tempfile::tempdir().unwrap();
        let workspace = Workspace::open(root.path().join("data"), Arc::new(NoSecrets)).unwrap();
        let cache = CacheInstance {
            id: "cache-a".into(),
            project_id: "owner-a".into(),
            package: "garnet".into(),
            runtime: "dotnet".into(),
            port: 1234,
            username: "username".into(),
            secret_ref: "opaque-ref".into(),
            admin_ref: "opaque-admin".into(),
            ready: false,
        };
        let registry =
            rusqlite::Connection::open(workspace.projects.root().join("projects.sqlite3")).unwrap();
        registry
            .execute(
                "INSERT INTO cache_instances(id,record) VALUES(?1,?2)",
                rusqlite::params![cache.id, serde_json::to_string(&cache).unwrap()],
            )
            .unwrap();
        let mut snapshot = workspace.snapshot().unwrap();
        for command in [
            Command::StartCache {
                instance: "cache-a".into(),
            },
            Command::StopCache {
                instance: "cache-a".into(),
            },
        ] {
            assert_eq!(
                resource_subject(&command, &snapshot),
                Ok(Some(id("owner-a")))
            );
        }
        snapshot.caches.push(snapshot.caches[0].clone());
        assert_eq!(
            resource_subject(
                &Command::StartCache {
                    instance: "cache-a".into()
                },
                &snapshot
            ),
            Err(SubjectError::AmbiguousResource)
        );
    }
    fn create_fixture() -> (tempfile::TempDir, Workspace, CreateInput) {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("projects-parent");
        fs::create_dir(&parent).unwrap();
        let workspace = Workspace::open(root.path().join("data"), Arc::new(NoSecrets)).unwrap();
        let catalog = Catalog::bundled();
        let target = devforge_runtime::package::Target::current().unwrap();
        let php = catalog
            .packages
            .iter()
            .find(|(_, p)| p.kind == "php" && p.variant == "cli" && p.target == target)
            .unwrap()
            .0
            .clone();
        let node = catalog
            .packages
            .iter()
            .find(|(_, p)| p.kind == "node" && p.target == target)
            .unwrap()
            .0
            .clone();
        let input = CreateInput {
            source: devforge_workspace::setup::Source::Laravel {
                version: "^13".into(),
            },
            parent,
            folder: "new-app".into(),
            name: "New app".into(),
            php,
            node,
            domain: "new-app.devforge.test".into(),
            https: false,
            options: serde_json::from_value(
                json!({"composer":false,"npm":false,"create_env":false,"generate_key":false,
                "run_scripts":false,"trusted":true,"initialize_sqlite":false}),
            )
            .unwrap(),
        };
        (root, workspace, input)
    }
    fn register(workspace: &Workspace, input: &CreateInput) -> String {
        let path = input.parent.join(&input.folder);
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("public/index.php"), "<?php").unwrap();
        fs::write(
            path.join("composer.json"),
            r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
        )
        .unwrap();
        workspace
            .projects
            .import(
                &path,
                &input.name,
                &input.php,
                &input.node,
                &workspace.catalog,
            )
            .unwrap()
            .id
    }
    #[test]
    fn recovery_requires_same_operation_root_unique_new_record_and_actual_filesystem_identity() {
        let (root, workspace, input) = create_fixture();
        let data = root.path().join("data");
        let before = workspace.snapshot().unwrap();
        let operation = id("create-operation");
        let attempt = CreateAttempt::capture(&data, &operation, &input, &before).unwrap();
        let created = register(&workspace, &input);
        let after = workspace.snapshot().unwrap();
        let actual = |path: &Path| devforge_projects::discovery::identity(path).ok();
        assert_eq!(
            attempt.recover_project(&data, &operation, &after, &created, actual),
            Some(id(&created))
        );
        assert!(
            attempt
                .recover_project(&data, &id("other-operation"), &after, &created, actual)
                .is_none()
        );
        assert!(
            attempt
                .recover_project(root.path(), &operation, &after, &created, actual)
                .is_none()
        );
        assert!(
            attempt
                .recover_project(&data, &operation, &after, "arbitrary-error-id", actual)
                .is_none()
        );
        assert!(
            attempt
                .recover_project(&data, &operation, &after, &created, |_| Some(
                    "file:replacement".into()
                ))
                .is_none()
        );
        assert!(CreateAttempt::capture(&data, &operation, &input, &after).is_none());
        let mut duplicate = after.clone();
        duplicate.projects.push(duplicate.projects[0].clone());
        assert!(
            attempt
                .recover_project(&data, &operation, &duplicate, &created, actual)
                .is_none()
        );
    }
    #[test]
    fn recovery_rejects_wrong_runtime_worktree_and_preexisting_registration() {
        let (root, workspace, input) = create_fixture();
        let data = root.path().join("data");
        let before = workspace.snapshot().unwrap();
        let operation = id("create-operation");
        let mut attempt = CreateAttempt::capture(&data, &operation, &input, &before).unwrap();
        let created = register(&workspace, &input);
        let mut after = workspace.snapshot().unwrap();
        let actual = |path: &Path| devforge_projects::discovery::identity(path).ok();
        after.projects[0].project.php = "other-runtime".into();
        assert!(
            attempt
                .recover_project(&data, &operation, &after, &created, actual)
                .is_none()
        );
        after.projects[0].project.php = input.php.clone();
        after.projects[0].project.worktree = Some("worktree".into());
        assert!(
            attempt
                .recover_project(&data, &operation, &after, &created, actual)
                .is_none()
        );
        after.projects[0].project.worktree = None;
        attempt.before.insert(created.clone());
        assert!(
            attempt
                .recover_project(&data, &operation, &after, &created, actual)
                .is_none()
        );
    }

    // These fixtures persist ordinary project/receipt metadata and use real OS
    // file identities. They intentionally do NOT claim Git/native create passes.
    fn partial_worktree() -> (
        tempfile::TempDir,
        Workspace,
        WorktreeAttempt,
        Snapshot,
        String,
    ) {
        let (root, workspace, source_input) = create_fixture();
        let source_id = register(&workspace, &source_input);
        let before = workspace.snapshot().unwrap();
        let source = workspace.projects.get(&source_id).unwrap();
        let input = input(&source_id);
        let operation = id("worktree-create-operation");
        let review = "c".repeat(64);
        let data = fs::canonicalize(root.path().join("data")).unwrap();
        let attempt =
            WorktreeAttempt::capture(&data, &operation, &input, &review, &before).unwrap();
        let directory = data.join("worktrees").join("a".repeat(32));
        fs::create_dir(&directory).unwrap();
        let mut child_input = source_input;
        child_input.parent = directory.clone();
        child_input.folder = "tree".into();
        child_input.name = input.name.clone();
        let child_id = register(&workspace, &child_input);
        let mut view = worktree(&source_id, Some(&child_id), "failed");
        view.record.input = input;
        view.record.source_path = source.path;
        view.record.review_digest = review;
        view.record.last_error = Some("setup_composer_failed".into());
        view.record.configuration.runtimes.php = runtime_version(&before, &source.php, "php")
            .unwrap()
            .split('-')
            .next()
            .unwrap()
            .into();
        view.record.configuration.runtimes.node = runtime_version(&before, &source.node, "node")
            .unwrap()
            .trim_start_matches('v')
            .into();
        view.record.configuration.runtimes.composer =
            runtime_version(&before, &source.composer, "composer").unwrap();
        fs::write(
            directory.join("receipt.json"),
            serde_json::to_vec(&view.record).unwrap(),
        )
        .unwrap();
        let after = workspace.snapshot().unwrap();
        (root, workspace, attempt, after, child_id)
    }

    #[test]
    fn worktree_recovery_associates_only_new_child_and_preserves_source_namespace_through_claim_window()
     {
        let (root, workspace, attempt, after, child) = partial_worktree();
        let data = root.path().join("data");
        let operation = id("worktree-create-operation");
        let actual = |path: &Path| devforge_projects::discovery::identity(path).ok();
        assert_eq!(
            attempt.recover_project(
                &data,
                &operation,
                &after,
                &child,
                "setup_composer_failed",
                actual
            ),
            Some(id(&child))
        );
        let source = &attempt.source.id;
        assert_ne!(source, &id(&child));
        assert_eq!(
            resource_subject(&Command::WorktreeOpen { id: "a".repeat(32) }, &after),
            Ok(Some(source.clone()))
        );
        let project = workspace.projects.get(&child).unwrap();
        assert!(project.worktree.is_none());
        workspace
            .projects
            .claim_worktree(&child, project.revision, &"a".repeat(32))
            .unwrap();
        let claimed = workspace.snapshot().unwrap();
        assert_eq!(
            attempt.recover_project(
                &data,
                &operation,
                &claimed,
                &child,
                "setup_composer_failed",
                actual
            ),
            Some(id(&child))
        );
    }

    #[test]
    fn worktree_recovery_rejects_unrelated_error_stale_receipt_source_identity_and_existing_objects()
     {
        let (root, _workspace, mut attempt, after, child) = partial_worktree();
        let data = root.path().join("data");
        let operation = id("worktree-create-operation");
        let actual = |path: &Path| devforge_projects::discovery::identity(path).ok();
        assert!(
            attempt
                .recover_project(
                    &data,
                    &id("other-operation"),
                    &after,
                    &child,
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
        assert!(
            attempt
                .recover_project(
                    root.path(),
                    &operation,
                    &after,
                    &child,
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
        assert!(
            attempt
                .recover_project(
                    &data,
                    &operation,
                    &after,
                    attempt.source.id.as_str(),
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
        assert!(
            attempt
                .recover_project(&data, &operation, &after, &child, "different_error", actual)
                .is_none()
        );
        assert!(
            attempt
                .recover_project(
                    &data,
                    &operation,
                    &after,
                    &child,
                    "setup_composer_failed",
                    |_| Some("file:replacement".into())
                )
                .is_none()
        );
        let mut changed = after.clone();
        changed.worktrees[0].record.review_digest = "d".repeat(64);
        assert!(
            attempt
                .recover_project(
                    &data,
                    &operation,
                    &changed,
                    &child,
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
        changed = after.clone();
        changed.worktrees[0].record.input.branch = "other-branch".into();
        assert!(
            attempt
                .recover_project(
                    &data,
                    &operation,
                    &changed,
                    &child,
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
        changed = after.clone();
        changed.worktrees[0].record.configuration.runtimes.composer = "other-version".into();
        assert!(
            attempt
                .recover_project(
                    &data,
                    &operation,
                    &changed,
                    &child,
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
        changed = after.clone();
        changed.worktrees.push(changed.worktrees[0].clone());
        assert!(
            attempt
                .recover_project(
                    &data,
                    &operation,
                    &changed,
                    &child,
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
        attempt.before_worktrees.insert("a".repeat(32));
        assert!(
            attempt
                .recover_project(
                    &data,
                    &operation,
                    &after,
                    &child,
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
        attempt.before_worktrees.clear();
        attempt.before_projects.insert(child.clone());
        assert!(
            attempt
                .recover_project(
                    &data,
                    &operation,
                    &after,
                    &child,
                    "setup_composer_failed",
                    actual
                )
                .is_none()
        );
    }
}
