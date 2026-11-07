//! Task 154 (owner, 2026-11-07, after phase 3 went badly): a member launches
//! with the rights its role needs, and a task never goes to a member that
//! cannot do it.
//!
//! - A profile (read, write, build, commit, network, operate) maps to each
//!   agent's own launch mode: Claude's permission mode, Codex's sandbox,
//!   Kimi's plan mode and DeepSeek's preset (`profile_selection`).
//! - A member's launch mode gives its capabilities (`capabilities`): what it
//!   can do from its own session. The real limit is the agent's sandbox: a
//!   Codex or DeepSeek session in a workspace sandbox runs at Low integrity,
//!   outside the session host, without network.
//! - A task's needs come from an explicit `needs` or from its words
//!   (`needs`); plan validation and the hand-out refuse a member that lacks
//!   one, naming who can do the task and which rights card fixes it.
use super::model::{Member, MemberState, Task, Team};
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// The profiles, narrowest first.
pub(crate) const PROFILES: [&str; 6] = ["read", "write", "build", "commit", "network", "operate"];

/// What a member's session can do: read and edit its folder, build and
/// commit through Zerus's templates, reach the network, launch or inspect
/// sessions, read terminals and run a team worker.
pub(crate) const CAPABILITIES: [&str; 8] = ["read", "edit", "build", "commit", "network", "sessions", "terminals", "worker"];

/// A profile's rank: a wider one needs the owner.
pub(crate) fn rank(profile: &str) -> u8 {
    match profile {
        "read" => 1,
        "build" => 3,
        "commit" => 4,
        "network" => 5,
        "operate" => 6,
        _ => 2, // write is the default
    }
}

/// The native selection (`hgs AGENT DIR --permissions ...`) a profile
/// launches an agent with; None when the profile keeps the agent's own
/// default (Claude's mode goes as `--permission-mode`, see `profile_mode`).
pub(crate) fn profile_selection(agent: &str, profile: &str) -> Option<String> {
    // No profile keeps the account's own mode, as before.
    let native = |value: Value| Some(format!("native:{value}"));
    match (agent, profile) {
        ("codex", "read") => native(json!({"approval":"on-request","sandbox":"read-only"})),
        ("codex", "write" | "build" | "commit") => native(json!({"approval":"on-request","sandbox":"workspace-write"})),
        ("codex", "network" | "operate") => native(json!({"approval":"on-request","sandbox":"danger-full-access"})),
        ("dsh", "read") => native(json!({"preset":"read-only"})),
        ("dsh", "write" | "build" | "commit") => native(json!({"preset":"workspace-write"})),
        ("dsh", "network" | "operate") => native(json!({"preset":"danger-full-access"})),
        ("kimi", "read") => native(json!({"plan":"on"})),
        _ => None,
    }
}

/// Claude's mode for a profile as its native selection, for a relaunch in
/// the conversation (a resume takes no agent flags).
fn claude_selection(profile: &str) -> Option<String> {
    let mode = match claude_mode(profile)? {
        "default" => "manual",
        mode => mode,
    };
    Some(format!("native:{}", json!({"mode":mode})))
}

/// Claude's permission mode for a profile (C3/B5), passed after `--`.
pub(crate) fn claude_mode(profile: &str) -> Option<&'static str> {
    match profile {
        "read" => Some("plan"),
        "write" => Some("default"),
        "build" | "commit" | "network" | "operate" => Some("acceptEdits"),
        _ => None,
    }
}

/// The native controls and values each agent's launch takes (the account
/// catalogs of `hgs account ls`); the launch revalidates against the fresh
/// catalog, this only refuses what no catalog offers.
fn controls(agent: &str) -> &'static [(&'static str, &'static [&'static str])] {
    match agent {
        "claude" => &[("mode", &["acceptEdits", "auto", "bypassPermissions", "manual", "dontAsk", "plan"])],
        "codex" => &[
            ("approval", &["on-request", "never", "untrusted", "on-failure"]),
            ("sandbox", &["read-only", "workspace-write", "danger-full-access"]),
            ("approve_for_me", &["on"]),
        ],
        "kimi" => &[("mode", &["yolo", "auto"]), ("plan", &["on"])],
        // DeepSeek also names presets of its own; any preset name is checked
        // against its catalog at launch.
        "dsh" => &[("preset", &[])],
        _ => &[],
    }
}

