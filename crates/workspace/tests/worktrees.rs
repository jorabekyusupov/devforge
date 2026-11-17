use devforge_core::{ErrorCode, SecretStore};
use devforge_serving::site::Site;
use devforge_workspace::{
    Workspace,
    worktrees::{DatabaseMode, Input, Record},
};
use std::{fs, path::Path, process::Command, sync::Arc};
#[derive(Default)]
struct Secrets;
impl SecretStore for Secrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        Ok(None)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        Ok(())
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        Ok(())
    }
}
fn git(path: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn fixture(root: &Path) -> (Workspace, String, Input) {
    let path = root.join("Atlas Ўзбек & ' repo");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::create_dir_all(path.join("database")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(path.join("database/.gitkeep"), "").unwrap();
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(
        path.join(".env.example"),
        "APP_KEY=\nDB_CONNECTION=sqlite\n",
    )
    .unwrap();
    fs::write(
        path.join(".gitignore"),
        ".env\n/database/*.sqlite\n/vendor\n/node_modules\n",
    )
    .unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.name", "Fixture"]);
    git(&path, &["config", "user.email", "fixture@example.test"]);
    git(&path, &["add", "."]);
    git(&path, &["commit", "-qm", "base"]);
    let w = Workspace::open(root.join("data"), Arc::new(Secrets)).unwrap();
    w.manifest_init(&path, "8.5.12", "24.22.2", "2.11.2")
        .unwrap();
    git(&path, &["add", "devforge.yaml"]);
    git(&path, &["commit", "-qm", "environment configuration"]);
    let plan = w.manifest_plan(&path, false).unwrap();
    let p = w
        .projects
        .import(
            &path,
            "Atlas",
            &plan.packages["php"],
            &plan.packages["node"],
            &w.catalog,
        )
        .unwrap();
    w.sites
        .configure(
            &w.projects,
            Site {
                project_id: p.id.clone(),
                domain: "atlas.devforge.test".into(),
                https: false,
                vite: false,
            },
        )
        .unwrap();
    let input = Input {
        source_id: p.id.clone(),
        name: "Search workspace".into(),
        branch: "feature/search".into(),
        base: "HEAD".into(),
        new_branch: true,
        database: DatabaseMode::Empty,
        dependencies: false,
        copy_environment: false,
    };
    (w, p.id, input)
}
#[test]
fn plan_freezes_committed_history_and_refuses_stale_trust_or_invalid_branch() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, id, mut input) = fixture(temp.path());
    let p = w.projects.get(&id).unwrap();
    let plan = w.worktree_plan(&input).unwrap();
    assert!(plan.blockers.is_empty());
    assert_eq!(
        plan.configuration.pins,
        w.manifest_definition(&id).unwrap().pins
    );
    assert_eq!(
        w.worktree_create(input.clone(), &plan.digest, false, &Default::default())
            .err()
            .unwrap()
            .code,
        "setup_trust_required"
    );
    fs::write(p.path.join("public/index.php"), "<?php // local only").unwrap();
    let dirty = w.worktree_plan(&input).unwrap();
    assert!(dirty.source_dirty);
    assert_ne!(dirty.digest, plan.digest);
    assert_eq!(dirty.commit, plan.commit);
    assert_eq!(
        w.worktree_create(input.clone(), &plan.digest, true, &Default::default())
            .err()
            .unwrap()
            .code,
        "worktree_review_changed"
    );
    input.branch = "--upload-pack=evil".into();
    assert!(w.worktree_plan(&input).is_err());
    input.branch = "feature/search".into();
    input.database = DatabaseMode::Clone;
    assert!(
        w.worktree_plan(&input)
            .unwrap()
            .blockers
            .contains(&"worktree_sqlite_scope".into())
    );
    assert_eq!(w.projects.list().unwrap().len(), 1);
    input.database = DatabaseMode::Empty;
    let head = git(&p.path, &["rev-parse", "HEAD"]);
    git(
        &p.path,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{head},module"),
        ],
    );
    git(&p.path, &["commit", "-qm", "submodule fixture"]);
    assert!(
        w.worktree_plan(&input).is_err(),
        "a gitlink without .gitmodules cannot be reviewed"
    );
}
#[test]
fn cleanup_refuses_dirty_foreign_and_cancelled_worktrees_and_preserves_source() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, id, input) = fixture(temp.path());
    let source = w.projects.get(&id).unwrap();
    let plan = w.worktree_plan(&input).unwrap();
    let wid = devforge_serving::id();
    let dir = temp.path().join("data/worktrees").join(&wid);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tree");
    git(
        &source.path,
        &[
            "worktree",
            "add",
            "-qb",
            &input.branch,
            path.to_str().unwrap(),
        ],
    );
    let child = w
        .projects
        .import(&path, &input.name, &source.php, &source.node, &w.catalog)
        .unwrap();
    w.projects
        .claim_worktree(&child.id, child.revision, &wid)
        .unwrap();
    let common = fs::canonicalize(source.path.join(".git")).unwrap();
    let admin = fs::canonicalize(git(&path, &["rev-parse", "--absolute-git-dir"])).unwrap();
    let mut record = Record {
        isolated: false,
        schema: 1,
        id: wid.clone(),
        input,
        source_path: source.path.clone(),
        common_dir: common,
        git_dir: Some(admin),
        commit: plan.commit,
        domain: plan.domain,
        review_digest: plan.digest,
        project_id: Some(child.id.clone()),
        stage: "ready".into(),
        last_error: None,
        configuration: plan.configuration,
    };
    fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let clean = w.worktree_cleanup_plan(&wid).unwrap();
    assert!(!clean.dirty);
    assert!(clean.blockers.is_empty());
    assert!(w.external_worktree_record(&id, &wid).is_ok());
    assert!(w.external_worktree_record(&child.id, &wid).is_err());
    let calls = std::cell::Cell::new(0);
    let deny_before_lifecycle = || {
        let next = calls.get() + 1;
        calls.set(next);
        if next >= 4 {
            Err(devforge_workspace::Error {
                scope: "workspace",
                code: "grant_revoked".into(),
                project_id: None,
            })
        } else {
            Ok(())
        }
    };
    let denied = w
        .worktree_cleanup_guarded(
            &wid,
            &clean.digest,
            true,
            &Default::default(),
            &deny_before_lifecycle,
        )
        .err()
        .unwrap();
    assert_eq!(denied.code, "grant_revoked");
    assert!(
        path.is_dir(),
        "revocation before archive must not rename worktree"
    );
    assert!(!dir.join("retained").exists());
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), record.commit);
    #[cfg(unix)]
    {
        let moved = dir.join("original-tree");
        let calls = std::cell::Cell::new(0);
        let swap_before_rename = || {
            let next = calls.get() + 1;
            calls.set(next);
            if next == 5 {
                fs::rename(&path, &moved).unwrap();
                fs::create_dir(&path).unwrap();
            }
            Ok(())
        };
        let result = w.worktree_cleanup_guarded(
            &wid,
            &clean.digest,
            true,
            &Default::default(),
            &swap_before_rename,
        );
        assert_eq!(result.err().unwrap().code, "worktree_ownership");
        assert!(!dir.join("retained").exists());
        fs::remove_dir(&path).unwrap();
        fs::rename(&moved, &path).unwrap();
        fs::write(
            dir.join("receipt.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), record.commit);
    }
    let nested = path.join("vendor/nested");
    fs::create_dir_all(nested.join("public")).unwrap();
    fs::write(nested.join("public/index.php"), "<?php").unwrap();
    fs::write(
        nested.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let registered = w
        .projects
        .import(
            &nested,
            "Nested user project",
            &source.php,
            &source.node,
            &w.catalog,
        )
        .unwrap();
    assert!(
        w.worktree_cleanup_plan(&wid)
            .unwrap()
            .blockers
            .contains(&"worktree_nested_project".into())
    );
    w.projects
        .forget(&registered.id, registered.revision)
        .unwrap();
    git(&path, &["checkout", "--detach", "-q"]);
    assert!(
        w.worktree_cleanup_plan(&wid)
            .unwrap()
            .blockers
            .contains(&"worktree_detached".into())
    );
    git(&path, &["checkout", "-q", "feature/search"]);
    let head = git(&path, &["rev-parse", "HEAD"]);
    git(
        &path,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{head},module"),
        ],
    );
    assert!(
        w.worktree_cleanup_plan(&wid)
            .unwrap()
            .blockers
            .contains(&"worktree_submodules_unsupported".into())
    );
    git(&path, &["update-index", "--force-remove", "module"]);
    let cancel = devforge_runtime::Cancellation::default();
    cancel.cancel();
    assert_eq!(
        w.worktree_cleanup(&wid, &clean.digest, true, &cancel)
            .err()
            .unwrap()
            .code,
        "setup_cancelled"
    );
    assert!(path.is_dir());
    fs::write(path.join("public/index.php"), "<?php // staged only").unwrap();
    git(&path, &["add", "public/index.php"]);
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    assert!(
        w.worktree_cleanup_plan(&wid).unwrap().dirty,
        "staged changes must be retained even when working content matches HEAD"
    );
    git(&path, &["reset", "-q", "HEAD", "--", "public/index.php"]);
    fs::write(path.join("user-file"), "keep").unwrap();
    let dirty = w.worktree_cleanup_plan(&wid).unwrap();
    assert!(dirty.dirty);
    assert_eq!(
        w.worktree_cleanup(&wid, &dirty.digest, true, &Default::default())
            .err()
            .unwrap()
            .code,
        "worktree_dirty"
    );
    assert_eq!(fs::read(path.join("user-file")).unwrap(), b"keep");
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), record.commit);
    record.common_dir = temp.path().join("foreign");
    fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    assert_eq!(
        w.worktree_cleanup_plan(&wid).err().unwrap().code,
        "worktree_ownership"
    );
}
#[test]
fn project_layer_blocks_generic_forget_and_claim_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let (w, id, _) = fixture(temp.path());
    let p = w.projects.get(&id).unwrap();
    let wid = devforge_serving::id();
    w.projects.claim_worktree(&id, p.revision, &wid).unwrap();
    let p = w.projects.get(&id).unwrap();
    assert_eq!(
        w.projects.forget(&id, p.revision).unwrap_err(),
        devforge_projects::ProjectError::WorkspaceManaged
    );
    assert!(
        w.projects
            .retire_worktree(&id, p.revision, &devforge_serving::id())
            .is_err()
    );
    w.projects.retire_worktree(&id, p.revision, &wid).unwrap();
    assert!(p.path.exists());
}

