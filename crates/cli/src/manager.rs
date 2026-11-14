//! Local private owner entrypoint. Public agent tools are not exposed by this router.
use devforge_manager::{
    Client, Endpoint, LegacyExclusion,
    bootstrap::BootstrapWriter,
    controller::{Controller, Options, StartIntent},
};
use devforge_platform::app_data_dir;
use std::path::Path;

fn ensure_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("manager_invalid_data_root".into());
    }
    match std::fs::symlink_metadata(root) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err("manager_invalid_data_root".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder
                .create(root)
                .map_err(|_| "manager_data_root_unavailable".into())
        }
        Err(_) => Err("manager_data_root_unavailable".into()),
    }
}

pub fn run() -> Result<(), String> {
    let args = std::env::args().skip(2).collect::<Vec<_>>();
    #[cfg(feature = "e2e")]
    if args == ["__fixture-root-init", crate::stores::FIXTURE_FLAG] {
        let root = std::env::var_os("DEVFORGE_DATA_DIR").ok_or("fixture_data_root_required")?;
        devforge_workspace::testing::FixtureRoot::create_new(std::path::Path::new(&root))
            .map_err(|_| "fixture_root_invalid")?;
        println!("{}", serde_json::json!({"initialized":true}));
        return Ok(());
    }
    if args == ["--help"] || args == ["-h"] || args.is_empty() {
        println!(
            "{}",
            serde_json::json!({"commands":["manager serve [--automatic]","manager health"],"mode":"private_legacy_standalone","public_agent_tools":false})
        );
        return Ok(());
    }
    if args == ["health"] {
        let root = app_data_dir().map_err(|_| "manager_data_root_unavailable")?;
        let endpoint = Endpoint::existing(&root).map_err(|error| error.to_string())?;
        let response = Client::new(endpoint)
            .request(&serde_json::json!({"method":"health"}))
            .map_err(|error| error.to_string())?;
        println!("{}", response);
        return Ok(());
    }
    if args.first().map(String::as_str) != Some("serve") {
        return Err("manager_invalid_arguments".into());
    }
    let mut automatic = false;
    let mut private_bootstrap = false;
    let mut fixture_store = false;
    for flag in &args[1..] {
        match flag.as_str() {
            "--automatic" if !automatic => automatic = true,
            "--private-desktop-bootstrap" if !private_bootstrap => private_bootstrap = true,
            crate::stores::FIXTURE_FLAG if !fixture_store => fixture_store = true,
            _ => return Err("manager_invalid_arguments".into()),
        }
    }
    // Validate the private transport before touching a data directory. This mode
    // never writes a credential to a terminal, regular file or null stdout.
    let bootstrap = if private_bootstrap {
        Some(BootstrapWriter::capture_inherited_stdout().map_err(|error| error.to_string())?)
    } else {
        None
    };
    let selection = crate::stores::Selection::select(fixture_store)?;
    let root = selection.root()?;
    if !selection.is_fixture() {
        ensure_root(&root)?;
    }
    let stores = selection.manager_stores();
    let controller = Controller::with_options(
        &root,
        if automatic {
            StartIntent::AutoStart
        } else {
            StartIntent::ExplicitUserStart
        },
        stores,
        Options {
            terminal_arguments: if selection.is_fixture() {
                vec![crate::stores::FIXTURE_FLAG.into()]
            } else {
                Vec::new()
            },
            ..Options::default()
        },
    )
    .map_err(|error| error.to_string())?;
    let quitter = controller.quitter();
    ctrlc::set_handler(move || {
        if let Err(error) = quitter.request_explicit() {
            eprintln!("{}", serde_json::json!({"error":error.to_string()}));
        }
    })
    .map_err(|_| "manager_signal_unavailable")?;
    controller
        .serve(bootstrap)
        .map_err(|error| error.to_string())
}

/// Transitional exclusion for legacy foreground/mutating CLI owners. They must
/// not create an independent Workspace/supervisor beside the new manager. Read-
/// only metadata paths remain available; M13.2 will forward the guarded commands.
pub fn legacy_guard(top: &str, sub: Option<&str>) -> Result<Option<LegacyExclusion>, String> {
    if top == "doctor"
        || top == "inspect"
        || (top == "runtime" && sub == Some("catalog"))
        || (top == "project" && matches!(sub, Some("list" | "inspect")))
    {
        return Ok(None);
    }
    let root = app_data_dir().map_err(|_| "manager_data_root_unavailable")?;
    ensure_root(&root)?;
    LegacyExclusion::acquire(&root).map(Some).map_err(|error| {
        if matches!(error, devforge_manager::Error::Busy) {
            "manager_active_use_control_api".into()
        } else {
            error.to_string()
        }
    })
}

/// Only this exact interactive route can coexist with the manager before M13.2.
/// It enters a current-schema read-only snapshot path, never the legacy router.
pub fn terminal_snapshot_id(args: &[String]) -> Option<&str> {
    match args {
        [top, sub, id] if top == "project" && sub == "shell" && !id.is_empty() => Some(id),
        #[cfg(feature = "e2e")]
        [top, sub, id, flag]
            if top == "project"
                && sub == "shell"
                && !id.is_empty()
                && flag == crate::stores::FIXTURE_FLAG =>
        {
            Some(id)
        }
        _ => None,
    }
}

#[cfg(test)]
mod terminal_route_tests {
    #[test]
    fn only_exact_interactive_shell_shape_bypasses_legacy_owner() {
        let route = |args: &[&str]| {
            super::terminal_snapshot_id(&args.iter().map(|v| v.to_string()).collect::<Vec<_>>())
                .is_some()
        };
        assert!(route(&["project", "shell", "abc"]));
        #[cfg(feature = "e2e")]
        {
            assert!(route(&[
                "project",
                "shell",
                "abc",
                crate::stores::FIXTURE_FLAG
            ]));
            assert!(!route(&[
                "project",
                "shell",
                "abc",
                crate::stores::FIXTURE_FLAG,
                "extra"
            ]));
        }
        for args in [
            vec!["project", "exec", "abc"],
            vec!["project", "terminal", "abc"],
            vec!["project", "shell", "abc", "extra"],
            vec!["project", "shell"],
            vec!["project", "shell", ""],
            vec!["projects", "shell", "abc"],
        ] {
            assert!(!route(&args));
        }
    }
}
