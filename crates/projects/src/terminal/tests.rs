use super::*;
use devforge_core::{ErrorCode, SecretStore};
use std::{collections::BTreeMap, fs};

#[test]
fn trusted_terminal_arguments_preserve_exact_shell_route_without_opening_window() {
    let extra = vec![std::ffi::OsString::from("--internal-fixture-store")];
    let args = shell_arguments("registered-project", &extra).unwrap();
    assert_eq!(
        args,
        vec![
            std::ffi::OsString::from("project"),
            "shell".into(),
            "registered-project".into(),
            "--internal-fixture-store".into()
        ]
    );
    assert_eq!(shell_arguments("registered-project", &[]).unwrap().len(), 3);
    assert!(shell_arguments("registered-project", &["bad\nargument".into()]).is_err());
    assert!(shell_arguments("registered-project", &vec!["flag".into(); 9]).is_err());
}

#[derive(Default)]
struct Secrets;
impl SecretStore for Secrets {
    fn get(&self, _: &str) -> std::result::Result<Option<String>, ErrorCode> {
        Ok(None)
    }
    fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
        unreachable!()
    }
    fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
        unreachable!()
    }
}

#[test]
fn existing_metadata_open_is_readonly_current_schema_and_never_initializes() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    assert!(ProjectStore::open_existing_current(&missing).is_err());
    assert!(!missing.exists());
    let root = temp.path().join("projects");
    drop(ProjectStore::open(&root).unwrap());
    let path = root.join("projects.sqlite3");
    let before = fs::read(&path).unwrap();
    let readonly = ProjectStore::open_existing_current(&root).unwrap();
    assert!(
        readonly
            .connection
            .execute("CREATE TABLE forbidden(id)", [])
            .is_err()
    );
    drop(readonly);
    assert_eq!(fs::read(&path).unwrap(), before);
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.pragma_update(None, "user_version", 7).unwrap();
    drop(writer);
    let before = fs::read(&path).unwrap();
    assert!(ProjectStore::open_existing_current(&root).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn existing_metadata_rejects_symlink_without_repair() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    drop(ProjectStore::open(&root).unwrap());
    let alias = temp.path().join("alias");
    symlink(&root, &alias).unwrap();
    assert!(ProjectStore::open_existing_current(&alias).is_err());
    let path = root.join("projects.sqlite3");
    let moved = temp.path().join("outside.sqlite3");
    fs::rename(&path, &moved).unwrap();
    symlink(&moved, &path).unwrap();
    let before = fs::read(&moved).unwrap();
    assert!(ProjectStore::open_existing_current(&root).is_err());
    assert_eq!(fs::read(&moved).unwrap(), before);
}

#[test]
fn terminal_provider_lookup_never_creates_or_repairs_shared_registry() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path().join("metadata")).unwrap();
    let packages = devforge_runtime::package::PackageStore::open(
        temp.path().join("packages"),
        devforge_runtime::package::Limits::default(),
    )
    .unwrap();
    let registry = store.root().join("debug-runtimes");
    assert!(
        devforge_debugger::provider::resolve_existing(
            store.root(),
            "missing",
            "8.5.0",
            &packages,
            &Secrets
        )
        .is_err()
    );
    assert!(!registry.exists());
    fs::create_dir(&registry).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&registry, fs::Permissions::from_mode(0o750)).unwrap();
    }
    let permissions = fs::metadata(&registry).unwrap().permissions();
    assert!(
        devforge_debugger::provider::resolve_existing(
            store.root(),
            "missing",
            "8.5.0",
            &packages,
            &Secrets
        )
        .is_err()
    );
    assert_eq!(fs::metadata(&registry).unwrap().permissions(), permissions);
}

