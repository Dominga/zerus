//! The team dispatcher (`hgs team worker`). Each tick it reads
//! every member session once, starts members that have work, hands ready tasks
//! to free members and forwards reports to the lead. A hand-off is recorded
//! before it is sent and never sent again once it may have arrived.
use super::super::*;
use super::model::*;
use super::plan;
use super::store::{Event, Store};
use anyhow::Context;
use std::collections::BTreeSet;
use std::fs;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_secs(3);
/// A worker with nothing to serve exits after this long.
const IDLE_EXIT: Duration = Duration::from_secs(300);
/// After a refused hand-off the member gets no new one for this long.
const REFUSAL_PAUSE: f64 = 30.0;

pub(crate) fn lock_path() -> PathBuf {
    root().join("teams/worker.lock")
}

/// One member session as `hgs ls --json` showed it in this tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Seen {
    /// SSH alias of the session's computer; empty for this computer.
    pub host: String,
    pub name: String,
    pub run_id: String,
    pub conversation_id: String,
    pub launch_id: String,
    /// Idle, with nothing waiting on the owner and no Background work.
    pub free: bool,
    pub idle: bool,
    pub reply_at: Option<f64>,
}

/// Every session of this computer and its peers from one listing.
pub(crate) fn seen(listing: &Value) -> Vec<Seen> {
    let mut sessions = Vec::new();
    for (index, machine) in listing.as_array().into_iter().flatten().enumerate() {
        let host = if index == 0 { String::new() } else { machine["host"].as_str().unwrap_or_default().to_owned() };
        for record in machine["sessions"].as_array().into_iter().flatten() {
            let field = |key: &str| record[key].as_str().unwrap_or_default();
            // Terminal sessions report their process, DeepSeek ones a state.
            let live = !matches!(field("state"), "paused" | "stopped" | "archived")
                && (field("state") == "running" || field("process_state") == "running" || field("runtime_state") == "live");
            let idle = live && field("activity") == "idle";
            let free = idle
                && matches!(field("phase"), "" | "idle" | "ready")
                && record["optional_questions"]["count"].as_u64().unwrap_or(0) == 0
                && record["auth_required"] != true
                && record["send_available"] != false
                && record["process_work"]["count"].as_u64().unwrap_or(0) == 0;
            sessions.push(Seen {
                host: host.clone(),
                name: field("name").to_owned(),
                run_id: field("run_id").to_owned(),
                conversation_id: field("conversation_id").to_owned(),
                launch_id: field("launch_id").to_owned(),
                free,
                idle,
                reply_at: record["reply_at"].as_f64(),
            });
        }
    }
    sessions
}

/// One thing the dispatcher does for a team in a tick.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Step {
    /// The member's started session appeared, or its identity changed.
    Adopt { member: String, session: SessionRef },
    Launch { member: String },
    HandOut { task: u32, member: String },
    TurnEnded { task: u32, at: f64 },
    NoReport { task: u32 },
    NotifyLead { task: u32 },
    /// A DeepSeek member's model and effort, before its first task.
    Settings { member: String },
}

fn session_of<'a>(member: &Member, seen: &'a [Seen]) -> Option<&'a Seen> {
    let session = member.session.as_ref()?;
    seen.iter().find(|s| s.host == session.host && s.name == session.name)
}

