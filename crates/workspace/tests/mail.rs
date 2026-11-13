use devforge_core::{ErrorCode, SecretStore};
use devforge_workspace::{Command, Workspace};
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
fn forgetting_project_stops_only_its_mail_listener_and_preserves_inbox_storage() {
    let temp = tempfile::tempdir().unwrap();
    let mut w = Workspace::open(temp.path().join("data"), Arc::new(Secrets::default())).unwrap();
    let target = devforge_runtime::package::Target::current().unwrap();
    let select = |kind: &str| {
        w.catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.kind == kind
                    && p.target == target
                    && (kind != "php" || p.executables.contains_key("php"))
            })
            .unwrap()
            .0
            .clone()
    };
    let php = select("php");
    let node = select("node");
    let mut ids = vec![];
    for name in ["A", "B"] {
        let path = temp.path().join(name);
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("public/index.php"), "<?php").unwrap();
        fs::write(
            path.join("composer.json"),
            r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
        )
        .unwrap();
        let p = w
            .projects
            .import(&path, name, &php, &node, &w.catalog)
            .unwrap();
        w.configure_mail(&p.id, true).unwrap();
        ids.push(p.id);
    }
    let p = w.projects.get(&ids[0]).unwrap();
    w.execute(
        Command::Forget {
            id: p.id,
            revision: p.revision,
        },
        &Default::default(),
    )
    .unwrap();
    assert!(
        devforge_inspector::mail::read_session(w.projects.root(), &ids[0])
            .unwrap()
            .is_none()
    );
    assert!(
        devforge_inspector::mail::read_session(w.projects.root(), &ids[1])
            .unwrap()
            .is_some()
    );
    assert!(devforge_inspector::mail::MailStore::open(w.projects.root(), &ids[0]).is_ok());
}
