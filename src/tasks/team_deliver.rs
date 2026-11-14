//! A stored team effect is the permit for one ordinary task attempt.
//! A pre-reservation refusal has no record; a repeat of its still-open effect may deliver.
use super::*;
mod cancel;
mod hosted;
pub(super) mod completion;
pub(super) mod stage;
use crate::swarm::fence::{Admission, Closed};
use crate::teams::{store::Store, tasks::context};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use zerus_replica::execution::{EventKind, Origin};
use zerus_team_plan::model::{Member, MemberState, Node, TaskRef, Team, TeamStatus};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: u32,
    id: String,
    team: String,
    node: u32,
    member: String,
}

impl Input {
    fn validate(&self) -> Result<()> {
        let id =
            |s: &str| uuid::Uuid::parse_str(s).is_ok_and(|id| !id.is_nil() && id.to_string() == s);
        if self.schema != 1
            || !id(&self.id)
            || !id(&self.team)
            || self.node == 0
            || self.member.is_empty()
            || self.member.len() > 256
            || self.member.chars().any(char::is_control)
        {
            return Err(rule("Invalid team delivery request."));
        }
        Ok(())
    }
    fn reply(&self, state: &str, error: &str, receipt: Option<Value>) -> Value {
        let mut out = json!({"schema":1,"id":self.id,"team":self.team,"node":self.node,"member":self.member,"state":state});
        if !error.is_empty() {
            out["error"] = json!(error);
        }
        if let Some(receipt) = receipt {
            out["receipt"] = receipt;
        }
        out
    }
}

const RULE: i32 = 2;
const CUTTING_OVER: i32 = 3;
const STALE: i32 = 4;
const NOT_STARTED: i32 = 5;
const HANDED_BACK: i32 = 6;
/// A rule refusal of the project's own limit: the answer carries its typed
/// cause (zerus-63rru.23).
const PROJECT_LIMIT_CODE: i32 = 7;
/// The first task-store open waited past its own bound: nothing was reserved
/// or sent (zerus-63rru.72).
const STORE_BUSY_CODE: i32 = 8;
/// The typed cause of that answer, beside its plain sentence.
pub(crate) const TASK_STORE_BUSY: &str = "task_store_busy";
/// The typed cause of a refused hand-out whose member's terminal holds input
/// (zerus-63rru.79): the reply's `cause`. A reader that does not know the
/// field reads a plain refusal, as before it.
pub(crate) const TERMINAL_WAITING: &str = "terminal_waiting";
/// The typed cause of a refused hand-out refused by the project's own limits
/// (zerus-63rru.23): the reply's `cause`. The sentence keeps its prefixes;
/// nothing parses them.
pub(crate) const PROJECT_LIMIT: &str = "project_limit";
/// The lead's sentence when a reopen's member no longer has the task's
/// conversation (zerus-63rru.64).
fn handed_back(member: &str) -> String {
    format!(
        "Member {member} no longer has this task's conversation (it was cleared or replaced since the task was handed to it), so this reopen cannot continue it; the task is held for you to hand out again or reassign."
    )
}
fn rule(message: impl std::fmt::Display) -> Error {
    Error::new(RULE, message.to_string())
}
/// A failure after this command saw, under the task admission, that no
/// attempt exists for its ID, and before its reservation: nothing was
/// reserved or sent. A rule's refusal, a stale observation or a cutover keeps
/// its own answer; any other failure (a busy store, a failed read) is a
/// non-start the caller may retry (zerus-63rru.58).
fn not_started(error: Error) -> Error {
    if matches!(
        error.code,
        RULE | STALE | CUTTING_OVER | NOT_STARTED | HANDED_BACK | PROJECT_LIMIT_CODE
    ) {
        error
    } else {
        Error::new(NOT_STARTED, error.message)
    }
}
fn cutting_over() -> Error {
    Error::new(CUTTING_OVER, BUSY)
}
fn replica(error: zerus_replica::Error) -> Error {
    if error.kind() == zerus_replica::ErrorKind::Refused {
        rule(error)
    } else {
        fail(error)
    }
}

const UNCONFIRMED_WRITE: &str = "Unconfirmed team task write:";

