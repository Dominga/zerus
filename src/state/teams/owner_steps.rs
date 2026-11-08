//! Task 159 (owner, 2026-11-07): owner-only steps are planned for when the
//! owner is there, in one click.
//!
//! - A lead's card names the tasks that need it (`for_tasks`): those tasks are
//!   never handed out while the card is open, and a denied card holds them for
//!   the lead with the owner's reason.
//! - Cards wait for the owner's presence: while the desktop says the owner is
//!   away (locked, or idle past `AWAY_AFTER`), `hgs team waiting` marks them
//!   `notify:false`; once the owner is back they are one batch (`batch`), each
//!   with what waits on it.
//! - One owner rule, granted once by card: scratch teams a lead creates in a
//!   named folder start without a per-plan approval, with that folder trusted
//!   for the listed agents. Kept in Settings > Teams (the teams settings
//!   file), listed and revoked with `hgs team scratch-rules`.
use super::model::{Team, TeamStatus};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// The owner counts as away after this long without input (or locked).
pub(crate) const AWAY_AFTER: f64 = 600.0;
/// At most this many scratch rules are kept.
const RULES_MAX: usize = 20;
/// The agents a scratch rule may name.
pub(crate) const AGENTS: [&str; 5] = ["claude", "codex", "kimi", "dsh", "opencode"];
/// The agents whose start-up folder trust Zerus records from a rule.
pub(crate) const TRUSTED_AGENTS: [&str; 2] = ["claude", "codex"];

/// One owner rule for the lead's scratch teams.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct ScratchRule {
    pub id: String,
    pub folder: String,
    pub agents: Vec<String>,
    #[serde(default)]
    pub granted_at: f64,
    /// The team whose card (or owner request) granted it.
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub request: String,
}