/// What to do for one team now. Pure, so the rules are tested without
/// processes.
pub(crate) fn steps(team: &Team, seen: &[Seen], now: f64) -> Vec<Step> {
    let mut steps = Vec::new();
    let active = matches!(team.status, TeamStatus::Running | TeamStatus::Paused);
    if !active {
        return steps;
    }
    for member in &team.members {
        if member.state == MemberState::Removed {
            continue;
        }
        if member.session.is_none() && !member.launch_id.is_empty() {
            if let Some(found) = seen.iter().find(|s| s.host == member.computer && s.launch_id == member.launch_id) {
                steps.push(Step::Adopt {
                    member: member.id.clone(),
                    session: SessionRef {
                        host: found.host.clone(),
                        name: found.name.clone(),
                        run_id: found.run_id.clone(),
                        conversation_id: found.conversation_id.clone(),
                    },
                });
            }
        } else if let (Some(session), Some(found)) = (&member.session, session_of(member, seen)) {
            if !found.run_id.is_empty()
                && (session.run_id != found.run_id || session.conversation_id != found.conversation_id)
            {
                steps.push(Step::Adopt {
                    member: member.id.clone(),
                    session: SessionRef { run_id: found.run_id.clone(), conversation_id: found.conversation_id.clone(), ..session.clone() },
                });
            }
        }
    }
    let ready = plan::ready(team);
    // Turn ends, No report and reports for the lead, for open work.
    for task in &team.tasks {
        let Some(attempt) = task.attempts.last() else { continue };
        if attempt.state == AttemptState::Delivered {
            let found = team.member(&attempt.member).and_then(|m| session_of(m, seen));
            match (attempt.turn_ended_at, found) {
                (None, Some(found)) => {
                    let delivered = attempt.delivered_at.unwrap_or(attempt.sent_at);
                    if let Some(at) = found.reply_at.filter(|at| found.idle && *at > delivered) {
                        steps.push(Step::TurnEnded { task: task.id, at });
                    }
                }
                (Some(ended), _) if !attempt.no_report && now - ended >= team.settings.report_timeout_seconds as f64 => {
                    steps.push(Step::NoReport { task: task.id });
                }
                _ => {}
            }
        }
        if attempt.state == AttemptState::Reported && attempt.notice == Notice::Pending {
            let lead = team.member(&team.lead).and_then(|m| session_of(m, seen));
            if lead.is_some_and(|lead| lead.free) {
                steps.push(Step::NotifyLead { task: task.id });
            }
        }
    }
    if team.status != TeamStatus::Running {
        return steps;
    }
    // Members busy with an open attempt, or handed one in this tick.
    let mut busy: BTreeSet<&str> = team
        .tasks
        .iter()
        .filter_map(|task| task.open_attempt())
        .map(|attempt| attempt.member.as_str())
        .collect();
    let mut launching = BTreeSet::new();
    for id in ready {
        let Some(task) = team.task(id) else { continue };
        let Some(member) = team.member(&task.assignee) else { continue };
        if member.state == MemberState::Removed || busy.contains(member.id.as_str()) {
            continue;
        }
        if member.session.is_none() {
            if member.launch_id.is_empty() && member.error.is_empty() && launching.insert(member.id.clone()) {
                steps.push(Step::Launch { member: member.id.clone() });
            }
            continue;
        }
        let refused_recently = team.tasks.iter().flat_map(|t| &t.attempts).any(|a| {
            a.member == member.id && a.state == AttemptState::Refused && now - a.sent_at < REFUSAL_PAUSE
        });
        if refused_recently || !session_of(member, seen).is_some_and(|s| s.free && !s.run_id.is_empty()) {
            continue;
        }
        busy.insert(member.id.as_str());
        steps.push(if member.needs_settings() { Step::Settings { member: member.id.clone() } } else { Step::HandOut { task: id, member: member.id.clone() } });
    }
    steps
}

/// The message that gives a member its task.
pub(crate) fn hand_off_text(team: &Team, task: &Task, attempt: &Attempt, report_host: &str, member_host: &str) -> String {
    let mut text = format!(
        "[Zerus team \u{b7} from: Lead \u{b7} task: #{} \u{b7} attempt: {}]\nTeam: {}. Your role: {}.\n",
        task.id,
        attempt.n,
        team.name,
        team.role_of(&attempt.member)
    );
    if !team.goal.is_empty() {
        text += &format!("Team goal: {}\n", team.goal);
    }
    text += &format!("\nTask #{}: {}\n", task.id, task.title);
    if !task.description.is_empty() {
        text += &format!("{}\n", task.description);
    }
    if !task.criteria.is_empty() {
        text += "\nAcceptance criteria:\n";
        for criterion in &task.criteria {
            text += &format!("- {criterion}\n");
        }
    }
    let deps: Vec<String> = task
        .deps
        .iter()
        .filter_map(|dep| team.task(dep.task))
        .map(|dep| {
            let status = serde_json::to_value(dep.status).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
            format!("#{} {} ({})", dep.id, dep.title, status.replace('_', " "))
        })
        .collect();
    if !deps.is_empty() {
        text += &format!("\nDepends on: {}\n", deps.join(", "));
    }
    if let Some(previous) = task.attempts.iter().rev().find(|a| a.n < attempt.n && a.state == AttemptState::Rejected) {
        text += &format!("\nAttempt {} was rejected: {}\n", previous.n, task.decision_reason);
    }
    let at = if member_host.is_empty() { String::new() } else { format!("@{report_host} ") };
    text += &format!(
        "\nWhen you finish, report once (hgs guide team) with JSON on stdin:\n  hgs {at}team report --json\n  {}\n\
         Summarize what changed, where, and how you checked it.",
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":team.id,"task":task.id,"attempt":attempt.id,"text":"..."})
    );
    text
}

/// The message that tells the lead about a report.
pub(crate) fn notice_text(team: &Team, task: &Task, attempt: &Attempt) -> String {
    let report = attempt.report.as_ref().map(|r| r.text.as_str()).unwrap_or_default();
    format!(
        "[Zerus team \u{b7} from: {} \u{b7} task: #{} \u{b7} attempt: {}]\nReport for task #{} {}:\n\n{}\n\n\
         Decide with hgs team accept --json or reject --json (with a reason):\n  {}",
        team.role_of(&attempt.member),
        task.id,
        attempt.n,
        task.id,
        task.title,
        report,
        json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":team.id,"task":task.id})
    )
}

