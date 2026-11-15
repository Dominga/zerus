//! One on-demand provider inspection. Configuration is data, never executable.
use super::read::TeamError;
use crate::actions::{Exit, Invocation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
pub use zerus_team_plan::validation::{Config, Inspection, Selection};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigRead {
    pub team: String,
    /// Omitted for the saved selection; null clears the override for Check.
    pub candidate: Option<Candidate>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub override_config: Option<Config>,
}
fn invalid(s: impl Into<String>) -> TeamError {
    TeamError::Invalid { message: s.into() }
}
pub fn invocation(read: &ConfigRead) -> Result<Invocation, TeamError> {
    if !super::read::valid_team(&read.team) {
        return Err(invalid("Not a team identity."));
    }
    if let Some(config) = read
        .candidate
        .as_ref()
        .and_then(|c| c.override_config.as_ref())
    {
        config.check().map_err(invalid)?;
    }
    let mut input = json!({"team":read.team});
    if let Some(candidate) = &read.candidate {
        input["override_config"] = json!(candidate.override_config);
    }
    let bytes = serde_json::to_vec(&input).map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > 65_536 {
        return Err(invalid("Provider override exceeds its bound."));
    }
    Ok(Invocation {
        args: vec!["team".into(), "validation-config".into(), "--json".into()],
        stdin: Some(bytes),
        timeout: Duration::from_secs(15),
        request_id: None,
    })
}

pub fn decode(read: &ConfigRead, exit: Exit) -> Result<Inspection, TeamError> {
    invocation(read)?;
    let stdout = super::read::output(exit)?;
    let value: Value =
        serde_json::from_slice(&stdout).map_err(|_| invalid("Invalid provider reply."))?;
    if value["ok"] != true {
        if let Some((code, message)) = super::refusal::parse(&stdout) {
            return Err(TeamError::Refused { code, message });
        }
        return Err(invalid("Provider reply has no success flag."));
    }
    if !value["actor"].is_object() {
        return Err(invalid("Provider reply has no actor."));
    }
    let inspection: Inspection = serde_json::from_value(value["inspection"].clone())
        .map_err(|_| invalid("Unsupported provider inspection. Update hgs and Zerus."))?;
    if inspection.schema != 1
        || inspection.team != read.team
        || inspection.providers.len() > 16
        || inspection.diagnostics.len() > 16
        || inspection.unsupported.len() > 16
    {
        return Err(invalid(
            "Provider inspection has a wrong scope or exceeds its bounds.",
        ));
    }
    if inspection.valid {
        inspection.selection().check().map_err(invalid)?;
        Config {
            schema: 1,
            provider: inspection.providers.clone(),
        }
        .check()
        .map_err(invalid)?;
    }
    if let Some(candidate) = &read.candidate {
        if candidate.override_config != inspection.override_config {
            return Err(invalid("Provider check returned another override."));
        }
    }
    Ok(inspection)
}

pub fn read(
    hgs: &Path,
    asked: &ConfigRead,
    resources: Arc<dyn Send + Sync>,
) -> Result<Inspection, TeamError> {
    let exit = crate::run::run_accounted(
        hgs,
        &invocation(asked)?,
        &[],
        256 * 1024,
        &AtomicBool::new(false),
        resources,
    )
    .map_err(|s| match s {
        crate::run::Stopped::TooLarge(t) => TeamError::TooLarge { limit: t.cap },
        crate::run::Stopped::Cancelled => TeamError::Cancelled,
    })?;
    decode(asked, exit)
}

#[cfg(test)]
mod tests;
