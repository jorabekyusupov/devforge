use devforge_core::{ErrorCode, SecretStore};
use devforge_projects::{manifest::Manifest, project::Project};
use devforge_runtime::package::Target;
use devforge_workspace::{
    Workspace,
    environment_report::{Difference, ReportLimits, Support},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};
struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("report must not query credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("report must not mutate credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("report must not delete credentials")
    }
}
fn fixture(root: &Path) -> (Workspace, Project, Manifest) {
    let path = root.join("Project");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(path.join("composer.json"),r#"{"require":{"php":"^8.3","laravel/framework":"^13","ext-pdo_mysql":"*"},"require-dev":{"ext-mbstring":"*"},"extra":{"PRIVATE_COMPOSER_VALUE":"PRIVATE_REPORT_CANARY"}}"#).unwrap();
    fs::write(path.join(".env"), "PRIVATE_VALUE=PRIVATE_REPORT_CANARY\n").unwrap();
    let w = Workspace::open(root.join("data"), Arc::new(NoSecrets)).unwrap();
    w.manifest_init(&path, "8.5.12", "24.22.2", "2.11.2")
        .unwrap();
    let m = Manifest::parse(&fs::read(path.join("devforge.yaml")).unwrap()).unwrap();
    let packages = m.resolve(&w.catalog, Target::current().unwrap()).unwrap();
    let p = w
        .projects
        .import_with_composer(
            &path,
            "Project",
            &packages["php"],
            &packages["node"],
            &packages["composer"],
            &w.catalog,
        )
        .unwrap();
    w.sites
        .configure(
            &w.projects,
            devforge_serving::site::Site {
                project_id: p.id.clone(),
                domain: m.project.domain.clone(),
                https: false,
                vite: false,
            },
        )
        .unwrap();
    (w, p, m)
}
fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for e in fs::read_dir(path).unwrap() {
            let p = e.unwrap().path();
            let m = fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                visit(root, &p, out)
            } else if m.is_file() {
                out.insert(p.strip_prefix(root).unwrap().into(), fs::read(p).unwrap());
            }
        }
    }
    visit(root, root, &mut out);
    out
}
fn seed(w: &Workspace, p: &Project) {
    let db = rusqlite::Connection::open(w.projects.root().join("projects.sqlite3")).unwrap();
    db.execute(
        "UPDATE projects SET record=?1 WHERE id=?2",
        rusqlite::params![serde_json::to_string(p).unwrap(), p.id],
    )
    .unwrap();
    db.close().unwrap();
}
#[test]
fn report_compares_existing_metadata_without_credentials_probes_or_mutations() {
    let temp = tempfile::tempdir().unwrap();
    let (w, mut p, mut m) = fixture(temp.path());
    m.environment.insert("API_TOKEN".into(), "api_token".into());
    fs::write(p.path.join("devforge.yaml"), m.yaml().unwrap()).unwrap();
    p.environment_names.push("API_TOKEN".into());
    p.environment_references
        .insert("API_TOKEN".into(), "PRIVATE_SECRET_REFERENCE_CANARY".into());
    seed(&w, &p);
    let before = tree(temp.path());
    let report = w
        .environment_report(&p.id, ReportLimits::default())
        .unwrap();
    assert_eq!(report.matches_recorded_configuration, Some(true));
    assert_ne!(report.environment_ready, Some(true));
    assert_eq!(report.portability.len(), 3);
    assert!(
        report
            .portability
            .iter()
            .all(|p| p.native_support == Support::Unverified)
    );
    assert_eq!(report.secrets.len(), 1);
    assert!(report.secrets[0].registered_name);
    assert_eq!(report.secrets[0].availability, Support::Unverified);
    assert_eq!(report.extensions.len(), 2);
    assert!(
        report
            .extensions
            .iter()
            .all(|e| e.cached_present.is_none() && e.current_support == Support::Unverified)
    );
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("PRIVATE_REPORT_CANARY"));
    assert!(!json.contains("PRIVATE_SECRET_REFERENCE_CANARY"));
    assert!(!json.contains(&p.path.display().to_string()));
    assert_eq!(tree(temp.path()), before);
}
#[test]
fn runtime_version_drift_and_missing_target_do_not_change_selected_packages() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p, mut m) = fixture(temp.path());
    let old = p.node.clone();
    m.runtimes.node = "99.0.0".into();
    fs::write(p.path.join("devforge.yaml"), m.yaml().unwrap()).unwrap();
    let report = w
        .environment_report(&p.id, ReportLimits::default())
        .unwrap();
    let node = report
        .comparisons
        .iter()
        .find(|c| c.key == "runtimes.node.version")
        .unwrap();
    assert_eq!(node.state, Difference::Mismatch);
    assert_eq!(node.expected.as_deref(), Some("99.0.0"));
    assert_eq!(report.matches_recorded_configuration, Some(false));
    assert_eq!(report.environment_ready, Some(false));
    assert!(
        report
            .portability
            .iter()
            .all(|p| p.catalog_support == Support::Unsupported)
    );
    assert_eq!(w.projects.get(&p.id).unwrap().node, old);
}
#[test]
fn matching_version_is_not_matching_exact_pin_or_a_valid_manifest_digest() {
    let temp = tempfile::tempdir().unwrap();
    let (mut w, mut p, mut m) = fixture(temp.path());
    let original = p.node.clone();
    let package = w.catalog.get(&original).unwrap().clone();
    w.catalog
        .packages
        .insert("zz-explicit-alternative-node".into(), package);
    p.node = "zz-explicit-alternative-node".into();
    seed(&w, &p);
    let report = w
        .environment_report(&p.id, ReportLimits::default())
        .unwrap();
    assert_eq!(
        report
            .comparisons
            .iter()
            .find(|c| c.key == "runtimes.node.version")
            .unwrap()
            .state,
        Difference::Match
    );
    assert_eq!(
        report
            .comparisons
            .iter()
            .find(|c| c.key == "runtimes.node.package")
            .unwrap()
            .state,
        Difference::Mismatch
    );
    m.pins.insert(p.php.clone(), "0".repeat(64));
    fs::write(p.path.join("devforge.yaml"), m.yaml().unwrap()).unwrap();
    let report = w
        .environment_report(&p.id, ReportLimits::default())
        .unwrap();
    assert!(report.problems.contains(&"manifest_pin_digest_mismatch"));
    assert!(
        report
            .portability
            .iter()
            .any(|p| p.catalog_support == Support::Unverified)
    );
    assert_eq!(report.environment_ready, Some(false));
}
#[test]
fn incomplete_registry_and_low_file_budget_are_explicitly_unverified() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p, _) = fixture(temp.path());
    let report = w
        .environment_report(
            &p.id,
            ReportLimits {
                max_file_bytes: 16,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(report.problems.contains(&"metadata_file_byte_limit"));
    assert_eq!(report.matches_recorded_configuration, None);
    assert!(report.manifest_sha256.is_none());
    let report = w
        .environment_report(
            &p.id,
            ReportLimits {
                max_registry_rows: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert_ne!(report.matches_recorded_configuration, Some(true));
    assert!(
        w.environment_report(
            &p.id,
            ReportLimits {
                max_facts: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[cfg(unix)]
#[test]
fn linked_manifest_is_not_followed_and_replaced_root_is_not_visited() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let (w, p, _) = fixture(temp.path());
    let external = temp.path().join("private-outside.yaml");
    fs::write(&external, "PRIVATE_EXTERNAL_CANARY").unwrap();
    fs::remove_file(p.path.join("devforge.yaml")).unwrap();
    symlink(&external, p.path.join("devforge.yaml")).unwrap();
    let report = w
        .environment_report(&p.id, ReportLimits::default())
        .unwrap();
    assert!(report.problems.contains(&"metadata_file_not_regular"));
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("PRIVATE_EXTERNAL_CANARY")
    );
    fs::rename(&p.path, temp.path().join("Original")).unwrap();
    fs::create_dir(&p.path).unwrap();
    fs::write(p.path.join("devforge.yaml"), "PRIVATE_REPLACED_CANARY").unwrap();
    let report = w
        .environment_report(&p.id, ReportLimits::default())
        .unwrap();
    assert!(
        report
            .problems
            .contains(&"project_identity_unverified_no_repository_files_visited")
    );
    assert_eq!(report.file_bytes_read, 0);
    assert!(
        !serde_json::to_string(&report)
            .unwrap()
            .contains("PRIVATE_REPLACED_CANARY")
    );
}

#[test]
fn name_only_and_missing_alias_registration_are_known_metadata_drift() {
    let temp = tempfile::tempdir().unwrap();
    let (w, p, mut m) = fixture(temp.path());
    m.project.name = "Renamed repository project".into();
    fs::write(p.path.join("devforge.yaml"), m.yaml().unwrap()).unwrap();
    let report = w
        .environment_report(&p.id, ReportLimits::default())
        .unwrap();
    assert_eq!(
        report
            .comparisons
            .iter()
            .find(|c| c.key == "project.name")
            .unwrap()
            .state,
        Difference::Mismatch
    );
    assert_eq!(report.matches_recorded_configuration, Some(false));
    m.project.name = p.name.clone();
    m.environment
        .insert("API_TOKEN".into(), "local_token".into());
    fs::write(p.path.join("devforge.yaml"), m.yaml().unwrap()).unwrap();
    let report = w
        .environment_report(&p.id, ReportLimits::default())
        .unwrap();
    assert_eq!(
        report
            .comparisons
            .iter()
            .find(|c| c.key == "environment.API_TOKEN.registered_name")
            .unwrap()
            .state,
        Difference::Mismatch
    );
    assert_eq!(report.matches_recorded_configuration, Some(false));
    assert!(!report.secrets[0].registered_name);
    assert_eq!(report.secrets[0].availability, Support::Unverified);
}
