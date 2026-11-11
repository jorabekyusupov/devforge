mod support;
use devforge_runtime::{
    Cancellation, RuntimeError,
    package::{Archive, InstallStage, Limits, PackageStore, Target, hash_file},
};
use std::{fs, io::Read};
use support::*;

#[test]
fn install_reuse_pins_exact_identity_and_leases_prevent_removal() {
    let f = Fixture::new();
    let mut stages = vec![];
    let installed = f
        .store
        .install_cached(&f.package, &f.archive, &Cancellation::default(), |p| {
            stages.push(p.stage)
        })
        .unwrap();
    assert!(!installed.reused);
    assert!(stages.contains(&InstallStage::Activating));
    assert_eq!(stages.last(), Some(&InstallStage::Installed));
    assert!(
        f.store
            .install_cached(&f.package, &f.archive, &Cancellation::default(), |_| {})
            .unwrap()
            .reused
    );
    let executable = f.store.executable(&f.package, "fixture").unwrap();
    assert_eq!(f.store.remove(&f.package).err(), Some(RuntimeError::Busy));
    drop(executable);
    f.store.remove(&f.package).unwrap();
    assert!(!installed.directory.exists());
}
#[test]
fn cancelled_download_and_activation_leave_no_partial_install() {
    for when in [InstallStage::Downloading, InstallStage::Activating] {
        let f = Fixture::new();
        let cancel = Cancellation::default();
        let mut terminal = None;
        let outcome = f
            .store
            .install_cached(&f.package, &f.archive, &cancel, |p| {
                terminal = Some(p.stage);
                if p.stage == when {
                    cancel.cancel();
                }
            });
        assert_eq!(outcome.err(), Some(RuntimeError::Cancelled));
        assert_eq!(terminal, Some(InstallStage::Cancelled));
        assert_eq!(
            fs::read_dir(f.temp.path().join("runtimes/packages"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            fs::read_dir(f.temp.path().join("runtimes/staging"))
                .unwrap()
                .count(),
            0
        );
    }
}
#[test]
fn checksum_failure_and_wrong_architecture_preserve_prior_version() {
    let f = Fixture::new();
    f.install();
    let before = fs::read(f.store.executable(&f.package, "fixture").unwrap().path()).unwrap();
    let mut bad = f.package.clone();
    bad.version = "2".into();
    bad.sha256 = "0".repeat(64);
    assert_eq!(
        f.store
            .install_cached(&bad, &f.archive, &Cancellation::default(), |_| {})
            .err(),
        Some(RuntimeError::IntegrityMismatch)
    );
    let path = f.temp.path().join("wrong.tar.gz");
    tar_files(&path, vec![("bin/fixture.exe", vec![0; 128])]);
    bad.sha256 = hash_file(&path).unwrap();
    assert_eq!(
        f.store
            .install_cached(&bad, &path, &Cancellation::default(), |_| {})
            .err(),
        Some(RuntimeError::UnsupportedTarget)
    );
    assert_eq!(
        fs::read(f.store.executable(&f.package, "fixture").unwrap().path()).unwrap(),
        before
    );
}
#[test]
fn rejects_foreign_target_and_same_identity_with_changed_digest() {
    let f = Fixture::new();
    f.install();
    let mut bad = f.package.clone();
    bad.sha256 = "0".repeat(64);
    assert_eq!(
        f.store
            .install_cached(&bad, &f.archive, &Cancellation::default(), |_| {})
            .err(),
        Some(RuntimeError::PackageConflict)
    );
    bad = f.package.clone();
    bad.target = if bad.target == Target::WindowsX64 {
        Target::MacosArm64
    } else {
        Target::WindowsX64
    };
    assert_eq!(
        f.store
            .install_cached(&bad, &f.archive, &Cancellation::default(), |_| {})
            .err(),
        Some(RuntimeError::UnsupportedTarget)
    );
}
#[test]
fn rejects_traversal_windows_devices_duplicate_names_and_escaping_links() {
    for (name, link) in [
        ("../escape", None),
        ("/absolute", None),
        ("C:/escape", None),
        ("CON", None),
        ("bin/link", Some("../../outside")),
    ] {
        let f = Fixture::new();
        malicious_tar(&f.archive, name, link);
        let mut package = f.package.clone();
        package.sha256 = hash_file(&f.archive).unwrap();
        assert_eq!(
            f.store
                .install_cached(&package, &f.archive, &Cancellation::default(), |_| {})
                .err(),
            Some(RuntimeError::UnsafeArchive),
            "{name}"
        );
        assert!(!f.temp.path().join("escape").exists());
    }
    let f = Fixture::new();
    tar_files(&f.archive, vec![("same", vec![1]), ("SAME", vec![2])]);
    let mut p = f.package.clone();
    p.sha256 = hash_file(&f.archive).unwrap();
    assert_eq!(
        f.store
            .install_cached(&p, &f.archive, &Cancellation::default(), |_| {})
            .err(),
        Some(RuntimeError::UnsafeArchive)
    );
}
#[test]
fn enforces_download_extraction_and_entry_limits() {
    let f = Fixture::new();
    for limits in [
        Limits {
            download_bytes: 1,
            ..Limits::default()
        },
        Limits {
            unpacked_bytes: 1,
            ..Limits::default()
        },
    ] {
        let root = f.temp.path().join(format!(
            "limit-{}-{}",
            limits.download_bytes, limits.unpacked_bytes
        ));
        let store = PackageStore::open(root, limits).unwrap();
        assert_eq!(
            store
                .install_cached(&f.package, &f.archive, &Cancellation::default(), |_| {})
                .err(),
            Some(RuntimeError::LimitExceeded)
        );
    }
    let store = PackageStore::open(
        f.temp.path().join("entries"),
        Limits {
            entries: 1,
            ..Limits::default()
        },
    )
    .unwrap();
    tar_files(&f.archive, vec![("one", vec![1]), ("two", vec![2])]);
    let mut p = f.package.clone();
    p.sha256 = hash_file(&f.archive).unwrap();
    assert_eq!(
        store
            .install_cached(&p, &f.archive, &Cancellation::default(), |_| {})
            .err(),
        Some(RuntimeError::LimitExceeded)
    );
}
#[test]
fn recovery_cleans_only_owned_staging_and_remove_preserves_project_data() {
    let f = Fixture::new();
    let abandoned = f.temp.path().join("runtimes/staging/install-abandoned");
    fs::create_dir(&abandoned).unwrap();
    fs::write(abandoned.join("partial"), b"partial").unwrap();
    let keep = f.temp.path().join("runtimes/staging/unrelated");
    fs::create_dir(&keep).unwrap();
    let project = f.temp.path().join("project-database");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("data"), b"keep").unwrap();
    f.install();
    assert!(!abandoned.exists());
    assert!(keep.exists());
    f.store.remove(&f.package).unwrap();
    assert_eq!(fs::read(project.join("data")).unwrap(), b"keep");
}
#[test]
fn installed_executable_tampering_is_detected() {
    let f = Fixture::new();
    f.install();
    let binary = f.store.executable(&f.package, "fixture").unwrap();
    fs::write(binary.path(), b"changed").unwrap();
    assert_eq!(
        f.store.executable(&f.package, "fixture").err(),
        Some(RuntimeError::ExecutableChanged)
    );
}
#[test]
fn zip_and_nested_xz_install_native_executable() {
    let f = Fixture::new();
    let bytes = fs::read(env!("CARGO_BIN_EXE_devforge-process-fixture")).unwrap();
    zip_files(&f.archive, vec![("bin/fixture.exe", bytes)]);
    let mut p = f.package.clone();
    p.archive = Archive::Zip;
    p.sha256 = hash_file(&f.archive).unwrap();
    f.store
        .install_cached(&p, &f.archive, &Cancellation::default(), |_| {})
        .unwrap();
    let f = Fixture::new();
    let mut gzip = flate2::read::GzDecoder::new(fs::File::open(&f.archive).unwrap());
    let mut tar = vec![];
    gzip.read_to_end(&mut tar).unwrap();
    let mut writer =
        lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1)).unwrap();
    std::io::Write::write_all(&mut writer, &tar).unwrap();
    let xz = writer.finish().unwrap();
    zip_files(&f.archive, vec![("runtime.txz", xz)]);
    let mut p = f.package.clone();
    p.archive = Archive::ZipTarXz {
        member: "runtime.txz".into(),
    };
    p.sha256 = hash_file(&f.archive).unwrap();
    f.store
        .install_cached(&p, &f.archive, &Cancellation::default(), |_| {})
        .unwrap();
}
#[test]
fn invalid_catalog_and_unowned_root_are_rejected() {
    let f = Fixture::new();
    let mut p = f.package.clone();
    p.version = "../outside".into();
    assert_eq!(p.key().err(), Some(RuntimeError::InvalidPackage));
    let path = f.temp.path().join("user-folder");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("user-data"), b"keep").unwrap();
    assert_eq!(
        PackageStore::open(path, Limits::default()).err(),
        Some(RuntimeError::PackageConflict)
    );
}

