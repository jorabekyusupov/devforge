//! Fresh OS elevation is required per invocation. No listener/daemon/shell IPC.
fn main() {
    if let Err(error) = run() {
        eprintln!("{}", serde_json::json!({"error":error}));
        std::process::exit(1);
    }
}
fn run() -> devforge_serving::Result<()> {
    use devforge_serving::{ServingError as E, system::Plan};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 || args[0] != "apply" {
        return Err(E::InvalidConfiguration);
    }
    let bytes = read_plan(std::path::Path::new(&args[1]))?;
    let plan: Plan = serde_json::from_slice(&bytes).map_err(|_| E::InvalidConfiguration)?;
    let effect = plan.apply(&args[2])?;
    println!("{}", serde_json::to_string(&effect).unwrap());
    Ok(())
}

fn read_plan(path: &std::path::Path) -> devforge_serving::Result<Vec<u8>> {
    use devforge_serving::ServingError as E;
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::{
            Security::SECURITY_IDENTIFICATION, Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
        };
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .security_qos_flags(SECURITY_IDENTIFICATION);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 128 * 1024 {
        return Err(E::InvalidConfiguration);
    }
    let mut bytes = Vec::new();
    file.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 {
        return Err(E::InvalidConfiguration);
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_input_is_a_bounded_regular_file() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_plan(root.path()).is_err());
        let path = root.path().join("plan");
        std::fs::write(&path, "{}").unwrap();
        assert_eq!(read_plan(&path).unwrap(), b"{}");
        std::fs::write(&path, vec![0; 128 * 1024 + 1]).unwrap();
        assert!(read_plan(&path).is_err());
    }
}