/// Every writer rechecks the number, current ID and conflict flag under project
/// admission and the team transaction. Only the pair is a cancellation reply fact.
fn observed_attempt(
    store: &Store,
    team: &str,
    node: u32,
    view: &zerus_replica::TaskView,
) -> Result<(u64, String)> {
    let current = crate::teams::tasks::attempt(view);
    let observation = store.task_observation(team, node).map_err(problem)?;
    let observed = observation.as_ref().map_or_else(
        || (0, String::new(), false),
        |o| (o.attempt, o.attempt_id.clone(), o.attempt_conflict),
    );
    if current.0 < observed.0 {
        return Err(rule(format!(
            "Task document attempt {} ({}) is older than the team's observed attempt {} ({}).",
            current.0, current.1, observed.0, observed.1
        )));
    }
    if current != observed {
        return Err(Error::new(STALE,format!("Task document attempt {} ({}, conflict={}) differs from the team's observed attempt {} ({}, conflict={}); observe the task before trying again.",current.0,current.1,current.2,observed.0,observed.1,observed.2)));
    }
    // A reservation can be observed before it runs. Let the next running
    // observation judge that execution before any service command drives it.
    let running = view.stage == Stage::Active && view.cancelled_at.is_none();
    if running
        && !observation
            .as_ref()
            .is_some_and(|o| o.stage == crate::teams::ops::TaskStage::InProgress)
    {
        return Err(Error::new(STALE, "The task's execution start has not been observed; observe the task before trying again."));
    }
    Ok((current.0, current.1))
}

struct Permit {
    team: Team,
    node: Node,
    member: Member,
    lead: Member,
    task: TaskRef,
    kind: String,
    reason: String,
    return_actor: String,
}

fn problem(p: zerus_team_plan::Problem) -> Error {
    fail(format!("{}: {}", p.code, p.message))
}

fn scope(store: &Store, body: &Input, here: &str) -> Result<(Team, Node, TaskRef, String)> {
    let team = store
        .team(&body.team)
        .map_err(problem)?
        .ok_or_else(|| rule("Team not found."))?;
    if team.home != here {
        return Err(rule("Team belongs to another computer."));
    }
    let db = store.db.as_ref().ok_or_else(|| fail("No team store."))?;
    let effect: Option<(u32, String, String)> = db
        .query_row(
            "SELECT node,member,kind FROM effects WHERE team=?1 AND request_id=?2",
            params![body.team, body.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(fail)?;
    let (node, member, kind) =
        effect.ok_or_else(|| rule("No recorded delivery effect for this ID."))?;
    if node != body.node || member != body.member || !matches!(kind.as_str(), "handout" | "reopen")
    {
        return Err(rule("Delivery effect binding differs."));
    }
    let node = crate::teams::store::rows::node(db, &body.team, body.node)
        .map_err(problem)?
        .ok_or_else(|| rule("Node not found."))?;
    let task = node
        .task
        .clone()
        .ok_or_else(|| rule("Node has no task document."))?;
    let exact:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM node_identity WHERE team=?1 AND id=?2 AND task_project=?3 AND task_uuid=?4)",
        params![body.team,body.node,task.project,task.uuid],|r|r.get(0)).map_err(fail)?;
    if !exact || task.project != team.project {
        return Err(rule("Node task identity differs."));
    }
    Ok((team, node, task, kind))
}