/// Runs this hgs with JSON on stdin; the child is no session's own.
fn hgs(host: &str, args: &[&str], input: Option<&Value>, timeout: Duration) -> anyhow::Result<Output> {
    let mut command = Command::new(std::env::current_exe()?);
    if !host.is_empty() {
        command.arg(format!("@{host}"));
    }
    command.args(args);
    for name in ["TMUX", "HGS_HOST_PANE", "HGS_SESSION", "HGS_RUN_ID"] {
        command.env_remove(name);
    }
    match input {
        Some(input) => {
            let mut file = tempfile::tempfile()?;
            std::io::Write::write_all(&mut file, input.to_string().as_bytes())?;
            std::io::Seek::rewind(&mut file)?;
            command.stdin(Stdio::from(file));
        }
        None => {
            command.stdin(Stdio::null());
        }
    }
    // Not platform::output_until: on Windows its job ends every descendant
    // with the call, which would end a session host or DeepSeek host that a
    // launch starts, and it closes stdin. Output goes to files, so a chatty
    // child never blocks on a pipe.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command.stdout(Stdio::from(stdout.try_clone()?)).stderr(Stdio::from(stderr.try_clone()?));
    let mut child = command.spawn()?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut output = Output { status, stdout: Vec::new(), stderr: Vec::new() };
    std::io::Seek::rewind(&mut stdout)?;
    std::io::Seek::rewind(&mut stderr)?;
    std::io::Read::read_to_end(&mut stdout, &mut output.stdout)?;
    std::io::Read::read_to_end(&mut stderr, &mut output.stderr)?;
    Ok(output)
}

/// The result of one `hgs send`.
#[derive(Debug, PartialEq)]
pub(crate) enum Sent {
    Delivered,
    /// Refused before anything reached the agent.
    Refused(String),
    /// It may have arrived.
    Uncertain(String),
}

pub(crate) fn classify(output: &anyhow::Result<Output>) -> Sent {
    let output = match output {
        Ok(output) => output,
        Err(error) => return Sent::Uncertain(error.to_string()),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if output.status.success() {
        let receipt: Value = serde_json::from_str(stdout.trim()).unwrap_or_default();
        return if receipt["status"] == "submitted" { Sent::Delivered } else { Sent::Uncertain(format!("unexpected receipt: {}", stdout.trim())) };
    }
    // hgs reports every possibly delivered input as uncertain; ssh exits 255
    // when the connection, not hgs, failed.
    if stderr.to_lowercase().contains("uncertain") || output.status.code() != Some(1) {
        Sent::Uncertain(stderr)
    } else {
        Sent::Refused(stderr)
    }
}

fn send(session: &SessionRef, request_id: &str, text: &str) -> Sent {
    let input = json!({"request_id":request_id,"expected_run_id":session.run_id,
        "expected_conversation_id":session.conversation_id,"text":text});
    classify(&hgs(&session.host, &["send", &session.name, "--json"], Some(&input), Duration::from_secs(120)))
}

fn tag(text: &str) -> String {
    let tag: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    tag.chars().take(24).collect()
}

/// The folder the member works in: the team's (or its own) folder, or a new
/// worktree of it.
fn member_folder(team: &Team, member: &Member) -> anyhow::Result<String> {
    let folder = if member.directory.is_empty() { team.folder.clone() } else { member.directory.clone() };
    anyhow::ensure!(!folder.is_empty(), "The team has no folder");
    if member.folder != MemberFolder::Worktree {
        return Ok(folder);
    }
    let short = &team.id[..8.min(team.id.len())];
    let path = Path::new(&folder);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("project");
    let destination = path
        .parent()
        .unwrap_or(path)
        .join(format!("{name}-team-{short}-{}", tag(&member.id)))
        .display()
        .to_string();
    let branch = format!("team/{short}/{}", tag(&member.id));
    let output = hgs(
        &member.computer,
        &["worktrees", "create", "--path", &folder, "--branch", &branch, "--destination", &destination, "--base", "HEAD", "--json"],
        None,
        Duration::from_secs(120),
    )?;
    anyhow::ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    Ok(destination)
}

/// The `hgs` arguments that start a member's session in `folder`.
pub(crate) fn launch_args(team: &Team, member: &Member, folder: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        member.agent.clone(),
        folder.into(),
        "--new".into(),
        "-n".into(),
        format!("{}-{}", tag(&team.name), tag(&member.id)),
        "--launch-id".into(),
        member.launch_id.clone(),
    ];
    // The same options the desktop passes for a new session.
    let terminal = member.agent != "dsh" && member.agent != "opencode";
    if terminal {
        args.push("-d".into());
    }
    if !member.account.is_empty() {
        args.extend(["--account".into(), member.account.clone()]);
    }
    if !member.permissions.is_empty() {
        args.extend(["--permissions".into(), member.permissions.clone()]);
    }
    if member.agent == "opencode" && !member.model.is_empty() {
        args.extend(["--model".into(), member.model.clone()]);
        if !member.effort.is_empty() {
            args.extend(["--effort".into(), member.effort.clone()]);
        }
    }
    if terminal {
        // A terminal agent reports its status only once a conversation
        // starts; a short first turn makes the member Ready for its task.
        // Its model and effort go on its own command line.
        args.push("--".into());
        args.extend(agent_args(&member.agent, &member.model, &member.effort));
        args.extend([
            format!(
                "You are {} in the Zerus agent team {}. Tasks arrive as messages that begin with [Zerus team.                  Until one arrives do nothing and reply only: Ready.",
                member.role, team.name
            ),
        ]);
    }
    args
}

