//! Task 161 (owner, 2026-11-07): neither the lead nor a member turns a task
//! into a waiting loop; waiting on an outside event is always a park (task
//! 160), which Zerus watches and reports.
//!
//! - The lead's reply or message telling a member to wait for an outside event
//!   (an install, a CI run, a card, a time) is refused with the park command.
//! - A member's `hgs run` or team run whose purpose is waiting (a job named a
//!   wait, a loop of sleeps or pings around a repeated check) is refused with
//!   the park command; a member ending its turn waiting is offered the park.
//! - After the fact: a session building only for its own wait job, running a
//!   waiting background command, or repeating the same read-only check is
//!   parked automatically, and the lead is told once with the guessed
//!   condition.
use serde_json::{json, Value};

/// A member's attempt is looked at for a wait loop this long after its
/// hand-out, and at most this often.
pub(crate) const DETECT_AFTER: f64 = 600.0;
pub(crate) const DETECT_EVERY: f64 = 300.0;
/// The estimate of a park Zerus makes from a guessed condition.
pub(crate) const GUESSED_ETA_MINUTES: u64 = 60;

fn lower(text: &str) -> String {
    text.to_lowercase()
}

fn any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|word| text.contains(word))
}

/// Words that name waiting.
const WAIT: [&str; 14] = [
    "wait", "keep polling", "poll", "check again", "check back", "until", "sleep", "re-check", "recheck", "ожида", "жди", "подожд", "дождись", "пока не",
];
/// Words that name an event outside the task.
const OUTSIDE: [&str; 28] = [
    "install", "version", "release", "ci ", "ci.", "ci,", "ci run", "pipeline", "workflow", "card", "approval", "approve", "the owner",
    "deploy", "tomorrow", "minute", "hour", "o'clock", "nightly", "dev build", "new build", "build number", "build to ", "the build is",
    "установ", "сборк", "релиз", "владел",
];

/// The lead's text that tells a member to wait for an outside event.
pub(crate) fn lead_wait_instruction(text: &str) -> bool {
    let text = lower(text);
    any(&text, &WAIT) && any(&text, &OUTSIDE)
}

/// A job name or command whose purpose is waiting, with why; None for real
/// work. A loop needs a pause and a repetition around a check.
pub(crate) fn waiting_job(label: &str, command: &str) -> Option<String> {
    let label = lower(label);
    let command = lower(command);
    let named = ["wait", "poll", "watch for", "until ", "ожида"].iter().find(|word| label.contains(*word));
    if let Some(word) = named {
        return Some(format!("its name says it waits (\"{word}\")"));
    }
    let pause = ["sleep ", "start-sleep", "timeout /t", "timeout -t", "ping -n", "ping -c", "choice /t"].iter().find(|word| command.contains(*word));
    let repeat = ["while ", "while(", "until ", "for /l", "for ((", "loop", "goto ", ":retry", "repeat"].iter().find(|word| command.contains(*word));
    match (pause, repeat) {
        (Some(pause), Some(repeat)) => Some(format!("it loops (\"{}\") around a pause (\"{}\")", repeat.trim(), pause.trim())),
        _ => None,
    }
}

/// A member's last reply that says it waits or is blocked on something
/// outside its task.
pub(crate) fn waiting_reply(text: &str) -> bool {
    let text = lower(text);
    let waiting = any(&text, &["waiting for", "i'll wait", "i will wait", "blocked on", "blocked until", "keep polling", "will check again", "жду", "ожидаю", "заблокирован"]);
    waiting && any(&text, &OUTSIDE)
}

