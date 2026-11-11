use std::{env, fs, path::PathBuf};

fn main() {
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
