//! Task 156 (owner, 2026-11-07): owner decisions live in the team's records
//! and reach the right work by relevance.
//!
//! Every owner answer to a card, and `hgs team decide` (the owner, or the lead
//! on the owner's verified word), is a decision at once: question, choice,
//! why, who, when, with refs (the spec section it amends, the output paths
//! and tasks it concerns, and the terms it forbids). A later decision on the
//! same question supersedes the earlier one, which stays, marked. The records
//! live in Zerus state for every memory mode (plain included); a team with a
//! RustCodeLore workspace also gets them as lore decision records.
//!
//! A decision binds a task by its listed tasks, by an output path it touches,
//! or by meaning (the content words of the question and choice against the
//! task's title, description and criteria, and lore's own `find` hits for a
//! workspace team). The binding, current decisions go into the hand-out; an
//! in-flight task a new decision touches gets one message; a report whose
//! changes contain a term a decision forbids is flagged before accept; and
//! each spec a decision amends gets a generated Decisions section.
use super::model::{Decision, Task, Team};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// At most this many decisions are kept per team (the oldest superseded go first).
pub(crate) const KEPT: usize = 500;
/// The hand-out's block is at most this long.
const BLOCK_BYTES: usize = 3000;
/// The generated spec section's markers.
pub(crate) const BEGIN: &str = "<!-- zerus:decisions:begin -->";
pub(crate) const END: &str = "<!-- zerus:decisions:end -->";

/// A decision as a request names it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NewDecision {
    pub question: String,
    pub choice: String,
    pub why: String,
    pub by: String,
    pub card: String,
    pub spec: String,
    pub files: Vec<String>,
    pub tasks: Vec<u32>,
    pub forbids: Vec<String>,
    pub supersedes: String,
    /// Task #279: "rule" for a standing rule, empty for an ordinary decision.
    pub kind: String,
    /// Task #279: the cited owner message {id, quote, at}.
    pub owner_message: Option<serde_json::Value>,
}

fn clip(text: &str, limit: usize) -> String {
    let flat = text.trim();
    if flat.chars().count() <= limit { flat.to_owned() } else { flat.chars().take(limit.saturating_sub(1)).collect::<String>() + "\u{2026}" }
}

fn relative_path(path: &str) -> bool {
    let path = path.trim();
    !path.is_empty() && path.len() <= 300 && !path.starts_with('/') && !path.contains(':') && !path.contains('\\')
        && !path.split('/').any(|part| part == "..") && !path.chars().any(char::is_control)
}