#[cfg(any(unix, windows))]
fn directory_link(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let system = std::env::var_os("SystemRoot").unwrap();
        let link_arg = dunce::simplified(link).to_str().unwrap();
        let target_arg = dunce::simplified(target).to_str().unwrap();
        // Only disposable fixture paths enter this fixed builtin. Forbid quote
        // termination and percent expansion; /V:OFF disables delayed expansion.
        for value in [link_arg, target_arg] {
            assert!(
                !value.contains(['"', '%', '\r', '\n', '\0']),
                "unsupported fixture path syntax"
            );
        }
        let script = format!("mklink /J \"{link_arg}\" \"{target_arg}\"");
        // cmd /S /C strips this outer quote pair, preserving the two path pairs.
        // raw_arg avoids applying C-runtime argv escaping to cmd's command text.
        let result =
            std::process::Command::new(std::path::PathBuf::from(system).join("System32/cmd.exe"))
                .args(["/D", "/V:OFF", "/S", "/C"])
                .raw_arg(format!("\"{script}\""))
                .output()
                .unwrap();
        let mut diagnostic = String::from_utf8_lossy(&result.stderr).into_owned();
        for path in [
            link,
            target,
            link.parent().unwrap(),
            target.parent().unwrap(),
        ] {
            for spelling in [path, dunce::simplified(path)] {
                diagnostic =
                    diagnostic.replace(spelling.to_string_lossy().as_ref(), "[FIXTURE_PATH]");
            }
        }
        assert!(
            result.status.success(),
            "disposable junction fixture unavailable: exit={:?}, stderr={}",
            result.status.code(),
            diagnostic.chars().take(1024).collect::<String>()
        );
    }
}

#[cfg(any(unix, windows))]
#[test]
fn snapshot_store_guards_reject_directory_links_without_outside_changes() {
    let temp = tempfile::tempdir().unwrap();
    let owned = temp.path().join("owned");
    fs::create_dir(&owned).unwrap();
    let outside = temp.path().join("outside");
    drop(PackageStore::open(&outside, devforge_runtime::package::Limits::default()).unwrap());
    fs::write(outside.join("canary"), b"must remain unchanged").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o750)).unwrap();
    }
    let permissions = fs::metadata(&outside).unwrap().permissions();
    let linked = owned.join("runtimes");
    directory_link(&outside, &linked);
    assert!(existing_packages(&linked).is_err());
    assert_eq!(
        fs::read(outside.join("canary")).unwrap(),
        b"must remain unchanged"
    );
    assert_eq!(fs::metadata(&outside).unwrap().permissions(), permissions);
    let metadata = temp.path().join("metadata");
    drop(ProjectStore::open(&metadata).unwrap());
    let alias = owned.join("projects");
    directory_link(&metadata, &alias);
    let before = fs::read(metadata.join("projects.sqlite3")).unwrap();
    assert!(ProjectStore::open_existing_current(&alias).is_err());
    assert_eq!(fs::read(metadata.join("projects.sqlite3")).unwrap(), before);
    assert!(existing_packages(&owned.join("missing")).is_err());
    assert!(!owned.join("missing").exists());
    // A valid fixed store remains usable without creating missing layout parts.
    assert!(existing_packages(&outside).is_ok());
}

#[cfg(any(target_os = "macos", windows))]
mod native {
    use super::*;
    use devforge_runtime::{
        Cancellation, RuntimeError,
        package::{Archive, Limits, Package, Target, hash_file},
    };
    use std::cell::Cell;

