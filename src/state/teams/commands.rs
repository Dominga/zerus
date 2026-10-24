//! `hgs [@host] team <operation> --json`. Every operation reads
//! one JSON object on stdin; changes carry a canonical `request_id` and replay
//! their first receipt.
use super::super::session_graph::caller_record;
use super::super::*;
use super::model::*;
use super::ops::{self, Actor};
use super::plan;
use super::store::{self, canonical_uuid, Store, PAGE};
use anyhow::{bail, ensure, Context};
use std::collections::BTreeSet;

pub(crate) const USAGE: &str = "usage: hgs [@host] team create|plan|approve|discard|pause|resume|stop|archive|status|list|events|report|accept|reject|reassign|take-over|cancel|retry|check|use-last-reply|relaunch --json (JSON stdin) | hgs team worker";

const MUTATIONS: &[&str] = &[
    "plan", "approve", "discard", "pause", "resume", "stop", "archive", "report", "accept", "reject", "reassign",
    "take-over", "cancel", "retry", "check", "use-last-reply", "relaunch",
];

fn text<'a>(input: &'a Value, key: &str) -> &'a str {
    input[key].as_str().unwrap_or_default()
}

/// The member whose session runs this CLI, proven by process ancestry.
fn calling_member(team: &Team, record: &Value) -> Option<String> {
    let name = record["name"].as_str()?;
    let run = record["run_id"].as_str().unwrap_or_default();
    team.members
        .iter()
        .find(|member| {
            member.session.as_ref().is_some_and(|session| {
                session.host.is_empty() && session.name == name && (session.run_id.is_empty() || session.run_id == run)
            })
        })
        .map(|member| member.id.clone())
}

/// Who asks. A team session is proven by ancestry; a report from elsewhere (a
/// member on another computer) is authorized by its attempt id; anything else
/// must say it is you.
fn actor(team: &Team, op: &str, input: &Value) -> anyhow::Result<Actor> {
    if let Some(record) = caller_record() {
        let member = calling_member(team, &record).with_context(|| format!("This session is not a member of team {}", team.name))?;
        return Ok(Actor::Member(member));
    }
    ensure!(nonempty_env("HGS_SESSION").is_none(), "Zerus could not verify the calling session");
    if matches!(op, "report") && text(input, "by") != "human" {
        let attempt = text(input, "attempt");
        let member = team.tasks.iter().flat_map(|task| &task.attempts).find(|a| !attempt.is_empty() && a.id == attempt);
        return Ok(Actor::Member(member.map(|a| a.member.clone()).unwrap_or_default()));
    }
    ensure!(text(input, "by") == "human", "Run this from the lead's session, or add \"by\":\"human\" to act as yourself");
    Ok(Actor::Human)
}

/// The team with what Zerus derives from it: plan findings, layers and the
/// tasks ready to hand out.
pub(crate) fn view(team: &Team) -> Value {
    let mut value = serde_json::to_value(team).unwrap_or_default();
    value["validation"] = json!(plan::validate(team, &BTreeSet::new()));
    value["layers"] = json!(plan::layers(&team.tasks));
    value["cycle"] = json!(plan::cycle_tasks(&team.tasks));
    value["ready"] = json!(plan::ready(team));
    value
}

fn summary(team: &Team) -> Value {
    let count = |status: TaskStatus| team.tasks.iter().filter(|task| task.status == status).count();
    json!({
        "id":team.id,"name":team.name,"status":team.status,"project":team.project,"lead":team.role_of(&team.lead),
        "members":team.members.iter().filter(|m| m.state != MemberState::Removed).count(),
        "tasks":team.tasks.len(),"accepted":count(TaskStatus::Accepted),"submitted":count(TaskStatus::Submitted),
        "revision":team.revision,"updated":team.updated,
    })
}

