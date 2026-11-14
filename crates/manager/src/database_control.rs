//! Private DB review-state capture on the one Workspace actor. No state or
//! checksum is deserialized from an agent or exported in a tool response.
use devforge_control::{Digest, Request, mutations::MutationInput};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
};
type Check = Box<dyn Fn(&devforge_workspace::Workspace) -> devforge_workspace::Result<()> + Send>;
pub(crate) struct DatabaseReadResult {
    pub state: devforge_workspace::database_control::DatabaseActionState,
    pub snapshot_digest: Digest,
}
pub(crate) struct DatabaseRead {
    pub request: Request<MutationInput>,
    pub command: devforge_workspace::Command,
    pub check: Check,
    pub cancelled: Arc<AtomicBool>,
    pub reply: SyncSender<devforge_workspace::Result<DatabaseReadResult>>,
}
impl DatabaseRead {
    pub(crate) fn execute(self, workspace: &devforge_workspace::Workspace) {
        let run = || {
            let check = || {
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(devforge_workspace::Error {
                        scope: "workspace",
                        code: "cancelled".into(),
                        project_id: None,
                    });
                }
                (self.check)(workspace)
            };
            check()?;
            let state = workspace.database_action_state(&self.command, &check)?;
            let snapshot_digest = crate::service_control::database_snapshot_digest(
                &workspace.snapshot()?,
                &self.request,
            )
            .map_err(crate::service_control::ws_error)?;
            check()?;
            Ok(DatabaseReadResult {
                state,
                snapshot_digest,
            })
        };
        let _ = self.reply.send(run());
    }
}