#[test]
fn archive_restore_and_purge_preserve_identity_ignored_data_and_source_history() {
    for isolated in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut w, id, input) = fixture(temp.path());
        let source = w.projects.get(&id).unwrap();
        let plan = w.worktree_plan(&input).unwrap();
        let wid = devforge_serving::id();
        let dir = temp.path().join("data/worktrees").join(&wid);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tree");
        if isolated {
            git(
                &source.path,
                &[
                    "clone",
                    "--no-hardlinks",
                    dunce::simplified(&source.path).to_str().unwrap(),
                    dunce::simplified(&path).to_str().unwrap(),
                ],
            );
            git(&path, &["checkout", "-qb", &input.branch]);
        } else {
            git(
                &source.path,
                &[
                    "worktree",
                    "add",
                    "-qb",
                    &input.branch,
                    path.to_str().unwrap(),
                ],
            );
        }
        let p = w
            .projects
            .import(&path, &input.name, &source.php, &source.node, &w.catalog)
            .unwrap();
        w.projects.claim_worktree(&p.id, p.revision, &wid).unwrap();
        let record = Record {
            isolated,
            schema: 1,
            id: wid.clone(),
            input,
            source_path: source.path.clone(),
            common_dir: fs::canonicalize(source.path.join(".git")).unwrap(),
            git_dir: Some(
                fs::canonicalize(git(&path, &["rev-parse", "--absolute-git-dir"])).unwrap(),
            ),
            commit: plan.commit.clone(),
            domain: plan.domain,
            review_digest: plan.digest,
            project_id: Some(p.id.clone()),
            stage: "ready".into(),
            last_error: None,
            configuration: plan.configuration,
        };
        fs::write(
            dir.join("receipt.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        w.execute(
            devforge_workspace::Command::Configure {
                id: p.id.clone(),
                revision: w.projects.get(&p.id).unwrap().revision,
                name: p.name.clone(),
                php: p.php.clone(),
                node: p.node.clone(),
                domain: format!("edited-{}.devforge.test", &wid[..8]),
                https: false,
                vite: false,
                queue: true,
            },
            &Default::default(),
        )
        .unwrap();
        git(&path, &["config", "core.autocrlf", "true"]);
        // Commit a freshly written LF file, then restore it over a CRLF checkout.
        fs::write(path.join("reviewed.txt"), "first line\nsecond line\n").unwrap();
        git(&path, &["add", "reviewed.txt"]);
        git(
            &path,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "-qm",
                "LF archive fixture",
            ],
        );
        fs::write(path.join(".env"), "PRIVATE=keep").unwrap();
        fs::write(path.join("database/local.sqlite"), "owned-local-data").unwrap();
        let cleanup = w.worktree_cleanup_plan(&wid).unwrap();
        assert!(cleanup.blockers.is_empty(), "{:?}", cleanup.blockers);
        w.worktree_cleanup(&wid, &cleanup.digest, true, &Default::default())
            .unwrap();
        assert!(w.projects.get(&p.id).is_err());
        let restore = w.worktree_archive_plan(&wid, "restore").unwrap();
        assert!(restore.blockers.is_empty(), "{:?}", restore.blockers);
        fs::write(dir.join("retained/.env"), "PRIVATE=changed-after-review").unwrap();
        assert_eq!(
            w.worktree_archive(&wid, "restore", &restore.digest, true, &Default::default())
                .err()
                .unwrap()
                .code,
            "worktree_review_changed"
        );
        let cancelled = devforge_runtime::Cancellation::default();
        cancelled.cancel();
        assert_eq!(
            w.worktree_archive_review(&wid, "restore", &cancelled)
                .err()
                .unwrap()
                .code,
            "setup_cancelled"
        );
        // Resume a journal interrupted before the Git checkout, using the same archive.
        let mut interrupted: Record =
            serde_json::from_slice(&fs::read(dir.join("receipt.json")).unwrap()).unwrap();
        interrupted.stage = "restoring".into();
        fs::write(
            dir.join("receipt.json"),
            serde_json::to_vec(&interrupted).unwrap(),
        )
        .unwrap();
        let restore = w.worktree_archive_plan(&wid, "restore").unwrap();
        w.worktree_archive(&wid, "restore", &restore.digest, true, &Default::default())
            .unwrap();
        assert_eq!(w.projects.get(&p.id).unwrap().id, p.id);
        assert_eq!(
            w.projects.get(&p.id).unwrap().filesystem_identity,
            devforge_projects::discovery::identity(&path).ok()
        );
        let restored = w
            .snapshot()
            .unwrap()
            .projects
            .into_iter()
            .find(|r| r.project.id == p.id)
            .unwrap();
        assert!(restored.development.queue);
        assert_eq!(
            restored.site.unwrap().domain,
            format!("edited-{}.devforge.test", &wid[..8])
        );
        assert_eq!(
            fs::read_to_string(path.join(".env")).unwrap(),
            "PRIVATE=changed-after-review"
        );
        assert_eq!(
            fs::read_to_string(path.join("database/local.sqlite")).unwrap(),
            "owned-local-data"
        );
        let cleanup = w.worktree_cleanup_plan(&wid).unwrap();
        w.worktree_cleanup(&wid, &cleanup.digest, true, &Default::default())
            .unwrap();
        let purge = w.worktree_archive_plan(&wid, "purge").unwrap();
        assert!(
            w.worktree_archive(&wid, "purge", &purge.digest, false, &Default::default())
                .is_err()
        );
        // A revoked caller after the recoverable journal write must not start
        // irreversible payload deletion. A fresh native retry can recover.
        let deny_after_journal = || -> devforge_workspace::Result<()> {
            let bytes = fs::read(dir.join("receipt.json"))?;
            let record: Record = serde_json::from_slice(&bytes).unwrap();
            if record.stage == "purging" {
                return Err(devforge_workspace::Error {
                    scope: "workspace",
                    code: "grant_revoked".into(),
                    project_id: None,
                });
            }
            Ok(())
        };
        let denied = w
            .worktree_archive_guarded(
                &wid,
                "purge",
                &purge.digest,
                true,
                &Default::default(),
                &deny_after_journal,
            )
            .err()
            .unwrap();
        assert_eq!(denied.code, "grant_revoked");
        assert!(dir.join("retained").exists());
        assert_eq!(w.worktrees().unwrap()[0].record.stage, "purging");
        let retry = w.worktree_archive_plan(&wid, "purge").unwrap();
        w.worktree_archive(&wid, "purge", &retry.digest, true, &Default::default())
            .unwrap();
        assert!(!dir.join("retained").exists());
        assert_eq!(w.worktrees().unwrap()[0].record.stage, "purged");
        assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), plan.commit);
        assert!(source.path.join("public/index.php").exists());
        if isolated {
            assert!(dir.join("history.bundle").is_file());
            git(
                &source.path,
                &[
                    "bundle",
                    "verify",
                    dir.join("history.bundle").to_str().unwrap(),
                ],
            );
        }
    }
}

