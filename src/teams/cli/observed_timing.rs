//! A bounded display read. Authority remains the ordinary person/lead reader.
use super::{
    actor::{self, Caller},
    input::{self, Options},
    storage,
};
use crate::{config::Config, teams::store::Store};
use serde_json::{json, Value};
use zerus_team_plan::{observed_timing::DEFAULT_WINDOW_MS, Problem};

pub(super) fn read(config: &Config, caller: &Caller, args: &[String]) -> Result<Value, Problem> {
    let options = Options::parse(
        args,
        &[
            "--team",
            "--phase",
            "--node",
            "--window-ms",
            "--limit",
            "--after",
        ],
        &[],
    )?;
    options.nothing_else("observed")?;
    let parse = |key: &str| {
        options
            .get(key)
            .map(|s| {
                s.parse::<u64>().map_err(|_| {
                    input::invalid("The observation scope must use nonnegative integers.")
                })
            })
            .transpose()
    };
    let phase = u32::try_from(
        parse("--phase")?.ok_or_else(|| input::invalid("Select an observed phase."))?,
    )
    .map_err(|_| input::invalid("Invalid phase."))?;
    let node = parse("--node")?
        .map(u32::try_from)
        .transpose()
        .map_err(|_| input::invalid("Invalid node."))?;
    let window = parse("--window-ms")?.unwrap_or(DEFAULT_WINDOW_MS);
    let limit = parse("--limit")?
        .map(u32::try_from)
        .transpose()
        .map_err(|_| input::invalid("Invalid page size."))?
        .unwrap_or(200);
    let after = options.get("--after");
    let store = Store::open(&config.state)?;
    let identity = actor::resolve(&store, caller)?;
    let team = actor::target_team(&store, &identity, options.team()?)?;
    actor::lead_reader(&store, &identity, &team)?;
    let observed = store.observed_timing(&team, phase, node, window, limit, after)?;
    let reply =
        json!({"ok":true,"actor":actor::view(&identity.actor),"team":team,"observed":observed});
    if serde_json::to_vec(&reply).map_err(storage)?.len()
        > zerus_team_plan::observed_timing::MAX_PAGE_BYTES
    {
        return Err(input::invalid(
            "The observation reply exceeds its byte bound.",
        ));
    }
    Ok(reply)
}