/// The same read-only check a member's turn repeated at intervals (three
/// times or more, each a minute or more after the one before): the command.
pub(crate) fn repeated_check(events: &[Value], since: f64) -> Option<(String, usize)> {
    // One event per call: the PreToolUse ones, or the PostToolUse ones of an
    // agent that journals only those.
    let kind = if events.iter().any(|event| event["type"] == "PreToolUse") { "PreToolUse" } else { "PostToolUse" };
    let mut runs: Vec<(String, f64)> = Vec::new();
    for event in events {
        if event["type"] != kind {
            continue;
        }
        let at = event["at"].as_str().and_then(|text| text.parse::<f64>().ok()).or_else(|| event["at"].as_f64()).unwrap_or(0.0);
        if at <= since {
            continue;
        }
        let tool = event["tool"].as_str().unwrap_or_default();
        if !matches!(tool, "Bash" | "PowerShell" | "shell" | "exec_command") {
            continue;
        }
        let command = event["detail"].as_str().unwrap_or_default().lines().next().unwrap_or_default().trim().to_owned();
        if command.is_empty() || !read_only(&command) {
            continue;
        }
        runs.push((command, at));
    }
    let mut best: Option<(String, usize)> = None;
    for (command, _) in &runs {
        let times: Vec<f64> = runs.iter().filter(|(other, _)| other == command).map(|(_, at)| *at).collect();
        let spaced = times.windows(2).filter(|pair| pair[1] - pair[0] >= 60.0).count() + 1;
        if spaced >= 3 && best.as_ref().is_none_or(|(_, count)| spaced > *count) {
            best = Some((command.clone(), spaced));
        }
    }
    best
}

/// A command that only reads: a version, a status, a listing, a CI run.
fn read_only(command: &str) -> bool {
    let command = lower(command);
    any(&command, &["--version", "-v ", "--build-info", " status", " ls", "inspect", "gh run", "actions/runs", "git log", "git fetch", "curl ", "hgs team current", "hgs team inbox"])
        && !any(&command, &["install", " build", " test", "commit", "push", " rm ", "del "])
}

/// The condition a wait most likely means, with what it is: a CI run when
/// the text names one, an installed build when it names a version, else a
/// time half an hour on (the lead sets the real condition).
pub(crate) fn guess_condition(text: &str, now: f64) -> (Value, String) {
    let lower = lower(text);
    if let Some(at) = lower.find("actions/runs/") {
        let after = &text[at + "actions/runs/".len()..];
        let run: String = after.chars().take_while(char::is_ascii_digit).collect();
        let before = &text[..at];
        let repo = before.trim_end_matches('/').rsplit("repos/").next().unwrap_or_default().trim_end_matches('/');
        let repo: String = repo.rsplit("github.com/").next().unwrap_or(repo).trim_end_matches('/').to_owned();
        if let (Ok(run), true) = (run.parse::<u64>(), repo.split('/').count() == 2) {
            return (json!({"kind":"ci","repo":repo,"run":run}), format!("CI run {run} of {repo}"));
        }
    }
    let version: Option<String> = lower
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .find(|word| word.matches('.').count() >= 1 && word.split('.').all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit())))
        .map(str::to_owned);
    if let (true, Some(version)) = (any(&lower, &["--version", "install", "build", "version"]), version) {
        return (json!({"kind":"build","version":version}), format!("hgs {version} installed"));
    }
    (json!({"kind":"time","at":(now + 1800.0).round()}), "a check in 30 minutes (guessed; set the real condition)".into())
}

/// The park request a refusal names, ready to send.
pub(crate) fn park_command(team: &str, task: Option<u32>, attempt: &str) -> String {
    let task = task.map_or("N".to_owned(), |id| id.to_string());
    let attempt = if attempt.is_empty() { "ATTEMPT" } else { attempt };
    format!(
        "{{\"team\":\"{team}\",\"task\":{task},\"attempt\":\"{attempt}\",\"condition\":{{\"kind\":\"build\",\"version\":\"1.58.0\"}},\"eta_minutes\":60,\"note\":\"...\"}} | hgs team park --json (conditions: ci, build, task, card, time; hgs guide team-park)"
    )
}

/// The refusal of a wait the lead tells a member to do.
pub(crate) fn lead_refusal(team: &str, task: Option<u32>, attempt: &str) -> String {
    format!(
        "Waiting is a park, never an instruction: park the task on its condition and Zerus watches it, frees the member and tells you the outcome: {}",
        park_command(team, task, attempt)
    )
}

