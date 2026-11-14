//! `hgs task team-clear REF --json` for one task of a stopped or archived
//! team, written against the envelope of the plan: `{schema, team, node}`
//! in, `{schema, team, node, state, error}` out. A reply that does not echo
//! its request, an unknown state, a malformed reply and a failed child are
//! no answer, tried again on a later pass.
use super::*;
use serde_json::Value;

const CODES: Codes = Codes {
    unobserved: "team_clear_unobserved",
    unreadable: "team_clear_unreadable",
    signalled: "team_clear_signalled",
    timed_out: "team_clear_timed_out",
    retained: "team_clear_retained",
};

pub(super) fn run(n: &Native<'_>, task: &TaskRef, request: &ClearRequest) -> Answer {
    let Ok(input) = serde_json::to_vec(request) else {
        return Answer::None("The request could not be encoded".into());
    };
    let id = uuid::Uuid::new_v4().to_string();
    let args = [
        "task",
        "team-clear",
        task.uuid.as_str(),
        "--project",
        task.project.as_str(),
        "--json",
    ];
    match n.original(
        &id,
        &args,
        &input,
        CODES,
        Output::Stdout {
            bytes: 64 << 10,
            too_large: "team_timer_output_large",
        },
        Duration::from_secs(30),
        || Ok(()),
    ) {
        Ended::Exited { code: 0, output } => answer(request, &output),
        Ended::Exited { .. } => Answer::None("The team-clear child failed".into()),
        Ended::NotStarted => Answer::None("The team-clear child did not start".into()),
        Ended::Unknown(why) => Answer::None(why.into()),
    }
}
pub(super) fn answer(request: &ClearRequest, output: &[u8]) -> Answer {
    let Ok(reply) = serde_json::from_slice::<Value>(output) else {
        return Answer::None("The reply is malformed".into());
    };
    if reply["schema"] != request.schema
        || reply["team"] != request.team.as_str()
        || reply["node"] != request.node
    {
        return Answer::None("The reply does not echo its request".into());
    }
    match reply["state"].as_str() {
        Some("cleared") => Answer::Cleared,
        // The command's own sentence, cleaned where it is taken, as the
        // actor's clear refusal is (zerus-63rru.59).
        Some("refused") => Answer::Refused(crate::teams::clean::sentence(
            reply["error"].as_str().unwrap_or(""),
            512,
        )),
        Some("cutting_over") => Answer::CuttingOver,
        _ => Answer::None("The reply's state is unknown".into()),
    }
}