/// A terminal agent's own model and effort options, after `--`.
pub(crate) fn agent_args(agent: &str, model: &str, effort: &str) -> Vec<String> {
    let mut args = Vec::new();
    if !model.is_empty() {
        args.extend(["--model".into(), model.to_owned()]);
    }
    if !effort.is_empty() {
        match agent {
            "claude" => args.extend(["--effort".into(), effort.to_owned()]),
            "codex" => args.extend(["-c".into(), format!("model_reasoning_effort=\"{effort}\"")]),
            _ => {}
        }
    }
    args
}

/// DeepSeek's model is provider/model; a bare name keeps the session's own.
fn apply_settings(member: &Member) -> anyhow::Result<()> {
    let session = member.session.as_ref().context("no session")?;
    let mut input = json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":session.run_id,
        "expected_conversation_id":session.conversation_id});
    if member.model.contains('/') {
        input["model"] = json!(member.model);
    }
    if !member.effort.is_empty() {
        input["effort"] = json!(member.effort);
    }
    let output = hgs(&session.host, &["settings", &session.name, "--json"], Some(&input), Duration::from_secs(120))?;
    anyhow::ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    Ok(())
}

fn launch(team: &Team, member: &Member) -> anyhow::Result<()> {
    let folder = member_folder(team, member)?;
    let args = launch_args(team, member, &folder);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = hgs(&member.computer, &args, None, Duration::from_secs(120))?;
    anyhow::ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    Ok(())
}

/// One change to a team outside any request: the dispatcher's own record.
fn change(store: &mut Store, team: &str, change: impl FnOnce(&mut Team, &mut Vec<Event>) -> anyhow::Result<()>) -> anyhow::Result<()> {
    store.mutate(None, team, None, |team, events| change(team, events).map(|()| Value::Null))?;
    Ok(())
}

fn hand_out(store: &mut Store, team_id: &str, task_id: u32, member_id: &str, host: &str) -> anyhow::Result<()> {
    // Record the attempt before sending; a crash leaves it Unconfirmed.
    let attempt_id = uuid::Uuid::new_v4().to_string();
    let mut message = None;
    change(store, team_id, |team, events| {
        anyhow::ensure!(team.status == TeamStatus::Running && plan::ready(team).contains(&task_id), "no longer ready");
        let session = team.member(member_id).and_then(|m| m.session.clone()).context("no session")?;
        let member_host = team.member(member_id).map(|m| m.computer.clone()).unwrap_or_default();
        let task = team.task_mut(task_id).context("unknown task")?;
        anyhow::ensure!(task.assignee == member_id, "reassigned");
        let n = task.attempts.len() as u32 + 1;
        task.attempts.push(Attempt { n, id: attempt_id.clone(), member: member_id.to_owned(), sent_at: now(), ..Default::default() });
        task.status = TaskStatus::Assigned;
        let task = team.task(task_id).unwrap();
        message = Some((session, hand_off_text(team, task, task.attempts.last().unwrap(), host, &member_host)));
        events.push(Event::new("handed_out", json!({"by":"Lead","task":task_id,"to":team.role_of(member_id),"attempt":n})));
        Ok(())
    })?;
    let (session, text) = message.unwrap();
    let outcome = send(&session, &attempt_id, &text);
    change(store, team_id, |team, events| {
        let task = team.task_mut(task_id).context("unknown task")?;
        let Some(attempt) = task.attempts.iter_mut().find(|a| a.id == attempt_id && a.state == AttemptState::Delivering) else {
            return Ok(());
        };
        let n = attempt.n;
        match &outcome {
            Sent::Delivered => {
                attempt.state = AttemptState::Delivered;
                attempt.delivered_at = Some(now());
                task.status = TaskStatus::InProgress;
                events.push(Event::new("delivered", json!({"task":task_id,"attempt":n})));
            }
            Sent::Refused(error) => {
                attempt.state = AttemptState::Refused;
                attempt.error = error.clone();
                task.status = TaskStatus::Pending;
                events.push(Event::new("refused", json!({"task":task_id,"attempt":n,"error":error})));
            }
            Sent::Uncertain(error) => {
                attempt.state = AttemptState::Unconfirmed;
                attempt.error = error.clone();
                events.push(Event::new("unconfirmed", json!({"task":task_id,"attempt":n,"error":error})));
            }
        }
        Ok(())
    })
}

