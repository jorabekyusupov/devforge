use devforge_core::SecretStore;
use devforge_platform::{
    APP_IDENTIFIER, NativeSecretStore, app_data_dir, distribution::OperationLease,
};
use devforge_workspace::{Error, Workspace, worktrees::Input};
use std::{path::Path, sync::Arc};
fn error(code: &str) -> Error {
    Error {
        scope: "workspace",
        code: code.into(),
        project_id: None,
    }
}
pub fn run() -> Result<(), Error> {
    let args = std::env::args().skip(2).collect::<Vec<_>>();
    let arg = |n| {
        args.get(n)
            .map(String::as_str)
            .ok_or_else(|| error("worktree_usage"))
    };
    let action = arg(0)?;
    let root = app_data_dir().map_err(|_| error("storage_unavailable"))?;
    let _lease = if ["create", "cleanup", "restore", "purge", "remote-copy"].contains(&action) {
        OperationLease::exclusive(&root)
    } else {
        OperationLease::acquire(&root)
    }
    .map_err(|_| error("worktree_busy"))?;
    let secrets: Arc<dyn SecretStore + Send + Sync> = {
        #[cfg(feature = "test-support")]
        {
            if std::env::var_os("DEVFORGE_TEST_SECRETS").is_some() {
                Arc::new(devforge_workspace::testing::FixtureSecrets::open(
                    root.join("e2e-secrets.json"),
                ))
            } else {
                Arc::new(NativeSecretStore::new(format!("{APP_IDENTIFIER}.projects")))
            }
        }
        #[cfg(not(feature = "test-support"))]
        {
            Arc::new(NativeSecretStore::new(format!("{APP_IDENTIFIER}.projects")))
        }
    };
    let mut w = Workspace::open(root, secrets)?;
    let cancel = devforge_runtime::Cancellation::default();
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.cancel()).map_err(|_| error("worktree_busy"))?;
    let input = |path: &str| -> Result<Input, Error> {
        let bytes = devforge_projects::manifest::read_file(Path::new(path), 65536)?;
        serde_json::from_slice(&bytes).map_err(|_| error("worktree_usage"))
    };
    let value = match action {
        "list" if args.len() == 1 => serde_json::json!({"schema":1,"workspaces":w.worktrees()?}),
        "plan" if args.len() == 2 => {
            serde_json::to_value(w.worktree_plan(&input(arg(1)?)?)?).unwrap()
        }
        "create" if args.len() == 5 && arg(2)? == "--review" && arg(4)? == "--trust" => {
            serde_json::to_value(w.worktree_create(input(arg(1)?)?, arg(3)?, true, &cancel)?)
                .unwrap()
        }
        "restore-plan" | "purge-plan" if args.len() == 2 => serde_json::to_value(
            w.worktree_archive_review(arg(1)?, action.strip_suffix("-plan").unwrap(), &cancel)?,
        )
        .unwrap(),
        "restore" | "purge"
            if args.len() == 5 && arg(2)? == "--review" && arg(4)? == "--confirm" =>
        {
            serde_json::to_value(w.worktree_archive(arg(1)?, action, arg(3)?, true, &cancel)?)
                .unwrap()
        }
        "remote-plan" if args.len() == 3 => {
            let source = serde_json::from_slice(&devforge_projects::manifest::read_file(
                Path::new(arg(2)?),
                65536,
            )?)
            .map_err(|_| error("worktree_usage"))?;
            w.worktree_remote_plan(arg(1)?, &source)?
        }
        "remote-copy" if args.len() == 6 && arg(3)? == "--review" && arg(5)? == "--confirm" => {
            let source = serde_json::from_slice(&devforge_projects::manifest::read_file(
                Path::new(arg(2)?),
                65536,
            )?)
            .map_err(|_| error("worktree_usage"))?;
            w.worktree_remote_copy(arg(1)?, &source, arg(4)?, true, &cancel)?
        }
        "cleanup-plan" if args.len() == 2 => {
            serde_json::to_value(w.worktree_cleanup_plan(arg(1)?)?).unwrap()
        }
        "cleanup" if args.len() == 5 && arg(2)? == "--review" && arg(4)? == "--confirm" => {
            serde_json::to_value(w.worktree_cleanup(arg(1)?, arg(3)?, true, &cancel)?).unwrap()
        }
        _ => return Err(error("worktree_usage")),
    };
    w.shutdown()?;
    println!("{}", serde_json::json!({"schema":1,"result":value}));
    Ok(())
}