/// The refs a request or a card names: `spec`, `files`, `tasks` (or the
/// card's `binds`), `forbids`; checked against the team.
pub(crate) fn refs_from(team: &Team, input: &Value, decision: &mut NewDecision) -> anyhow::Result<()> {
    let spec = input["spec"].as_str().unwrap_or_default().trim().to_owned();
    anyhow::ensure!(spec.is_empty() || relative_path(spec.split('#').next().unwrap_or_default()), "A decision's \"spec\" is a repository path, such as docs/lore.md#2.1");
    decision.spec = spec;
    let strings = |key: &str| -> Vec<String> { input[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(|text| text.trim().to_owned()).filter(|text| !text.is_empty()).collect() };
    decision.files = strings("files");
    anyhow::ensure!(decision.files.len() <= 20 && decision.files.iter().all(|path| relative_path(path)), "A decision names at most 20 repository paths in \"files\"");
    decision.forbids = strings("forbids").into_iter().map(|term| term.to_lowercase()).collect();
    anyhow::ensure!(decision.forbids.len() <= 20 && decision.forbids.iter().all(|term| term.chars().count() <= 60), "A decision forbids at most 20 terms of 60 characters");
    let tasks: Vec<u64> = input["tasks"].as_array().or(input["binds"].as_array()).into_iter().flatten().filter_map(Value::as_u64).collect();
    anyhow::ensure!(tasks.len() <= 50, "A decision names at most 50 tasks");
    for id in tasks {
        let id = u32::try_from(id).map_err(|_| anyhow::anyhow!("Unknown task #{id}"))?;
        anyhow::ensure!(team.task(id).is_some(), "Unknown task #{id}");
        if !decision.tasks.contains(&id) {
            decision.tasks.push(id);
        }
    }
    Ok(())
}

/// `hgs team decide`'s request: question, choice, why and the refs.
pub(crate) fn from_input(team: &Team, input: &Value) -> anyhow::Result<NewDecision> {
    let question = input["question"].as_str().unwrap_or_default().trim().to_owned();
    let choice = input["choice"].as_str().unwrap_or_default().trim().to_owned();
    anyhow::ensure!(!question.is_empty() && question.chars().count() <= 300, "A decision names its question (1 to 300 characters)");
    anyhow::ensure!(!choice.is_empty() && choice.chars().count() <= 600, "A decision names the owner's choice (1 to 600 characters)");
    let why = input["why"].as_str().unwrap_or_default().trim().to_owned();
    anyhow::ensure!(why.chars().count() <= 800, "A decision's why is at most 800 characters");
    // Task #279: a standing rule is a decision kind of its own, recalled in
    // every lead brief and in the hand-out of every task it binds; it may cite
    // the owner's message it came from.
    let kind = match input["kind"].as_str().unwrap_or_default().trim().to_ascii_lowercase().replace(' ', "-").as_str() {
        "rule" | "standing-rule" => "rule",
        _ => "",
    };
    let owner_message = input["owner_message"].as_object().map(|message| {
        serde_json::json!({"id":message.get("id").cloned().unwrap_or_default(),
            "quote":message.get("quote").cloned().unwrap_or_default(),
            "at":message.get("at").cloned().unwrap_or_default()})
    });
    let mut decision = NewDecision {
        question,
        choice,
        why,
        supersedes: input["supersedes"].as_str().unwrap_or_default().to_owned(),
        kind: kind.into(),
        owner_message,
        ..NewDecision::default()
    };
    refs_from(team, input, &mut decision)?;
    if !decision.supersedes.is_empty() {
        anyhow::ensure!(team.decisions.iter().any(|found| found.id == decision.supersedes), "Unknown decision {}", decision.supersedes);
    }
    Ok(decision)
}

/// The decision an owner's answer to a card is: what was asked, what the
/// owner chose and why, with the refs the card named.
pub(crate) fn from_card(card: &Value, state: &str, answer: &str, reason: &str) -> NewDecision {
    let kind = card["kind"].as_str().unwrap_or("proposal");
    let summary = card["change"]["summary"].as_str().filter(|text| !text.is_empty()).or(card["reason"].as_str()).unwrap_or_default();
    let question = if kind == "question" {
        card["change"]["question"].as_str().unwrap_or(summary).to_owned()
    } else {
        format!("{kind}: {summary}")
    };
    let choice = match state {
        "answered" => answer.to_owned(),
        "accepted" => format!("allowed: {summary}"),
        _ => format!("denied: {summary}"),
    };
    let refs = &card["refs"];
    let strings = |key: &str| -> Vec<String> { refs[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect() };
    let mut tasks: Vec<u32> = refs["tasks"].as_array().into_iter().flatten().filter_map(Value::as_u64).map(|id| id as u32).collect();
    for id in card["for_tasks"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
        if !tasks.contains(&(id as u32)) {
            tasks.push(id as u32);
        }
    }
    NewDecision {
        question: clip(&question, 300),
        choice: clip(&choice, 600),
        why: clip(if reason.trim().is_empty() { "no reason given" } else { reason }, 800),
        by: "owner".into(),
        card: card["id"].as_str().unwrap_or_default().to_owned(),
        spec: refs["spec"].as_str().unwrap_or_default().to_owned(),
        files: strings("files"),
        tasks,
        forbids: strings("forbids"),
        supersedes: String::new(),
        kind: String::new(),
        owner_message: None,
    }
}

/// The key two decisions on the same question share: its content words.
pub(crate) fn topic(question: &str) -> String {
    let mut words: Vec<String> = words(question).into_iter().collect();
    words.sort();
    words.join(" ")
}

/// Records a decision now; one on the same question (or the one it names)
/// is superseded and kept, marked. Returns its id.
pub(crate) fn record(team: &mut Team, new: NewDecision, at: f64, events: &mut Vec<super::store::Event>) -> String {
    let next = team.decisions.iter().filter_map(|found| found.id.strip_prefix('d').and_then(|n| n.parse::<u32>().ok())).max().unwrap_or(0) + 1;
    let id = format!("d{next}");
    let key = topic(&new.question);
    let mut superseded = Vec::new();
    for old in team.decisions.iter_mut().filter(|old| old.superseded_by.is_empty()) {
        if old.id == new.supersedes || (!key.is_empty() && old.topic == key) {
            old.superseded_by = id.clone();
            superseded.push(old.id.clone());
        }
    }
    let decision = Decision {
        id: id.clone(),
        question: new.question,
        choice: new.choice,
        why: new.why,
        by: if new.by.is_empty() { "owner".into() } else { new.by },
        at,
        card: new.card,
        spec: new.spec,
        files: new.files,
        tasks: new.tasks,
        forbids: new.forbids,
        superseded_by: String::new(),
        topic: key,
        kind: new.kind,
        owner_message: new.owner_message,
    };
    events.push(super::store::Event::new("decision_recorded", json!({"id":id,"question":decision.question,"choice":decision.choice,
        "by":decision.by,"card":decision.card,"spec":decision.spec,"files":decision.files,"tasks":decision.tasks,"supersedes":superseded,
        "kind":decision.kind,"owner_message":decision.owner_message})));
    team.decisions.push(decision);
    while team.decisions.len() > KEPT {
        let Some(oldest) = team.decisions.iter().position(|found| !found.superseded_by.is_empty()) else { break };
        team.decisions.remove(oldest);
    }
    id
}

/// The decisions still in force.
pub(crate) fn current(team: &Team) -> impl Iterator<Item = &Decision> {
    team.decisions.iter().filter(|decision| decision.superseded_by.is_empty())
}

const STOP: [&str; 40] = [
    "the", "and", "for", "with", "that", "this", "from", "into", "only", "when", "what", "which", "have", "has", "will", "must", "never",
    "always", "should", "would", "could", "about", "their", "there", "these", "those", "than", "then", "them", "they", "your", "task",
    "team", "owner", "allowed", "denied", "proposal", "make", "uses", "use",
];

/// A text's content words: lowercase, at least four letters, no stop words.
pub(crate) fn words(text: &str) -> BTreeSet<String> {
    text.to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| word.chars().count() >= 4 && !STOP.contains(word))
        .map(str::to_owned)
        .collect()
}

/// Two word sets mean the same thing when they share two or more words
/// making a third of the smaller one.
pub(crate) fn similar(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    let shared = a.intersection(b).count();
    let smaller = a.len().min(b.len()).max(1);
    shared >= 2 && shared * 3 >= smaller
}

fn normal(path: &str) -> String {
    path.trim().split('#').next().unwrap_or_default().replace('\\', "/").trim_matches('/').to_owned()
}

/// Whether two repository paths touch: one is the other or holds it.
pub(crate) fn touches(a: &str, b: &str) -> bool {
    let (a, b) = (normal(a), normal(b));
    !a.is_empty() && !b.is_empty() && (a == b || a.starts_with(&format!("{b}/")) || b.starts_with(&format!("{a}/")))
}

fn task_text(task: &Task) -> String {
    format!("{} {} {}", task.title, task.description, task.criteria.join(" "))
}

/// Why a decision binds a task, or None.
pub(crate) fn binds(team: &Team, decision: &Decision, task: &Task) -> Option<&'static str> {
    if decision.tasks.contains(&task.id) {
        return Some("names this task");
    }
    let paths: Vec<&String> = decision.files.iter().chain(std::iter::once(&decision.spec).filter(|spec| !spec.is_empty())).collect();
    if task.output.iter().any(|output| paths.iter().any(|path| touches(output, path))) {
        return Some("touches its outputs");
    }
    if team.decision_hits.get(&task.id).is_some_and(|hits| hits.ids.contains(&decision.id)) {
        return Some("found by lore");
    }
    let about = words(&format!("{} {}", decision.question, decision.choice));
    if similar(&about, &words(&task_text(task))) {
        return Some("same subject");
    }
    None
}