fn notify_lead(store: &mut Store, team_id: &str, task_id: u32) -> anyhow::Result<()> {
    let notice_id = uuid::Uuid::new_v4().to_string();
    let mut message = None;
    change(store, team_id, |team, _| {
        let lead = team.member(&team.lead).and_then(|m| m.session.clone()).context("the lead has no session")?;
        let task = team.task(task_id).context("unknown task")?;
        let attempt = task.attempts.last().filter(|a| a.notice == Notice::Pending).context("nothing to forward")?;
        message = Some((lead, notice_text(team, task, attempt)));
        let attempt = team.task_mut(task_id).unwrap().attempts.last_mut().unwrap();
        attempt.notice = Notice::Sending;
        attempt.notice_id = notice_id.clone();
        Ok(())
    })?;
    let (lead, text) = message.unwrap();
    let outcome = send(&lead, &notice_id, &text);
    change(store, team_id, |team, events| {
        let task = team.task_mut(task_id).context("unknown task")?;
        let Some(attempt) = task.attempts.iter_mut().find(|a| a.notice_id == notice_id && a.notice == Notice::Sending) else {
            return Ok(());
        };
        let n = attempt.n;
        attempt.notice = match &outcome {
            Sent::Delivered => Notice::Sent,
            Sent::Refused(_) => Notice::Pending,
            Sent::Uncertain(_) => Notice::Unconfirmed,
        };
        if attempt.notice == Notice::Unconfirmed {
            events.push(Event::new("notice_unconfirmed", json!({"task":task_id,"attempt":n})));
        }
        Ok(())
    })
}

fn run_step(store: &mut Store, team: &Team, step: &Step, host: &str) -> anyhow::Result<()> {
    match step {
        Step::Adopt { member, session } => change(store, &team.id, |team, events| {
            let found = team.members.iter_mut().find(|m| m.id == *member).context("unknown member")?;
            let started = found.session.is_none();
            found.session = Some(session.clone());
            found.state = MemberState::Active;
            if started {
                let role = team.role_of(member);
                events.push(Event::new("member_started", json!({"member":role,"session":session.name})));
            }
            Ok(())
        }),
        Step::Launch { member } => {
            let launch_id = uuid::Uuid::new_v4().to_string();
            let mut planned = None;
            change(store, &team.id, |team, _| {
                let found = team.members.iter_mut().find(|m| m.id == *member).context("unknown member")?;
                anyhow::ensure!(found.session.is_none() && found.launch_id.is_empty(), "already started");
                found.launch_id = launch_id.clone();
                let found = found.clone();
                planned = Some((team.clone(), found));
                Ok(())
            })?;
            let (team, found) = planned.unwrap();
            if let Err(error) = launch(&team, &found) {
                let error = error.to_string();
                change(store, &team.id, |team, events| {
                    if let Some(found) = team.members.iter_mut().find(|m| m.id == *member && m.session.is_none()) {
                        found.error = error.clone();
                    }
                    events.push(Event::new("launch_failed", json!({"member":team.role_of(member),"error":error})));
                    Ok(())
                })?;
            }
            Ok(())
        }
        Step::HandOut { task, member } => hand_out(store, &team.id, *task, member, host),
        Step::TurnEnded { task, at } => change(store, &team.id, |team, _| {
            if let Some(attempt) = team.task_mut(*task).and_then(|t| t.attempts.last_mut()) {
                if attempt.state == AttemptState::Delivered && attempt.turn_ended_at.is_none() {
                    attempt.turn_ended_at = Some(*at);
                }
            }
            Ok(())
        }),
        Step::NoReport { task } => change(store, &team.id, |team, events| {
            let role = team.task(*task).and_then(|t| t.attempts.last()).map(|a| team.role_of(&a.member)).unwrap_or_default();
            if let Some(attempt) = team.task_mut(*task).and_then(|t| t.attempts.last_mut()) {
                if attempt.state == AttemptState::Delivered && !attempt.no_report {
                    attempt.no_report = true;
                    events.push(Event::new("no_report", json!({"member":role,"task":task,"attempt":attempt.n})));
                }
            }
            Ok(())
        }),
        Step::NotifyLead { task } => notify_lead(store, &team.id, *task),
        Step::Settings { member } => {
            let found = team.member(member).cloned().context("unknown member")?;
            let result = apply_settings(&found);
            change(store, &team.id, |team, events| {
                let role = team.role_of(member);
                let found = team.members.iter_mut().find(|m| m.id == *member).context("unknown member")?;
                found.settings_applied = true;
                if let Err(error) = &result {
                    events.push(Event::new("member_settings_failed", json!({"member":role,"error":error.to_string()})));
                }
                Ok(())
            })
        }
    }
}