/// The rule a request names, checked: an absolute folder and 1 to 5 known
/// agents.
pub(crate) fn rule_from(input: &Value) -> anyhow::Result<ScratchRule> {
    let folder = input["folder"].as_str().unwrap_or_default().trim().to_owned();
    anyhow::ensure!(
        !folder.is_empty() && std::path::Path::new(&folder).is_absolute() && folder.chars().count() <= 1000 && !folder.chars().any(char::is_control),
        "A scratch rule names its folder, an absolute path: {{\"kind\":\"scratch-rule\",\"folder\":\"D:/scratch\",\"agents\":[\"claude\",\"codex\"],\"reason\":\"...\"}}"
    );
    let mut agents: Vec<String> = input["agents"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    agents.sort();
    agents.dedup();
    anyhow::ensure!(!agents.is_empty() && agents.len() <= AGENTS.len(), "A scratch rule names 1 to {} agents", AGENTS.len());
    for agent in &agents {
        anyhow::ensure!(AGENTS.contains(&agent.as_str()), "{agent} is no agent; a scratch rule names {}", AGENTS.join(", "));
    }
    Ok(ScratchRule { folder, agents, ..ScratchRule::default() })
}

/// The owner's rules, from the teams settings file.
pub(crate) fn rules() -> Vec<ScratchRule> {
    serde_json::from_value(super::templates::read_settings()["scratch_rules"].clone()).unwrap_or_default()
}

/// Stores a granted rule; the same folder and agents keep their first grant.
pub(crate) fn add(mut rule: ScratchRule) -> anyhow::Result<ScratchRule> {
    let mut settings = super::templates::read_settings();
    let mut list: Vec<ScratchRule> = serde_json::from_value(settings["scratch_rules"].clone()).unwrap_or_default();
    if let Some(found) = list.iter().find(|found| same_folder(&found.folder, &rule.folder) && found.agents == rule.agents) {
        return Ok(found.clone());
    }
    anyhow::ensure!(list.len() < RULES_MAX, "You keep at most {RULES_MAX} scratch rules; revoke one first");
    if rule.granted_at == 0.0 {
        rule.granted_at = super::super::now();
    }
    let digest = Sha256::digest(format!("{}|{}|{}", rule.folder, rule.agents.join(","), rule.granted_at).as_bytes());
    rule.id = format!("s{}", digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect::<String>());
    list.push(rule.clone());
    settings["scratch_rules"] = json!(list);
    super::templates::write_settings(&settings)?;
    Ok(rule)
}

/// Task #228: one standing rule per team, at its scratch root. A new grant
/// replaces the team's earlier one, so a team never has two.
pub(crate) fn add_standing(mut rule: ScratchRule) -> anyhow::Result<ScratchRule> {
    anyhow::ensure!(!rule.team.is_empty(), "A standing scratch rule names its team");
    let mut settings = super::templates::read_settings();
    let mut list: Vec<ScratchRule> = serde_json::from_value(settings["scratch_rules"].clone()).unwrap_or_default();
    // The same grant is kept once, even when it names a wider folder or agents
    // (a re-grant changes nothing); a different grant replaces the team's rule.
    if let Some(found) = list.iter().find(|found| found.team == rule.team && same_folder(&found.folder, &rule.folder) && found.agents == rule.agents) {
        return Ok(found.clone());
    }
    list.retain(|found| found.team != rule.team);
    anyhow::ensure!(list.len() < RULES_MAX, "You keep at most {RULES_MAX} scratch rules; revoke one first");
    if rule.granted_at == 0.0 {
        rule.granted_at = super::super::now();
    }
    let digest = Sha256::digest(format!("{}|{}|{}|{}", rule.team, rule.folder, rule.agents.join(","), rule.granted_at).as_bytes());
    rule.id = format!("s{}", digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect::<String>());
    list.push(rule.clone());
    settings["scratch_rules"] = json!(list);
    super::templates::write_settings(&settings)?;
    Ok(rule)
}

/// Task #228: the standing rule of a team, when the owner granted one at this
/// exact root.
pub(crate) fn standing(team_id: &str, root: &str) -> Option<ScratchRule> {
    rules().into_iter().find(|rule| rule.team == team_id && same_folder(&rule.folder, root))
}

/// Revokes a rule; teams created under it keep what they were given.
pub(crate) fn revoke(id: &str) -> anyhow::Result<bool> {
    let mut settings = super::templates::read_settings();
    let mut list: Vec<ScratchRule> = serde_json::from_value(settings["scratch_rules"].clone()).unwrap_or_default();
    let before = list.len();
    list.retain(|rule| rule.id != id);
    if list.len() == before {
        return Ok(false);
    }
    settings["scratch_rules"] = json!(list);
    super::templates::write_settings(&settings)?;
    Ok(true)
}

fn normal(path: &str) -> String {
    let text = path.trim().replace('\\', "/");
    let text = text.trim_end_matches('/').to_owned();
    if cfg!(windows) { text.to_lowercase() } else { text }
}

fn same_folder(a: &str, b: &str) -> bool {
    normal(a) == normal(b)
}

/// Whether `folder` is the rule's folder or inside it.
pub(crate) fn inside(folder: &str, root: &str) -> bool {
    let (folder, root) = (normal(folder), normal(root));
    !root.is_empty() && (folder == root || folder.starts_with(&format!("{root}/")))
}

/// The first rule whose folder holds `folder`.
pub(crate) fn covering<'a>(rules: &'a [ScratchRule], folder: &str) -> Option<&'a ScratchRule> {
    rules.iter().find(|rule| !folder.is_empty() && inside(folder, &rule.folder))
}

/// Whether the owner is at the desktop: Some(false) when it is locked or idle
/// past `AWAY_AFTER`, None when no desktop reports (then nothing is held).
pub(crate) fn presence(listing: Option<&Value>) -> Option<bool> {
    let listing = listing?;
    if listing["locked"] == true {
        return Some(false);
    }
    Some(listing["idle_seconds"].as_f64().unwrap_or(0.0) < AWAY_AFTER)
}

/// This computer's presence, read from the desktop's sample.
pub(crate) fn owner_present() -> Option<bool> {
    let config = crate::config::Config::load().ok()?;
    presence(crate::presence::listing(&config.state, crate::presence::now()).as_ref())
}

