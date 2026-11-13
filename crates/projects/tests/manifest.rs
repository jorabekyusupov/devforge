use devforge_projects::{manifest::*, project::Catalog};
use devforge_runtime::package::Target;
fn sample() -> Manifest {
    let mut m = Manifest {
        schema: 1,
        mail: None,
        project: Project {
            name: "Atlas".into(),
            domain: "atlas.devforge.test".into(),
            https: true,
            vite: false,
            queue: false,
        },
        runtimes: Runtimes {
            php: "8.5.12".into(),
            node: "24.22.2".into(),
            composer: "2.11.2".into(),
        },
        database: None,
        redis: None,
        services: Default::default(),
        php_settings: Default::default(),
        environment: std::collections::BTreeMap::from([("API_TOKEN".into(), "api_token".into())]),
        setup: Default::default(),
        pins: Default::default(),
    };
    m.pin_all(&Catalog::bundled()).unwrap();
    m
}
#[test]
fn portable_pin_resolution_has_no_machine_paths_or_ranges() {
    let m = sample();
    let yaml = m.yaml().unwrap();
    assert!(!yaml.contains("/Users/"));
    let parsed = Manifest::parse(yaml.as_bytes()).unwrap();
    for t in [Target::MacosArm64, Target::MacosX64, Target::WindowsX64] {
        let roles = parsed.verify_pins(&Catalog::bundled(), t);
        assert!(
            roles.is_ok(),
            "target={t:?} pins={:?}",
            parsed.pins.keys().collect::<Vec<_>>()
        );
        let roles = roles.unwrap();
        assert!(roles.contains_key("php_server"));
    }
    let mut bad = m;
    bad.runtimes.php = "^8.5".into();
    assert!(bad.validate().is_err());
}
#[test]
fn yaml_rejects_unknown_keys_duplicates_aliases_and_multi_documents() {
    let yaml = sample().yaml().unwrap();
    for suffix in [
        "\nschema: 1\n",
        "\nhooks: [dangerous]\n",
        "\n---\nschema: 1\n",
    ] {
        assert!(Manifest::parse(format!("{yaml}{suffix}").as_bytes()).is_err());
    }
    let alias = yaml.replace("name: Atlas", "name: &a Atlas");
    assert!(Manifest::parse(alias.as_bytes()).is_err());
    let tag = yaml.replace("name: Atlas", "name: !include /private/secret");
    assert!(Manifest::parse(tag.as_bytes()).is_err());
}
#[test]
fn pin_tampering_and_unsafe_environment_references_fail_closed() {
    let mut m = sample();
    let roles = m
        .resolve(&Catalog::bundled(), Target::current().unwrap())
        .unwrap();
    m.pins.insert(roles["php"].clone(), "0".repeat(64));
    assert!(
        m.verify_pins(&Catalog::bundled(), Target::current().unwrap())
            .is_err()
    );
    for name in [
        "PATH",
        "DB_PASSWORD",
        "REDIS_PASSWORD",
        "APP_KEY",
        "QUEUE_CONNECTION",
        "DEVFORGE_DATA_DIR",
    ] {
        assert!(environment_name(name).is_err());
    }
    m.environment
        .insert("API_TOKEN".into(), "project/other/secret".into());
    assert!(m.validate().is_err());
}

#[test]
fn dotenv_profile_writes_only_reviewed_environment_names_and_checks_staleness() {
    use devforge_core::{ErrorCode, SecretStore};
    use devforge_projects::{env_file::EnvPlan, environment, project::ProjectStore};
    use std::{collections::BTreeMap, fs, sync::Mutex};
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
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repo");
    fs::create_dir_all(root.join("public")).unwrap();
    fs::write(root.join("public/index.php"), "<?php").unwrap();
    fs::write(
        root.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let c = Catalog::bundled();
    let m = sample();
    let roles = m.resolve(&c, Target::current().unwrap()).unwrap();
    let store = ProjectStore::open(temp.path().join("data")).unwrap();
    let p = store
        .import(&root, "A", &roles["php"], &roles["node"], &c)
        .unwrap();
    let s = Secrets::default();
    let p = environment::set(&store, &s, &p.id, p.revision, "API_TOKEN", "approved").unwrap();
    environment::set(
        &store,
        &s,
        &p.id,
        p.revision,
        "UNREVIEWED_TOKEN",
        "must-not-leak",
    )
    .unwrap();
    fs::write(root.join(".env"), "UNRELATED=keep\n").unwrap();
    let keys = vec!["API_TOKEN".into()];
    let plan = EnvPlan::profile(&store, &p.id, &s, &keys).unwrap();
    assert_eq!(plan.changed_keys(), keys);
    plan.apply_if_changed(&store).unwrap();
    let content = fs::read_to_string(root.join(".env")).unwrap();
    assert!(content.contains("approved") && !content.contains("must-not-leak"));
    assert!(
        EnvPlan::profile(&store, &p.id, &s, &keys)
            .unwrap()
            .apply_if_changed(&store)
            .unwrap()
            .backup
            .is_none()
    );
    let plan = EnvPlan::profile(&store, &p.id, &s, &keys).unwrap();
    fs::write(root.join(".env"), "EXTERNAL=changed\n").unwrap();
    assert!(plan.apply_if_changed(&store).is_err());
}