/// Checks a member's launch selection the way a solo launch takes it:
/// provider, bypass, auto (not for DeepSeek) or the agent's native
/// `native:{...}` selection. Answers the canonical form.
pub(crate) fn check_selection(agent: &str, selection: &str) -> anyhow::Result<String> {
    match selection {
        "" | "provider" => return Ok(selection.to_owned()),
        "bypass" => return Ok("bypass".into()),
        "auto" if agent == "dsh" => anyhow::bail!(
            "DeepSeek has no auto mode: use a native preset, such as native:{{\"preset\":\"workspace-write\"}}, or a permissions_profile"
        ),
        "auto" => return Ok("auto".into()),
        _ => {}
    }
    let body = selection
        .strip_prefix("native:")
        .ok_or_else(|| anyhow::anyhow!("\"{selection}\" is not a launch selection: use provider, bypass, auto or native:{{...}} (the agent's own selection)"))?;
    anyhow::ensure!(body.len() <= 1024, "The native selection is too long");
    let value: Value = serde_json::from_str(body).map_err(|_| anyhow::anyhow!("The native selection is not JSON"))?;
    let object = value.as_object().ok_or_else(|| anyhow::anyhow!("The native selection is a JSON object"))?;
    let known = controls(agent);
    anyhow::ensure!(!known.is_empty(), "{agent} takes no native selection; use provider or bypass");
    let mut canonical = serde_json::Map::new();
    for (control, chosen) in object {
        let options = known
            .iter()
            .find(|(name, _)| name == control)
            .map(|(_, options)| *options)
            .ok_or_else(|| {
                let names: Vec<&str> = known.iter().map(|(name, _)| *name).collect();
                anyhow::anyhow!("{agent} has no native control \"{control}\"; its controls are {}", names.join(", "))
            })?;
        let chosen = chosen.as_str().ok_or_else(|| anyhow::anyhow!("The native control \"{control}\" takes a text value"))?;
        anyhow::ensure!(
            options.is_empty() && !chosen.trim().is_empty() && chosen.len() <= 128 && !chosen.chars().any(char::is_control)
                || options.contains(&chosen),
            "{agent}'s \"{control}\" is one of {}, not \"{chosen}\"",
            options.join(", ")
        );
        canonical.insert(control.clone(), json!(chosen));
    }
    Ok(format!("native:{}", Value::Object(canonical)))
}

/// What a launch selection lets an agent's session reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    /// It only reads (a read-only sandbox or plan mode).
    Read,
    /// It edits and builds in its folder, in a sandbox (Low integrity, no
    /// network, outside the session host).
    Workspace,
    /// Nothing holds it back: no sandbox, or a full-access one.
    Full,
}

fn level(agent: &str, selection: &str) -> Level {
    if selection == "bypass" {
        return Level::Full;
    }
    let native: Value = selection
        .strip_prefix("native:")
        .and_then(|body| serde_json::from_str(body).ok())
        .unwrap_or_default();
    let control = |name: &str| native[name].as_str().unwrap_or_default().to_owned();
    match agent {
        // Codex's and DeepSeek's sandboxes; their default is the workspace one.
        "codex" | "dsh" => match control(if agent == "codex" { "sandbox" } else { "preset" }).as_str() {
            "read-only" => Level::Read,
            "danger-full-access" => Level::Full,
            _ => Level::Workspace,
        },
        "claude" if control("mode") == "plan" => Level::Read,
        "kimi" if control("plan") == "on" => Level::Read,
        // Claude, Kimi and OpenCode run in no sandbox of their own.
        _ => Level::Full,
    }
}

/// The selection a member launches with: its own, or its profile's, else
/// its account's mode ("provider").
pub(crate) fn launch_selection(member: &Member) -> String {
    if !member.permissions.is_empty() {
        return member.permissions.clone();
    }
    let profile = if member.agent == "claude" {
        claude_selection(&member.permissions_profile)
    } else {
        profile_selection(&member.agent, &member.permissions_profile)
    };
    profile.unwrap_or_else(|| "provider".into())
}

/// What the member's session can do.
pub(crate) fn capabilities(member: &Member) -> BTreeSet<&'static str> {
    match level(&member.agent, &launch_selection(member)) {
        Level::Read => ["read"].into_iter().collect(),
        Level::Workspace => ["read", "edit", "build", "commit"].into_iter().collect(),
        Level::Full => CAPABILITIES.into_iter().collect(),
    }
}

/// Words that say a task launches, inspects or drives sessions, reads
/// terminals or runs a worker. Only these needs are read from a task's words;
/// any other need is named in `needs`.
fn derived(task: &Task) -> BTreeSet<&'static str> {
    let text = format!("{}\n{}\n{}", task.title, task.description, task.criteria.join("\n")).to_lowercase();
    let mut needs = BTreeSet::new();
    let any = |words: &[&str]| words.iter().any(|word| text.contains(word));
    if any(&["scratch session", "launch scratch", "launches scratch", "launch sessions", "launches sessions", "launch a session", "start scratch sessions", "hgs team look"]) {
        needs.insert("sessions");
    }
    if any(&["hgs terminal", "terminal snapshot", "read terminals", "reads terminals", "read the terminal", "reads the terminal"]) {
        needs.insert("terminals");
    }
    if any(&["team worker", "start a worker", "starts a worker", "run a worker", "runs a worker", "own hgs_state_dir", "as its worker"]) {
        needs.insert("worker");
    }
    needs
}