/// The tasks each open card of the team holds back: task -> card ids.
pub(crate) fn waiting_on_cards(team: &Team) -> BTreeMap<u32, Vec<String>> {
    let mut waiting: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for card in team.proposals.iter().filter(|card| card["state"] == "open") {
        let id = card["id"].as_str().unwrap_or_default().to_owned();
        for task in card["for_tasks"].as_array().into_iter().flatten().filter_map(Value::as_u64) {
            waiting.entry(task as u32).or_default().push(id.clone());
        }
    }
    waiting
}

/// The `for_tasks` a card names, checked against the team.
pub(crate) fn for_tasks(team: &Team, input: &Value) -> anyhow::Result<Vec<u32>> {
    let Some(list) = input.get("for_tasks").filter(|value| !value.is_null()) else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| anyhow::anyhow!("\"for_tasks\" lists the task numbers that need this card"))?;
    anyhow::ensure!(list.len() <= 50, "A card holds back at most 50 tasks");
    let mut out = Vec::new();
    for value in list {
        let id = value.as_u64().and_then(|id| u32::try_from(id).ok()).ok_or_else(|| anyhow::anyhow!("\"for_tasks\" lists task numbers"))?;
        let task = team.task(id).ok_or_else(|| anyhow::anyhow!("Unknown task #{id}"))?;
        anyhow::ensure!(!task.status.finished(), "Task #{id} is already decided; it waits for nothing");
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

/// `hgs team waiting`'s cards, marked for the owner's presence: held
/// (`notify:false`) while away, else one batch of every card shown together,
/// each with the tasks that wait on it.
pub(crate) fn mark_cards(items: &mut [Value], teams: &[Team], present: Option<bool>) {
    let cards: Vec<usize> = items.iter().enumerate().filter(|(_, item)| item["kind"] == "owner_word_asked").map(|(index, _)| index).collect();
    let mut ids: Vec<String> = cards.iter().map(|index| items[*index]["id"].as_str().unwrap_or_default().to_owned()).collect();
    ids.sort();
    let batch = format!("cards-{}", Sha256::digest(ids.join("|").as_bytes()).iter().take(6).map(|byte| format!("{byte:02x}")).collect::<String>());
    for index in cards {
        let item = &mut items[index];
        let team = teams.iter().find(|team| item["team"] == team.id.as_str());
        let card = team.and_then(|team| team.proposals.iter().find(|card| item["proposal"] == card["id"]));
        if let (Some(team), Some(card)) = (team, card) {
            let waiting: Vec<Value> = card["for_tasks"].as_array().into_iter().flatten().filter_map(Value::as_u64)
                .filter_map(|id| team.task(id as u32).map(|task| json!({"task":task.id,"title":task.title})))
                .collect();
            if !waiting.is_empty() {
                let named: Vec<String> = waiting.iter().map(|task| format!("#{}", task["task"])).collect();
                let title = item["title"].as_str().unwrap_or_default().to_owned();
                item["title"] = json!(format!("{title} ({} wait for it)", named.join(", ")));
                item["waiting_tasks"] = json!(waiting);
            }
        }
        match present {
            Some(false) => {
                item["notify"] = json!(false);
                item["held_for_presence"] = json!(true);
            }
            _ => {
                item["notify"] = json!(true);
                item["batch"] = json!(batch);
                item["batch_size"] = json!(ids.len());
            }
        }
    }
}

/// The active teams a scratch rule could matter to are those a lead runs.
pub(crate) fn lead_creating(record: &Value, teams: &[Team]) -> bool {
    teams.iter().any(|team| matches!(team.status, TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused)
        && super::commands::calling_member(team, record).as_deref() == Some(team.lead.as_str()))
}

/// The agents of a rule Zerus may trust the folder for, of those it names.
pub(crate) fn trusted(rule: &ScratchRule) -> BTreeSet<&str> {
    rule.agents.iter().map(String::as_str).filter(|agent| TRUSTED_AGENTS.contains(agent)).collect()
}

#[cfg(test)]
#[path = "tests/owner_steps.rs"]
mod tests;
