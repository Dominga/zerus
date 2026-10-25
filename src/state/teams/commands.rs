//! WhiteWh fork: `hgs [@host] team <operation> --json`. Every operation reads
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

pub(crate) const USAGE: &str = "usage: hgs [@host] team create|plan|extend|approve-phase|discard-phase|finish|approve|answer|discard|pause|resume|stop|archive|status|list|events|current|report|ask|reply|accept|reject|reassign|take-over|cancel|retry|check|use-last-reply|relaunch|add-member|pause-member|resume-member|remove-member --json (JSON stdin) | hgs [@host] team catalog get|set|propose|decide|apply|discard|route|designer --json | hgs team worker";

const MUTATIONS: &[&str] = &[
    "plan", "extend", "approve-phase", "discard-phase", "finish", "approve", "discard", "pause", "resume", "stop",
    "archive", "report", "ask", "reply", "accept", "reject", "reassign", "take-over", "cancel", "retry", "check",
    "use-last-reply", "relaunch", "add-member", "pause-member", "resume-member", "remove-member",
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

/// The member whose dsh session carries this conversation id. A dsh session
/// has no process environment of an hgs session; its harness exports the
/// conversation id as `DSH_SESSION_ID` instead (A3, A4).
fn dsh_member(team: &Team, conversation: &str) -> Option<String> {
    if conversation.is_empty() {
        return None;
    }
    team.members
        .iter()
        .find(|member| {
            member.session.as_ref().is_some_and(|session| session.host.is_empty() && session.conversation_id == conversation)
        })
        .map(|member| member.id.clone())
}

/// Who asks. A team session is proven by ancestry; a dsh session by the
/// conversation its harness exports; a report from elsewhere (a member on
/// another computer) is authorized by its attempt id; anything else must say
/// it is you.
fn actor(team: &Team, op: &str, input: &Value) -> anyhow::Result<Actor> {
    if let Some(record) = caller_record() {
        let member = calling_member(team, &record).with_context(|| format!("This session is not a member of team {}", team.name))?;
        return Ok(Actor::Member(member));
    }
    ensure!(nonempty_env("HGS_SESSION").is_none(), "Zerus could not verify the calling session");
    if let Some(member) = dsh_member(team, nonempty_env("DSH_SESSION_ID").unwrap_or_default().as_str()) {
        return Ok(Actor::Member(member));
    }
    if matches!(op, "report") && text(input, "by") != "human" {
        let attempt = text(input, "attempt");
        let member = team.tasks.iter().flat_map(|task| &task.attempts).find(|a| !attempt.is_empty() && a.id == attempt);
        return Ok(Actor::Member(member.map(|a| a.member.clone()).unwrap_or_default()));
    }
    ensure!(text(input, "by") == "human", "Run this from the lead's session, or add \"by\":\"human\" to act as yourself");
    Ok(Actor::Human)
}

/// A running team whose every task is decided waits for the lead to extend it
/// or finish it.
fn waiting_for_lead(team: &Team) -> bool {
    team.status == TeamStatus::Running && ops::all_decided(team)
}

/// The phase record still waiting for the owner, if any. A team that is over
/// shows none, even if an old record was left in draft.
fn pending_phase(team: &Team) -> Option<u32> {
    if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
        return None;
    }
    team.phases.iter().find(|phase| phase.state == PhaseState::Draft).map(|phase| phase.n)
}

/// Operations after which new work may be ready for the dispatcher, or a
/// question or answer waits to be delivered.
fn starts_worker(op: &str) -> bool {
    matches!(op, "approve" | "resume" | "extend" | "approve-phase" | "ask" | "reply")
}

