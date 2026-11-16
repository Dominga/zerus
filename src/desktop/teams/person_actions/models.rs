//! Explicit native-original model for producer-backed unit fixtures. This is
//! not a window gesture, and it is not reachable in a production executable.
use super::*;
use crate::{config::Config, desktop::ui_authority::Validated, teams::store};
use std::sync::{atomic::AtomicBool, Arc};
use zerus_client_core::desktop::{creation_clone::Scope, team_person_actions as wire};

pub(crate) fn intake(
    config: &Config,
    draft: wire::TaskDraft,
) -> Result<serde_json::Value, zerus_team_plan::Problem> {
    let scope = Scope {
        configuration: "a".repeat(64),
        service_epoch: uuid::Uuid::new_v4().to_string(),
    };
    let value = intake::review(config, &scope, draft)?;
    let plan: wire::Plan = serde_json::from_value(value["plan"].clone()).map_err(store::storage)?;
    let review: wire::Review =
        serde_json::from_value(value["review"].clone()).map_err(store::storage)?;
    let prepared = Arc::new(prepared::Prepared::resolve(config, Arc::new(plan))?);
    let authority =
        Validated::fixture(&serde_json::json!({"op":"team_person_action","intent":review.intent}));
    intake::execute(
        config,
        scope,
        Arc::new(AtomicBool::new(false)),
        authority,
        prepared,
    )
}