/// The current decisions that bind a task, newest first.
pub(crate) fn relevant<'a>(team: &'a Team, task: &Task) -> Vec<(&'a Decision, &'static str)> {
    let mut found: Vec<(&Decision, &'static str)> = current(team).filter_map(|decision| binds(team, decision, task).map(|why| (decision, why))).collect();
    found.sort_by(|a, b| b.0.at.total_cmp(&a.0.at));
    found
}

fn date(at: f64) -> String {
    chrono::DateTime::from_timestamp(at as i64, 0).map(|time| time.format("%Y-%m-%d").to_string()).unwrap_or_default()
}

/// One decision as a line: its id, question, choice, why and refs. A standing
/// rule (#279) says so and quotes the owner's message it was verified against.
pub(crate) fn line(decision: &Decision) -> String {
    let what = if decision.kind == "rule" { format!("standing rule {}", decision.id) } else { decision.id.clone() };
    let mut text = format!("- {} ({}, {}): {} -> {}", what, decision.by, date(decision.at), decision.question, decision.choice);
    if !decision.why.is_empty() {
        text += &format!(". Why: {}", decision.why);
    }
    if !decision.forbids.is_empty() {
        text += &format!(". Forbids: {}", decision.forbids.join(", "));
    }
    if !decision.spec.is_empty() {
        text += &format!(". Amends {}", decision.spec);
    }
    if let Some(message) = &decision.owner_message {
        let id = message["id"].as_i64().or_else(|| message["id"].as_str().and_then(|text| text.parse().ok())).unwrap_or_default();
        let quote = message["quote"].as_str().unwrap_or_default();
        if !quote.is_empty() {
            text += &format!(". From your message {id}: \"{}\"", clip(quote, 300));
        }
    }
    text
}

/// Task #279: the owner's standing rules, newest first: recalled in every lead
/// brief.
pub(crate) fn rules(team: &Team) -> Vec<&Decision> {
    let mut found: Vec<&Decision> = current(team).filter(|decision| decision.kind == "rule").collect();
    found.sort_by(|a, b| b.at.total_cmp(&a.at));
    found
}

/// Task #279: the standing rules as the lead brief shows them.
pub(crate) fn rules_block(team: &Team) -> String {
    let found = rules(team);
    if found.is_empty() {
        return String::new();
    }
    let mut text = "\nYour standing rules (the owner's, cited; they bind every plan and hand-out, and hgs team ask if one seems wrong):\n".to_owned();
    for decision in found {
        let next = format!("{}\n", line(decision));
        if text.len() + next.len() > BLOCK_BYTES {
            text += "- (more: hgs team decisions --json)\n";
            break;
        }
        text += &next;
    }
    text
}

/// The hand-out's block: the current decisions that bind the task.
pub(crate) fn block(team: &Team, task: &Task) -> String {
    let found = relevant(team, task);
    if found.is_empty() {
        return String::new();
    }
    let mut text = "\nDecisions that bind this task (the owner's; follow them, and hgs team ask if one seems wrong):\n".to_owned();
    for (decision, why) in found {
        let next = format!("{} [{why}]\n", line(decision));
        if text.len() + next.len() > BLOCK_BYTES {
            text += "- (more: hgs team decisions --json)\n";
            break;
        }
        text += &next;
    }
    text
}

/// The current decisions a report's added text contradicts: a term a
/// decision forbids, as a whole word.
pub(crate) fn conflicts(team: &Team, task: &Task, added: &str) -> Vec<Value> {
    let lower = added.to_lowercase();
    let mut found = Vec::new();
    for decision in current(team) {
        // A decision for other work cannot be broken by this task, unless it
        // binds everything (no paths, no tasks).
        let general = decision.files.is_empty() && decision.tasks.is_empty() && decision.spec.is_empty();
        if !general && binds(team, decision, task).is_none() {
            continue;
        }
        for term in &decision.forbids {
            if contains_word(&lower, term) {
                found.push(json!({"decision":decision.id,"term":term,"choice":decision.choice}));
                break;
            }
        }
    }
    found
}

fn contains_word(text: &str, term: &str) -> bool {
    let term = term.trim();
    if term.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(at) = text[start..].find(term) {
        let begin = start + at;
        let end = begin + term.len();
        let before = text[..begin].chars().next_back().is_none_or(|ch| !ch.is_alphanumeric());
        let after = text[end..].chars().next().is_none_or(|ch| !ch.is_alphanumeric());
        if before && after {
            return true;
        }
        start = end;
    }
    false
}

/// The lead's text that hands down a decision as a message, which `hgs team
/// decide` records instead. Task #36 (team ede724c4, 2026-11-10): only a
/// sentence where the owner decides - the owner followed closely by a decision
/// verb ("the owner decided", "owner says no", "владелец решил") or the owner's
/// decision named as such ("the owner's decision", "decision of the owner",
/// "решение владельца"). The word "owner" and a decision word anywhere in the
/// text are not one: a task hand-out quotes "Owner, 2026-11-10." and its
/// "Decisions that bind this task (the owner's; ...)" as context, and was
/// refused.
pub(crate) fn decision_like(text: &str) -> bool {
    const VERBS: [&str; 12] = ["decided", "decides", "forbids", "forbade", "forbidden", "решил", "решила", "решили", "решает", "запретил", "запретила", "запрещает"];
    const NO: [&str; 3] = ["says", "said", "wants"];
    let lower = text.to_lowercase();
    for sentence in lower.split(['.', '!', '?', '\n', ';']) {
        let words: Vec<&str> = sentence
            .split(|ch: char| !(ch.is_alphanumeric() || ch == '\'' || ch == '’'))
            .filter(|word| !word.is_empty())
            .collect();
        let owner = |word: &str| word.starts_with("owner") || word.starts_with("владел");
        for (at, word) in words.iter().enumerate() {
            if !owner(word) {
                continue;
            }
            // The owner, then a decision verb within five words.
            let after = &words[at + 1..words.len().min(at + 6)];
            if after.iter().any(|next| VERBS.contains(next)) {
                return true;
            }
            if after.windows(2).any(|pair| NO.contains(&pair[0]) && pair[1] == "no") {
                return true;
            }
            // "the owner's decision", "решение владельца".
            if words.get(at + 1).is_some_and(|next| next.starts_with("decision")) {
                return true;
            }
            if at > 0 && words[at - 1].starts_with("решени") {
                return true;
            }
            // "a decision of the owner".
            let before = &words[at.saturating_sub(3)..at];
            if before.first().is_some_and(|first| first.starts_with("decision")) && before.contains(&"of") {
                return true;
            }
        }
    }
    false
}

/// The generated Decisions section of a spec: every decision that amends it,
/// superseded ones marked.
pub(crate) fn spec_section(team: &Team, spec: &str) -> String {
    let path = normal(spec);
    let mut lines = vec![BEGIN.to_owned(), "## Decisions".into(), String::new(),
        "Generated by Zerus from the team's decision records (hgs team decisions); edit them there, not here.".into(), String::new()];
    for decision in team.decisions.iter().filter(|decision| normal(&decision.spec) == path) {
        let mut text = line(decision);
        if !decision.superseded_by.is_empty() {
            text = format!("- ~~{}~~ (superseded by {})", text.trim_start_matches("- "), decision.superseded_by);
        }
        lines.push(text);
    }
    lines.push(END.to_owned());
    lines.join("\n")
}

/// A spec with its Decisions section replaced, or appended when it has none.
pub(crate) fn render_into(file: &str, section: &str) -> String {
    match (file.find(BEGIN), file.find(END)) {
        (Some(begin), Some(end)) if end > begin => format!("{}{}{}", &file[..begin], section, &file[end + END.len()..]),
        _ => {
            let mut text = file.trim_end().to_owned();
            text += "\n\n";
            text += section;
            text += "\n";
            text
        }
    }
}

/// The specs the team's decisions amend.
pub(crate) fn specs(team: &Team) -> BTreeSet<String> {
    team.decisions.iter().map(|decision| normal(&decision.spec)).filter(|path| !path.is_empty()).collect()
}

/// The hash the generated sections were last written for.
pub(crate) fn specs_hash(team: &Team) -> String {
    use sha2::{Digest, Sha256};
    let parts: Vec<String> = team.decisions.iter().filter(|decision| !decision.spec.is_empty())
        .map(|decision| format!("{}|{}|{}|{}", decision.id, decision.spec, decision.superseded_by, decision.choice)).collect();
    format!("{:x}", Sha256::digest(parts.join("\n").as_bytes()))
}

/// Task 156: the lead's message to a member that hands down a decision is
/// refused: `hgs team decide` records it and Zerus gives it to the work.
pub(crate) fn lead_send_check(target: &str, text: &str) -> anyhow::Result<()> {
    if !decision_like(text) {
        return Ok(());
    }
    let Some(record) = super::super::session_graph::caller_record() else { return Ok(()) };
    let Ok(store) = super::store::Store::open(false) else { return Ok(()) };
    for team in store.active_teams(1000).unwrap_or_default() {
        if super::commands::calling_member(&team, &record).as_deref() != Some(team.lead.as_str()) {
            continue;
        }
        if team.members.iter().any(|member| member.id != team.lead && member.session.as_ref().is_some_and(|session| session.name == target.trim())) {
            anyhow::bail!("{}", refusal(&team.id));
        }
    }
    Ok(())
}

/// The refusal a decision sent as a message gets.
pub(crate) fn refusal(team: &str) -> String {
    format!(
        "A decision of the owner is recorded, not messaged: {{\"request_id\":\"UUID\",\"team\":\"{team}\",\"question\":\"...\",\"choice\":\"...\",\"why\":\"...\",\
\"files\":[\"PATH\"],\"forbids\":[\"TERM\"],\"owner_word\":{{\"text\":\"the owner's words\"}}}} | hgs team decide --json. Zerus gives it to every task it binds."
    )
}

#[cfg(test)]
#[path = "tests/decisions.rs"]
mod tests;