/// The team with what Zerus derives from it: plan findings, layers and the
/// tasks ready to hand out.
pub(crate) fn view(team: &Team) -> Value {
    let mut value = serde_json::to_value(team).unwrap_or_default();
    value["validation"] = json!(plan::validate(team, &BTreeSet::new()));
    value["layers"] = json!(plan::layers(&team.tasks));
    value["cycle"] = json!(plan::cycle_tasks(&team.tasks));
    value["ready"] = json!(plan::ready(team));
    value["waiting_for_lead"] = json!(waiting_for_lead(team));
    value["pending_phase"] = json!(pending_phase(team));
    // Each task's last attempt carries its derived phase (spec section 2). The
    // stored record knows no live session, so `stopped` stays to the dispatcher.
    if let Some(tasks) = value["tasks"].as_array_mut() {
        for (task, value) in team.tasks.iter().zip(tasks) {
            if let Some(phase) = ops::attempt_phase(task, false) {
                if let Some(attempt) = value["attempts"].as_array_mut().and_then(|attempts| attempts.last_mut()) {
                    attempt["phase"] = json!(phase);
                }
            }
        }
    }
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
                "attempt":attempt.map(|a| json!({"n":a.n,"state":a.state,"no_report":a.no_report,
                    "phase":ops::attempt_phase(t, false)}))})
        })
        .collect();
    json!({
        "id":team.id,"name":team.name,"status":team.status,"project":team.project,"folder":team.folder,
        "lead":team.lead,"members":members,"tasks":tasks,
        "accepted":count(TaskStatus::Accepted),"submitted":count(TaskStatus::Submitted),
        "revision":team.revision,"updated":team.updated,
        "waiting_for_lead":waiting_for_lead(team),"pending_phase":pending_phase(team),
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

/// What `hgs team current` answers for one member of a team (spec section 5):
/// the live attempt, its phase, its open questions and the report command.
/// Read only.
fn current_view(team: &Team, member: &str) -> anyhow::Result<Value> {
    let Some((task, attempt)) = team
        .tasks
        .iter()
        .rev()
        .filter_map(|task| task.open_attempt().map(|attempt| (task, attempt)))
        .find(|(_, attempt)| attempt.member == member)
    else {
        bail!("{}", no_attempt_text(team, member));
    };
    let phase = ops::attempt_phase(task, false).unwrap_or("unconfirmed");
    let open_questions: Vec<&str> =
        attempt.questions.iter().filter(|question| question.open()).map(|question| question.id.as_str()).collect();
    Ok(json!({
        "team": team.id,
        "task": task.id,
        "title": task.title,
        "attempt": attempt.id,
        "n": attempt.n,
        "phase": phase,
        "open_questions": open_questions,
        "report": {
            "request_id": uuid::Uuid::new_v4().to_string(),
            "team": team.id,
            "task": task.id,
            "attempt": attempt.id,
            "text": "..."
        }
    }))
}

/// Why a member has no open attempt: its last task and its state, such as
/// `#7 was rejected and is held by the lead` (spec section 5).
fn no_attempt_text(team: &Team, member: &str) -> String {
    let last = team
        .tasks
        .iter()
        .filter(|task| task.assignee == member || task.attempts.iter().any(|attempt| attempt.member == member))
        .max_by_key(|task| task.id);
    let Some(task) = last else { return "You have no open attempt".into() };
    let attempt = task.attempts.iter().rev().find(|attempt| attempt.member == member);
    let state = match attempt.map(|attempt| attempt.state) {
        Some(AttemptState::Rejected) if task.hold.is_some() => "was rejected and is held by the lead",
        Some(AttemptState::Rejected) => "was rejected and waits for another hand-out",
        Some(AttemptState::Reported) => "has a report waiting for the lead's decision",
        Some(AttemptState::Revoked) => "was returned to the lead",
        Some(AttemptState::Refused) => "was not delivered; Zerus tries it again",
        Some(AttemptState::Delivering | AttemptState::Delivered | AttemptState::Unconfirmed) => "is open to another attempt",
        None => match task.status {
            TaskStatus::Accepted => "was accepted",
            TaskStatus::Cancelled => "was cancelled",
            TaskStatus::Submitted => "has a report waiting for the lead's decision",
            TaskStatus::Draft => "is a draft",
            _ => "is waiting for a hand-out",
        },
    };
    format!("#{} {state}", task.id)
}

/// The member's live attempt from its own session; `team` is optional when the
/// session belongs to one team (spec section 5). Read only.
fn current(input: &Value) -> anyhow::Result<Value> {
    let record = caller_record();
    let conversation = nonempty_env("DSH_SESSION_ID").unwrap_or_default();
    if record.is_none() {
        ensure!(!conversation.is_empty(), "hgs team current runs in a member's session");
    }
    let named = text(input, "team");
    let teams = Store::open(false)?.teams(false, 1000)?;
    let mut found: Vec<(&Team, String)> = Vec::new();
    for team in &teams {
        if !named.is_empty() && team.id != named {
            continue;
        }
        let member = match &record {
            Some(record) => calling_member(team, record),
            None => dsh_member(team, &conversation),
        };
        if let Some(member) = member {
            found.push((team, member));
        }
    }
    let (team, member) = match found.as_slice() {
        [] if named.is_empty() => bail!("This session is not a member of a team"),
        [] => bail!("This session is not a member of team {named}"),
        [only] => (only.0, only.1.clone()),
        many => bail!("This session is a member of {} teams; name one with \"team\"", many.len()),
    };
    current_view(team, &member)
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
            // stopped (an update, a crash) starts again while a team needs one.
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
        "current" => current(input),
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
            if starts_worker(op) {
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

    #[test]
    fn the_phase_commands_are_mutations_named_in_the_usage() {
        for op in ["extend", "approve-phase", "discard-phase", "finish"] {
            assert!(MUTATIONS.contains(&op), "{op} is not a mutation");
            assert!(USAGE.contains(op), "{op} is not in the usage");
        }
    }

    #[test]
    fn status_shows_waiting_for_lead_and_the_pending_phase() {
        let team = Team {
            id: "t".into(),
            name: "T".into(),
            lead: "lead".into(),
            status: TeamStatus::Running,
            tasks: vec![Task { id: 1, title: "Done".into(), status: TaskStatus::Accepted, assignee: "lead".into(), ..Default::default() }],
            phases: vec![Phase { n: 2, state: PhaseState::Draft, ..Default::default() }],
            ..Default::default()
        };
        let value = view(&team);
        assert_eq!(value["waiting_for_lead"], true);
        assert_eq!(value["pending_phase"], 2);
        // A paused team with every task decided is Paused, not waiting.
        let mut paused = team.clone();
        paused.status = TeamStatus::Paused;
        assert_eq!(view(&paused)["waiting_for_lead"], false);
        // The session list carries both too.
        let listed = summary(&paused);
        assert_eq!((listed["waiting_for_lead"].as_bool(), listed["pending_phase"].as_u64()), (Some(false), Some(2)));
        // A team that is over shows no pending phase, even with a stale record.
        let mut done = team.clone();
        done.status = TeamStatus::Done;
        assert!(view(&done)["pending_phase"].is_null());
        assert_eq!(pending_phase(&done), None);
    }

    #[test]
    fn ask_and_reply_are_mutations_and_current_is_a_read_named_in_the_usage() {
        for op in ["ask", "reply"] {
            assert!(MUTATIONS.contains(&op), "{op} is not a mutation");
            assert!(USAGE.contains(op), "{op} is not in the usage");
        }
        assert!(USAGE.contains("current"));
        assert!(!MUTATIONS.contains(&"current"));
    }

    /// A running team with an open hand-out to dev.
    fn running_team() -> Team {
        let (mut team, _) = ops::create(
            "t",
            &json!({"name":"T","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
        )
        .unwrap();
        let mut events = Vec::new();
        ops::apply(
            &mut team,
            "plan",
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}}],
                "tasks":[{"id":1,"title":"API","assignee":"dev"}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        {
            let task = team.task_mut(1).unwrap();
            task.attempts.push(Attempt {
                n: 1,
                id: "00000000-0000-4000-8000-000000000001".into(),
                member: "dev".into(),
                state: AttemptState::Delivered,
                sent_at: 10.0,
                delivered_at: Some(10.0),
                started_at: Some(11.0),
                ..Default::default()
            });
            task.status = TaskStatus::InProgress;
        }
        team
    }

    #[test]
    fn a_question_shows_on_its_attempt_in_status() {
        let mut team = running_team();
        let mut events = Vec::new();
        let asked = ops::apply(
            &mut team,
            "ask",
            &json!({"text":"Must I keep the old field?"}),
            &Actor::Member("dev".into()),
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!((asked["status"].as_str(), asked["question"].as_str()), (Some("asked"), Some("q1")));
        // The attempt stays open and status shows it waiting for an answer.
        assert!(team.task(1).unwrap().open_attempt().is_some());
        let value = view(&team);
        assert_eq!(value["tasks"][0]["attempts"][0]["phase"], "asking");
        assert_eq!(value["tasks"][0]["attempts"][0]["questions"][0]["text"], "Must I keep the old field?");
        assert_eq!(value["tasks"][0]["attempts"][0]["questions"][0]["answer"], "");
        // The lead's answer closes it; the attempt is still open and working.
        ops::apply(&mut team, "reply", &json!({"question":"q1","text":"Yes"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        assert!(team.task(1).unwrap().open_attempt().is_some());
        assert_eq!(view(&team)["tasks"][0]["attempts"][0]["phase"], "working");
        assert_eq!(view(&team)["tasks"][0]["attempts"][0]["questions"][0]["answer"], "Yes");
    }

    #[test]
    fn current_names_the_live_attempt_and_its_report_command() {
        let mut team = running_team();
        let mut events = Vec::new();
        ops::apply(&mut team, "ask", &json!({"text":"Must I keep?"}), &Actor::Member("dev".into()), &BTreeSet::new(), &mut events).unwrap();
        let value = current_view(&team, "dev").unwrap();
        assert_eq!((value["team"].as_str(), value["task"].as_u64(), value["n"].as_u64()), (Some("t"), Some(1), Some(1)));
        assert_eq!((value["title"].as_str(), value["phase"].as_str()), (Some("API"), Some("asking")));
        assert_eq!(value["attempt"], "00000000-0000-4000-8000-000000000001");
        assert_eq!(value["open_questions"], json!(["q1"]));
        // The report command the member fills in and pipes to hgs team report.
        let report = &value["report"];
        assert_eq!((report["team"].as_str(), report["task"].as_u64()), (Some("t"), Some(1)));
        assert_eq!(report["attempt"], "00000000-0000-4000-8000-000000000001");
        assert_eq!(report["text"], "...");
        assert!(canonical_uuid(report["request_id"].as_str().unwrap()));
        // Every call offers a fresh request id.
        assert_ne!(current_view(&team, "dev").unwrap()["report"]["request_id"], report["request_id"]);
    }

    #[test]
    fn current_refuses_a_member_without_an_open_attempt() {
        let mut team = running_team();
        {
            let task = team.task_mut(1).unwrap();
            task.attempts[0].state = AttemptState::Rejected;
            task.status = TaskStatus::Pending;
            task.hold = Some(Hold { by: "Lead".into(), at: 12.0, reason: "Later".into() });
        }
        assert_eq!(current_view(&team, "dev").unwrap_err().to_string(), "#1 was rejected and is held by the lead");
        // A member without a task, and the lead with none.
        assert_eq!(current_view(&team, "qa").unwrap_err().to_string(), "You have no open attempt");
        assert_eq!(current_view(&team, "lead").unwrap_err().to_string(), "You have no open attempt");
        // A reported attempt waits for the lead, not for the member.
        let mut reported = running_team();
        {
            let attempt = &mut reported.task_mut(1).unwrap().attempts[0];
            attempt.state = AttemptState::Reported;
            attempt.report = Some(Report { text: "Done".into(), at: 20.0, ..Default::default() });
        }
        reported.task_mut(1).unwrap().status = TaskStatus::Submitted;
        assert_eq!(current_view(&reported, "dev").unwrap_err().to_string(), "#1 has a report waiting for the lead's decision");
    }

    #[test]
    fn a_dsh_session_is_known_by_the_conversation_its_harness_exports() {
        let mut team = running_team();
        {
            let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.session = Some(SessionRef {
                name: "dsh/x/dev".into(),
                run_id: "r1".into(),
                conversation_id: "session-1".into(),
                ..Default::default()
            });
        }
        assert_eq!(dsh_member(&team, "session-1").as_deref(), Some("dev"));
        assert_eq!(dsh_member(&team, "session-2"), None);
        assert_eq!(dsh_member(&team, ""), None);
        // A member's session on another computer is not proven by this one.
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().host = "msi".into();
        assert_eq!(dsh_member(&team, "session-1"), None);
    }

    #[test]
    fn a_captured_report_is_replaced_through_the_report_operation() {
        let mut team = running_team();
        {
            let attempt = &mut team.task_mut(1).unwrap().attempts[0];
            attempt.state = AttemptState::Reported;
            attempt.report = Some(Report { text: "Building and running...".into(), at: 20.0, captured: true, ..Default::default() });
        }
        team.task_mut(1).unwrap().status = TaskStatus::Submitted;
        let mut events = Vec::new();
        let receipt = ops::apply(
            &mut team,
            "report",
            &json!({"task":1,"attempt":"00000000-0000-4000-8000-000000000001","text":"Done: the tests pass."}),
            &Actor::Member("dev".into()),
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!(receipt["status"], "submitted");
        let report = team.task(1).unwrap().attempts[0].report.as_ref().unwrap();
        assert!(!report.captured);
        assert_eq!(report.text, "Done: the tests pass.");
        assert_eq!(report.replaced, vec![ReplacedReport { text: "Building and running...".into(), at: 20.0 }]);
        assert_eq!(events.last().unwrap().kind, "report_replaced");
    }

    #[test]
    fn the_commands_that_create_work_start_the_worker() {
        for op in ["approve", "resume", "extend", "approve-phase", "ask", "reply"] {
            assert!(starts_worker(op), "{op}");
        }
        for op in ["report", "cancel", "finish", "discard-phase"] {
            assert!(!starts_worker(op), "{op}");
        }
    }
}
