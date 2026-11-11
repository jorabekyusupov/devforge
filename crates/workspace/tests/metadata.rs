use devforge_core::{ErrorCode, SecretStore};
use devforge_runtime::{Cancellation, package::Target};
use devforge_workspace::{Command, Workspace};
use std::{fs, sync::Arc};
struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("import must not read secrets")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("import must not write secrets")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("import must not delete secrets")
    }
}
#[test]
fn registration_preserves_project_files_and_rejects_colliding_domains_before_import() {
    let root = tempfile::tempdir().unwrap();
    let mut w = Workspace::open(root.path().join("data"), Arc::new(NoSecrets)).unwrap();
    let target = Target::current().unwrap();
    let php = w
        .catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "php" && p.executables.contains_key("php") && p.target == target)
        .unwrap()
        .0
        .clone();
    let node = w
        .catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "node" && p.target == target)
        .unwrap()
        .0
        .clone();
    let fixture = |name: &str| {
        let p = root.path().join(name);
        fs::create_dir_all(p.join("public")).unwrap();
        fs::write(p.join("public/index.php"), "<?php").unwrap();
        fs::write(p.join("composer.json"),r#"{"require":{"php":"^8.3","laravel/framework":"^13"},"scripts":{"post-install-cmd":"touch should-not-run"}}"#).unwrap();
        fs::write(p.join(".env"), "UNRELATED=keep\n").unwrap();
        p
    };
    let a = fixture("A & Ўзбек");
    let b = fixture("B");
    let command = |path| Command::Import {
        path,
        name: "A".into(),
        php: php.clone(),
        node: node.clone(),
        domain: "atlas.devforge.test".into(),
        https: true,
        vite: false,
        queue: true,
    };
    w.execute(command(a.clone()), &Cancellation::default())
        .unwrap();
    assert_eq!(
        fs::read_to_string(a.join(".env")).unwrap(),
        "UNRELATED=keep\n"
    );
    assert!(!a.join("should-not-run").exists());
    assert!(w.execute(command(b), &Cancellation::default()).is_err());
    assert_eq!(w.snapshot().unwrap().projects.len(), 1);
    let row = w.snapshot().unwrap().projects.remove(0);
    assert_eq!(
        w.open_target(&row.project.id, "browser").unwrap_err().code,
        "not_running"
    );
    assert!(w.open_target(&row.project.id, "arbitrary-shell").is_err());
    drop(w);
    let w = Workspace::open(root.path().join("data"), Arc::new(NoSecrets)).unwrap();
    assert!(w.snapshot().unwrap().projects[0].development.queue);
}
