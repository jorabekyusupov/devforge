//! Fixture selection is an explicit debug CLI composition decision. No marker,
//! test-support environment variable or data-root override selects it implicitly.
use devforge_core::SecretStore;
use devforge_manager::controller::Stores;
use devforge_platform::{APP_IDENTIFIER, NativeSecretStore};
use std::{path::PathBuf, sync::Arc};
pub const FIXTURE_FLAG: &str = "--internal-fixture-store";
pub fn validate_fixture_arguments(args: &[String]) -> Result<(), String> {
    if !args.iter().any(|arg| arg.starts_with(FIXTURE_FLAG)) {
        return Ok(());
    }
    #[cfg(not(feature = "e2e"))]
    return Err("fixture_feature_disabled".into());
    #[cfg(feature = "e2e")]
    {
        if args
            .iter()
            .any(|arg| arg.starts_with(FIXTURE_FLAG) && arg != FIXTURE_FLAG)
        {
            return Err("fixture_invalid_arguments".into());
        }
        let manager = args.first().is_some_and(|arg| arg == "manager")
            && args.get(1).is_some_and(|arg| arg == "serve");
        let shell = matches!(args,[top,sub,id,flag]if top=="project"&&sub=="shell"&&!id.is_empty()&&flag==FIXTURE_FLAG);
        let initialize = matches!(args,[top,sub,flag]if top=="manager"&&sub=="__fixture-root-init"&&flag==FIXTURE_FLAG);
        if !manager && !shell && !initialize {
            return Err("fixture_invalid_arguments".into());
        }
        Ok(())
    }
}
pub enum Selection {
    Native,
    #[cfg(feature = "e2e")]
    Fixture(devforge_workspace::testing::FixtureRoot),
}
impl Selection {
    pub fn select(explicit: bool) -> Result<Self, String> {
        if !explicit {
            #[cfg(feature = "e2e")]
            if std::env::var_os("DEVFORGE_DATA_DIR").is_some_and(|root| {
                std::fs::symlink_metadata(
                    std::path::Path::new(&root).join(devforge_workspace::testing::FIXTURE_MARKER),
                )
                .is_ok()
            }) {
                return Err("fixture_explicit_flag_required".into());
            }
            return Ok(Self::Native);
        }
        #[cfg(not(feature = "e2e"))]
        return Err("fixture_feature_disabled".into());
        #[cfg(feature = "e2e")]
        {
            let root = std::env::var_os("DEVFORGE_DATA_DIR").ok_or("fixture_data_root_required")?;
            let root =
                devforge_workspace::testing::FixtureRoot::validate(std::path::Path::new(&root))
                    .map_err(|_| "fixture_root_invalid")?;
            Ok(Self::Fixture(root))
        }
    }
    pub fn root(&self) -> Result<PathBuf, String> {
        match self {
            Self::Native => devforge_platform::app_data_dir()
                .map_err(|_| "manager_data_root_unavailable".into()),
            #[cfg(feature = "e2e")]
            Self::Fixture(root) => Ok(root.root().to_owned()),
        }
    }
    pub fn project(&self) -> Arc<dyn SecretStore + Send + Sync> {
        match self {
            Self::Native => Arc::new(NativeSecretStore::new(format!("{APP_IDENTIFIER}.projects"))),
            #[cfg(feature = "e2e")]
            Self::Fixture(root) => Arc::new(devforge_workspace::testing::FixtureSecrets::open(
                root.project_store_path(),
            )),
        }
    }
    pub fn manager_stores(&self) -> Stores {
        Stores {
            workspace: self.project(),
            desktop: match self {
                Self::Native => {
                    Arc::new(NativeSecretStore::new(format!("{APP_IDENTIFIER}.control")))
                }
                #[cfg(feature = "e2e")]
                Self::Fixture(root) => Arc::new(devforge_workspace::testing::FixtureSecrets::open(
                    root.control_store_path(),
                )),
            },
        }
    }
    pub fn is_fixture(&self) -> bool {
        !matches!(self, Self::Native)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_activation_is_never_inferred_for_ordinary_composition() {
        assert!(matches!(
            Selection::select(false).unwrap(),
            Selection::Native
        ));
        assert!(validate_fixture_arguments(&["manager".into(), "serve".into()]).is_ok());
        #[cfg(not(feature = "e2e"))]
        assert!(
            validate_fixture_arguments(&["manager".into(), "serve".into(), FIXTURE_FLAG.into()])
                .is_err()
        );
    }
    #[cfg(feature = "e2e")]
    #[test]
    fn explicit_fixture_shell_and_manager_factories_share_only_fixed_guarded_files() {
        let parent = tempfile::tempdir().unwrap();
        let root =
            devforge_workspace::testing::FixtureRoot::create_new(&parent.path().join("fixture"))
                .unwrap();
        let project = root.project_store_path();
        let control = root.control_store_path();
        let selection = Selection::Fixture(root);
        let shell_store = selection.project();
        shell_store.set("shell-fixture", "synthetic-only").unwrap();
        let manager = selection.manager_stores();
        assert!(
            manager
                .workspace
                .get("shell-fixture")
                .unwrap()
                .is_some_and(|value| value == "synthetic-only")
        );
        manager
            .desktop
            .set("control-fixture", "synthetic-control")
            .unwrap();
        assert!(project.is_file());
        assert!(control.is_file());
        assert!(project != control);
        assert!(matches!(
            Selection::select(false).unwrap(),
            Selection::Native
        ));
    }
}