/// The refusal of a member's job whose purpose is waiting.
pub(crate) fn job_refusal(why: &str, team: &str, task: Option<u32>, attempt: &str) -> String {
    format!(
        "This job waits ({why}); a wait is a park, never a job: {}. Zerus watches the condition and gives the task back to you.",
        park_command(team, task, attempt)
    )
}

/// `hgs send` from the lead's session to a member of its team: a wait
/// instruction is refused with the park of the member's open task.
pub(crate) fn lead_send_check(target: &str, text: &str) -> anyhow::Result<()> {
    if !lead_wait_instruction(text) {
        return Ok(());
    }
    let Some(record) = super::super::session_graph::caller_record() else { return Ok(()) };
    let Ok(store) = super::store::Store::open(false) else { return Ok(()) };
    for team in store.active_teams(1000).unwrap_or_default() {
        if super::commands::calling_member(&team, &record).as_deref() != Some(team.lead.as_str()) {
            continue;
        }
        let Some(member) = team.members.iter().find(|member| member.id != team.lead && member.session.as_ref().is_some_and(|session| session.name == target.trim())) else {
            continue;
        };
        let open = team.tasks.iter().find_map(|task| task.open_attempt().filter(|attempt| attempt.member == member.id).map(|attempt| (task.id, attempt.id.clone())));
        anyhow::bail!("{}", lead_refusal(&team.id, open.as_ref().map(|found| found.0), open.as_ref().map_or("", |found| found.1.as_str())));
    }
    Ok(())
}

/// The script a job runs, when its command names one in its folder (a .cmd,
/// .bat, .ps1, .sh or .py file): its first 64 KB, so a loop inside it is seen.
pub(crate) fn script_text(command: &str, cwd: &std::path::Path) -> String {
    let first = command.trim().trim_start_matches(['&', ' ']).trim();
    let first = if let Some(rest) = first.strip_prefix('"') { rest.split('"').next().unwrap_or_default() } else { first.split_whitespace().next().unwrap_or_default() };
    let mut candidates = vec![first.to_owned()];
    // `powershell -File x.ps1`, `bash x.sh`, `cmd /c x.cmd`: the script argument.
    candidates.extend(command.split_whitespace().skip(1).map(|word| word.trim_matches('"').to_owned()));
    for candidate in candidates.iter().take(8) {
        let lower = candidate.to_lowercase();
        if ![".cmd", ".bat", ".ps1", ".sh", ".py"].iter().any(|ext| lower.ends_with(ext)) {
            continue;
        }
        let path = std::path::Path::new(candidate);
        let path = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
        if let Ok(mut file) = std::fs::File::open(&path) {
            use std::io::Read;
            let mut bytes = Vec::new();
            let _ = (&mut file).take(64 * 1024).read_to_end(&mut bytes);
            return String::from_utf8_lossy(&bytes).into_owned();
        }
    }
    String::new()
}

/// The refusal of `hgs run` for a waiting job, naming the caller's own task
/// and attempt when it is a team member.
pub(crate) fn caller_job_refusal(why: &str) -> String {
    // Tests never read the live team store.
    if cfg!(test) {
        return job_refusal(why, "TEAM", None, "");
    }
    let found = super::super::session_graph::caller_record().and_then(|record| {
        let store = super::store::Store::open(false).ok()?;
        store.active_teams(1000).unwrap_or_default().into_iter().find_map(|team| {
            let member = super::commands::calling_member(&team, &record)?;
            let open = team.tasks.iter().find_map(|task| task.open_attempt().filter(|attempt| attempt.member == member).map(|attempt| (task.id, attempt.id.clone())));
            Some((team.id.clone(), open))
        })
    });
    match found {
        Some((team, open)) => job_refusal(why, &team, open.as_ref().map(|found| found.0), open.as_ref().map_or("", |found| found.1.as_str())),
        None => job_refusal(why, "TEAM", None, ""),
    }
}

#[cfg(test)]
#[path = "tests/wait_loops.rs"]
mod tests;
