mod common;
use devforge_control::ErrorCode;
use devforge_manager::credential_file::CredentialFile;
use serde_json::json;
use std::path::Path;

#[test]
fn native_export_creates_only_fresh_private_leaves_and_loads_without_repair() {
    use devforge_manager::{
        auth::TrustedDesktopAction,
        credential_file::{create_new_private_export_dir, write_new_private_export_file},
    };
    let root = common::root("df-native-export-");
    let canonical_root = std::fs::canonicalize(root.path()).unwrap();
    let dir = canonical_root.join("client");
    let action = TrustedDesktopAction::from_explicit_desktop_action();
    create_new_private_export_dir(&dir, &action).unwrap();
    assert!(create_new_private_export_dir(&dir, &action).is_err());
    let path = dir.join("credential.json");
    let bytes = serde_json::to_vec(
        &json!({"schema":1,"project_id":"project-a","credential":"3".repeat(64)}),
    )
    .unwrap();
    write_new_private_export_file(&path, &bytes, &action).unwrap();
    assert_eq!(
        CredentialFile::load(&path).unwrap().project_id().as_str(),
        "project-a"
    );
    assert!(write_new_private_export_file(&path, b"overwrite", &action).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(
        write_new_private_export_file(&dir.join("oversized"), &vec![0; 65_537], &action).is_err()
    );
    assert!(!dir.join("oversized").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let alias = root.path().join("alias");
        symlink(&dir, &alias).unwrap();
        assert!(write_new_private_export_file(&alias.join("unsafe"), b"secret", &action).is_err());
        assert!(!dir.join("unsafe").exists());
    }
}

#[test]
fn bounded_private_file_requires_complete_strict_content_and_never_creates_missing_files() {
    let root = common::root("df-mcp-file-");
    let missing = root.path().join("missing");
    assert!(matches!(
        CredentialFile::load(&missing),
        Err(ErrorCode::Unauthorized)
    ));
    assert!(!missing.exists());
    assert!(matches!(
        CredentialFile::load(Path::new("relative")),
        Err(ErrorCode::InvalidInput)
    ));
    let token = "1".repeat(64);
    let text = serde_json::to_vec(&json!({"schema":1,"project_id":"project-a","credential":token}))
        .unwrap();
    let path = root.path().join("valid");
    common::credential_file(&path, &text);
    let loaded = CredentialFile::load(&path).unwrap();
    assert_eq!(loaded.project_id().as_str(), "project-a");
    assert!(!format!("{loaded:?}").contains(&token));
    let trailing = root.path().join("trailing");
    let mut bytes = text.clone();
    bytes.extend_from_slice(b"\n{}");
    common::credential_file(&trailing, &bytes);
    assert!(matches!(
        CredentialFile::load(&trailing),
        Err(ErrorCode::InvalidInput)
    ));
    let extra = root.path().join("role");
    common::credential_file(
        &extra,
        &serde_json::to_vec(
            &json!({"schema":1,"project_id":"project-a","credential":token,"role":"desktop"}),
        )
        .unwrap(),
    );
    assert!(matches!(
        CredentialFile::load(&extra),
        Err(ErrorCode::InvalidInput)
    ));
    let large = root.path().join("large");
    common::credential_file(&large, &vec![b' '; 4097]);
    assert!(matches!(
        CredentialFile::load(&large),
        Err(ErrorCode::OutputLimitExceeded)
    ));
    let invalid = root.path().join("invalid");
    common::credential_file(&invalid, b"\xff");
    assert!(matches!(
        CredentialFile::load(&invalid),
        Err(ErrorCode::InvalidInput)
    ));
}

#[cfg(unix)]
#[test]
fn linked_nonregular_or_nonprivate_files_fail_closed_without_permissions_repair() {
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };
    let root = common::root("df-mcp-file-kind-");
    let file = root.path().join("credential");
    common::credential_file(
        &file,
        &serde_json::to_vec(
            &json!({"schema":1,"project_id":"project-a","credential":"2".repeat(64)}),
        )
        .unwrap(),
    );
    let alias = root.path().join("alias");
    symlink(&file, &alias).unwrap();
    assert!(matches!(
        CredentialFile::load(&alias),
        Err(ErrorCode::Unauthorized)
    ));
    let link = root.path().join("hardlink");
    fs::hard_link(&file, &link).unwrap();
    assert!(matches!(
        CredentialFile::load(&file),
        Err(ErrorCode::Unauthorized)
    ));
    fs::remove_file(link).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        CredentialFile::load(&file),
        Err(ErrorCode::Unauthorized)
    ));
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o644
    );
    assert!(matches!(
        CredentialFile::load(root.path()),
        Err(ErrorCode::Unauthorized)
    ));
}
