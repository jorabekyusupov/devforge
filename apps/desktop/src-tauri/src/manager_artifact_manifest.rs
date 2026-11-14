//! Build-time parser shared with focused unit tests. No runtime environment trust.
const LIMIT: usize = 1024;
#[derive(Debug, PartialEq, Eq)]
pub struct Manifest {
    pub target: String,
    pub filename: String,
    pub sha256: String,
}
pub fn enabled(gate: Option<&str>) -> Result<bool, &'static str> {
    match gate {
        None => Ok(false),
        Some("1") => Ok(true),
        _ => Err("invalid_manager_packaging_gate"),
    }
}
pub fn parse(bytes: &[u8], target: &str) -> Result<Manifest, &'static str> {
    if bytes.len() > LIMIT {
        return Err("manager_manifest_too_large");
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "invalid_manager_manifest")?;
    let fields = value.as_object().ok_or("invalid_manager_manifest")?;
    if fields.len() != 4 || value["schema"] != 1 {
        return Err("invalid_manager_manifest");
    }
    let filename = match target {
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => "devforge",
        "x86_64-pc-windows-msvc" => "devforge.exe",
        _ => return Err("unsupported_manager_target"),
    };
    if value["target"].as_str() != Some(target) || value["filename"].as_str() != Some(filename) {
        return Err("manager_manifest_target_or_filename_mismatch");
    }
    let sha256 = value["sha256"].as_str().ok_or("invalid_manager_digest")?;
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("invalid_manager_digest");
    }
    Ok(Manifest {
        target: target.into(),
        filename: filename.into(),
        sha256: sha256.into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        serde_json::json!({"schema":1,"target":"aarch64-apple-darwin","filename":"devforge","sha256":"a".repeat(64)})
    }
    #[test]
    fn packaged_descriptor_checks_target_filename_digest_and_bounds() {
        let value = fixture();
        assert_eq!(
            parse(value.to_string().as_bytes(), "aarch64-apple-darwin")
                .unwrap()
                .filename,
            "devforge"
        );
        assert!(parse(value.to_string().as_bytes(), "x86_64-apple-darwin").is_err());
        assert!(parse(value.to_string().as_bytes(), "unknown-target").is_err());
        for name in [
            "../devforge",
            "/tmp/devforge",
            "devforge-other",
            "devforge.exe",
        ] {
            let mut value = fixture();
            value["filename"] = name.into();
            assert!(parse(value.to_string().as_bytes(), "aarch64-apple-darwin").is_err());
        }
        for hash in [
            "a".repeat(63),
            "A".repeat(64),
            "g".repeat(64),
            String::new(),
        ] {
            let mut value = fixture();
            value["sha256"] = hash.into();
            assert!(parse(value.to_string().as_bytes(), "aarch64-apple-darwin").is_err());
        }
        assert!(parse(&[b' '; 1025], "aarch64-apple-darwin").is_err());
        assert!(parse(b"{", "aarch64-apple-darwin").is_err());
        let mut extra = fixture();
        extra["path"] = "/tmp/evil".into();
        assert!(parse(extra.to_string().as_bytes(), "aarch64-apple-darwin").is_err());
    }
    #[test]
    fn ordinary_build_ignores_stale_manifest_and_explicit_gate_fails_closed() {
        assert!(!enabled(None).unwrap());
        assert!(enabled(Some("1")).unwrap());
        for gate in ["", "0", "true", "manifest.json"] {
            assert!(enabled(Some(gate)).is_err());
        }
        // This is the actual build selection order: no gate means no parsing,
        // including a stale/invalid on-disk manifest.
        let stale = b"invalid stale manifest";
        let selected = if enabled(None).unwrap() {
            Some(parse(stale, "aarch64-apple-darwin"))
        } else {
            None
        };
        assert!(selected.is_none());
        let mut windows = fixture();
        windows["target"] = "x86_64-pc-windows-msvc".into();
        windows["filename"] = "devforge.exe".into();
        assert!(parse(windows.to_string().as_bytes(), "x86_64-pc-windows-msvc").is_ok());
    }
}
