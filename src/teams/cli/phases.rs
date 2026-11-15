//! Phase reads are metadata and bounded drafts, never a serialized whole team.
use super::actor::{self, Caller};
use super::input::{self, Options};
use super::{not_found_team, storage};
use crate::{config::Config, teams::store::Store};
use serde_json::{json, Value};
use zerus_team_plan::Problem;

pub(super) fn read(
    config: &Config,
    caller: &Caller,
    command: &str,
    args: &[String],
) -> Result<Value, Problem> {
    let options = Options::parse(args, &["--team", "--limit", "--after", "--phase"], &[])?;
    options.nothing_else(command)?;
    let store = Store::open(&config.state)?;
    let who = actor::resolve(&store, caller)?;
    let id = actor::target_team(&store, &who, options.team()?)?;
    actor::lead_reader(&store, &who, &id)?;
    if command == "phase" {
        if options.get("--after").is_some() || options.get("--limit").is_some() {
            return Err(input::invalid(
                "A phase draft is bounded by its task cap; use --phase only.",
            ));
        }
        let phase = options
            .get("--phase")
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| input::invalid("--phase needs a phase number."))?;
        return Ok(
            json!({"ok":true,"actor":actor::view(&who.actor),"team":id,"draft":store.phase_draft(&id,phase)?}),
        );
    }
    if options.get("--phase").is_some() {
        return Err(input::invalid("Use phase --phase N to read one phase."));
    }
    let (limit, after) = options.page()?;
    let cursor = after.map(input::parse_cursor).transpose()?;
    let db = store.db.as_ref().ok_or_else(|| not_found_team(&id))?;
    let tx = db.unchecked_transaction().map_err(storage)?;
    let team = store.team(&id)?.ok_or_else(|| not_found_team(&id))?;
    let page = store.phases(&id, limit, cursor.as_ref())?;
    if team.bundle.pipelines.len() > 200 {
        return Err(Problem::new(
            zerus_team_plan::problem::code::OVER_CAP,
            "The draft routing choices exceed their bound.",
        ));
    }
    let routes: Vec<_> = team
        .bundle
        .pipelines
        .iter()
        .map(|p| json!({"pipeline":p.id,"role":p.implementer}))
        .collect();
    let next = page.next.as_ref().map(input::cursor_token).transpose()?;
    let answer = json!({"ok":true,"actor":actor::view(&who.actor),"team":id,"revision":team.revision,
        "rows":page.rows,"next":next,"next_phase":store.next_draft_phase(&id)?,"next_node":store.next_draft_node(&id)?,
        "drafts_available":true,"approval_available":false,"routes":routes,"default_pipeline":team.bundle.template.default_pipeline});
    tx.commit().map_err(storage)?;
    Ok(answer)
}