#[test]
fn configuration_save_validates_schema_and_detects_external_changes() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, id, _) = fixture(temp.path());
    let p = w.projects.get(&id).unwrap();
    let original = w.configuration_document(&id).unwrap();
    let expected = original["expected"].as_str();
    let text = original["text"].as_str().unwrap();
    assert!(
        w.configuration_save(&id, expected, "schema: invalid", true)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(p.path.join("devforge.yaml")).unwrap(),
        text
    );
    let updated = format!("{text}\n# local editor\n");
    assert!(
        w.configuration_save(&id, expected, &updated, false)
            .is_err()
    );
    let saved = w.configuration_save(&id, expected, &updated, true).unwrap();
    assert_ne!(saved["expected"], original["expected"]);
    assert_eq!(
        w.configuration_save(&id, expected, text, true)
            .err()
            .unwrap()
            .code,
        "manifest_review_changed"
    );
    fs::write(
        p.path.join("devforge.yaml"),
        format!("{updated}# external\n"),
    )
    .unwrap();
    assert!(
        w.configuration_save(&id, saved["expected"].as_str(), text, true)
            .is_err()
    );
    assert!(
        fs::read_to_string(p.path.join("devforge.yaml"))
            .unwrap()
            .ends_with("# external\n")
    );
}

#[test]
fn configuration_refuses_a_replaced_registered_directory() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, id, _) = fixture(temp.path());
    let project = w.projects.get(&id).unwrap();
    let doc = w.configuration_document(&id).unwrap();
    let original = temp.path().join("original-directory");
    fs::rename(&project.path, &original).unwrap();
    fs::create_dir_all(project.path.join("public")).unwrap();
    fs::write(project.path.join("public/index.php"), "<?php").unwrap();
    fs::copy(
        original.join("composer.json"),
        project.path.join("composer.json"),
    )
    .unwrap();
    fs::copy(
        original.join("devforge.yaml"),
        project.path.join("devforge.yaml"),
    )
    .unwrap();
    assert_eq!(
        w.configuration_document(&id).err().unwrap().code,
        "discovery_unavailable"
    );
    assert_eq!(
        w.configuration_save(
            &id,
            doc["expected"].as_str(),
            doc["text"].as_str().unwrap(),
            true
        )
        .err()
        .unwrap()
        .code,
        "discovery_unavailable"
    );
}

