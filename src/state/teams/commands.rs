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

pub(crate) const USAGE: &str = "usage: hgs [@host] team create|plan|approve|answer|discard|pause|resume|stop|archive|status|list|events|report|accept|reject|reassign|take-over|cancel|retry|check|use-last-reply|relaunch|add-member|pause-member|resume-member|remove-member --json (JSON stdin) | hgs [@host] team catalog get|set|propose|decide|apply|discard|route|designer --json | hgs team worker";

const MUTATIONS: &[&str] = &[
    "plan", "approve", "discard", "pause", "resume", "stop", "archive", "report", "accept", "reject", "reassign",
    "take-over", "cancel", "retry", "check", "use-last-reply", "relaunch", "add-member", "pause-member", "resume-member", "remove-member",
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

/// A team as the session list shows it: members with their sessions and each
/// task's state, without descriptions, reports or the feed.
fn summary(team: &Team) -> Value {
    let count = |status: TaskStatus| team.tasks.iter().filter(|task| task.status == status).count();
    let members: Vec<Value> = team
        .members
        .iter()
        .filter(|m| m.state != MemberState::Removed)
        .map(|m| {
            json!({"id":m.id,"role":team.role_of(&m.id),"agent":m.agent,"model":m.model,"computer":m.computer,
                "session":m.session,"state":m.state,"error":m.error})
        })
        .collect();
    let tasks: Vec<Value> = team
        .tasks
        .iter()
        .map(|t| {
            let attempt = t.attempts.last();
            json!({"id":t.id,"title":t.title,"status":t.status,"assignee":t.assignee,
                "attempt":attempt.map(|a| json!({"n":a.n,"state":a.state,"no_report":a.no_report}))})
        })
        .collect();
    json!({
        "id":team.id,"name":team.name,"status":team.status,"project":team.project,"folder":team.folder,
        "lead":team.lead,"members":members,"tasks":tasks,
        "accepted":count(TaskStatus::Accepted),"submitted":count(TaskStatus::Submitted),
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
    let start = team.brief == Notice::Pending;
    // The request's identity is what the caller sent, so a replay matches.
    let receipt = Store::open(true)?.create(request, input, team, events)?;
    if start {
        if let Err(error) = super::dispatcher::ensure_worker() {
            eprintln!("hgs: the team dispatcher did not start: {error}");
        }
    }
    Ok(receipt)
}

/// The lead (or you) answers a member's question or permission request in the
/// member's session, by its exact id and hash.
fn answer(input: &Value) -> anyhow::Result<Value> {
    let team = Store::open(false)?.load(text(input, "team"))?.context("Unknown team")?;
    let by = actor(&team, "answer", input)?;
    ensure!(matches!(&by, Actor::Human) || matches!(&by, Actor::Member(id) if *id == team.lead), "Only the lead answers a member's question");
    let member = team.member(text(input, "member")).context("Unknown member")?;
    let session = member.session.as_ref().context("The member has no session")?;
    let (question, hash) = (text(input, "question_id"), text(input, "expected_question_hash"));
    ensure!(!question.is_empty() && !hash.is_empty() && input["answers"].is_array(), "question_id, expected_question_hash and answers are required");
    let inspected = super::dispatcher::hgs(&session.host, &["inspect", &session.name, "--skip-processes"], None, std::time::Duration::from_secs(45))?;
    ensure!(inspected.status.success(), "{}", String::from_utf8_lossy(&inspected.stderr).trim());
    let record: Value = serde_json::from_slice(&inspected.stdout).context("invalid inspect output")?;
    record["pending_questions"].as_array().into_iter().flatten()
        .find(|card| card["question_id"] == question && card["question_hash"] == hash)
        .context("This question changed or is no longer pending")?;
    let request = json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":session.run_id,
        "expected_conversation_id":session.conversation_id,"question_id":question,"expected_question_hash":hash,"answers":input["answers"]});
    let output = super::dispatcher::hgs(&session.host, &["answer", &session.name, "--json"], Some(&request), std::time::Duration::from_secs(120))?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    Ok(serde_json::from_slice(&output.stdout).unwrap_or_else(|_| json!({"status":"answered"})))
}

/// The team's files refuse writes here: SQLite opened them read-only, or the
/// system denies access (in any language: Windows localizes its message).
fn read_only(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}").to_lowercase();
    ["readonly", "read-only", "access is denied", "permission denied", "os error 5)", "os error 13)", "unable to open database"]
        .iter()
        .any(|sign| text.contains(sign))
}

