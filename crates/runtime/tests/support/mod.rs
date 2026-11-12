#![allow(dead_code)]
use devforge_runtime::{
    Cancellation,
    package::{Archive, Limits, Package, PackageStore, Target, hash_file},
    process::{LogPolicy, ProcessSpec, Readiness, RestartPolicy},
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
pub struct Fixture {
    pub temp: tempfile::TempDir,
    pub store: PackageStore,
    pub package: Package,
    pub archive: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let store = PackageStore::open(temp.path().join("runtimes"), Limits::default()).unwrap();
        let archive = temp.path().join("fixture.tar.gz");
        tar_files(
            &archive,
            vec![(
                "bin/fixture.exe",
                std::fs::read(env!("CARGO_BIN_EXE_devforge-process-fixture")).unwrap(),
            )],
        );
        let package = Package {
            kind: "fixture".into(),
            version: "1.0.0".into(),
            variant: "native".into(),
            target: Target::current().unwrap(),
            url: "https://example.invalid/fixture.tar.gz".into(),
            sha256: hash_file(&archive).unwrap(),
            archive: Archive::TarGz,
            executables: BTreeMap::from([("fixture".into(), "bin/fixture.exe".into())]),
            resources: BTreeMap::new(),
        };
        Self {
            temp,
            store,
            package,
            archive,
        }
    }
    pub fn install(&self) {
        self.store
            .install_cached(
                &self.package,
                &self.archive,
                &Cancellation::default(),
                |_| {},
            )
            .unwrap();
    }
    pub fn spec(&self, id: &str, mode: &str) -> ProcessSpec {
        ProcessSpec {
            input: None,
            id: id.into(),
            executable: self.store.executable(&self.package, "fixture").unwrap(),
            args: vec![mode.into()],
            directory: self.temp.path().into(),
            environment: BTreeMap::new(),
            readiness: Readiness::Output {
                stream: "stdout",
                needle: "FIXTURE_READY".into(),
            },
            readiness_timeout: Duration::from_secs(3),
            stop_grace: Duration::from_millis(100),
            restart: RestartPolicy::default(),
            logs: LogPolicy::default(),
        }
    }
}
pub fn tar_files(path: &Path, files: Vec<(&str, Vec<u8>)>) {
    let encoder =
        flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(encoder);
    for (name, data) in files {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        tar.append_data(&mut h, name, data.as_slice()).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
}
pub fn malicious_tar(path: &Path, name: &str, link: Option<&str>) {
    let encoder =
        flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(encoder);
    let mut h = tar::Header::new_gnu();
    if let Some(target) = link {
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_link_name(target).unwrap();
    } else {
        h.set_size(1);
        h.set_entry_type(tar::EntryType::Regular);
    }
    // Write raw path bytes so the test can exercise paths Builder normally rejects.
    h.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
    h.set_cksum();
    if link.is_some() {
        tar.append(&h, &[][..]).unwrap();
    } else {
        tar.append(&h, &b"x"[..]).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap();
}
pub fn zip_files(path: &Path, files: Vec<(&str, Vec<u8>)>) {
    let mut writer = zip::ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in files {
        writer
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap();
}