    struct Fixture {
        temp: tempfile::TempDir,
        store: ProjectStore,
        packages: PackageStore,
        catalog: Catalog,
        project: crate::project::Project,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let store = ProjectStore::open(temp.path().join("metadata")).unwrap();
            let packages =
                PackageStore::open(temp.path().join("runtimes"), Limits::default()).unwrap();
            let mut catalog = Catalog {
                packages: BTreeMap::new(),
                defaults: BTreeMap::new(),
                superseded: BTreeMap::new(),
            };
            // Compile a tiny owned native argv reporter. Relocating restricted
            // Apple system binaries is not a valid executable fixture.
            let source = temp.path().join("argv.rs");
            let reporter = temp
                .path()
                .join(if cfg!(windows) { "argv.exe" } else { "argv" });
            fs::write(&source, b"fn main() { for arg in std::env::args().skip(1) { print!(\"{} \", arg); } println!(); }").unwrap();
            assert!(
                std::process::Command::new("rustc")
                    .arg(&source)
                    .arg("-o")
                    .arg(&reporter)
                    .status()
                    .unwrap()
                    .success()
            );
            for (id, kind, version, executable) in [
                ("php-old", "php", "8.4.0", Some("php")),
                ("php-new", "php", "8.5.0", Some("php")),
                ("node", "node", "24.0.0", Some("node")),
                ("composer-old", "composer", "2.11.2", None),
                ("composer-new", "composer", "2.11.3", None),
            ] {
                let archive = temp.path().join(format!("{id}.source"));
                if executable.is_some() {
                    fs::copy(&reporter, &archive).unwrap();
                } else {
                    fs::write(
                        &archive,
                        b"synthetic Composer resource; native echo exposes selected path",
                    )
                    .unwrap();
                }
                let relative = executable
                    .map(|name| {
                        if cfg!(windows) {
                            format!("{name}.exe")
                        } else {
                            name.into()
                        }
                    })
                    .unwrap_or_else(|| "composer.phar".into());
                let package = Package {
                    kind: kind.into(),
                    version: version.into(),
                    variant: id.into(),
                    target: Target::current().unwrap(),
                    url: format!("https://fixture.invalid/{id}"),
                    sha256: hash_file(&archive).unwrap(),
                    archive: Archive::File {
                        path: relative.clone(),
                    },
                    executables: executable
                        .map(|name| BTreeMap::from([(name.into(), relative)]))
                        .unwrap_or_default(),
                    resources: if executable.is_none() {
                        BTreeMap::from([("composer".into(), "composer.phar".into())])
                    } else {
                        BTreeMap::new()
                    },
                };
                packages
                    .install_cached(&package, &archive, &Cancellation::default(), |_| {})
                    .unwrap();
                catalog.packages.insert(id.into(), package);
            }
            catalog.defaults.insert("composer".into(), "2.11.2".into());
            let path = temp.path().join("Project Ўзбек & ' test");
            fs::create_dir_all(path.join("public")).unwrap();
            fs::write(path.join("public/index.php"), b"<?php").unwrap();
            fs::write(
                path.join("composer.json"),
                br#"{"require":{"php":"^8.4","laravel/framework":"^13"}}"#,
            )
            .unwrap();
            fs::write(path.join("package.json"), br#"{"engines":{"node":"^24"}}"#).unwrap();
            fs::write(path.join("artisan"), b"<?php").unwrap();
            let project = store
                .import(&path, "Snapshot", "php-old", "node", &catalog)
                .unwrap();
            Self {
                temp,
                store,
                packages,
                catalog,
                project,
            }
        }
    }