fn permit(store: &Store, body: &Input, here: &str) -> Result<Permit> {
    let (team, node, task, kind) = scope(store, body, here)?;
    if team.status != TeamStatus::Running {
        return Err(rule("Team is not Running."));
    }
    let db = store.db.as_ref().expect("scoped store");
    let (generation, state, outcome): (u32, String, Option<String>) = db
        .query_row(
            "SELECT generation,state,outcome FROM effects WHERE team=?1 AND request_id=?2",
            params![body.team, body.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(fail)?;
    if Some(generation) != store.generation(&body.team, body.node).map_err(problem)?
        || state != "reserved"
        || outcome.is_some()
    {
        return Err(rule("Effect is closed or belongs to another generation."));
    }
    let member = store
        .member(&body.team, &body.member)
        .map_err(problem)?
        .ok_or_else(|| rule("Member not found."))?;
    let lead = store
        .member(&body.team, &team.lead)
        .map_err(problem)?
        .ok_or_else(|| rule("Lead not found."))?;
    for m in [&member, &lead] {
        if m.state != MemberState::Active
            || m.session
                .as_ref()
                .is_none_or(|s| s.machine != here || s.session_id.is_empty())
        {
            return Err(rule("Member or lead has no active local session binding."));
        }
    }
    if member.id == lead.id {
        return Err(rule("Lead cannot receive a member hand-out."));
    }
    let owns:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM live_tasks WHERE task_project=?1 AND task_uuid=?2 AND team=?3 AND node=?4)",params![task.project,task.uuid,body.team,body.node],|r|r.get(0)).map_err(fail)?;
    if !owns {
        return Err(rule("Task is not this team's live node."));
    }
    let (reason, return_actor) = if kind == "reopen" {
        let (text, actor): (String,String)=db.query_row("SELECT text,actor FROM decisions WHERE team=?1 AND node=?2 AND generation=?3 AND kind IN ('return','reassign') AND state='effect_reserved' ORDER BY seq DESC LIMIT 1",params![body.team,body.node,generation],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(fail)?.ok_or_else(||rule("No matching reopen decision."))?;
        let actor: Value = serde_json::from_str(&actor).map_err(fail)?;
        (text, return_author(store, &team, &actor)?)
    } else {
        (String::new(), String::new())
    };
    Ok(Permit {
        team,
        node,
        member,
        lead,
        task,
        kind,
        reason,
        return_actor,
    })
}

fn return_author(store: &Store, team: &Team, actor: &Value) -> Result<String> {
    match actor["kind"].as_str() {
        Some("person") => Ok("The person".into()),
        Some("session") if actor["team"].as_str() == Some(team.id.as_str()) => {
            let id = actor["member"]
                .as_str()
                .ok_or_else(|| fail("Stored return has no member identity."))?;
            if id == team.lead {
                return Ok("The team lead".into());
            }
            let member = store
                .member(&team.id, id)
                .map_err(problem)?
                .ok_or_else(|| fail("Stored return's member is missing."))?;
            let role = team
                .bundle
                .roles
                .iter()
                .find(|r| r.id == member.role)
                .ok_or_else(|| fail("Stored return's role is missing."))?;
            Ok(format!("The stage holder ({})", role.name))
        }
        _ => Err(fail("Stored return decision has an unreadable author.")),
    }
}

fn session(machines: &Machines, member: &Member) -> Result<SessionKey> {
    let bound = member
        .session
        .as_ref()
        .ok_or_else(|| rule("Session is unbound."))?;
    // Labels come from saved metadata only after selection by exact identity.
    let key = SessionKey {
        machine: bound.machine.clone(),
        session_id: bound.session_id.clone(),
        session: String::new(),
    };
    machines
        .resolve(&key)
        .filter(|s| !s.session.is_empty() && !s.session_id.is_empty())
        .ok_or_else(|| rule("Bound session identity is unavailable."))
}

fn project_policy(settings: &Settings, starts: usize, who: &Actor, to: &SessionKey) -> Result<()> {
    if let Actor::Agent(own) = who {
        if !settings.agents_other && own != to {
            return Err(Error::new(
                PROJECT_LIMIT_CODE,
                "project_agents_other: This project allows agents to start tasks only in their own session.",
            ));
        }
        if settings.agent_limit > 0 && starts as i64 >= settings.agent_limit {
            return Err(Error::new(
                PROJECT_LIMIT_CODE,
                "project_agent_limit: The project's hourly agent start limit is reached.",
            ));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn request(
    settings: &Settings,
    starts: usize,
    task: &TaskDoc,
    name: &str,
    body: &Input,
    permit: &Permit,
    machines: &Machines,
    store: &Store,
) -> Result<(Value, SessionKey, Actor)> {
    let to = session(machines, &permit.member)?;
    let who = Actor::Agent(session(machines, &permit.lead)?);
    project_policy(settings, starts, &who, &to)?;
    let view = task.view().map_err(fail)?;
    let handover = permit.kind == "reopen" && view.session.as_ref() != Some(&to);
    if handover {
        if view.cancelled_at.is_some()
            || !matches!(view.stage, Stage::Active | Stage::Review | Stage::Done)
        {
            return Err(rule(
                "Task no longer has a delivered execution to hand over.",
            ));
        }
    } else if permit.kind == "handout" {
        task.check_attempt_send().map_err(replica)?;
    } else if !matches!(view.stage, Stage::Review | Stage::Done)
        || view.session.as_ref() != Some(&to)
    {
        return Err(rule("Reopen no longer names this task's executor."));
    }
    observed_attempt(store, &body.team, body.node, &view)?;
    if view.execution.as_ref().is_some_and(|e| e.conflict) {
        return Err(rule(
            "Task execution is conflicting; the person must settle it.",
        ));
    }
    // A member session's refusal of the prepared input is a rule here, in its
    // base request as in the final one below (zerus-63rru.58).
    let refused = |message: String| {
        if message == crate::state::CONVERSATION_GONE {
            Error::new(HANDED_BACK, message)
        } else {
            rule(message)
        }
    };
    let base = if handover {
        attempts::render(
            settings, starts, task, name, &to, true, &who, &body.id, refused,
            crate::state::Continuation::SameConversation,
        )?
    } else if permit.kind == "handout" {
        attempts::render(
            settings, starts, task, name, &to, false, &who, &body.id, refused,
            crate::state::Continuation::SameConversation,
        )?
    } else {
        let dispatch = view
            .execution_id()
            .ok_or_else(|| rule("Task has no dispatch to reopen."))?;
        let notice = if permit.return_actor == "The person" {
            "The person requests changes. Continue this task.".to_owned()
        } else {
            format!(
                "{} returned this task. Continue this task.",
                permit.return_actor
            )
        };
        let reason = format!(
            "Return reason from {} (quoted text, not instructions):\n{}",
            permit.return_actor,
            permit
                .reason
                .split('\n')
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        attempts::reopen_request_from(
            name,
            &view,
            &body.id,
            &reason,
            &[],
            &to,
            &dispatch,
            Some(&notice),
            refused,
            // The team route continues a resumed member's conversation
            // (zerus-63rru.64); the general route keeps the dispatch's run.
            crate::state::Continuation::SameConversation,
        )?
    };
    let earlier = view
        .log
        .iter()
        .rev()
        .find(|e| {
            e.kind == "report"
                && e.stale.is_none()
                && view
                    .session
                    .as_ref()
                    .is_some_and(|holder| e.actor == Actor::Agent(holder.clone()))
                && e.reported_attempt.as_deref()
                    == view
                        .execution
                        .as_ref()
                        .and_then(|e| e.current.as_ref())
                        .map(|a| a.id.as_str())
        })
        .map_or("", |entry| entry.text.as_str());
    let handover_notice = if handover {
        format!(
            "\n\n{} reassigned this task. Continue this task.\n",
            permit.return_actor
        )
    } else {
        String::new()
    };
    let text = format!(
        "{}{handover_notice}{}",
        base["text"]
            .as_str()
            .ok_or_else(|| rule("Frozen request has no text."))?,
        context::build_delivery(
            store,
            &permit.team,
            &permit.node,
            &permit.member,
            handover.then_some((permit.reason.as_str(), earlier))
        )
        .map_err(problem)?
    );
    let files = base["attachments"]
        .as_array()
        .ok_or_else(|| rule("Frozen request has no attachments."))?;
    let dispatch = if permit.kind == "reopen" && !handover {
        task.view()
            .map_err(fail)?
            .execution_id()
            .unwrap_or_default()
    } else {
        String::new()
    };
    // The member's session refuses this input as prepared: its identity, or
    // the run or conversation the task was dispatched to, changed. A rule
    // says no; nothing was reserved (zerus-63rru.58).
    let frozen = crate::state::prepare_task_reply(
        &to,
        &dispatch,
        &body.id,
        &text,
        files,
        crate::state::Continuation::SameConversation,
    )
    .map_err(refused)?;
    if frozen["expected_run_id"] != base["expected_run_id"]
        || frozen["expected_conversation_id"] != base["expected_conversation_id"]
    {
        return Err(rule(dispatch::CHANGED));
    }
    Ok((frozen, to, who))
}

trait Delivery {
    fn deliver(
        &self,
        config: &Config,
        task: &TaskRef,
        body: &Input,
        to: &SessionKey,
        request: &Value,
    ) -> attempts::Delivered;
    fn before_reserve(&self) {}
}
struct Native;
#[cfg(test)]
type NativeInputFixture = Box<dyn Fn(&Value, &SessionKey) -> Value>;
#[cfg(test)]
thread_local! {
    pub(crate) static FIXTURE_NATIVE_INPUT: std::cell::RefCell<Option<NativeInputFixture>> = const {std::cell::RefCell::new(None)};
}
impl Delivery for Native {
    fn deliver(
        &self,
        config: &Config,
        task: &TaskRef,
        body: &Input,
        to: &SessionKey,
        request: &Value,
    ) -> attempts::Delivered {
        #[cfg(test)]
        if let Some(receipt) =
            FIXTURE_NATIVE_INPUT.with(|slot| slot.borrow().as_ref().map(|input| input(request, to)))
        {
            return match attempts::settle(config, &task.project, &task.uuid, &body.id, &receipt) {
                Ok(()) => attempts::Delivered::Submitted(receipt),
                Err(error) => attempts::Delivered::Uncertain(error.message),
            };
        }
        attempts::deliver(config, &task.project, &task.uuid, &body.id, to, request)
    }
}

/// Whether an error's code is a refusal the command answers with `refused`
/// in its own JSON. A busy task store is never one (zerus-63rru.72).
fn refusal(error: &Error) -> bool {
    matches!(error.code, RULE | PROJECT_LIMIT_CODE)
}

pub(super) fn dispatch(config: &Config, args: &[String]) -> Result<i32> {
    match execute(config, args, &Native) {
        Ok(reply) => {
            println!("{reply}");
            Ok(0)
        }
        // Only a rule refusal prints a final refusal. A busy store's own code
        // must never reach this arm: it is a non-start, whether execute has
        // already built its reply or a later path returns the code itself.
        Err(error) if !refusal(&error) => Err(error),
        Err(error) => {
            println!(
                "{}",
                json!({"schema":1,"state":"refused","error":error.message})
            );
            Ok(1)
        }
    }
}

fn execute(config: &Config, args: &[String], delivery: &dyn Delivery) -> Result<Value> {
    if !crate::teams::enabled(&config.state) {
        return Err(rule("teams_inactive: Agent Teams preview is off."));
    }
    arguments(args, true, true)?;
    let deadline = Instant::now() + std::time::Duration::from_secs(20);
    let text = stdin_text("team delivery request")?;
    let body: Input = serde_json::from_str(&text).map_err(|error| {
        rule(format!(
            "{}: {error}",
            if error.to_string().contains("unknown field") {
                "unknown_field"
            } else {
                "invalid_request"
            }
        ))
    })?;
    body.validate()?;
    match run(config, args, &body, delivery, deadline) {
        Ok(reply) => Ok(reply),
        Err(error) if error.code == RULE => Ok(body.reply("refused", &error.message, None)),
        // A refusal of the project's own limit carries its typed cause; the
        // sentence keeps its prefixes and nothing parses them (zerus-63rru.23).
        Err(error) if error.code == PROJECT_LIMIT_CODE => {
            let mut reply = body.reply("refused", &error.message, None);
            reply["cause"] = json!(PROJECT_LIMIT);
            Ok(reply)
        }
        Err(error) if error.code == STALE => Ok(body.reply("stale", &error.message, None)),
        Err(error) if error.code == NOT_STARTED => {
            Ok(body.reply("not_started", &error.message, None))
        }
        Err(error) if error.code == STORE_BUSY_CODE => Ok(store_busy_reply(&body)),
        // The member no longer has the task's conversation: a final refusal,
        // which the service hands back to the lead (zerus-63rru.64).
        Err(error) if error.code == HANDED_BACK => {
            let mut reply = body.reply("refused", &handed_back(&body.member), None);
            reply["final"] = json!(true);
            Ok(reply)
        }
        Err(error) => Err(error),
    }
}

fn arguments(args: &[String], reference: bool, recover: bool) -> Result<()> {
    let mut at = usize::from(reference);
    if reference && args.first().is_none_or(|s| s.starts_with('-')) {
        return Err(rule("A task reference is required."));
    }
    let mut seen = std::collections::BTreeSet::new();
    while at < args.len() {
        let key = args[at].as_str();
        if !seen.insert(key) {
            return Err(rule("Duplicate team service option."));
        }
        match key {
            "--json" => at += 1,
            "--recover-only" if recover => at += 1,
            "--project" if reference && at + 1 < args.len() => at += 2,
            _ => return Err(rule("Unknown team service option.")),
        }
    }
    if !seen.contains("--json") {
        return Err(rule("Team service commands require --json."));
    }
    Ok(())
}

fn identity(config: &Config) -> Result<String> {
    #[derive(Deserialize)]
    struct Identity {
        node_id: String,
    }
    let file = crate::platform::no_follow(std::fs::OpenOptions::new().read(true))
        .open(config.dir.join("swarm/catalog.json"))
        .map_err(fail)?;
    let identity: Identity = serde_json::from_reader(file.take(32 * 1024 * 1024)).map_err(fail)?;
    if !zerus_replica::ids::is_uuid(&identity.node_id) {
        return Err(fail("Invalid local machine identity."));
    }
    Ok(identity.node_id)
}

#[allow(clippy::too_many_arguments)]
fn recover(
    config: &Config,
    body: &Input,
    task: &TaskDoc,
    reference: &TaskRef,
    kind: &str,
    home: &str,
    store: &Store,
    deadline: Instant,
) -> Result<Value> {
    let attempt = task
        .attempt(&body.id)
        .map_err(fail)?
        .ok_or_else(|| fail("No recorded attempt for this delivery ID."))?;
    let member = store
        .member(&body.team, &body.member)
        .map_err(problem)?
        .and_then(|m| m.session)
        .ok_or_else(|| rule("Member has no recorded session."))?;
    let lead_id = store
        .team(&body.team)
        .map_err(problem)?
        .ok_or_else(|| rule("Team not found."))?
        .lead;
    let lead = store
        .member(&body.team, &lead_id)
        .map_err(problem)?
        .and_then(|m| m.session)
        .ok_or_else(|| rule("Lead has no recorded session."))?;
    let same = |key: &SessionKey, binding: &zerus_team_plan::model::SessionKey| {
        key.machine == binding.machine && key.session_id == binding.session_id
    };
    if !same(&attempt.executor, &member)
        || !matches!(&attempt.actor, Actor::Agent(key) if same(key, &lead))
    {
        return Err(rule(
            "Attempt recipient or actor does not belong to this team effect.",
        ));
    }
    if !(if kind == "reopen" {
        matches!(attempt.origin, Origin::Reopen | Origin::Handoff)
    } else {
        attempt.origin == Origin::Send
    }) || attempt.executor.machine != home
    {
        return Err(rule("Attempt does not belong to this team effect."));
    }
    let native = attempt
        .delivery
        .ok_or_else(|| rule("Attempt has no native delivery binding."))?;
    if native.name.starts_with("dsh/") {
        return hosted::recover(config, body, reference, &native, store, home, deadline);
    }
    let facts = task
        .attempt_facts(&body.id)
        .map_err(fail)?
        .unwrap_or_default();
    if facts.contains(&EventKind::NotStarted) {
        return Ok(body.reply("refused", "The attempt was not admitted.", None));
    }
    match crate::state::task_reply_receipt_bound(&native.name, &body.id, &native.digest) {
        Ok(crate::state::ReceiptLookup::Found(receipt)) if receipt["status"] == "submitted" => {
            if let Err(error) = attempts::settle(
                config,
                &reference.project,
                &reference.uuid,
                &body.id,
                &receipt,
            ) {
                return Ok(body.reply("uncertain", &error.message, None));
            }
            Ok(body.reply("submitted", "", Some(receipt)))
        }
        _ if facts.contains(&EventKind::Submitted) => Ok(body.reply("submitted", "", None)),
        _ => Ok(body.reply(
            "uncertain",
            "The original delivery is not confirmed; nothing was sent again.",
            None,
        )),
    }
}

/// The bound of the first task-store open: the command's own wait, never past
/// the caller's deadline.
fn first_open_by(deadline: Instant) -> Instant {
    deadline.min(Instant::now() + attempts::STORE_WAIT)
}
/// The task-store-busy answer: not_started, with the typed cause beside the
/// plain sentence the lead reads (zerus-63rru.72).
fn store_busy_reply(body: &Input) -> Value {
    let mut reply = body.reply(
        "not_started",
        "The task store was busy; nothing was reserved or sent.",
        None,
    );
    reply["cause"] = json!(TASK_STORE_BUSY);
    reply
}

fn run(
    config: &Config,
    args: &[String],
    body: &Input,
    delivery: &dyn Delivery,
    deadline: Instant,
) -> Result<Value> {
    let here = identity(config)?;
    let teams_store = Store::open(&config.state).map_err(problem)?;
    let (team, _, reference, kind) = scope(&teams_store, body, &here)?;
    let mut scoped = args.to_vec();
    if option(args, "--project")?.is_none() {
        scoped.extend(["--project".into(), reference.project.clone()]);
    }
    let r = super::reference(config, &scoped, &Actor::Person)?;
    if r.project.0 != reference.project {
        return Err(rule("REF is not the stored effect's project."));
    }
    if !super::store_dir(config, &reference.project).is_dir() {
        return Err(fail("Task store is unavailable."));
    }
    // The first open waits at most STORE_WAIT (well below the actor's child
    // deadline): a held lock is a non-start, never a killed child.
    let open_by = first_open_by(deadline);
    let tasks = super::store_until(config, &reference.project, open_by).map_err(|error| {
        if error.message == super::BUSY {
            Error::new(
                STORE_BUSY_CODE,
                "The task store was busy; nothing was reserved or sent.",
            )
        } else {
            error
        }
    })?;
    let task = tasks
        .admit_until(deadline)
        .ok_or_else(|| fail(BUSY))?
        .load_complete(&reference.uuid)
        .map_err(fail)?;
    if r.task != reference.uuid && r.task != task.id() {
        return Err(rule("REF is not the stored effect's task."));
    }
    if task.attempt(&body.id).map_err(fail)?.is_some() {
        drop(tasks);
        return recover(
            config,
            body,
            &task,
            &reference,
            &kind,
            &team.home,
            &teams_store,
            deadline,
        );
    }
    if flag(args, "--recover-only") {
        return Ok(body.reply(
            "uncertain",
            "No attempt was recorded; recovery sends nothing.",
            None,
        ));
    }
    // From here to the reservation nothing is reserved or sent: every failure
    // is a refusal or a non-start, never unknown (zerus-63rru.58).
    let prepared = (|| -> Result<Option<_>> {
        let state: String = teams_store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT state FROM effects WHERE team=?1 AND request_id=?2",
                params![body.team, body.id],
                |r| r.get(0),
            )
            .map_err(fail)?;
        if !matches!(
            state.as_str(),
            "reserved" | "uncertain" | "submitted" | "cutting_over"
        ) {
            return Err(rule("Effect is closed or belongs to another generation."));
        }
        if state != "reserved" {
            return Ok(None);
        }
        let permit = permit(&teams_store, body, &here)?;
        marker(&task, &body.team, &here)?;
        let machines = machines_for(config, here.clone())?;
        let settings = tasks.project().map_err(fail)?.settings();
        let (frozen, to, who) = request(
            &settings,
            0,
            &task,
            &r.project.1,
            body,
            &permit,
            &machines,
            &teams_store,
        )?;
        let bound = task.view().map_err(fail)?;
        Ok(Some((permit, machines, frozen, to, who, bound)))
    })()
    .map_err(not_started)?;
    let Some((permit, machines, frozen, to, who, bound)) = prepared else {
        return Ok(body.reply(
            "uncertain",
            "Effect already has an outcome; no delivery was repeated.",
            None,
        ));
    };
    drop(tasks);
    drop(teams_store);
    delivery.before_reserve();
    let detail = std::cell::RefCell::new(String::new());
    let fatal = std::cell::RefCell::new(None);
    let admitted = attempts::admit(
        config,
        &reference.project,
        &reference.uuid,
        deadline,
        |admitted, task| {
            if task.attempt(&body.id)?.is_some() {
                return Ok(Err(Closed {
                    node: None,
                    reason: "existing",
                }));
            }
            let result = (|| -> Result<Store> {
                let settings = admitted.project().map_err(fail)?.settings();
                let starts =
                    attempts::admitted_starts(admitted, &who, &settings, deadline).map_err(fail)?;
                #[cfg(test)]
                locked_test_pause(config, "count");
                let store = Store::open(&config.state).map_err(problem)?;
                // Hold the team's operation lock through the attempt's save.
                store
                    .db
                    .as_ref()
                    .ok_or_else(|| fail("Team store disappeared."))?
                    .execute_batch("BEGIN IMMEDIATE")
                    .map_err(|e| {
                        fail(format!("The team store is busy; nothing was sent ({e})."))
                    })?;
                let fresh = permit_for_reservation(&store, body, &here, &permit, &bound, task)?;
                #[cfg(test)]
                locked_test_pause(config, "deliver");
                let (current, current_to, current_actor) = request(
                    &settings,
                    starts,
                    task,
                    &r.project.1,
                    body,
                    &fresh,
                    &machines,
                    &store,
                )?;
                if current != frozen || current_to != to || current_actor != who {
                    return Err(rule(dispatch::CHANGED));
                }
                hosted::reserve(config, body, &to, &frozen)?;
                reserve(task, body, &fresh, &to, &who, &frozen)?;
                Ok(store)
            })();
            match result {
                Ok(_permit_lock) => {
                    // An error once saving starts can leave a durable attempt.
                    // Let the fence turn it into Uncertain; never a refusal.
                    admitted.save(task)?;
                    #[cfg(test)]
                    if std::env::var_os("HGS_TEST_TEAM_DELIVERY_SAVE_ERROR").is_some() {
                        anyhow::bail!("fixture error after the durable task save");
                    }
                    Ok(Ok(()))
                }
                Err(error) => {
                    // Nothing was saved: a failure before the reservation is a
                    // refusal or a non-start (zerus-63rru.58).
                    if error.code != RULE {
                        *fatal.borrow_mut() = Some(not_started(error));
                        return Ok(Err(Closed {
                            node: None,
                            reason: "read_failed",
                        }));
                    }
                    *detail.borrow_mut() = error.message;
                    Ok(Err(Closed {
                        node: None,
                        reason: "not_sendable",
                    }))
                }
            }
        },
    );
    if let Some(error) = fatal.into_inner() {
        return Err(error);
    }
    match admitted {
        Ok(Admission::Reserved { .. }) if to.session.starts_with("dsh/") => {
            hosted::deliver(config, &reference, body, &to, &frozen, deadline)
        }
        Ok(Admission::Reserved { .. }) => Ok(delivered(body, delivery.deliver(config, &reference, body, &to, &frozen))),
        Ok(Admission::Refused(closed)) if closed.reason == "existing" => {
            let task = super::store(config, &reference.project)?
                .load(&reference.uuid)
                .map_err(fail)?;
            let store = Store::open(&config.state).map_err(problem)?;
            recover(config, body, &task, &reference, &kind, &here, &store, deadline)
        }
        Ok(Admission::Refused(closed)) => {
            let (state, error) = attempts::refusal(&closed, &detail.borrow());
            Ok(body.reply(state, &error, None))
        }
        Ok(Admission::Legacy(closed)) => Ok(body.reply(
            "cutting_over",
            &format!(
                "Attempt admission is unavailable ({}); nothing was delivered.",
                closed.reason
            ),
            None,
        )),
        Ok(Admission::Uncertain(error)) => Ok(body.reply("uncertain", &error, None)),
        Err(error) => Err(error),
    }
}

fn permit_for_reservation(
    store: &Store,
    body: &Input,
    here: &str,
    old: &Permit,
    bound: &zerus_replica::TaskView,
    task: &TaskDoc,
) -> Result<Permit> {
    let fresh = permit(store, body, here)?;
    marker(task, &body.team, here)?;
    let current = task.view().map_err(fail)?;
    if fresh.task != old.task
        || fresh.kind != old.kind
        || current.stage_at != bound.stage_at
        || current.target != bound.target
        || current.session != bound.session
    {
        return Err(rule(dispatch::CHANGED));
    }
    Ok(fresh)
}

fn marker(task: &TaskDoc, team: &str, here: &str) -> Result<()> {
    if task
        .team()
        .map_err(fail)?
        .as_ref()
        .is_none_or(|key| key.team != team || key.home != here)
        || task.team_invalid()
    {
        return Err(rule("Task no longer has this team's exact home marker."));
    }
    Ok(())
}

/// A bounded metadata read, under the caller's project/admission locks. A bad
/// file or unreadable path is present or an error, never an absent task.
fn task_present(config: &Config, reference: &TaskRef) -> Result<bool> {
    if !zerus_replica::ids::is_uuid(&reference.uuid) {
        return Err(rule("Invalid task identity."));
    }
    let directory = super::store_dir(config, &reference.project).join("tasks");
    for suffix in ["loro", "updates"] {
        match std::fs::symlink_metadata(directory.join(format!("{}.{suffix}", reference.uuid))) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(rule(error)),
        }
    }
    Ok(false)
}

fn reserve(
    task: &mut TaskDoc,
    body: &Input,
    permit: &Permit,
    to: &SessionKey,
    who: &Actor,
    request: &Value,
) -> Result<()> {
    if permit.kind == "reopen" && task.view().map_err(fail)?.session.as_ref() != Some(to) {
        task.reserve_team_handover(&body.id, to, attempts::native(to, request), who, now())
            .map_err(replica)?;
        return Ok(());
    }
    let origin = if permit.kind == "reopen" {
        let view = task.view().map_err(fail)?;
        let note = zerus_replica::TaskDelivery {
            mode: "note".into(),
            state: "note".into(),
            stage: view.stage.as_str().into(),
            stage_at: view.stage_at,
            dispatch_id: view.execution_id().unwrap_or_default(),
            submitted_id: String::new(),
            run_id: String::new(),
            conversation_id: String::new(),
            prompt: String::new(),
            error: String::new(),
        };
        task.reserve_message(&body.id, &permit.reason, &[], &note, None, (who, now()))
            .map_err(replica)?;
        Origin::Reopen
    } else {
        Origin::Send
    };
    task.reserve_attempt(
        &body.id,
        origin,
        to,
        attempts::native(to, request),
        who,
        now(),
    )
    .map_err(replica)?;
    Ok(())
}

fn delivered(body: &Input, result: attempts::Delivered) -> Value {
    match result {
        attempts::Delivered::Submitted(receipt) => body.reply("submitted", "", Some(receipt)),
        attempts::Delivered::NotAdmitted {
            reason,
            waiting,
            dialog,
        } => {
            let mut reply = body.reply("refused", &reason, None);
            // The member waits for a person: the typed cause the service
            // reads instead of the sentence (zerus-63rru.79).
            if waiting {
                reply["cause"] = json!(TERMINAL_WAITING);
                // Which dialog, when the gate knows one: the actor's card
                // names it (zerus-63rru.83). An older actor reads the
                // answer as today; the field is additive.
                if let Some(dialog) = dialog {
                    reply["dialog"] = json!(dialog);
                }
            }
            reply
        }
        attempts::Delivered::Uncertain(error) => body.reply("uncertain", &error, None),
    }
}

#[cfg(all(test, unix))]
fn delivered_test_answer(body: &Input, result: attempts::Delivered) -> Value {
    delivered(body, result)
}

#[cfg(test)]
mod tests;
#[cfg(all(test, unix))]
pub(crate) use tests::processes::{
    blocked_handout, removed_stage_cancel, waiting_handout, Blocked, RealCancel, RealHandout,
};

#[cfg(test)]
fn locked_test_pause(config: &Config, phase: &str) {
    if std::env::var("HGS_TEST_TEAM_LOCK_AT").as_deref() != Ok(phase) {
        return;
    }
    std::fs::write(config.state.join(format!("{phase}-ready")), "ready").unwrap();
    let until = Instant::now() + std::time::Duration::from_secs(8);
    while !config.state.join(format!("{phase}-release")).exists() && Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(config.state.join(format!("{phase}-release")).exists());
}
