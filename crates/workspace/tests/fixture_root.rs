#![cfg(feature = "test-support")]
use devforge_workspace::testing::{FIXTURE_MARKER, FixtureRoot};
use std::fs;

#[test]
fn marker_initializes_only_new_leaf_and_binds_paths_and_filesystem_identity() {
    let parent = tempfile::tempdir().unwrap();
    let path = parent.path().join("new-root");
    let root = FixtureRoot::create_new(&path).unwrap();
    assert_eq!(root.root(), fs::canonicalize(&path).unwrap());
    assert_eq!(
        root.project_store_path(),
        root.root().join("e2e-secrets.json")
    );
    assert_eq!(
        root.control_store_path(),
        root.root().join("e2e-control-secrets.json")
    );
    assert!(FixtureRoot::create_new(&path).is_err());
    let original = fs::read(root.root().join(FIXTURE_MARKER)).unwrap();
    let mut wrong: serde_json::Value = serde_json::from_slice(&original).unwrap();
    wrong["filesystem_identity"] = serde_json::Value::String("file:wrong-identity".into());
    fs::write(
        root.root().join(FIXTURE_MARKER),
        serde_json::to_vec(&wrong).unwrap(),
    )
    .unwrap();
    assert!(FixtureRoot::validate(root.root()).is_err());
    fs::write(root.root().join(FIXTURE_MARKER), original).unwrap();
    assert!(FixtureRoot::validate(root.root()).is_ok());
    let other = FixtureRoot::create_new(&parent.path().join("other")).unwrap();
    fs::copy(
        root.root().join(FIXTURE_MARKER),
        other.root().join(FIXTURE_MARKER),
    )
    .unwrap();
    assert!(FixtureRoot::validate(other.root()).is_err());
    assert!(FixtureRoot::validate(&parent.path().join("absent")).is_err());
    assert!(!parent.path().join("absent").exists());
}
#[test]
fn malformed_missing_and_large_markers_are_not_repaired() {
    let parent = tempfile::tempdir().unwrap();
    let root = FixtureRoot::create_new(&parent.path().join("root")).unwrap();
    let marker = root.root().join(FIXTURE_MARKER);
    fs::remove_file(&marker).unwrap();
    assert!(FixtureRoot::validate(root.root()).is_err());
    assert!(!marker.exists());
    fs::write(&marker, b"malformed").unwrap();
    assert!(FixtureRoot::validate(root.root()).is_err());
    assert_eq!(fs::read(&marker).unwrap(), b"malformed");
    fs::write(&marker, vec![b'x'; 4097]).unwrap();
    assert!(FixtureRoot::validate(root.root()).is_err());
    assert_eq!(fs::metadata(marker).unwrap().len(), 4097);
}
#[cfg(unix)]
#[test]
fn symlinked_and_hardlinked_markers_and_roots_fail_closed() {
    use std::os::unix::fs::symlink;
    let parent = tempfile::tempdir().unwrap();
    let root = FixtureRoot::create_new(&parent.path().join("root")).unwrap();
    let marker = root.root().join(FIXTURE_MARKER);
    let outside = parent.path().join("outside-marker");
    fs::rename(&marker, &outside).unwrap();
    symlink(&outside, &marker).unwrap();
    assert!(FixtureRoot::validate(root.root()).is_err());
    fs::remove_file(&marker).unwrap();
    fs::hard_link(&outside, &marker).unwrap();
    assert!(FixtureRoot::validate(root.root()).is_err());
    symlink(root.root(), parent.path().join("root-link")).unwrap();
    assert!(FixtureRoot::validate(&parent.path().join("root-link")).is_err());
}
