//! The writes of `hgs team`. Every one builds the operation envelope the store
//! defines and lets the store decide authority; the CLI only refuses a command
//! whose actor kind is wrong before any request is read.
use super::actor::{self, Caller};
use super::input;
use super::{request_input, storage};
use crate::config::Config;
use crate::teams::ops::{self, Actor, Reply, Request};
use crate::teams::store::Store;
use serde_json::{json, Map, Value};
use zerus_team_plan::problem::code;
use zerus_team_plan::Problem;

/// The operation a command names. The command line, not the request, chooses
/// the operation; `create` is its own function because it reads the catalog.
pub(super) fn variant(command: &str) -> Option<&'static str> {
    Some(match command {
        "settings" => "change_settings",
        "pause" => "pause",
        "resume" => "resume",
        "stop" => "stop",
        "archive" => "archive",
        "decide" => "decide_card",
        "admit" => "admit",
        "route" => "route",
        "accept" => "accept",
        "return" => "return",
        "reassign" => "reassign",
        "hold" => "hold",
        "release" => "release",
        "cancel" => "cancel",
        "reply" => "reply",
        "ask" => "ask",
        "verdict" => "verdict",
        "pause-member" => "pause_member",
        "resume-member" => "resume_member",
        "stop-member" => "stop_member",
        "archive-member" => "archive_member",
        _ => return None,
    })
}

/// One operation command, or `not_found` for a word no package implements.
pub(super) fn run(
    config: &Config,
    caller: &Caller,
    command: &str,
    args: &[String],
    provided: Option<&str>,
) -> Result<Value, Problem> {
    let variant = variant(command).ok_or_else(|| super::unknown(command))?;
    if !args.is_empty() {
        return Err(input::usage(format!(
            "{command} takes only its JSON request on stdin"
        )));
    }
    let mut store = Store::open(&config.state)?;
    let identity = actor::resolve(&store, caller)?;
    if matches!(command, "ask" | "verdict") {
        actor::member_session(&identity, command)?;
    }
    let request = build(variant, &request_input(provided)?)?;
    let reply = ops::apply(&mut store, &identity.actor, &request, input::now_ms())?;
    reply_view(&identity.actor, &request.team, &reply)
}

/// `hgs team create --template ID`: the template comes from the catalog, the
/// rest of the team from the request. It records the team and launches nothing.
pub(super) fn create(
    config: &Config,
    caller: &Caller,
    args: &[String],
    provided: Option<&str>,
) -> Result<Value, Problem> {
    super::create::run(config, caller, args, provided)
}

/// The request is the operation's own fields plus the envelope; it never names
/// its actor and never chooses the operation.
fn build(variant: &str, text: &str) -> Result<Request, Problem> {
    let mut fields = input::object(text)?;
    refuse_operation_word(&fields)?;
    fields.insert("op".into(), json!(variant));
    serde_json::from_value(Value::Object(fields))
        .map_err(|error| input::serde_problem(error.to_string()))
}

pub(super) fn refuse_operation_word(fields: &Map<String, Value>) -> Result<(), Problem> {
    if !fields.contains_key("op") {
        return Ok(());
    }
    Err(Problem::new(
        code::UNKNOWN_FIELD,
        "The request has an unknown field: op; the command names the operation.",
    )
    .with_refs(["op"]))
}

/// One reply in the shape every `hgs team` write uses: the store's own reply,
/// the team it was about, and the actor it was recorded for.
pub(super) fn reply_view(actor: &Actor, team: &str, reply: &Reply) -> Result<Value, Problem> {
    let mut value = serde_json::to_value(reply).map_err(storage)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("actor".into(), actor::view(actor));
        object.insert("team".into(), json!(team));
    }
    Ok(value)
}