#[test]
fn worktree_create_guard_refuses_before_reservation_and_preserves_source() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, id, input) = fixture(temp.path());
    let source = w.projects.get(&id).unwrap();
    let head = git(&source.path, &["rev-parse", "HEAD"]);
    let plan = w.worktree_plan(&input).unwrap();
    let records_before = w.worktrees().unwrap().len();
    let calls = std::cell::Cell::new(0);
    let deny_after_plan = || {
        let next = calls.get() + 1;
        calls.set(next);
        if next >= 2 {
            Err(devforge_workspace::Error {
                scope: "workspace",
                code: "grant_revoked".into(),
                project_id: None,
            })
        } else {
            Ok(())
        }
    };
    let result = w.worktree_create_guarded(
        input,
        &plan.digest,
        true,
        &Default::default(),
        &deny_after_plan,
    );
    assert_eq!(result.err().unwrap().code, "grant_revoked");
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), head);
    assert_eq!(w.projects.list().unwrap().len(), 1);
    assert_eq!(w.worktrees().unwrap().len(), records_before);
}

#[cfg(unix)]
#[test]
fn worktree_create_refuses_substituted_owner_or_reserved_directory() {
    use std::cell::Cell;
    for replace_at in [2, 3] {
        let temp = tempfile::tempdir().unwrap();
        let (mut w, id, input) = fixture(temp.path());
        let source = w.projects.get(&id).unwrap();
        let head = git(&source.path, &["rev-parse", "HEAD"]);
        let plan = w.worktree_plan(&input).unwrap();
        let root = temp.path().join("data/worktrees");
        let calls = Cell::new(0);
        let replacement = std::cell::RefCell::new(None::<std::path::PathBuf>);
        let guard = || {
            let n = calls.get() + 1;
            calls.set(n);
            if n == replace_at {
                let target = if n == 2 {
                    root.clone()
                } else {
                    fs::read_dir(&root)
                        .unwrap()
                        .map(|e| e.unwrap().path())
                        .find(|p| p.is_dir())
                        .unwrap()
                };
                let old = target.with_extension("moved");
                fs::rename(&target, &old).unwrap();
                fs::create_dir(&target).unwrap();
                *replacement.borrow_mut() = Some(target);
            }
            Ok(())
        };
        let result =
            w.worktree_create_guarded(input, &plan.digest, true, &Default::default(), &guard);
        assert_eq!(result.err().unwrap().code, "worktree_ownership");
        let replaced = replacement.borrow().clone().unwrap();
        assert!(
            !replaced.join("receipt.json").exists(),
            "must not write into substituted directory"
        );
        assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), head);
        assert_eq!(w.projects.list().unwrap().len(), 1);
    }
}

