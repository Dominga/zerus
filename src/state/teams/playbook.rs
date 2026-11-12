//! Task 271 (owner, 2026-11-08): Zerus coaches the lead.
//!
//! The owner's words: "let us make sure such mistakes in running a project do
//! not happen; let Zerus itself propose the methods of running it and remind
//! you after a compaction". The team's **lead playbook** is its working-mode
//! preset (task 270) plus the rules below, stored with the team (and carried in
//! its lore by the lead brief). Every rule is a check over the team record, so a
//! tick, a status read or an inbox read never runs Git: a breach becomes one
//! `playbook` item in the lead's inbox with the exact fix, and only what the
//! owner must decide is escalated.
//!
//! The **lead brief** (the preset, the open breaches, the owner's standing
//! decisions from the lore, the integration head, who idles) goes to the lead at
//! team start, at each phase start, and after every compaction or clear of its
//! session; `hgs team brief --lead` answers it on demand.
use super::model::{MemberState, TaskStatus, Team, TeamStatus};
use serde_json::{json, Value};

/// The playbook's rules, as the owner's list (a)-(i).
pub(crate) const RULES: [(&str, &str); 9] = [
    ("a", "The team runs in the working mode its project needs (the 270 preset: branch_per_task, base_branch and a pipeline for a code repository)."),
    ("b", "Accepted work lands on the integration line; nothing accepted waits unlanded."),
    ("c", "No member idles while ready work waits, and the lead hears it from Zeros, not from the owner."),
    ("d", "A task names the files or module paths it writes, never a whole folder."),
    ("e", "A subsystem that calls a binary (the lore) has a task that ships it in the package."),
    ("f", "One session, one team: live checks run in their own session, never the lead's."),
    ("g", "A report's tests are run before the lead accepts it."),
    ("h", "A task starts from the integration line, never another base."),
    ("i", "No real-format token or secret reaches a task text or a report."),
];

/// One breach of the playbook: the rule, what it sees, and the exact fix.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Breach {
    /// The rule's letter, "a" to "i".
    pub rule: &'static str,
    pub title: String,
    pub detail: String,
    /// The command the lead runs to fix it (or the card it raises).
    pub fix: String,
    /// Only the owner may decide this one; the lead's item says so.
    pub owner: bool,
}

impl Breach {
    pub(crate) fn key(&self) -> String {
        format!("playbook:{}", self.rule)
    }
}

/// The team's playbook: the working-mode preset and the rules, stored with the
/// team (and in its lore through the lead brief).
pub(crate) fn playbook(team: &Team) -> Value {
    let mode = &team.working_mode;
    let title = if mode.is_empty() {
        "no working mode recorded".to_owned()
    } else {
        format!("the {mode} working mode")
    };
    json!({
        "mode": mode,
        "title": title,
        "settings": {
            "branch_per_task": team.settings.branch_per_task,
            "base_branch": team.settings.base_branch,
            "folder_policy": team.settings.folder_policy,
            "integration_folder": team.settings.integration_folder,
            "build_broker": team.settings.build_broker,
        },
        "rules": RULES.iter().map(|(id, text)| json!({"id":id,"text":text})).collect::<Vec<_>>(),
    })
}

/// A real-format token or secret in a text: the (i) rule. It matches formats the
/// public repositories must never carry; a placeholder never matches.
pub(crate) fn leaks_token(text: &str) -> Option<&'static str> {
    const FORMATS: [(&str, &str); 8] = [
        ("sk-", "an API key"),
        ("ghp_", "a GitHub token"),
        ("github_pat_", "a GitHub token"),
        ("xoxb-", "a Slack token"),
        ("xoxp-", "a Slack token"),
        ("AKIA", "an AWS key"),
        ("-----BEGIN ", "a private key"),
        ("eyJhbGciOi", "a JWT"),
    ];
    for (format, what) in FORMATS {
        // Task #42 (team ede724c4): every occurrence, each only where a token
        // starts. "task-tracker-phase3a-desktop" holds "sk-" inside a word and
        // was reported as an API key; a prefix inside a word or an identifier
        // (after a letter, digit, `_` or `-`) is no token.
        for (at, _) in text.match_indices(format) {
            let starts = text[..at].chars().next_back().is_none_or(|before| !(before.is_alphanumeric() || before == '_' || before == '-'));
            if !starts {
                continue;
            }
            let rest: String = text[at + format.len()..].chars().take(120).collect();
            let token: String = rest
                .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ')' || c == ']' || c == '}')
                .next()
                .unwrap_or_default()
                .to_owned();
            let placeholder = ["placeholder", "example", "dummy", "fake", "redacted", "xxxx", "test-", "<", "...", "your"]
                .iter()
                .any(|word| token.to_lowercase().contains(word));
            if !placeholder && token.len() >= 8 && real_format(format, &token) {
                return Some(what);
            }
        }
    }
    None
}