/// After a restart nothing is re-sent: an attempt or report notice that was
/// being sent may have arrived.
fn recover(store: &mut Store) -> anyhow::Result<()> {
    for team in store.teams(false, 10_000)? {
        let stuck = team.tasks.iter().flat_map(|t| &t.attempts).any(|a| a.state == AttemptState::Delivering || a.notice == Notice::Sending);
        if !stuck {
            continue;
        }
        change(store, &team.id, |team, events| {
            for task in &mut team.tasks {
                for attempt in &mut task.attempts {
                    if attempt.state == AttemptState::Delivering {
                        attempt.state = AttemptState::Unconfirmed;
                        attempt.error = "the dispatcher stopped while sending".into();
                        events.push(Event::new("unconfirmed", json!({"task":task.id,"attempt":attempt.n,"error":attempt.error})));
                    }
                    if attempt.notice == Notice::Sending {
                        attempt.notice = Notice::Unconfirmed;
                    }
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// One pass over every running team. Returns whether any team is active.
pub(crate) fn tick(store: &mut Store, host: &str) -> anyhow::Result<bool> {
    let teams: Vec<Team> = store
        .teams(false, 10_000)?
        .into_iter()
        .filter(|team| matches!(team.status, TeamStatus::Running | TeamStatus::Paused))
        .collect();
    if teams.is_empty() {
        return Ok(false);
    }
    let output = hgs("", &["ls", "--json", "--no-archives"], None, Duration::from_secs(60))?;
    anyhow::ensure!(output.status.success(), "hgs ls failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    let sessions = seen(&serde_json::from_slice(&output.stdout).context("invalid hgs ls output")?);
    for team in &teams {
        for step in steps(team, &sessions, now()) {
            if let Err(error) = run_step(store, team, &step, host) {
                eprintln!("hgs team worker: {}: {step:?}: {error}", team.name);
            }
        }
    }
    Ok(true)
}

pub(crate) fn worker() -> Result<i32> {
    private_dir(&root().join("teams"))?;
    let file = lock_file(&lock_path())?;
    if fs2::FileExt::try_lock_exclusive(&file).is_err() {
        return Ok(0);
    }
    let lock = crate::platform::FileLock::held(file);
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let started = fs::metadata(&executable).and_then(|m| m.modified()).ok();
    let config = crate::config::Config::load().map_err(|e| e.to_string())?;
    let host = config.host();
    let mut store = Store::open(true).map_err(|e| e.to_string())?;
    recover(&mut store).map_err(|e| e.to_string())?;
    let mut last_work = Instant::now();
    loop {
        if fs::metadata(&executable).and_then(|m| m.modified()).ok() != started {
            // An update replaced hgs: the new one carries on with the teams.
            drop(lock);
            if let Err(error) = ensure_worker() {
                eprintln!("hgs team worker: the updated dispatcher did not start: {error}");
            }
            return Ok(0);
        }
        match tick(&mut store, host) {
            Ok(true) => last_work = Instant::now(),
            Ok(false) if last_work.elapsed() >= IDLE_EXIT => return Ok(0),
            Ok(false) => {}
            Err(error) => eprintln!("hgs team worker: {error}"),
        }
        std::thread::sleep(TICK);
    }
}

/// Starts a detached worker unless one runs. It exits by itself once no team
/// runs.
pub(crate) fn ensure_worker() -> anyhow::Result<()> {
    if crate::status::locked(&lock_path()) {
        return Ok(());
    }
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["team", "worker"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for name in ["TMUX", "HGS_HOST_PANE", "HGS_SESSION", "HGS_RUN_ID"] {
        command.env_remove(name);
    }
    crate::platform::spawn_detached(&mut command)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::ops::{self, Actor};
    use super::*;

    fn team() -> Team {
        let (mut team, _) = ops::create(
            "t",
            &json!({"name":"Checkout","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
        )
        .unwrap();
        let mut events = Vec::new();
        ops::apply(
            &mut team,
            "plan",
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","session":{"name":"codex/x/dev","run_id":"r1"}},
                {"id":"qa","role":"Tester","agent":"kimi"}],
                "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"More API","assignee":"dev"},
                    {"id":3,"title":"Tests","assignee":"qa","deps":[{"task":1,"kind":"soft"}]}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        team
    }
    fn session(name: &str, run: &str, free: bool) -> Seen {
        Seen { name: name.into(), run_id: run.into(), conversation_id: "c".into(), free, idle: free, ..Default::default() }
    }

    #[test]
    fn a_free_member_gets_one_ready_task_and_a_new_member_starts() {
        let team = team();
        let seen = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
        let steps = steps(&team, &seen, 100.0);
        // Conversation ids were learned; dev gets #1 only; qa waits for #1.
        assert!(steps.contains(&Step::HandOut { task: 1, member: "dev".into() }));
        assert!(!steps.iter().any(|s| matches!(s, Step::HandOut { task: 2, .. } | Step::Launch { .. })));
    }

    #[test]
    fn a_busy_or_refused_member_gets_nothing() {
        let mut team = team();
        assert!(!steps(&team, &[session("codex/x/dev", "r1", false)], 100.0).iter().any(|s| matches!(s, Step::HandOut { .. })));
        team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Refused, sent_at: 90.0, ..Default::default() });
        assert!(!steps(&team, &[session("codex/x/dev", "r1", true)], 100.0).iter().any(|s| matches!(s, Step::HandOut { .. })));
        assert!(steps(&team, &[session("codex/x/dev", "r1", true)], 130.0).iter().any(|s| matches!(s, Step::HandOut { .. })));
    }

    #[test]
    fn an_unconfirmed_hand_off_is_never_sent_again() {
        let mut team = team();
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Unconfirmed, ..Default::default() });
        task.status = TaskStatus::Assigned;
        let steps = steps(&team, &[session("codex/x/dev", "r1", true)], 1000.0);
        // Task #2 waits too: dev still owns the unconfirmed attempt.
        assert!(!steps.iter().any(|s| matches!(s, Step::HandOut { .. })), "{steps:?}");
    }

    #[test]
    fn a_soft_dependency_starts_a_member_once_its_task_is_in_progress() {
        let mut team = team();
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, sent_at: 10.0, delivered_at: Some(10.0), ..Default::default() });
        task.status = TaskStatus::InProgress;
        let steps = steps(&team, &[session("codex/x/dev", "r1", false)], 20.0);
        assert!(steps.contains(&Step::Launch { member: "qa".into() }));
        team.settings.all_hard = true;
        assert!(!super::steps(&team, &[], 20.0).contains(&Step::Launch { member: "qa".into() }));
    }

    #[test]
    fn a_member_without_a_report_is_marked_after_its_turn_ends() {
        let mut team = team();
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, sent_at: 10.0, delivered_at: Some(10.0), ..Default::default() });
        task.status = TaskStatus::InProgress;
        let mut idle = session("codex/x/dev", "r1", true);
        idle.reply_at = Some(50.0);
        assert!(steps(&team, &[idle.clone()], 60.0).contains(&Step::TurnEnded { task: 1, at: 50.0 }));
        team.task_mut(1).unwrap().attempts[0].turn_ended_at = Some(50.0);
        assert!(!steps(&team, &[idle.clone()], 100.0).contains(&Step::NoReport { task: 1 }));
        assert!(steps(&team, &[idle], 171.0).contains(&Step::NoReport { task: 1 }));
    }

    #[test]
    fn a_report_reaches_the_lead_once_it_is_free_and_a_launch_is_adopted() {
        let mut team = team();
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Reported, notice: Notice::Pending, ..Default::default() });
        task.status = TaskStatus::Submitted;
        team.members[2].launch_id = "L".into();
        let mut started = session("kimi/x/qa", "r2", true);
        started.launch_id = "L".into();
        let busy_lead = steps(&team, &[session("claude/x/lead", "r0", false), started.clone()], 1.0);
        assert!(!busy_lead.contains(&Step::NotifyLead { task: 1 }));
        assert!(busy_lead.iter().any(|s| matches!(s, Step::Adopt { member, .. } if member == "qa")));
        assert!(steps(&team, &[session("claude/x/lead", "r0", true)], 1.0).contains(&Step::NotifyLead { task: 1 }));
    }

    #[test]
    fn listed_sessions_are_free_only_when_live_idle_and_unblocked() {
        let listing = json!([
            {"host":"laptop","sessions":[
                {"name":"codex/x/a","runtime_state":"live","process_state":"running","activity":"idle","phase":"idle",
                 "process_work":{"count":0},"run_id":"r","reply_at":5.0},
                {"name":"dsh/x/b","state":"running","activity":"idle","phase":"idle"},
                {"name":"codex/x/c","state":"paused","activity":"idle","phase":"idle"},
                {"name":"codex/x/d","process_state":"running","activity":"idle","phase":"approval"},
                {"name":"codex/x/e","process_state":"running","activity":"idle","phase":"idle","process_work":{"count":1}}]},
            {"host":"old","sessions":[{"name":"sh/y/f"}]}]);
        let seen = seen(&listing);
        let free: Vec<_> = seen.iter().filter(|s| s.free).map(|s| s.name.as_str()).collect();
        assert_eq!(free, ["codex/x/a", "dsh/x/b"]);
        assert_eq!((seen[0].host.as_str(), seen[5].host.as_str(), seen[0].reply_at), ("", "old", Some(5.0)));
        assert!(seen[4].idle && !seen[2].idle);
    }

    #[test]
    fn terminal_members_start_with_a_short_first_turn() {
        let team = team();
        let codex = Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), launch_id: "L".into(),
            permissions: "bypass".into(), ..Default::default() };
        let args = launch_args(&team, &codex, "D:/w");
        assert_eq!(args[..8], ["codex", "D:/w", "--new", "-n", "checkout-dev", "--launch-id", "L", "-d"]);
        assert!(args.windows(2).any(|w| w == ["--permissions", "bypass"]));
        assert!(args.last().unwrap().contains("You are Backend in the Zerus agent team Checkout"));
        // The role's model and effort go on the agent's own command line.
        let tuned = Member { model: "gpt-5.5".into(), effort: "low".into(), ..codex.clone() };
        let args = launch_args(&team, &tuned, "D:/w");
        let after = &args[args.iter().position(|a| a == "--").unwrap() + 1..];
        assert_eq!(after[..4], ["--model", "gpt-5.5", "-c", "model_reasoning_effort=\"low\""]);
        let claude = Member { agent: "claude".into(), model: "opus".into(), effort: "high".into(), ..codex.clone() };
        assert!(launch_args(&team, &claude, "D:/w").windows(4).any(|w| w == ["--model", "opus", "--effort", "high"]));
        let dsh = Member { agent: "dsh".into(), ..codex };
        assert!(!launch_args(&team, &dsh, "D:/w").iter().any(|a| a == "--" || a == "-d"));
    }

    #[test]
    fn a_deepseek_member_takes_its_settings_before_its_task() {
        let mut team = team();
        let dsh = team.members.iter_mut().find(|m| m.id == "dev").unwrap();
        dsh.agent = "dsh".into(); dsh.effort = "low".into();
        let seen = [session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true)];
        let steps_now = steps(&team, &seen, 1.0);
        assert!(steps_now.contains(&Step::Settings { member: "dev".into() }), "{steps_now:?}");
        assert!(!steps_now.iter().any(|s| matches!(s, Step::HandOut { member, .. } if member == "dev")));
        team.members.iter_mut().find(|m| m.id == "dev").unwrap().settings_applied = true;
        assert!(steps(&team, &seen, 1.0).iter().any(|s| matches!(s, Step::HandOut { member, .. } if member == "dev")));
    }

    #[test]
    fn send_outcomes_are_classified_conservatively() {
        let output = |code: i32, stdout: &str, stderr: &str| {
            #[cfg(windows)]
            let status = std::os::windows::process::ExitStatusExt::from_raw(code as u32);
            #[cfg(unix)]
            let status = std::os::unix::process::ExitStatusExt::from_raw(code << 8);
            Ok(Output { status, stdout: stdout.into(), stderr: stderr.into() })
        };
        assert_eq!(classify(&output(0, r#"{"status":"submitted"}"#, "")), Sent::Delivered);
        assert!(matches!(classify(&output(1, "", "session is busy")), Sent::Refused(_)));
        assert!(matches!(classify(&output(1, "", "delivery uncertain: x")), Sent::Uncertain(_)));
        assert!(matches!(classify(&output(255, "", "ssh: connection reset")), Sent::Uncertain(_)));
        assert!(matches!(classify(&Err(anyhow::anyhow!("timed out"))), Sent::Uncertain(_)));
    }

    #[test]
    fn the_hand_off_names_the_attempt_and_how_to_report() {
        let team = team();
        let task = team.task(1).unwrap();
        let attempt = Attempt { n: 2, id: "A-ID".into(), member: "dev".into(), ..Default::default() };
        let text = hand_off_text(&team, task, &attempt, "laptop", "devbox");
        assert!(text.starts_with("[Zerus team \u{b7} from: Lead \u{b7} task: #1 \u{b7} attempt: 2]"));
        assert!(text.contains("hgs @laptop team report --json") && text.contains("\"attempt\":\"A-ID\""));
        assert!(hand_off_text(&team, task, &attempt, "laptop", "").contains("hgs team report --json"));
    }
}