#[test]
fn merge_fast_forwards_owned_worktree_to_source_commit_without_changing_source() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, source_id, input) = fixture(temp.path());
    let source = w.projects.get(&source_id).unwrap();
    let existing = w.worktree_plan(&input).unwrap();
    let wid = devforge_serving::id();
    let dir = temp.path().join("data/worktrees").join(&wid);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tree");
    git(
        &source.path,
        &[
            "worktree",
            "add",
            "-qb",
            &input.branch,
            path.to_str().unwrap(),
        ],
    );
    let child = w
        .projects
        .import(&path, &input.name, &source.php, &source.node, &w.catalog)
        .unwrap();
    w.projects
        .claim_worktree(&child.id, child.revision, &wid)
        .unwrap();
    let record = Record {
        isolated: false,
        schema: 1,
        id: wid.clone(),
        input,
        source_path: source.path.clone(),
        common_dir: fs::canonicalize(source.path.join(".git")).unwrap(),
        git_dir: Some(fs::canonicalize(git(&path, &["rev-parse", "--absolute-git-dir"])).unwrap()),
        commit: existing.commit,
        domain: existing.domain,
        review_digest: existing.digest,
        project_id: Some(child.id.clone()),
        stage: "ready".into(),
        last_error: None,
        configuration: existing.configuration,
    };
    fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    fs::write(
        source.path.join("new-feature.txt"),
        "source committed content",
    )
    .unwrap();
    git(&source.path, &["add", "new-feature.txt"]);
    git(&source.path, &["commit", "-qm", "source change"]);
    fs::write(
        source.path.join("uncommitted-canary.txt"),
        "keep this dirty source",
    )
    .unwrap();
    let source_head = git(&source.path, &["rev-parse", "HEAD"]);
    let source_branch = git(&source.path, &["symbolic-ref", "HEAD"]);
    let source_status = git(&source.path, &["status", "--porcelain=v1"]);
    let target_before = git(&path, &["rev-parse", "HEAD"]);
    let plan = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert_eq!(plan.source_head, source_head);
    assert_eq!(plan.target_head, target_before);
    assert_eq!(plan.effect, "fast_forward");
    assert!(
        plan.source_dirty && plan.blockers.is_empty(),
        "{:?}",
        plan.blockers
    );
    // Preview's isolated object store must not silently ignore user-specified
    // replace refs or shallow/graft graph semantics.
    git(&source.path, &["replace", &target_before, &source_head]);
    assert_eq!(
        w.worktree_merge_plan(&source_id, &wid).err().unwrap().code,
        "worktree_merge_graph_unsupported"
    );
    git(&source.path, &["replace", "-d", &target_before]);
    for relative in [".git/shallow", ".git/info/grafts"] {
        let metadata = source.path.join(relative);
        fs::write(&metadata, b"").unwrap();
        assert_eq!(
            w.worktree_merge_plan(&source_id, &wid).err().unwrap().code,
            "worktree_merge_graph_unsupported"
        );
        fs::remove_file(metadata).unwrap();
    }
    let out = w
        .worktree_merge_guarded(&source_id, &wid, &plan.digest, &Default::default(), &|| {
            Ok(())
        })
        .unwrap();
    assert_eq!(out["effect"], "fast_forward");
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), source_head);
    assert_eq!(
        fs::read_to_string(path.join("new-feature.txt")).unwrap(),
        "source committed content"
    );
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_head);
    assert_eq!(git(&source.path, &["symbolic-ref", "HEAD"]), source_branch);
    assert_eq!(
        git(&source.path, &["status", "--porcelain=v1"]),
        source_status
    );
    assert_eq!(
        fs::read_to_string(source.path.join("uncommitted-canary.txt")).unwrap(),
        "keep this dirty source"
    );
    assert!(!path.join("uncommitted-canary.txt").exists());
    let again = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert_eq!(again.effect, "already_included");
    assert_eq!(
        w.worktree_merge_guarded(&source_id, &wid, &plan.digest, &Default::default(), &|| Ok(
            ()
        ))
        .err()
        .unwrap()
        .code,
        "worktree_review_changed"
    );
    fs::write(path.join("new-feature.txt"), "dirty target").unwrap();
    let dirty = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert!(dirty.blockers.iter().any(|b| b == "worktree_dirty"));
    assert!(
        w.worktree_merge_guarded(
            &source_id,
            &wid,
            &dirty.digest,
            &Default::default(),
            &|| Ok(())
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(path.join("new-feature.txt")).unwrap(),
        "dirty target"
    );
    fs::write(path.join("new-feature.txt"), "source committed content").unwrap();
    fs::write(path.join("child-only.txt"), "divergent child").unwrap();
    git(&path, &["add", "child-only.txt"]);
    git(&path, &["commit", "-qm", "child change"]);
    fs::write(source.path.join("source-only.txt"), "divergent source").unwrap();
    git(&source.path, &["add", "source-only.txt"]);
    git(&source.path, &["commit", "-qm", "other source change"]);
    let child_head = git(&path, &["rev-parse", "HEAD"]);
    let source_divergent = git(&source.path, &["rev-parse", "HEAD"]);
    let object_dir = fs::canonicalize(source.path.join(".git/objects")).unwrap();
    let object_fingerprint = |path: &Path| -> Vec<String> {
        fn walk(root: &Path, here: &Path, all: &mut Vec<String>) {
            for entry in fs::read_dir(here).unwrap() {
                let entry = entry.unwrap();
                let p = entry.path();
                all.push(p.strip_prefix(root).unwrap().to_string_lossy().to_string());
                if entry.file_type().unwrap().is_dir() {
                    walk(root, &p, all);
                }
            }
        }
        let mut all = Vec::new();
        walk(path, path, &mut all);
        all.sort();
        all
    };
    let objects_before = object_fingerprint(&object_dir);
    let refs_before = git(&source.path, &["show-ref"]);
    let source_status_before = git(&source.path, &["status", "--porcelain=v1"]);
    let divergent = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert_eq!(divergent.effect, "merge_commit");
    assert!(divergent.blockers.is_empty(), "{:?}", divergent.blockers);
    assert!(divergent.result_tree.is_some());
    assert_eq!(
        object_fingerprint(&object_dir),
        objects_before,
        "preview must not write source or target object store"
    );
    assert_eq!(git(&source.path, &["show-ref"]), refs_before);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        w.worktree_merge_plan(&source_id, &wid).unwrap().digest,
        divergent.digest,
        "identity timestamp must not change the review digest"
    );
    let merged = w
        .worktree_merge_guarded(
            &source_id,
            &wid,
            &divergent.digest,
            &Default::default(),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(merged["effect"], "merge_commit");
    let head = git(&path, &["rev-parse", "HEAD"]);
    assert_eq!(
        git(&path, &["rev-list", "--parents", "-n", "1", "HEAD"]),
        format!("{head} {child_head} {source_divergent}")
    );
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_divergent);
    assert_eq!(
        git(&source.path, &["status", "--porcelain=v1"]),
        source_status_before
    );
    assert_eq!(
        fs::read_to_string(path.join("source-only.txt")).unwrap(),
        "divergent source"
    );
    assert_eq!(
        fs::read_to_string(path.join("child-only.txt")).unwrap(),
        "divergent child"
    );
    // A configured but unrelated clean filter or merge driver must neither
    // block safe changed paths nor execute during preview or actual apply.
    fs::write(
        source.path.join(".gitattributes"),
        "public/index.php filter=evil\n",
    )
    .unwrap();
    git(&source.path, &["add", ".gitattributes"]);
    git(
        &source.path,
        &["commit", "-qm", "add harmless attribute fixture"],
    );
    git(
        &path,
        &[
            "merge",
            "--no-edit",
            &git(&source.path, &["rev-parse", "HEAD"]),
        ],
    );
    let marker = temp.path().join("unrelated-driver-executed");
    let marker_path = marker.to_string_lossy().replace('\\', "/");
    let command = format!("sh -c 'echo ran > \"{marker_path}\"; cat'");
    git(&source.path, &["config", "filter.evil.clean", &command]);
    git(&source.path, &["config", "merge.evil.driver", &command]);
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(source.path.join("later-feature.txt"), "later feature").unwrap();
    git(&source.path, &["add", "later-feature.txt"]);
    git(&source.path, &["commit", "-qm", "later source feature"]);
    let configured = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert!(
        configured
            .blockers
            .iter()
            .any(|v| v == "worktree_merge_custom_config_unsupported")
    );
    assert!(
        !marker.exists(),
        "custom merge driver must not execute in preview"
    );
    git(&source.path, &["config", "--unset", "merge.evil.driver"]);
    let unaffected = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert!(unaffected.blockers.is_empty(), "{:?}", unaffected.blockers);
    assert!(!marker.exists(), "preview must not run configured programs");
    w.worktree_merge_guarded(
        &source_id,
        &wid,
        &unaffected.digest,
        &Default::default(),
        &|| Ok(()),
    )
    .unwrap();
    assert!(
        !marker.exists(),
        "merge apply must not run unrelated configured programs"
    );
    assert_eq!(
        fs::read_to_string(path.join("later-feature.txt")).unwrap(),
        "later feature"
    );
    git(&source.path, &["config", "--unset", "filter.evil.clean"]);
    assert!(w.worktree_merge_plan(&child.id, &wid).is_err());
    // Disposable fixture only: real ignored user data is never reset.
    git(
        &path,
        &[
            "reset",
            "--hard",
            &git(&source.path, &["rev-parse", "HEAD"]),
        ],
    );
    fs::create_dir_all(path.join("database")).unwrap();
    fs::write(
        path.join("database/canary.sqlite"),
        "keep target ignored data",
    )
    .unwrap();
    fs::write(
        source.path.join("database/canary.sqlite"),
        "tracked upstream data",
    )
    .unwrap();
    git(&source.path, &["add", "-f", "database/canary.sqlite"]);
    git(
        &source.path,
        &["commit", "-qm", "add tracked sqlite fixture"],
    );
    let ignored = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert!(ignored.blockers.is_empty());
    let before = git(&path, &["rev-parse", "HEAD"]);
    assert!(
        w.worktree_merge_guarded(
            &source_id,
            &wid,
            &ignored.digest,
            &Default::default(),
            &|| Ok(())
        )
        .is_err()
    );
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        fs::read_to_string(path.join("database/canary.sqlite")).unwrap(),
        "keep target ignored data"
    );
    let cancelled = devforge_runtime::Cancellation::default();
    cancelled.cancel();
    assert!(
        w.worktree_merge_guarded(&source_id, &wid, &ignored.digest, &cancelled, &|| Ok(()))
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(path.join("database/canary.sqlite")).unwrap(),
        "keep target ignored data"
    );
    // Legacy or tampered project metadata without a physical identity is not trusted.
    let db = rusqlite::Connection::open(w.projects.root().join("projects.sqlite3")).unwrap();
    for project_id in [&child.id, &source_id] {
        let original: String = db
            .query_row(
                "SELECT record FROM projects WHERE id=?1",
                [project_id],
                |row| row.get(0),
            )
            .unwrap();
        let mut altered: serde_json::Value = serde_json::from_str(&original).unwrap();
        altered["filesystem_identity"] = serde_json::Value::Null;
        db.execute(
            "UPDATE projects SET record=?1 WHERE id=?2",
            rusqlite::params![serde_json::to_string(&altered).unwrap(), project_id],
        )
        .unwrap();
        assert_eq!(
            w.worktree_merge_plan(&source_id, &wid).err().unwrap().code,
            "worktree_ownership"
        );
        db.execute(
            "UPDATE projects SET record=?1 WHERE id=?2",
            rusqlite::params![original, project_id],
        )
        .unwrap();
    }
}