    #[test]
    fn immutable_terminal_sessions_keep_exact_pins_and_cleanup_only_their_directory() {
        let f = Fixture::new();
        let shared = f.store.root().join("config").join(&f.project.id);
        fs::create_dir_all(shared.join("bin")).unwrap();
        fs::write(shared.join("php.ini"), b"manager-owned-ini").unwrap();
        fs::write(
            shared.join(if cfg!(windows) {
                "bin/php.cmd"
            } else {
                "bin/php"
            }),
            b"manager-owned-shim",
        )
        .unwrap();
        fs::write(
            shared.join("npmrc"),
            b"//registry.fixture/:_authToken=fixture-existing-auth\n",
        )
        .unwrap();
        let reader = ProjectStore::open_existing_current(f.store.root()).unwrap();
        let first =
            Session::prepare(&reader, &f.project.id, &f.catalog, &f.packages, &Secrets).unwrap();
        let first_dir = first.directory().to_owned();
        let first_ini = fs::read(first.ini_path()).unwrap();
        let first_shim = fs::read(first.directory().join(if cfg!(windows) {
            "bin/php.cmd"
        } else {
            "bin/php"
        }))
        .unwrap();
        let changed = f
            .store
            .configure(
                &f.project.id,
                f.project.revision,
                "Changed",
                "php-new",
                "node",
                &f.catalog,
            )
            .unwrap();
        f.store
            .configure_composer(&changed.id, changed.revision, "composer-new", &f.catalog)
            .unwrap();
        let latest = f.store.get(&f.project.id).unwrap();
        crate::php_settings::save(
            &f.store,
            &latest.id,
            latest.revision,
            crate::php_settings::PhpSettings {
                memory_limit: Some("256M".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let second =
            Session::prepare(&reader, &f.project.id, &f.catalog, &f.packages, &Secrets).unwrap();
        let second_dir = second.directory().to_owned();
        assert_ne!(first_dir, second_dir);
        assert_eq!(first.project().php, "php-old");
        assert_eq!(first.project().composer, "composer-old");
        assert_eq!(second.project().php, "php-new");
        assert_eq!(second.project().composer, "composer-new");
        assert_eq!(fs::read(first.ini_path()).unwrap(), first_ini);
        assert!(!String::from_utf8_lossy(&first_ini).contains("memory_limit=256M"));
        assert!(
            fs::read_to_string(second.ini_path())
                .unwrap()
                .contains("memory_limit=256M")
        );
        assert_eq!(
            fs::read(first.directory().join(if cfg!(windows) {
                "bin/php.cmd"
            } else {
                "bin/php"
            }))
            .unwrap(),
            first_shim
        );
        let first_probe = first.probe().unwrap_or_else(|error| {
            panic!(
                "{error}: {}",
                fs::read_to_string(first.directory().join("terminal-probe-status.json"))
                    .unwrap_or_default()
            )
        });
        assert!(first_probe.contains(dunce::simplified(first.ini_path()).to_str().unwrap()));
        assert!(
            second
                .probe()
                .unwrap()
                .contains(dunce::simplified(second.ini_path()).to_str().unwrap())
        );
        assert_eq!(
            fs::read(shared.join("php.ini")).unwrap(),
            b"manager-owned-ini"
        );
        assert_eq!(
            fs::read(shared.join(if cfg!(windows) {
                "bin/php.cmd"
            } else {
                "bin/php"
            }))
            .unwrap(),
            b"manager-owned-shim"
        );
        assert_eq!(
            fs::read(shared.join("npmrc")).unwrap(),
            b"//registry.fixture/:_authToken=fixture-existing-auth\n"
        );
        for path in [
            first.ini_path().to_owned(),
            first
                .directory()
                .join("bin")
                .join(if cfg!(windows) { "php.cmd" } else { "php" }),
            first.directory().join("bin").join(if cfg!(windows) {
                "composer.cmd"
            } else {
                "composer"
            }),
        ] {
            assert!(
                !String::from_utf8_lossy(&fs::read(path).unwrap())
                    .contains("fixture-existing-auth")
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(first.directory())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(first.ini_path()).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(
            first
                .context
                .environment
                .get(std::ffi::OsStr::new("NPM_CONFIG_USERCONFIG"))
                .unwrap(),
            &dunce::simplified(&shared.join("npmrc")).as_os_str()
        );
        for id in ["php-old", "node", "composer-old"] {
            assert_eq!(
                f.packages.remove(f.catalog.get(id).unwrap()).err(),
                Some(RuntimeError::Busy)
            );
        }
        drop(first);
        assert!(!first_dir.exists());
        assert!(second_dir.exists());
        for id in ["php-old", "composer-old"] {
            f.packages.remove(f.catalog.get(id).unwrap()).unwrap();
        }
        assert_eq!(
            f.packages.remove(f.catalog.get("node").unwrap()).err(),
            Some(RuntimeError::Busy)
        );
        drop(second);
        assert!(!second_dir.exists());
        assert!(shared.join("npmrc").exists());
        assert!(f.temp.path().exists());
    }

    #[test]
    fn failed_snapshot_cleans_its_directory_without_touching_shared_ini() {
        let f = Fixture::new();
        let shared = f.store.root().join("config").join(&f.project.id);
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("php.ini"), b"manager-owned-ini").unwrap();
        let mut project = f.store.get(&f.project.id).unwrap();
        project
            .environment_names
            .push("MISSING_FIXTURE_SECRET".into());
        f.store
            .connection
            .execute(
                "UPDATE projects SET record=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&project).unwrap(), project.id],
            )
            .unwrap();
        let reader = ProjectStore::open_existing_current(f.store.root()).unwrap();
        assert!(matches!(
            Session::prepare(&reader, &f.project.id, &f.catalog, &f.packages, &Secrets),
            Err(E::SecretUnavailable)
        ));
        assert_eq!(
            fs::read(shared.join("php.ini")).unwrap(),
            b"manager-owned-ini"
        );
        assert!(!fs::read_dir(f.store.root()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".terminal-")
        }));
    }

    struct ChangingSecrets<'a> {
        fixture: &'a Fixture,
        changed: Cell<bool>,
    }
    impl SecretStore for ChangingSecrets<'_> {
        fn get(&self, key: &str) -> std::result::Result<Option<String>, ErrorCode> {
            if !self.changed.replace(true) {
                // This write must commit while the OS credential lookup runs:
                // no SQLite read transaction may be held across a prompt.
                let connection =
                    rusqlite::Connection::open(self.fixture.store.root().join("projects.sqlite3"))
                        .unwrap();
                connection.busy_timeout(std::time::Duration::ZERO).unwrap();
                connection.execute("UPDATE instances SET record=json_set(record,'$.port',5434) WHERE id='instance'", []).unwrap();
                connection.execute("UPDATE cache_instances SET record=json_set(record,'$.port',6381,'$.secret_ref','cache/new') WHERE id='cache'", []).unwrap();
                connection.execute("UPDATE database_bindings SET record=json_set(record,'$.database','new_database','$.secret_ref','db/new') WHERE project_id=?1", [&self.fixture.project.id]).unwrap();
            }
            Ok(Some(
                if key == "db/old" {
                    "old-fixture-password"
                } else if key == "db/new" {
                    "new-fixture-password"
                } else {
                    "fixture-value"
                }
                .into(),
            ))
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
            unreachable!()
        }
        fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
            unreachable!()
        }
    }
    #[test]
    fn binding_and_instance_are_frozen_before_secret_prompt_without_sql_lock() {
        let f = Fixture::new();
        let instance = crate::database::Instance {
            engine: crate::database::Engine::Postgresql,
            id: "instance".into(),
            package: "fixture".into(),
            port: 5433,
            admin: "admin".into(),
            secret_ref: "admin/ref".into(),
            ready: true,
        };
        f.store
            .connection
            .execute(
                "INSERT INTO instances VALUES(?1,?2)",
                rusqlite::params![instance.id, serde_json::to_string(&instance).unwrap()],
            )
            .unwrap();
        let binding = crate::database::Binding {
            project_id: f.project.id.clone(),
            instance_id: instance.id.clone(),
            database: "old_database".into(),
            role: "old_role".into(),
            secret_ref: "db/old".into(),
            ready: true,
        };
        f.store
            .connection
            .execute(
                "INSERT INTO database_bindings VALUES(?1,?2,?3)",
                rusqlite::params![
                    binding.project_id,
                    binding.instance_id,
                    serde_json::to_string(&binding).unwrap()
                ],
            )
            .unwrap();
        let mut project = f.store.get(&f.project.id).unwrap();
        project.environment_names.push("TEST_SNAPSHOT".into());
        project.services.cache = crate::cache::CacheDriver::Redis;
        project.services.redis_instance = Some("cache".into());
        fs::create_dir_all(project.path.join("vendor/predis/predis/src")).unwrap();
        fs::write(
            project.path.join("vendor/predis/predis/src/Client.php"),
            b"<?php",
        )
        .unwrap();
        let cache = crate::cache::CacheInstance {
            id: "cache".into(),
            project_id: project.id.clone(),
            package: "fixture".into(),
            runtime: "fixture".into(),
            port: 6380,
            username: "cache_user".into(),
            secret_ref: "cache/old".into(),
            admin_ref: "cache/admin".into(),
            ready: true,
        };
        f.store
            .connection
            .execute(
                "INSERT INTO cache_instances VALUES(?1,?2)",
                rusqlite::params![cache.id, serde_json::to_string(&cache).unwrap()],
            )
            .unwrap();
        f.store
            .connection
            .execute(
                "UPDATE projects SET record=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&project).unwrap(), project.id],
            )
            .unwrap();
        let reader = ProjectStore::open_existing_current(f.store.root()).unwrap();
        let secrets = ChangingSecrets {
            fixture: &f,
            changed: Cell::new(false),
        };
        let session =
            Session::prepare(&reader, &f.project.id, &f.catalog, &f.packages, &secrets).unwrap();
        let env = &session.context.environment;
        for (name, value) in [
            ("DB_DATABASE", "old_database"),
            ("DB_PORT", "5433"),
            ("DB_PASSWORD", "old-fixture-password"),
            ("REDIS_PORT", "6380"),
        ] {
            assert_eq!(env.get(std::ffi::OsStr::new(name)).unwrap(), value);
        }
        assert_eq!(
            f.store.binding(&f.project.id).unwrap().unwrap().database,
            "new_database"
        );
        assert_eq!(f.store.instance("instance").unwrap().port, 5434);
        for entry in fs::read_dir(session.directory()).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let contents = fs::read(entry.path()).unwrap();
                assert!(!String::from_utf8_lossy(&contents).contains("old-fixture-password"));
            }
        }
    }
}
