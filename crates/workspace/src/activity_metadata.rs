//! Scoped metadata-only adapter. No global Snapshot, native keys, listeners,
//! creating Store::open/MailStore::open or content query/filter path.
use super::*;
pub use devforge_inspector::metadata::{ActivityKind, MetadataItem, MetadataRow, MetadataWindow};
fn mapped(cause: devforge_inspector::metadata::Error) -> Error {
    error(match cause {
        devforge_inspector::metadata::Error::InvalidInput => "activity_invalid_input",
        _ => "activity_metadata_unavailable",
    })
}
impl Workspace {
    pub fn activity_metadata(
        &self,
        id: &str,
        kind: &ActivityKind,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<MetadataWindow> {
        kind.validate().map_err(mapped)?;
        check()?;
        self.available_project(id)?;
        let window = devforge_inspector::metadata::read(&self.root, id, kind).map_err(mapped)?;
        check()?;
        Ok(window)
    }
    pub fn activity_metadata_epoch(
        &self,
        id: &str,
        kind: &ActivityKind,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<String> {
        kind.validate().map_err(mapped)?;
        check()?;
        self.available_project(id)?;
        let epoch = devforge_inspector::metadata::epoch(&self.root, id, kind).map_err(mapped)?;
        check()?;
        Ok(epoch)
    }
}
