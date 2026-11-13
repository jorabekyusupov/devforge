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
        w.worktree_archive(&wid, "purge", &purge.digest, true, &Default::default())
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