/// Task #42: the shape a real token of `format` has after its prefix. An API
/// key (`sk-`, `sk-ant-...`, `sk-proj-...`) is at least 16 characters of
/// letters, digits, `_` and `-` with a digit among them; an AWS access key id
/// is `AKIA` and 16 upper-case letters or digits. Other formats keep the
/// length the caller checks.
fn real_format(format: &str, token: &str) -> bool {
    match format {
        "sk-" => {
            token.len() >= 16
                && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                && token.chars().any(|c| c.is_ascii_digit())
        }
        "AKIA" => token.len() >= 16 && token.chars().take(16).all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
        _ => true,
    }
}

/// A base other than the integration line named in a task text: the (h) rule.
fn names_another_base(text: &str, base: &str, integration: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if !lower.contains("base") && !lower.contains("branch") {
        return None;
    }
    const WORDS: [&str; 16] = [
        "is", "the", "a", "an", "of", "from", "into", "on", "to", "our", "this", "that", "its", "and", "or", "for",
    ];
    let known = [base.to_lowercase(), integration.to_lowercase(), "main".to_owned(), "master".to_owned(), "head".to_owned()];
    for marker in ["base_branch", "base branch", "base:"] {
        let Some(at) = lower.find(marker) else { continue };
        let rest = &lower[at + marker.len()..];
        let Some(first) = rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '/' || c == '.')).find(|word| !word.is_empty()) else { continue };
        if WORDS.contains(&first) || known.iter().any(|fine| fine == first) {
            continue;
        }
        if first.len() >= 4 {
            return Some(first.to_owned());
        }
    }
    None
}