/// What the task needs: its explicit `needs` (an empty list says none), else
/// what its words ask.
pub(crate) fn needs(task: &Task) -> BTreeSet<String> {
    match &task.needs {
        Some(needs) => needs.iter().cloned().collect(),
        None => derived(task).into_iter().map(str::to_owned).collect(),
    }
}

/// Checks a task's explicit `needs`.
pub(crate) fn check_needs(needs: &[String]) -> anyhow::Result<()> {
    anyhow::ensure!(needs.len() <= CAPABILITIES.len(), "A task names at most {} needs", CAPABILITIES.len());
    for need in needs {
        anyhow::ensure!(
            CAPABILITIES.contains(&need.as_str()),
            "A task's need is one of {}, not \"{need}\"",
            CAPABILITIES.join(", ")
        );
    }
    Ok(())
}

/// The needs of `task` that `member` lacks.
pub(crate) fn missing(task: &Task, member: &Member) -> Vec<String> {
    let has = capabilities(member);
    needs(task).into_iter().filter(|need| !has.contains(need.as_str())).collect()
}

/// The narrowest profile with which `agent` has every need; None when none.
pub(crate) fn profile_for(agent: &str, needs: &BTreeSet<String>) -> Option<&'static str> {
    // Sessions, terminals and a worker are what operate is for, whatever
    // narrower profile happens to reach them on this agent.
    let operating = needs.iter().any(|need| matches!(need.as_str(), "sessions" | "terminals" | "worker"));
    PROFILES.into_iter().filter(|profile| !operating || *profile == "operate").find(|profile| {
        let probe = Member { agent: agent.into(), permissions_profile: (*profile).into(), ..Default::default() };
        let has = capabilities(&probe);
        needs.iter().all(|need| has.contains(need.as_str()))
    })
}

/// Why `member` cannot do `task`, with the fix: the members who can, and the
/// rights card that lets this one. None when it can.
pub(crate) fn refusal(team: &Team, task: &Task, member: &Member) -> Option<String> {
    let lacks = missing(task, member);
    if lacks.is_empty() {
        return None;
    }
    let all = needs(task);
    let can: Vec<String> = team
        .members
        .iter()
        .filter(|other| other.id != member.id && other.state != MemberState::Removed)
        .filter(|other| missing(task, other).is_empty())
        .map(|other| team.name_of(&other.id))
        .collect();
    let mut text = format!(
        "#{} {} needs {}; {} ({}, {}) cannot: its launch lacks {}.",
        task.id,
        task.title,
        all.iter().cloned().collect::<Vec<_>>().join(", "),
        team.name_of(&member.id),
        member.agent,
        shown(member),
        lacks.join(", ")
    );
    if !can.is_empty() {
        text += &format!(" These members can: {}; reassign it (hgs team reassign).", can.join(", "));
    }
    if let Some(profile) = profile_for(&member.agent, &all) {
        text += &format!(
            " Or widen {}'s rights, which the owner allows: {} | hgs team propose --json; Zerus relaunches it in its conversation.",
            member.id,
            json!({"kind":"rights","member":member.id,"permissions_profile":profile,"reason":format!("#{} needs {}", task.id, lacks.join(", "))})
        );
    }
    Some(text)
}

/// How a member's rights read: its profile and the selection it launches with.
pub(crate) fn shown(member: &Member) -> String {
    let profile = if member.permissions_profile.is_empty() { "account" } else { &member.permissions_profile };
    format!("{profile}, {}", launch_selection(member))
}

/// The member's effective rights, for status and the New team form.
pub(crate) fn view(member: &Member) -> Value {
    json!({"profile":member.permissions_profile,
        "selection":launch_selection(member),"capabilities":capabilities(member)})
}

/// Task 154 (4): the rights a role's job needs, when its catalog entry sets
/// none: QA launches scratch sessions, reads terminals and runs a worker; a
/// merger commits. Other roles keep their account's mode.
pub(crate) fn role_profile(role_id: &str, role_name: &str) -> Option<&'static str> {
    let words = format!("{} {}", role_id.to_lowercase(), role_name.to_lowercase());
    let has = |word: &str| words.split(|ch: char| !ch.is_alphanumeric()).any(|part| part == word);
    if has("qa") {
        Some("operate")
    } else if has("merger") || has("merge") {
        Some("commit")
    } else {
        None
    }
}

#[cfg(test)]
#[path = "tests/rights.rs"]
mod tests;
