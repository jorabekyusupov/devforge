use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::project::{Catalog, Project};
use devforge_runtime::{
    Cancellation,
    package::{Archive, Package, Target, hash_file},
};
use devforge_workspace::{
    Workspace,
    catalog::{Availability, OfflineLimits},
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
struct DenySecrets;
impl SecretStore for DenySecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("offline report must not read secrets")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("offline report must not write secrets")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("offline report must not delete secrets")
    }
}
fn package(root: &Path, kind: &str, version: &str, variant: &str, tool: &str) -> Package {
    let archive = root.join(format!("{kind}-{version}-{variant}.fixture"));
    // Inert file-format headers only: no test runs a fake PHP/Node/Caddy binary
    // or claims a native runtime launch. The report explicitly verifies metadata.
    let mut header = vec![0; 96];
    match Target::current().unwrap() {
        Target::MacosArm64 => {
            header[..8].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe, 12, 0, 0, 1]);
        }
        Target::MacosX64 => {
            header[..8].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe, 7, 0, 0, 1]);
        }
        Target::WindowsX64 => {
            header[..2].copy_from_slice(b"MZ");
            header[60..64].copy_from_slice(&64u32.to_le_bytes());
            header[64..70].copy_from_slice(&[b'P', b'E', 0, 0, 0x64, 0x86]);
        }
    }
    fs::write(&archive, header).unwrap();
    Package {
        kind: kind.into(),
        version: version.into(),
        variant: variant.into(),
        target: Target::current().unwrap(),
        url: "https://example.invalid/inert-fixture".into(),
        sha256: hash_file(&archive).unwrap(),
        archive: Archive::File {
            path: "payload".into(),
        },
        executables: if kind == "composer" {
            BTreeMap::new()
        } else {
            BTreeMap::from([(tool.into(), "payload".into())])
        },
        resources: if kind == "composer" {
            BTreeMap::from([("composer".into(), "payload".into())])
        } else {
            BTreeMap::new()
        },
    }
}
fn fixture(root: &Path) -> (Workspace, Project) {
    let mut w = Workspace::open(root.join("data"), Arc::new(DenySecrets)).unwrap();
    w.catalog = Catalog {
        packages: BTreeMap::new(),
        defaults: BTreeMap::from([
            ("composer".into(), "2.11.2".into()),
            ("caddy".into(), "v2.11.9".into()),
        ]),
        superseded: BTreeMap::new(),
    };
    for (id, kind, version, variant, tool) in [
        ("php-cli", "php", "8.5.12", "cli", "php"),
        (
            "php-web",
            "php",
            "8.5.12",
            "fpm",
            if cfg!(windows) { "php-cgi" } else { "php-fpm" },
        ),
        ("node-pin", "node", "24.22.2", "default", "node"),
        ("composer-pin", "composer", "2.11.2", "default", "composer"),
        ("proxy", "caddy", "v2.11.9", "default", "caddy"),
    ] {
        let p = package(root, kind, version, variant, tool);
        let archive = root.join(format!("{kind}-{version}-{variant}.fixture"));
        w.packages
            .install_cached(&p, &archive, &Cancellation::default(), |_| {})
            .unwrap();
        w.catalog.packages.insert(id.into(), p);
    }
    let path = root.join("repo");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    fs::write(path.join(".env"), "TOKEN=OFFLINE_ENV_CANARY").unwrap();
    let p = w
        .projects
        .import(&path, "Fixture", "php-cli", "node-pin", &w.catalog)
        .unwrap();
    (w, p)
}
fn seed_project(w: &Workspace, p: &Project) {
    let db = rusqlite::Connection::open(w.projects.root().join("projects.sqlite3")).unwrap();
    db.execute(
        "UPDATE projects SET record=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(p).unwrap(), p.id],
    )
    .unwrap();
}
#[test]
fn persisted_composer_pin_survives_new_default_without_offline_success_claim() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, mut p) = fixture(temp.path());
    let lease_files_before = temp
        .path()
        .join("data/runtimes/leases")
        .read_dir()
        .unwrap()
        .count();
    let before = w
        .offline_readiness(&p.id, OfflineLimits::default())
        .unwrap();
    assert!(before.managed_artifacts_available);
    assert_eq!(before.offline_ready, None);
    assert!(
        before
            .problems
            .contains(&"minimum_os_and_provider_vendor_support_not_evaluated")
    );
    assert!(
        before
            .requirements
            .iter()
            .all(|r| r.state == Availability::Available)
    );
    let second = package(temp.path(), "composer", "2.99.0", "default", "composer");
    w.catalog.packages.insert("new-composer".into(), second);
    w.catalog
        .defaults
        .insert("composer".into(), "2.99.0".into());
    w.catalog
        .superseded
        .insert("composer-pin".into(), "new-composer".into());
    p.environment_names.push("PRIVATE_TOKEN".into());
    p.environment_references.insert(
        "PRIVATE_TOKEN".into(),
        "OFFLINE_SECRET_REFERENCE_CANARY".into(),
    );
    seed_project(&w, &p);
    let after = w
        .offline_readiness(&p.id, OfflineLimits::default())
        .unwrap();
    let composer = after
        .requirements
        .iter()
        .find(|r| r.role == "composer")
        .unwrap();
    assert_eq!(composer.package_id.as_deref(), Some("composer-pin"));
    assert_eq!(composer.state, Availability::Available);
    assert_eq!(composer.superseded_by.as_deref(), Some("new-composer"));
    assert!(
        composer
            .verification
            .contains("payload_hash_and_execution_unverified")
    );
    assert_ne!(before.catalog_fingerprint, after.catalog_fingerprint);
    let json = serde_json::to_string(&after).unwrap();
    assert!(!json.contains("OFFLINE_ENV_CANARY"));
    assert!(!json.contains("OFFLINE_SECRET_REFERENCE_CANARY"));
    assert!(!json.contains("https://"));
    assert_eq!(
        temp.path()
            .join("data/runtimes/leases")
            .read_dir()
            .unwrap()
            .count(),
        lease_files_before
    );
}
#[test]
fn missing_payload_foreign_target_and_unknown_pin_have_distinct_states() {
    let temp = tempfile::tempdir().unwrap();
    let (w, mut p) = fixture(temp.path());
    let package = w.catalog.get("node-pin").unwrap();
    fs::remove_file(
        temp.path()
            .join("data/runtimes/packages")
            .join(package.key().unwrap())
            .join("payload"),
    )
    .unwrap();
    let report = w
        .offline_readiness(&p.id, OfflineLimits::default())
        .unwrap();
    assert_eq!(
        report
            .requirements
            .iter()
            .find(|r| r.role == "node")
            .unwrap()
            .state,
        Availability::Missing
    );
    assert_eq!(report.offline_ready, Some(false));
    p.node = "unknown-exact-pin".into();
    seed_project(&w, &p);
    let report = w
        .offline_readiness(&p.id, OfflineLimits::default())
        .unwrap();
    assert_eq!(
        report
            .requirements
            .iter()
            .find(|r| r.role == "node")
            .unwrap()
            .state,
        Availability::Unverified
    );
    let mut foreign = w.catalog.clone();
    foreign.packages.get_mut("node-pin").unwrap().target =
        if Target::current().unwrap() == Target::WindowsX64 {
            Target::MacosArm64
        } else {
            Target::WindowsX64
        };
    let mut w = w;
    w.catalog = foreign;
    p.node = "node-pin".into();
    seed_project(&w, &p);
    let report = w
        .offline_readiness(&p.id, OfflineLimits::default())
        .unwrap();
    assert_eq!(
        report
            .requirements
            .iter()
            .find(|r| r.role == "node")
            .unwrap()
            .state,
        Availability::Unsupported
    );
}
#[test]
fn corrupt_receipt_and_receipt_budgets_are_unverified_not_missing() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p) = fixture(temp.path());
    let report = w
        .offline_readiness(
            &p.id,
            OfflineLimits {
                max_receipt_bytes: 64,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        report
            .requirements
            .iter()
            .any(|r| r.reason == "receipt_byte_limit")
    );
    assert!(
        report
            .requirements
            .iter()
            .all(|r| r.state == Availability::Unverified)
    );
    let package = w.catalog.get("composer-pin").unwrap();
    fs::write(
        temp.path()
            .join("data/runtimes/packages")
            .join(package.key().unwrap())
            .join(".devforge-receipt.json"),
        "corrupt",
    )
    .unwrap();
    let report = w
        .offline_readiness(&p.id, OfflineLimits::default())
        .unwrap();
    let composer = report
        .requirements
        .iter()
        .find(|r| r.role == "composer")
        .unwrap();
    assert_eq!(composer.state, Availability::Unverified);
    assert_eq!(composer.reason, "invalid_package_receipt");
    let report = w
        .offline_readiness(
            &p.id,
            OfflineLimits {
                max_total_receipt_bytes: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(report.problems.contains(&"receipt_byte_limit"));
    assert_eq!(report.receipt_bytes_read, 0);
}
#[test]
fn truncated_registry_and_requirements_never_claim_artifacts_available() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p) = fixture(temp.path());
    let mut other = p.clone();
    other.id = "0".repeat(32);
    let db = rusqlite::Connection::open(w.projects.root().join("projects.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO projects VALUES(?1,'other',1,1,?2)",
        rusqlite::params![other.id, serde_json::to_string(&other).unwrap()],
    )
    .unwrap();
    let report = w
        .offline_readiness(
            &p.id,
            OfflineLimits {
                max_registry_rows: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!report.registry_complete);
    assert!(!report.managed_artifacts_available);
    assert!(report.problems.contains(&"project_metadata_incomplete"));
    let report = w
        .offline_readiness(
            &p.id,
            OfflineLimits {
                max_requirements: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!report.managed_artifacts_available);
    assert!(report.problems.contains(&"requirement_limit"));
    assert_eq!(report.requirements.len(), 1);
}
#[cfg(unix)]
#[test]
fn symlinked_payload_is_unverified_and_project_dependencies_are_metadata_only() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let (w, p) = fixture(temp.path());
    fs::create_dir_all(p.path.join("vendor")).unwrap();
    fs::write(p.path.join("vendor/autoload.php"), "DO_NOT_READ_VENDOR").unwrap();
    let package = w.catalog.get("composer-pin").unwrap();
    let payload = temp
        .path()
        .join("data/runtimes/packages")
        .join(package.key().unwrap())
        .join("payload");
    fs::remove_file(&payload).unwrap();
    let outside = temp.path().join("outside");
    fs::write(&outside, "DO_NOT_READ_OUTSIDE").unwrap();
    symlink(&outside, payload).unwrap();
    let report = w
        .offline_readiness(&p.id, OfflineLimits::default())
        .unwrap();
    assert_eq!(
        report
            .requirements
            .iter()
            .find(|r| r.role == "composer")
            .unwrap()
            .reason,
        "link_not_followed"
    );
    let vendor = report
        .prerequisites
        .iter()
        .find(|p| p.code == "composer_dependencies")
        .unwrap();
    assert_eq!(vendor.state, Availability::Unverified);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("DO_NOT_READ_VENDOR"));
    assert!(!json.contains("DO_NOT_READ_OUTSIDE"));
}

#[test]
fn database_and_cache_report_exact_registry_pins_without_initializing_services() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, mut p) = fixture(temp.path());
    for (id, kind, version, tool) in [
        ("database-pin", "postgresql", "18.4", "postgres"),
        ("cache-pin", "garnet", "2.2.0", "garnet"),
        ("cache-host-pin", "dotnet", "10.0.0", "dotnet"),
    ] {
        let pkg = package(temp.path(), kind, version, "default", tool);
        w.catalog.packages.insert(id.into(), pkg);
    }
    let database_id = "a".repeat(32);
    let cache_id = "b".repeat(32);
    let db = rusqlite::Connection::open(w.projects.root().join("projects.sqlite3")).unwrap();
    // Synthetic records only. No database/native service has been initialized.
    let instance = serde_json::json!({"id":database_id,"package":"database-pin","port":1,"admin":"fixture","secret_ref":"DO_NOT_EXPORT_DB_REF","ready":false});
    db.execute(
        "INSERT INTO instances VALUES(?1,?2)",
        rusqlite::params![database_id, instance.to_string()],
    )
    .unwrap();
    let binding = serde_json::json!({"project_id":p.id,"instance_id":database_id,"database":"fixture","role":"fixture","secret_ref":"DO_NOT_EXPORT_BIND_REF","ready":false});
    db.execute(
        "INSERT INTO database_bindings VALUES(?1,?2,?3)",
        rusqlite::params![p.id, database_id, binding.to_string()],
    )
    .unwrap();
    let cache = serde_json::json!({"id":cache_id,"project_id":p.id,"package":"cache-pin","runtime":"cache-host-pin","port":1,"username":"fixture","secret_ref":"DO_NOT_EXPORT_CACHE_REF","admin_ref":"DO_NOT_EXPORT_ADMIN_REF","ready":false});
    db.execute(
        "INSERT INTO cache_instances VALUES(?1,?2)",
        rusqlite::params![cache_id, cache.to_string()],
    )
    .unwrap();
    p.services.cache = devforge_projects::cache::CacheDriver::Redis;
    p.services.redis_instance = Some(cache_id);
    seed_project(&w, &p);
    let report = w
        .offline_readiness(&p.id, OfflineLimits::default())
        .unwrap();
    for (role, pin) in [
        ("database", "database-pin"),
        ("cache", "cache-pin"),
        ("cache_runtime", "cache-host-pin"),
    ] {
        let requirement = report.requirements.iter().find(|r| r.role == role).unwrap();
        assert_eq!(requirement.package_id.as_deref(), Some(pin));
        assert_eq!(requirement.state, Availability::Missing);
    }
    assert!(
        report
            .problems
            .contains(&"database_initialization_unverified")
    );
    assert!(report.problems.contains(&"cache_initialization_unverified"));
    assert!(!w.projects.root().join("databases").exists());
    assert!(!w.projects.root().join("cache-services").exists());
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("DO_NOT_EXPORT")
    );
}
