use devforge_core::{ErrorCode, SecretStore};
use devforge_workspace::Workspace;
use std::{
    collections::BTreeMap,
    fs,
    sync::{Arc, Mutex},
};
#[derive(Default)]
struct Secrets(Mutex<BTreeMap<String, String>>);
impl SecretStore for Secrets {
    fn get(&self, k: &str) -> Result<Option<String>, ErrorCode> {
        Ok(self.0.lock().unwrap().get(k).cloned())
    }
    fn set(&self, k: &str, v: &str) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    fn delete(&self, k: &str) -> Result<(), ErrorCode> {
        self.0.lock().unwrap().remove(k);
        Ok(())
    }
}
#[test]
fn redacted_plan_is_stale_after_secret_env_or_config_changes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("Atlas Ўзбек & ' repo");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(path.join(".env"), "UNRELATED=preserve\n").unwrap();
    let secrets = Arc::new(Secrets::default());
    let mut w = Workspace::open(temp.path().join("data"), secrets.clone()).unwrap();
    w.manifest_init(&path, "8.5.12", "24.22.2", "2.11.2")
        .unwrap();
    let file = path.join("devforge.yaml");
    let mut m = devforge_projects::manifest::Manifest::parse(&fs::read(&file).unwrap()).unwrap();
    m.environment.insert("API_TOKEN".into(), "token".into());
    fs::write(&file, m.yaml().unwrap()).unwrap();
    let plan = w.manifest_plan(&path, true).unwrap();
    assert_eq!(plan.blockers, vec!["manifest_secret_missing"]);
    w.manifest_secret(&path, "token", "private-DO-NOT-EXPORT")
        .unwrap();
    let approved = w.manifest_plan(&path, true).unwrap();
    assert!(approved.blockers.is_empty());
    let json = serde_json::to_string(&approved).unwrap();
    assert!(!json.contains("private-DO-NOT-EXPORT"));
    assert!(json.contains("API_TOKEN"));
    w.manifest_secret(&path, "token", "rotated-DO-NOT-EXPORT")
        .unwrap();
    assert_ne!(
        w.manifest_plan(&path, true).unwrap().digest,
        approved.digest
    );
    assert_eq!(
        w.manifest_setup(&path, &approved.digest, true, true, &Default::default())
            .err()
            .unwrap()
            .code,
        "manifest_review_changed"
    );
    assert!(w.projects.list().unwrap().is_empty());
    let approved = w.manifest_plan(&path, true).unwrap();
    fs::write(path.join(".env"), "UNRELATED=changed\n").unwrap();
    assert_ne!(
        w.manifest_plan(&path, true).unwrap().digest,
        approved.digest
    );
    assert_eq!(
        w.manifest_setup(&path, &approved.digest, true, true, &Default::default())
            .err()
            .unwrap()
            .code,
        "manifest_review_changed"
    );
}
#[test]
fn exported_configuration_does_not_read_or_export_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("A");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let w = Workspace::open(temp.path().join("data"), Arc::new(Secrets::default())).unwrap();
    w.manifest_init(&path, "8.5.12", "24.22.2", "2.11.2")
        .unwrap();
    let plan = w.manifest_plan(&path, false).unwrap();
    let id = w
        .projects
        .import(
            &path,
            "A",
            &plan.packages["php"],
            &plan.packages["node"],
            &w.catalog,
        )
        .unwrap()
        .id;
    w.sites
        .configure(
            &w.projects,
            devforge_serving::site::Site {
                project_id: id.clone(),
                domain: "a.devforge.test".into(),
                https: false,
                vite: false,
            },
        )
        .unwrap();
    let output = temp.path().join("export.yaml");
    w.manifest_export(&id, Some(&output)).unwrap();
    assert!(!fs::read_to_string(output).unwrap().contains(&id));
    assert!(w.manifest_export(&id, None).is_err());
}

#[test]
fn old_project_json_without_secret_links_remains_readable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join("public")).unwrap();
    fs::write(root.join("public/index.php"), "<?php").unwrap();
    fs::write(
        root.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let data = temp.path().join("data");
    let w = Workspace::open(&data, Arc::new(Secrets::default())).unwrap();
    w.manifest_init(&root, "8.5.12", "24.22.2", "2.11.2")
        .unwrap();
    let plan = w.manifest_plan(&root, false).unwrap();
    let p = w
        .projects
        .import(
            &root,
            "Old",
            &plan.packages["php"],
            &plan.packages["node"],
            &w.catalog,
        )
        .unwrap();
    drop(w);
    // Missing local-link metadata defaults to the legacy per-project credential keys.
    let legacy = serde_json::to_value(&p).unwrap();
    let mut record = legacy.as_object().unwrap().clone();
    record.remove("environment_references");
    let decoded: devforge_projects::project::Project =
        serde_json::from_value(record.into()).unwrap();
    assert!(decoded.environment_references.is_empty());
    assert_eq!(decoded.php, p.php);
    assert_eq!(decoded.path, p.path);
}
