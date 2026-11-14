use std::{env, fs, io::Read, path::PathBuf};

#[path = "src/manager_artifact_manifest.rs"]
mod manager_artifact_manifest;

fn manager_artifact() {
    println!("cargo:rerun-if-env-changed=DEVFORGE_PACKAGE_MANAGER");
    let gate = env::var("DEVFORGE_PACKAGE_MANAGER").ok();
    let enabled =
        manager_artifact_manifest::enabled(gate.as_deref()).expect("manager packaging gate");
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../../distribution/generated/manager-artifact.json");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let descriptor = if enabled {
        let meta = fs::symlink_metadata(&manifest).expect("packaged manager manifest required");
        assert!(
            meta.is_file() && meta.len() <= 1024,
            "invalid manager manifest file"
        );
        let mut bytes = Vec::with_capacity(1025);
        fs::File::open(&manifest)
            .expect("manager manifest open")
            .take(1025)
            .read_to_end(&mut bytes)
            .expect("manager manifest read");
        let target = env::var("TARGET").expect("cargo target");
        let value =
            manager_artifact_manifest::parse(&bytes, &target).expect("manager manifest validation");
        let digest: Vec<u8> = value
            .sha256
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        format!(
            "Some(Descriptor {{ target: {:?}, filename: {:?}, expected_sha256: {:?} }})",
            value.target, value.filename, digest
        )
    } else {
        "None".to_string()
    };
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("manager_artifact.rs"),
        format!("const PACKAGED_MANAGER: Option<Descriptor> = {descriptor};\n"),
    )
    .expect("compiled manager descriptor");
}

fn main() {
    manager_artifact();
    let locales =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../../shared/locales");
    let registry: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(locales.join("registry.json")).unwrap()).unwrap();
    let mut catalogs = serde_json::Map::new();
    println!("cargo:rerun-if-changed={}", locales.display());
    for locale in registry["locales"].as_array().expect("locale registry") {
        let id = locale["id"].as_str().expect("locale id");
        let path = locales.join(format!("{id}.json"));
        let catalog: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).expect("locale catalog"))
                .expect("catalog JSON");
        catalogs.insert(id.into(), catalog);
    }
    let output = serde_json::json!({"registry":registry,"catalogs":catalogs});
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("locales.json"),
        output.to_string(),
    )
    .unwrap();
    tauri_build::build();
}