/// Every breach of the playbook the team record shows, cheapest checks first:
/// no Git, no session listing, no file system.
pub(crate) fn breaches(team: &Team) -> Vec<Breach> {
    let mut found: Vec<Breach> = Vec::new();
    if !matches!(team.status, TeamStatus::Running | TeamStatus::Paused | TeamStatus::Planning) {
        return found;
    }
    let integration = team.settings.integration_folder.trim();
    // (a) the working mode its project needs: the 270 proposal cached on the team.
    // Task #13 (team ede724c4): a preset the owner kept (Keep, or "none" at
    // Create team) is the owner's decision, not a missing mode.
    let kept = super::working_mode::kept(team).and_then(|kept| kept["preset"].as_str()).unwrap_or_default();
    // Task #19 (owner, 2026-11-10): a lead who lands by hand on a named line,
    // or a team created with working mode "none", gets no (a), (b) or (h).
    let hand = super::merge_queue::hand_landing(team);
    if !hand && team.working_mode.is_empty()
        && team.mode_proposal.as_ref().and_then(|proposal| proposal["preset"].as_str()).is_some_and(|preset| !preset.is_empty() && preset != kept)
    {
        let preset = team
            .mode_proposal
            .as_ref()
            .and_then(|proposal| proposal["preset"].as_str())
            .unwrap_or_default()
            .to_owned();
        found.push(Breach {
            rule: "a",
            title: format!("The project needs the {preset} working mode, and the team records none"),
            detail: "The project's profile (task 270) selects a preset the team is not running with; the branches and the pipeline are not set up for it.".into(),
            fix: format!(
                "{{\"team\":\"{}\",\"kind\":\"working-mode\",\"preset\":\"{}\",\"reason\":\"the project's profile selects it\"}} | hgs team propose --json",
                team.id, preset
            ),
            owner: true,
        });
    }
    if let Some((preset, wrong)) = super::working_mode::cached_disagreement(team).filter(|_| !hand) {
        found.push(Breach {
            rule: "a",
            title: format!("The settings do not follow the project's {preset} working mode"),
            detail: format!("What is missing: {}.", wrong.join("; ")),
            fix: format!(
                "{{\"team\":\"{}\",\"kind\":\"working-mode\",\"preset\":\"{}\",\"reason\":\"the settings disagree with the project's profile\"}} | hgs team propose --json",
                team.id, preset
            ),
            owner: true,
        });
    }
    // (b) accepted work unlanded: the queue's own record, no ancestry read.
    let queued = super::merge_queue::enabled(team);
    // DCB-svn report (2026-11-12): a task marked landed has left it.
    for task in team.tasks.iter().filter(|task| task.status == TaskStatus::Accepted && task.landed.is_none() && !hand) {
        match task.merge.as_ref() {
            Some(entry) if !matches!(entry.state.as_str(), "landed" | "merged" | "accepted") => {
                found.push(Breach {
                    rule: "b",
                    title: format!("#{} {} is accepted but not on the integration line", task.id, task.title),
                    detail: format!("The landing is {}: {}.", entry.state, if entry.note.is_empty() { "the queue is working on it" } else { entry.note.as_str() }),
                    fix: if let Some(resolve) = entry.resolve_task {
                        format!("hgs team status --json lists resolve task #{resolve}; decide it with hgs team accept --json (task {resolve})")
                    } else {
                        format!("{{\"team\":\"{}\"}} | hgs team look --json (the queue lands it; a conflict becomes a resolve task)", team.id)
                    },
                    owner: false,
                });
            }
            None if queued => {
                found.push(Breach {
                    rule: "b",
                    title: format!("#{} {} is accepted but has no landing queued", task.id, task.title),
                    detail: "The merge queue is on but the accepted task has no merge entry: it was accepted outside the queue.".into(),
                    fix: format!("hgs team accept --json with task {} again so the queue lands it (or ask the owner to land it by hand)", task.id),
                    owner: true,
                });
            }
            _ => {}
        }
    }
    // (d) task outputs naming whole folders: the plan's own broad-output check.
    for task in team.tasks.iter().filter(|task| !task.status.finished()) {
        let broad = task.broad_outputs();
        if broad.is_empty() {
            continue;
        }
        found.push(Breach {
            rule: "d",
            title: format!("#{} {} names whole areas ({})", task.id, task.title, broad.join(", ")),
            detail: "A folder-level output never holds another task, so two writers can overlap: name the files or module paths it writes.".into(),
            fix: format!("give the next task's exact files at the next plan: hgs team extend --json (or an owner-approved replan); #{} keeps its output until decided", task.id),
            owner: false,
        });
    }
    // (c) members idle while ready work waits: ready tasks, no attempt open at
    // all, and the team unchanged for a while.
    let ready = super::plan::ready(team);
    let any_open = team.tasks.iter().any(|task| task.open_attempt().is_some());
    if !ready.is_empty() && !any_open {
        let waiting = ready.len();
        found.push(Breach {
            rule: "c",
            title: format!("{waiting} ready task(s) wait while no attempt is open"),
            detail: format!("Ready: {}. Nobody is handed them: a blocked dependency, a held output or an idle member.", ready.iter().take(6).map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ")),
            fix: format!("{{\"team\":\"{}\"}} | hgs team look --json names the holders; then hgs team status --json", team.id),
            owner: false,
        });
    }
    // (e) the lore: a project memory the lead must ship with the package.
    let lore = team.settings.memory.as_ref().is_some_and(|memory| !memory.workspace.is_empty() || memory.mode == "project");
    if lore {
        let ships = team.tasks.iter().any(|task| {
            if task.status == TaskStatus::Cancelled {
                return false;
            }
            let text = format!("{} {}", task.title, task.description).to_lowercase();
            ["package", "release", "ship", "installer", "publish"].iter().any(|word| text.contains(word))
        });
        if !ships {
            found.push(Breach {
                rule: "e",
                title: "The lore (the project memory) calls its binary, and no task ships it in the package".into(),
                detail: "A subsystem that calls a binary must be shipped with the package: no task names package, release, ship or installer.".into(),
                fix: format!(
                    "{{\"team\":\"{}\",\"tasks\":[{{\"title\":\"Ship the lore binary in the package\",\"description\":\"The project memory calls the lore binary: add it to the release package and check it runs from the installed build.\"}}]}} | hgs team extend --json",
                    team.id
                ),
                owner: false,
            });
        }
    }
    // (f) one session, one team: a scratch team started from a lead's session.
    if team.scratch_parent.is_some() {
        let lead_session = team.member(&team.lead).and_then(|member| member.session.as_ref()).map(|session| session.name.clone());
        let shared = lead_session.is_some_and(|name| {
            team.members.iter().any(|member| member.id != team.lead && member.session.as_ref().is_some_and(|session| session.name == name))
        });
        if shared {
            found.push(Breach {
                rule: "f",
                title: "A scratch team shares the lead's session".into(),
                detail: "One session, one team: the lead created a scratch team from its own session; a live check gets its own session.".into(),
                fix: format!("{{\"team\":\"{}\"}} | hgs team stop --json, then run the check in a session of its own", team.id),
                owner: false,
            });
        }
    }
    // (g) a report's tests were not run here: the report names test lines and the
    // task landed without the queue's test run.
    for task in team.tasks.iter().filter(|task| task.status == TaskStatus::Accepted) {
        let named = task
            .attempts
            .iter()
            .rev()
            .find_map(|attempt| attempt.report.as_ref())
            .and_then(|report| report.tests.as_ref())
            .map(|tests| tests.to_string())
            .unwrap_or_default();
        let names_tests = ["\"added\"", "\"removed\"", "\"lines\"", "\"functions\""].iter().any(|key| named.contains(key)) && named.len() > 8;
        let ran = task.merge.as_ref().is_some_and(|entry| entry.state == "testing" || entry.state == "merged" || !entry.requests.is_empty());
        if names_tests && !ran {
            found.push(Breach {
                rule: "g",
                title: format!("#{} {} reports tests that were never run here", task.id, task.title),
                detail: "The report names test lines or functions, and Zerus ran no tests for it: the tests may only exist in the report.".into(),
                fix: "hgs test affected --json (run the affected tests; accept the report only after they pass)".to_string(),
                owner: false,
            });
        }
    }
    // (h) a task naming a base other than the integration line.
    let base = team.settings.base_branch.trim();
    for task in team.tasks.iter().filter(|_| !hand) {
        if let Some(named) = names_another_base(&format!("{}\n{}", task.title, task.description), base, integration) {
            found.push(Breach {
                rule: "h",
                title: format!("#{} {} names a base that is not the integration line ({named})", task.id, task.title),
                detail: "A task starts from the integration line (settings.integration_folder / base_branch), never from another branch.".into(),
                fix: format!("correct the task's text or replan it: hgs team extend --json (base: the integration line, {})", if base.is_empty() { "the team's folder" } else { base }),
                owner: false,
            });
            break;
        }
    }
    // (i) a real-format token in a task text, a report or a decision.
    let mut leak = None;
    for task in &team.tasks {
        let text = format!("{}\n{}\n{}", task.title, task.description, task.criteria.join("\n"));
        if let Some(what) = leaks_token(&text) {
            leak = Some((format!("#{} {}", task.id, task.title), what));
            break;
        }
        for attempt in &task.attempts {
            let text = attempt.report.as_ref().map(|report| report.text.clone()).unwrap_or_default();
            if let Some(what) = leaks_token(&text) {
                leak = Some((format!("#{} report", task.id), what));
                break;
            }
        }
        if leak.is_some() {
            break;
        }
    }
    if let Some((where_, what)) = leak {
        found.push(Breach {
            rule: "i",
            title: format!("{where_} carries {what}"),
            detail: "A real-format token or secret is in the team's record: remove it from the task or the report and rotate it; it may already be in a public repository.".into(),
            fix: "edit the task text or the report (and rotate the secret): hgs team extend --json, then tell the owner".to_string(),
            owner: true,
        });
    }
    found
}

/// The lead's brief: the playbook, the open breaches, the owner's standing
/// decisions, the integration head and who idles. Short, plain text.
pub(crate) fn brief(team: &Team) -> String {
    let mode = if team.working_mode.is_empty() { "none recorded".to_owned() } else { team.working_mode.clone() };
    let mut lines = vec!["[Zerus team · from: Zerus · brief] How this team runs (the lead playbook).".to_string()];
    lines.push(format!(
        "Working mode: {mode}. Base: {}. Integration folder: {}. The playbook rules (a)-(i): {}",
        if team.settings.base_branch.trim().is_empty() { "the team's folder" } else { team.settings.base_branch.trim() },
        if team.settings.integration_folder.trim().is_empty() { "the team's folder" } else { team.settings.integration_folder.trim() },
        RULES.iter().map(|(id, text)| format!("({id}) {text}")).collect::<Vec<_>>().join(" ")
    ));
    let breaches = breaches(team);
    if breaches.is_empty() {
        lines.push("Open breaches: none. The run follows the playbook.".to_owned());
    } else {
        lines.push(format!("Open breaches ({}):", breaches.len()));
        for breach in breaches {
            lines.push(format!(
                "- ({}) {}. Fix: {}{}",
                breach.rule,
                breach.title,
                breach.fix,
                if breach.owner { " (the owner's decision)" } else { "" }
            ));
        }
    }
    let decisions: Vec<String> = super::decisions::current(team)
        .filter(|decision| decision.kind.is_empty() || decision.kind == "rule" || decision.kind == "standing rule" || decision.kind == "allowed" || decision.kind == "refused")
        .take(8)
        .map(|decision| format!("{} -> {}{}", decision.question, decision.choice, if decision.why.is_empty() { String::new() } else { format!(" ({})", decision.why) }))
        .collect();
    if decisions.is_empty() {
        lines.push("The owner's standing decisions: none recorded.".to_owned());
    } else {
        lines.push(format!("The owner's standing decisions: {}", decisions.join("; ")));
    }
    let head = team
        .tasks
        .iter()
        .filter_map(|task| task.merge.as_ref())
        .map(|entry| if entry.merged.is_empty() { entry.commit.clone() } else { entry.merged.clone() })
        .rfind(|commit| !commit.is_empty())
        .unwrap_or_default();
    // Task #404 (owner, 2026-11-09): the lead answers the owner first and is
    // never the bottleneck: one command per tool call, long work in the
    // background, one task read at a time.
    lines.push(
        "Work fast: answer the owner first, before anything else; one team command per tool call (never chain mutations); read one task with hgs team status --task N, not the whole status; tests, reviews, waits and builds run in the background, never in the foreground; decide reports at once, and above 8 members or 20 open tasks keep a sub-lead and review pipelines so you are never the bottleneck."
            .to_owned(),
    );    lines.push(format!("The integration line's last landing: {}.", if head.is_empty() { "nothing landed yet".to_owned() } else { head.chars().take(9).collect() }));
    let idlers: Vec<String> = team
        .members
        .iter()
        .filter(|member| member.state == MemberState::Active && member.id != team.lead)
        .filter(|member| !team.tasks.iter().any(|task| task.assignee == member.id && !task.status.finished()))
        .map(|member| member.id.clone())
        .collect();
    lines.push(if idlers.is_empty() {
        "Idle members: none without work.".to_owned()
    } else {
        format!("Idle members without work: {}.", idlers.join(", "))
    });
    lines.push("After a compaction or a clear, read this brief first: the plan is below it, the method is here. Ask for it again with hgs team brief --lead.".to_owned());
    lines.join("\n")
}

#[cfg(test)]
#[path = "tests/playbook.rs"]
mod tests;