#[test]
fn merge_isolated_owned_worktree_supports_reviewed_commits_without_source_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, source_id, input) = fixture(temp.path());
    let source = w.projects.get(&source_id).unwrap();
    let original = w.worktree_plan(&input).unwrap();
    let wid = devforge_serving::id();
    let dir = temp.path().join("data/worktrees").join(&wid);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tree");
    git(
        &source.path,
        &[
            "clone",
            "--no-hardlinks",
            source.path.to_str().unwrap(),
            path.to_str().unwrap(),
        ],
    );
    git(&path, &["checkout", "-qb", &input.branch]);
    git(&path, &["config", "user.name", "Fixture"]);
    git(&path, &["config", "user.email", "fixture@example.invalid"]);
    let child = w
        .projects
        .import(&path, &input.name, &source.php, &source.node, &w.catalog)
        .unwrap();
    w.projects
        .claim_worktree(&child.id, child.revision, &wid)
        .unwrap();
    let target_git = fs::canonicalize(path.join(".git")).unwrap();
    let record = Record {
        isolated: true,
        schema: 1,
        id: wid.clone(),
        input,
        source_path: source.path.clone(),
        common_dir: fs::canonicalize(source.path.join(".git")).unwrap(),
        git_dir: Some(target_git.clone()),
        commit: original.commit,
        domain: original.domain,
        review_digest: original.digest,
        project_id: Some(child.id),
        stage: "ready".into(),
        last_error: None,
        configuration: original.configuration,
    };
    fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    let entries = |root: &Path| -> Vec<String> {
        fn walk(base: &Path, p: &Path, all: &mut Vec<String>) {
            for e in fs::read_dir(p).unwrap() {
                let e = e.unwrap();
                let path = e.path();
                all.push(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                );
                if e.file_type().unwrap().is_dir() {
                    walk(base, &path, all);
                }
            }
        }
        let mut all = Vec::new();
        walk(root, root, &mut all);
        all.sort();
        all
    };
    fs::write(source.path.join("upstream.txt"), "committed source").unwrap();
    git(&source.path, &["add", "upstream.txt"]);
    git(&source.path, &["commit", "-qm", "source update"]);
    let source_head = git(&source.path, &["rev-parse", "HEAD"]);
    fs::write(source.path.join("uncommitted.txt"), "preserve source").unwrap();
    let source_status = git(&source.path, &["status", "--porcelain=v1"]);
    let source_refs = git(&source.path, &["show-ref"]);
    let source_objects = fs::canonicalize(source.path.join(".git/objects")).unwrap();
    let target_objects = fs::canonicalize(path.join(".git/objects")).unwrap();
    let a = entries(&source_objects);
    let b = entries(&target_objects);
    let plan = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert!(
        plan.isolated && plan.source_dirty && plan.blockers.is_empty(),
        "{:?}",
        plan.blockers
    );
    assert_eq!(plan.effect, "fast_forward");
    assert_eq!(entries(&source_objects), a);
    assert_eq!(entries(&target_objects), b);
    assert_eq!(git(&source.path, &["show-ref"]), source_refs);
    w.worktree_merge_guarded(&source_id, &wid, &plan.digest, &Default::default(), &|| {
        Ok(())
    })
    .unwrap();
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), source_head);
    assert_eq!(
        git(&source.path, &["status", "--porcelain=v1"]),
        source_status
    );
    assert_eq!(git(&source.path, &["show-ref"]), source_refs);
    assert!(
        !target_git.join("FETCH_HEAD").exists(),
        "fetch must not write FETCH_HEAD"
    );
    assert!(!path.join("uncommitted.txt").exists());

    fs::write(path.join("child.txt"), "child side").unwrap();
    git(&path, &["add", "child.txt"]);
    git(&path, &["commit", "-qm", "child"]);
    let target_parent = git(&path, &["rev-parse", "HEAD"]);
    fs::write(source.path.join("upstream-2.txt"), "source side").unwrap();
    git(&source.path, &["add", "upstream-2.txt"]);
    git(&source.path, &["commit", "-qm", "source side"]);
    let source_parent = git(&source.path, &["rev-parse", "HEAD"]);
    let source_refs = git(&source.path, &["show-ref"]);
    let a = entries(&source_objects);
    let b = entries(&target_objects);
    let divergent = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert_eq!(divergent.effect, "merge_commit");
    assert!(divergent.blockers.is_empty(), "{:?}", divergent.blockers);
    assert_eq!(entries(&source_objects), a);
    assert_eq!(entries(&target_objects), b);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(
        w.worktree_merge_plan(&source_id, &wid).unwrap().digest,
        divergent.digest
    );
    w.worktree_merge_guarded(
        &source_id,
        &wid,
        &divergent.digest,
        &Default::default(),
        &|| Ok(()),
    )
    .unwrap();
    let head = git(&path, &["rev-parse", "HEAD"]);
    assert_eq!(
        git(&path, &["rev-list", "--parents", "-n", "1", "HEAD"]),
        format!("{head} {target_parent} {source_parent}")
    );
    assert_eq!(git(&source.path, &["show-ref"]), source_refs);
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_parent);
    assert!(!target_git.join("FETCH_HEAD").exists());
    assert_eq!(
        fs::read_to_string(path.join("child.txt")).unwrap(),
        "child side"
    );
    assert_eq!(
        fs::read_to_string(path.join("upstream-2.txt")).unwrap(),
        "source side"
    );

    fs::write(path.join("upstream.txt"), "child conflict").unwrap();
    git(&path, &["add", "upstream.txt"]);
    git(&path, &["commit", "-qm", "target conflict"]);
    fs::write(source.path.join("upstream.txt"), "source conflict").unwrap();
    git(&source.path, &["add", "upstream.txt"]);
    git(&source.path, &["commit", "-qm", "source conflict"]);
    let before = git(&path, &["rev-parse", "HEAD"]);
    let source_before = git(&source.path, &["rev-parse", "HEAD"]);
    let a = entries(&source_objects);
    let b = entries(&target_objects);
    let conflict = w.worktree_merge_plan(&source_id, &wid).unwrap();
    assert!(
        conflict
            .blockers
            .iter()
            .any(|v| v == "worktree_merge_conflict"),
        "{:?}",
        conflict.blockers
    );
    assert_eq!(entries(&source_objects), a);
    assert_eq!(entries(&target_objects), b);
    assert!(
        w.worktree_merge_guarded(
            &source_id,
            &wid,
            &conflict.digest,
            &Default::default(),
            &|| Ok(())
        )
        .is_err()
    );
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), before);
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_before);
    assert_eq!(
        fs::read_to_string(path.join("upstream.txt")).unwrap(),
        "child conflict"
    );

    // Existing create chooses isolated mode for real gitlinks. Those remain
    // explicitly unsupported until nested repositories can be merged safely.
    let module = temp.path().join("Synthetic module");
    fs::create_dir(&module).unwrap();
    git(&module, &["init", "-q"]);
    git(&module, &["config", "user.name", "Fixture"]);
    git(
        &module,
        &["config", "user.email", "fixture@example.invalid"],
    );
    fs::write(module.join("child.txt"), "module fixture").unwrap();
    git(&module, &["add", "child.txt"]);
    git(&module, &["commit", "-qm", "module"]);
    git(
        &source.path,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            module.to_str().unwrap(),
            "module",
        ],
    );
    git(&source.path, &["commit", "-qm", "add real gitlink"]);
    let source_with_module = git(&source.path, &["rev-parse", "HEAD"]);
    let child_before = git(&path, &["rev-parse", "HEAD"]);
    assert_eq!(
        w.worktree_merge_plan(&source_id, &wid).err().unwrap().code,
        "worktree_submodules_unsupported"
    );
    assert_eq!(
        git(&source.path, &["rev-parse", "HEAD"]),
        source_with_module
    );
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), child_before);
}