fn create(input: &Value) -> anyhow::Result<Value> {
    let request = text(input, "request_id");
    ensure!(canonical_uuid(request), "A canonical request_id is required");
    let mut filled = input.clone();
    if let Some(record) = caller_record() {
        // A lead creates its own team: its session is the lead's.
        let lead = &mut filled["lead"];
        ensure!(lead.is_object(), "Describe the lead");
        let session = json!({"name":record["name"],"run_id":record["run_id"],"conversation_id":record["conversation_id"]});
        ensure!(lead["session"].is_null() || lead["session"]["name"] == record["name"], "A session creates only a team it leads");
        lead["session"] = session;
        if lead["agent"].is_null() {
            lead["agent"] = json!(record["agent"]);
        }
    } else {
        ensure!(nonempty_env("HGS_SESSION").is_none(), "Zerus could not verify the calling session");
        ensure!(text(input, "by") == "human", "Run this from the lead's session, or add \"by\":\"human\" to act as yourself");
    }
    let (team, events) = ops::create(&uuid::Uuid::new_v4().to_string(), &filled)?;
    // The request's identity is what the caller sent, so a replay matches.
    Store::open(true)?.create(request, input, team, events)
}

fn run(op: &str, input: &Value) -> anyhow::Result<Value> {
    match op {
        "create" => create(input),
        "list" => {
            let path = store::path();
            if !path.exists() {
                return Ok(json!({"items":[]}));
            }
            let limit = input["limit"].as_i64().unwrap_or(200).clamp(1, 1000);
            let teams = Store::open(false)?.teams(input["include_archived"].as_bool() == Some(true), limit)?;
            // The desktop lists teams while it shows them: a dispatcher that
            // stopped (an update, a crash) starts again while a team runs.
            if teams.iter().any(|t| t.status == TeamStatus::Running) {
                if let Err(error) = super::dispatcher::ensure_worker() {
                    eprintln!("hgs: the team dispatcher did not start: {error}");
                }
            }
            Ok(json!({"items":teams.iter().map(summary).collect::<Vec<_>>()}))
        }
        "status" => {
            let team = Store::open(false)?.load(text(input, "team"))?.context("Unknown team")?;
            Ok(view(&team))
        }
        "events" => {
            let before = match &input["before"] {
                Value::Null => None,
                value => Some(value.as_i64().filter(|n| *n > 0).context("Invalid feed cursor")?),
            };
            Store::open(false)?.events(text(input, "team"), before, input["limit"].as_i64().unwrap_or(PAGE))
        }
        op if MUTATIONS.contains(&op) => {
            let request = text(input, "request_id");
            ensure!(canonical_uuid(request), "A canonical request_id is required");
            let team_id = text(input, "team");
            let revision = input["revision"].as_u64();
            let mut store = Store::open(true)?;
            let receipt = store.mutate(Some((request, input)), team_id, revision, |team, events| {
                let actor = actor(team, op, input)?;
                ops::apply(team, op, input, &actor, &BTreeSet::new(), events)
            })?;
            if matches!(op, "approve" | "resume") {
                // The dispatcher runs while any team runs; it exits by itself.
                if let Err(error) = super::dispatcher::ensure_worker() {
                    eprintln!("hgs: the team dispatcher did not start: {error}");
                }
            }
            if receipt["status"] == "stale" {
                bail!("This attempt is no longer current for task #{}; the report was not accepted", receipt["task"]);
            }
            Ok(receipt)
        }
        _ => bail!("Unknown team operation {op}"),
    }
}

pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() == 1 && args[0] == "worker" {
        return super::dispatcher::worker();
    }
    if args.len() != 2 || args[1] != "--json" {
        return Err(USAGE.into());
    }
    let mut bytes = Vec::new();
    io::stdin().take(256 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 256 * 1024 {
        return Err("Team request exceeds 256 KiB".into());
    }
    let input: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if !input.is_object() {
        return Err("A JSON object is required".into());
    }
    let result = run(&args[0], &input).map_err(|e| e.to_string())?;
    println!("{result}");
    Ok(0)
}
