//! Only writes the checked artifacts inside this crate, never caller-supplied paths.
use devforge_control::export;
use std::{fs, path::Path};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 1 || !matches!(args[0].as_str(), "--check" | "--write") {
        return Err("usage: export_contracts --check|--write".into());
    }
    for (name, expected) in [
        ("control-contracts.json", export::json_document()?),
        ("control-contracts.ts", export::typescript()),
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("contracts")
            .join(name);
        if args[0] == "--write" {
            fs::write(path, expected)?;
        } else if fs::read_to_string(&path)? != expected {
            return Err(format!("contract drift: {name}; run export_contracts --write").into());
        }
    }
    Ok(())
}
