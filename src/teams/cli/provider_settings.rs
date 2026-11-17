use super::{actor, input, request_input, storage};
use crate::{
    config::Config,
    teams::{providers, store::Store},
};
use serde_json::{json, Value};
use zerus_team_plan::{problem::code, Problem};

pub(super) fn read(
    config: &Config,
    caller: &actor::Caller,
    args: &[String],
    provided: Option<&str>,
) -> Result<Value, Problem> {
    if !args.is_empty() {
        return Err(input::usage(
            "validation-config takes one JSON object on stdin",
        ));
    }
    let store = Store::open(&config.state)?;
    let identity = actor::resolve(&store, caller)?;
    let mut fields = input::object(&request_input(provided)?)?;
    let id = fields
        .remove("team")
        .and_then(|v| v.as_str().map(str::to_owned))
        .ok_or_else(|| input::invalid("Name the team."))?;
    let team = actor::target_team(&store, &identity, Some(&id))?;
    actor::lead_reader(&store, &identity, &team)?;
    let candidate = fields
        .remove("override_config")
        .map(|v| {
            if v.is_null() {
                Ok(None)
            } else {
                serde_json::from_value(v)
                    .map(Some)
                    .map_err(|_| input::invalid("Invalid provider override."))
            }
        })
        .transpose()?;
    if !fields.is_empty() {
        return Err(Problem::new(
            code::UNKNOWN_FIELD,
            "Unknown provider read field.",
        ));
    }
    let db = store
        .db
        .as_ref()
        .ok_or_else(|| input::invalid("Team store is unavailable."))?;
    let tx = db.unchecked_transaction().map_err(storage)?;
    let row = crate::teams::store::rows::team(&tx, &team)?
        .ok_or_else(|| input::invalid("Team is unavailable."))?;
    let inspection = providers::inspect(config, &tx, &row, candidate)?;
    tx.commit().map_err(storage)?;
    Ok(json!({"ok":true,"actor":actor::view(&identity.actor),"inspection":inspection}))
}
