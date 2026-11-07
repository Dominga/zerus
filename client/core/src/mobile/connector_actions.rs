//! Explicit connector lifecycle calls; the existing connector owns supervision.
use super::{
    enrollment::Target, read_status, workspace_request, workspace_stop, Failure, Workspace,
    WORKSPACE_UNCONFIRMED,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Start,
    Stop,
}
pub const CHANGED: &str = "The connector status changed. Refresh status before continuing.";
pub fn admit(workspace: &Workspace, kind: Kind) -> Result<Target, String> {
    let target = Target::capture(workspace)?;
    let available = workspace.encrypted
        && match kind {
            Kind::Start => {
                workspace.state == "active"
                    && workspace.pending.is_none()
                    && !workspace.running
                    && workspace.connector_state == "stopped"
            }
            Kind::Stop => {
                matches!(workspace.state.as_str(), "active" | "retired")
                    && workspace.running
                    && workspace.connector_state == "running"
                    && workspace.stop_supported
                    && workspace.generation.is_some()
            }
        };
    if available {
        Ok(target)
    } else {
        Err(CHANGED.into())
    }
}
pub struct Answer {
    pub workspace: Workspace,
    pub notice: &'static str,
}
pub fn apply(
    connector: &Path,
    config: &Path,
    target: &Target,
    kind: Kind,
) -> Result<Answer, Failure> {
    let current = target.fresh(connector, config)?;
    if admit(&current, kind)? != *target {
        return Err(CHANGED.into());
    }
    let reply = match kind {
        Kind::Start => workspace_request(
            connector,
            config,
            "start",
            &json!({"expected_revision":target.config_revision}),
        )?,
        Kind::Stop => workspace_stop(
            connector,
            config,
            &target.config_revision,
            target.generation.as_deref().ok_or(CHANGED)?,
        )?,
    };
    let valid = super::lifecycle(&reply["state"], &reply["running"], &reply["generation"])
        && match kind {
            Kind::Start => reply["state"] == "running",
            Kind::Stop => {
                reply["state"] == "stopped"
                    || (reply["state"] == "stopping"
                        && reply["generation"].as_str() == target.generation.as_deref())
            }
        };
    if !valid {
        return Err(WORKSPACE_UNCONFIRMED.into());
    }
    let workspace = read_status(connector, config).map_err(|_| WORKSPACE_UNCONFIRMED)?;
    let observed = Target::capture(&workspace).map_err(|_| WORKSPACE_UNCONFIRMED)?;
    let mut expected = target.clone();
    expected.running = workspace.running;
    expected.generation = workspace.generation.clone();
    if observed != expected
        || (kind == Kind::Start && workspace.pending.is_some())
        || match kind {
            Kind::Start => {
                workspace.connector_state != "running"
                    || !workspace.running
                    || reply["generation"].as_str() != workspace.generation.as_deref()
            }
            Kind::Stop => {
                !matches!(workspace.connector_state.as_str(), "stopped" | "stopping")
                    || (workspace.running && workspace.generation != target.generation)
            }
        }
    {
        return Err(WORKSPACE_UNCONFIRMED.into());
    }
    let notice = match kind {
        Kind::Start => "",
        Kind::Stop if workspace.running => "The connector is draining its current request. Refresh status to confirm it has stopped.",
        Kind::Stop => "Connector stopped.",
    };
    Ok(Answer { workspace, notice })
}