fn drop_report(input: &Value) -> anyhow::Result<Value> {
    ensure!(!text(input, "attempt").is_empty() && !text(input, "team").is_empty(), "A report names its team and attempt");
    ensure!(!text(input, "text").trim().is_empty(), "A report needs text");
    let path = super::dispatcher::dropped_report(&std::env::current_dir()?, text(input, "attempt"));
    std::fs::create_dir_all(path.parent().context("no report folder")?)?;
    std::fs::write(&path, serde_json::to_vec(input)?)?;
    Ok(json!({"status":"queued","request_id":input["request_id"],"task":input["task"],
        "note":"The team's files are read-only here; Zerus records this report from your folder."}))
}

fn run(op: &str, input: &Value) -> anyhow::Result<Value> {
    match op {
        "create" => create(input),
        "answer" => answer(input),
        "list" => {
            let path = store::path();
            if !path.exists() {
                return Ok(json!({"items":[]}));
            }
            let limit = input["limit"].as_i64().unwrap_or(200).clamp(1, 1000);
            let teams = Store::open(false)?.teams(input["include_archived"].as_bool() == Some(true), limit)?;
            // The desktop lists teams while it shows them: a dispatcher that
            // stopped (an update, a crash) starts again while a team runs or awaits its brief.
            if teams.iter().any(|t| t.status == TeamStatus::Running || (t.status == TeamStatus::Planning && t.brief == Notice::Pending)) {
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
            // A member in a sandbox may not write the team's files: its report
            // waits in its own folder for the dispatcher, which checks it.
            // SQLite may open the store read-only without a word, so the
            // refusal can come with the write as well as with the open.
            let mut store = match Store::open(true) {
                Ok(store) => store,
                Err(_) if op == "report" => return drop_report(input),
                Err(error) => return Err(error),
            };
            let mut filled = input.clone();
            if op == "use-last-reply" && text(input, "attempt").is_empty() {
                // Without an attempt, the task's open one.
                let team = store.load(team_id)?.context("Unknown team")?;
                let task = team.task(input["task"].as_u64().unwrap_or_default() as u32).context("Unknown task")?;
                filled["attempt"] = json!(task.open_attempt().context("The task has no open attempt")?.id);
            }
            if op == "use-last-reply" && text(input, "text").trim().is_empty() {
                // The member's last reply becomes the report; reading it is not
                // part of the request's identity.
                let team = store.load(team_id)?.context("Unknown team")?;
                let task = team.task(input["task"].as_u64().unwrap_or_default() as u32).context("Unknown task")?;
                let attempt = task.attempts.iter().find(|a| a.id == text(&filled, "attempt")).context("Unknown attempt")?;
                let session = team.member(&attempt.member).and_then(|m| m.session.clone()).context("The member has no session")?;
                filled["text"] = json!(super::dispatcher::last_reply(&session)?);
            }
            let receipt = match store.mutate(Some((request, input)), team_id, revision, |team, events| {
                let actor = actor(team, op, &filled)?;
                ops::apply(team, op, &filled, &actor, &BTreeSet::new(), events)
            }) {
                Ok(receipt) => receipt,
                Err(error) if op == "report" && read_only(&error) => return drop_report(input),
                Err(error) => return Err(error),
            };
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
    let catalog = args.first().map(String::as_str) == Some("catalog");
    let args = if catalog { &args[1..] } else { args };
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
    let result = if catalog {
        // The designer proposes from its session; only you decide and apply.
        let human = caller_record().is_none() && nonempty_env("HGS_SESSION").is_none() && text(&input, "by") == "human";
        super::catalog::run(&args[0], &input, human)
    } else {
        run(&args[0], &input)
    }
    .map_err(|e| e.to_string())?;
    println!("{result}");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_write_is_recognised_in_any_language() {
        for message in [
            "attempt to write a readonly database",
            "Отказано в доступе. (os error 5)",
            "Permission denied (os error 13)",
            "unable to open database file: teams.sqlite3",
        ] {
            assert!(read_only(&anyhow::anyhow!(message.to_owned())), "{message}");
        }
        assert!(!read_only(&anyhow::anyhow!("This attempt is no longer current")));
    }
}