#[test]
fn verified_non_native_resources_are_leased_and_old_receipts_still_open() {
    let f = Fixture::new();
    let mut p = f.package.clone();
    std::fs::write(&f.archive, b"verified non-native composer fixture").unwrap();
    p.sha256 = hash_file(&f.archive).unwrap();
    p.kind = "composer".into();
    p.executables.clear();
    p.resources
        .insert("composer".into(), "composer.phar".into());
    p.archive = Archive::File {
        path: "composer.phar".into(),
    };
    f.store
        .install_cached(&p, &f.archive, &Cancellation::default(), |_| {})
        .unwrap();
    let resource = f.store.resource(&p, "composer").unwrap();
    assert_eq!(f.store.remove(&p).err(), Some(RuntimeError::Busy));
    assert_eq!(
        fs::read(resource.path()).unwrap(),
        b"verified non-native composer fixture"
    );
    drop(resource);
    f.store.remove(&p).unwrap();
    let f = Fixture::new();
    let installed = f
        .store
        .install_cached(&f.package, &f.archive, &Cancellation::default(), |_| {})
        .unwrap();
    let path = installed.directory.join(".devforge-receipt.json");
    let mut receipt: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    receipt.as_object_mut().unwrap().remove("resource_hashes");
    receipt["package"]
        .as_object_mut()
        .unwrap()
        .remove("resources");
    fs::write(path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    assert!(f.store.executable(&f.package, "fixture").is_ok());
}

#[test]
fn download_rejects_plain_http_and_failure_never_activates() {
    let f = Fixture::new();
    let mut p = f.package.clone();
    p.url = "http://example.invalid/package".into();
    assert_eq!(
        f.store.download(&p, &Cancellation::default(), |_| {}).err(),
        Some(RuntimeError::InvalidPackage)
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    p.url = format!("https://127.0.0.1:{port}/fixture");
    assert_eq!(
        f.store.download(&p, &Cancellation::default(), |_| {}).err(),
        Some(RuntimeError::DownloadFailed)
    );
    assert_eq!(
        fs::read_dir(f.temp.path().join("runtimes/packages"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        fs::read_dir(f.temp.path().join("runtimes/staging"))
            .unwrap()
            .count(),
        0
    );
}
