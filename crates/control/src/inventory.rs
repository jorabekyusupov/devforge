//! Embedded planning inventory, never advertised as implemented live tools.
use crate::{ErrorCode, Result, Scope};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Health,
    Projects,
    Runtimes,
    Services,
    Environment,
    Domains,
    Database,
    Logs,
    Inspector,
    Debugger,
    Mail,
    Execution,
    Http,
    Workspaces,
    Operations,
    Artifacts,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreStatus {
    Existing,
    New,
    Unsupported,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    ProjectGrant,
    OperationOwnerAndOriginalGrant,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub name: String,
    pub core_status: CoreStatus,
    pub required_scopes: Vec<Scope>,
    pub effects: Vec<String>,
    pub policy: Policy,
    pub review_required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub family: Family,
    pub existing_core: Vec<String>,
    pub new_adapter: Vec<String>,
    pub new_operations: Vec<String>,
    pub unsupported_operations: Vec<String>,
    pub scopes: Vec<Scope>,
    pub effects: Vec<String>,
    pub stages: Vec<String>,
    pub available: bool,
    pub operations: Vec<Operation>,
}

pub fn entries() -> Result<Vec<Entry>> {
    let entries: Vec<Entry> = serde_json::from_str(include_str!("tool-inventory.json"))
        .map_err(|_| ErrorCode::InvalidInput)?;
    validate(&entries)?;
    Ok(entries)
}
pub fn validate(entries: &[Entry]) -> Result<()> {
    if entries.len() != 16 {
        return Err(ErrorCode::InvalidInput);
    }
    for (i, entry) in entries.iter().enumerate() {
        if entries[..i]
            .iter()
            .any(|prior| prior.family == entry.family)
            || entry.scopes.is_empty()
            || entry.new_adapter.is_empty()
            || entry.effects.is_empty()
            || entry.stages.is_empty()
            || entry.available
            || entry.operations.is_empty()
            || entry.operations.iter().any(|op| {
                op.name.is_empty() || op.required_scopes.is_empty() || op.effects.is_empty()
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        let mut scopes = Vec::new();
        let mut effects = Vec::new();
        for (index, op) in entry.operations.iter().enumerate() {
            if entry.operations[..index]
                .iter()
                .any(|previous| previous.name == op.name)
            {
                return Err(ErrorCode::InvalidInput);
            }
            let (names, status) = if entry.new_adapter.contains(&op.name) {
                (&entry.new_adapter, CoreStatus::Existing)
            } else if entry.new_operations.contains(&op.name) {
                (&entry.new_operations, CoreStatus::New)
            } else {
                (&entry.unsupported_operations, CoreStatus::Unsupported)
            };
            if !names.contains(&op.name) || status != op.core_status {
                return Err(ErrorCode::InvalidInput);
            }
            for scope in &op.required_scopes {
                if !scopes.contains(scope) {
                    scopes.push(*scope);
                }
            }
            for effect in &op.effects {
                if !effects.contains(effect) {
                    effects.push(effect.clone());
                }
            }
        }
        if scopes.len() != entry.scopes.len()
            || effects.len() != entry.effects.len()
            || !scopes.iter().all(|scope| entry.scopes.contains(scope))
            || !effects.iter().all(|effect| entry.effects.contains(effect))
            || entry.operations.len()
                != entry.new_adapter.len()
                    + entry.new_operations.len()
                    + entry.unsupported_operations.len()
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    Ok(())
}
