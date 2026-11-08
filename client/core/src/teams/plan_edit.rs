//! Task 196: editing the plan while the team plans (Qt's TeamPanelPlan.cpp:
//! `savePlan`, `link`, `editTask`). Every edit sends the whole plan once
//! (`hgs team plan`) at the revision the owner saw, so a lead's concurrent
//! change is refused, never overwritten. The plan is built from a fresh read
//! of the whole team (`hgs team status`), never from the view's bounded
//! summary, which has no descriptions, criteria or prompts (C4, #548): what it
//! sends back is every field `plan` takes, as the team holds it.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The fields of a member a plan carries (hgs's Member, minus its runtime).
const MEMBER_FIELDS: &[&str] = &[
    "id", "role", "agent", "account", "model", "effort", "permissions", "permissions_profile", "computer", "directory", "prompt",
    "iron_rules", "folder", "models", "context_before_task", "session",
];
/// The fields of a task a plan carries (hgs's Task, minus its runtime).
const TASK_FIELDS: &[&str] = &[
    "id", "phase", "title", "description", "output", "kind", "check", "criteria", "deps", "origin", "assignee", "pinned", "lane",
    "pipeline", "models", "role",
];
/// What an edit of a task may set (the dialog's fields).
const EDITABLE: &[&str] = &["title", "description", "criteria", "assignee", "phase", "output", "role", "pipeline", "lane", "estimate_minutes", "deps"];

/// One change of the plan, as the view asks for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "edit", rename_all = "snake_case")]
pub enum PlanChange {
    /// Adds (or with `add: false` removes) the dependency of `task` on
    /// `dependency`: Qt's drag from a node's edge, and an edge's delete.
    Link {
        dependency: u64,
        task: u64,
        add: bool,
        #[serde(default)]
        soft: bool,
    },
    /// A task's fields, or a new task (`id` 0 or absent): Qt's "+ Task" and
    /// the task dialog.
    Task { task: Value },
    /// Removes a task from the plan; the dependencies on it go with it.
    RemoveTask { task: u64 },
}

impl PlanChange {
    pub fn check(&self) -> Result<(), String> {
        match self {
            Self::Link { dependency, task, .. } => {
                if *dependency == 0 || *task == 0 {
                    return Err("Not a task number.".into());
                }
                if dependency == task {
                    return Err("A task never depends on itself.".into());
                }
                Ok(())
            }
            Self::Task { task } => {
                let fields = task.as_object().ok_or("A task is an object.")?;
                for key in fields.keys() {
                    if key != "id" && !EDITABLE.contains(&key.as_str()) {
                        return Err(format!("A task has no field \"{key}\" to edit."));
                    }
                }
                let new = task.get("id").and_then(Value::as_u64).unwrap_or(0) == 0;
                match task.get("title") {
                    Some(title) => {
                        let title = title.as_str().ok_or("A task title is text.")?;
                        if title.trim().is_empty() {
                            return Err("A task needs a title.".into());
                        }
                        if title.chars().count() > 200 {
                            return Err("A task title has at most 200 characters.".into());
                        }
                    }
                    None if new => return Err("A new task needs a title.".into()),
                    None => {}
                }
                Ok(())
            }
            Self::RemoveTask { task } => {
                if *task == 0 {
                    return Err("Not a task number.".into());
                }
                Ok(())
            }
        }
    }
}

fn pick(value: &Value, fields: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for field in fields {
        if let Some(found) = value.get(*field).filter(|found| !found.is_null()) {
            out.insert((*field).to_owned(), found.clone());
        }
    }
    out
}

/// The whole plan (`members`, `tasks`, `phases`) of the team `status` reads,
/// with `change` applied. Refused when the team no longer plans or the change
/// names a task the plan does not have.
pub fn plan_input(status: &Value, change: &PlanChange) -> Result<Map<String, Value>, String> {
    if status.get("status").and_then(Value::as_str) != Some("planning") {
        return Err("The team no longer plans; read it again.".into());
    }
    let lead = status.get("lead").and_then(Value::as_str).unwrap_or_default();
    let members: Vec<Value> = status
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|member| member.get("id").and_then(Value::as_str) != Some(lead))
        .map(|member| {
            let mut kept = pick(member, MEMBER_FIELDS);
            // An adopted session keeps its identity (F8), nothing else of it.
            if let Some(session) = member.get("session").filter(|session| session.is_object()) {
                kept.insert("session".into(), Value::Object(pick(session, &["host", "name", "run_id", "conversation_id"])));
            }
            Value::Object(kept)
        })
        .collect();
    let all: Vec<&Value> = status.get("tasks").and_then(Value::as_array).into_iter().flatten().collect();
    let mut tasks: Vec<Map<String, Value>> = all
        .iter()
        .filter(|task| task.get("status").and_then(Value::as_str) != Some("cancelled"))
        .map(|task| {
            let mut kept = pick(task, TASK_FIELDS);
            // C3 (#548): the phase it is planned in; hgs puts none in phase 1.
            let phase = task.get("phase").and_then(Value::as_u64).unwrap_or(1).max(1);
            kept.insert("phase".into(), json!(phase));
            // #746: only the lead's own estimate goes back in a plan.
            if task.get("estimate_source").and_then(Value::as_str) == Some("lead") {
                if let Some(minutes) = task.get("estimate_minutes").filter(|minutes| minutes.is_number()) {
                    kept.insert("estimate_minutes".into(), minutes.clone());
                }
            }
            kept
        })
        .collect();
    let find = |tasks: &[Map<String, Value>], id: u64| tasks.iter().position(|task| task.get("id").and_then(Value::as_u64) == Some(id));
    match change {
        PlanChange::Link { dependency, task, add, soft } => {
            let at = find(&tasks, *task).ok_or_else(|| format!("The plan has no task #{task}."))?;
            if *add && find(&tasks, *dependency).is_none() {
                return Err(format!("The plan has no task #{dependency}."));
            }
            let deps: Vec<Value> = tasks[at]
                .get("deps")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|dep| dep.get("task").and_then(Value::as_u64) != Some(*dependency))
                .cloned()
                .chain(add.then(|| json!({"task": dependency, "kind": if *soft { "soft" } else { "hard" }})))
                .collect();
            tasks[at].insert("deps".into(), Value::Array(deps));
        }
        PlanChange::Task { task } => {
            let fields = task.as_object().cloned().unwrap_or_default();
            let id = fields.get("id").and_then(Value::as_u64).unwrap_or(0);
            if id == 0 {
                // A new task takes the next free number of the whole team.
                let next = all.iter().filter_map(|task| task.get("id").and_then(Value::as_u64)).max().unwrap_or(0) + 1;
                let mut created = Map::new();
                created.insert("id".into(), json!(next));
                created.insert("phase".into(), json!(1));
                created.insert("description".into(), json!(""));
                created.insert("criteria".into(), json!([]));
                created.insert("deps".into(), json!([]));
                for (key, value) in fields.into_iter().filter(|(key, _)| key != "id") {
                    created.insert(key, value);
                }
                tasks.push(created);
            } else {
                let at = find(&tasks, id).ok_or_else(|| format!("The plan has no task #{id}."))?;
                for (key, value) in fields.into_iter().filter(|(key, _)| key != "id") {
                    tasks[at].insert(key, value);
                }
            }
        }
        PlanChange::RemoveTask { task } => {
            let at = find(&tasks, *task).ok_or_else(|| format!("The plan has no task #{task}."))?;
            tasks.remove(at);
            for other in &mut tasks {
                if let Some(deps) = other.get_mut("deps").and_then(Value::as_array_mut) {
                    deps.retain(|dep| dep.get("task").and_then(Value::as_u64) != Some(*task));
                }
            }
        }
    }
    // C3 (#548): the roadmap of later phases, in their order; `plan` replaces
    // the team's phases with what it is sent.
    let mut phases: Vec<Value> = status
        .get("phases")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|phase| phase.get("n").and_then(Value::as_u64).is_some_and(|n| n >= 2))
        .map(|phase| json!({"n": phase["n"], "title": phase.get("title").cloned().unwrap_or(json!("")), "barrier": phase.get("barrier").and_then(Value::as_bool).unwrap_or(false)}))
        .collect();
    phases.sort_by_key(|phase| phase["n"].as_u64().unwrap_or(0));
    let mut input = Map::new();
    input.insert("members".into(), Value::Array(members));
    input.insert("tasks".into(), Value::Array(tasks.into_iter().map(Value::Object).collect()));
    input.insert("phases".into(), Value::Array(phases));
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> Value {
        json!({
            "status": "planning", "lead": "lead", "revision": 7,
            "members": [
                {"id": "lead", "role": "Lead", "agent": "claude"},
                {"id": "dev", "role": "Backend", "agent": "codex", "model": "gpt-5", "prompt": "Build it.", "session": null,
                 "session_state": "cold", "queue": [], "iron_rules": ["No unsafe."]}
            ],
            "tasks": [
                {"id": 1, "title": "API", "description": "The REST API", "criteria": ["Tests pass"], "assignee": "dev", "deps": [],
                 "phase": 1, "output": ["src/api"], "status": "draft", "attempts": [], "ready": false, "estimate_minutes": 30, "estimate_source": "lead"},
                {"id": 2, "title": "UI", "description": "", "criteria": [], "assignee": "dev", "deps": [{"task": 1, "kind": "hard"}],
                 "phase": 2, "status": "draft", "estimate_minutes": 12, "estimate_source": "history"},
                {"id": 3, "title": "Old", "status": "cancelled", "phase": 1}
            ],
            "phases": [{"n": 3, "title": "Later", "barrier": true, "state": "draft"}, {"n": 2, "title": "UI", "barrier": false}]
        })
    }

    #[test]
    fn the_plan_keeps_every_field_plan_takes_and_drops_the_runtime() {
        let input = plan_input(&status(), &PlanChange::Link { dependency: 1, task: 2, add: false, soft: false }).unwrap();
        let members = input["members"].as_array().unwrap();
        assert_eq!(members.len(), 1, "the lead is never sent");
        assert_eq!(members[0], json!({"id": "dev", "role": "Backend", "agent": "codex", "model": "gpt-5", "prompt": "Build it.", "iron_rules": ["No unsafe."]}));
        let tasks = input["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 2, "a cancelled task is not planned again");
        assert_eq!(tasks[0], json!({"id": 1, "title": "API", "description": "The REST API", "criteria": ["Tests pass"], "assignee": "dev",
            "deps": [], "phase": 1, "output": ["src/api"], "estimate_minutes": 30}));
        assert!(tasks[1].get("estimate_minutes").is_none(), "a history estimate stays the history's");
        assert_eq!(tasks[1]["deps"], json!([]), "the dependency was removed");
        assert_eq!(input["phases"], json!([{"n": 2, "title": "UI", "barrier": false}, {"n": 3, "title": "Later", "barrier": true}]));
    }

    #[test]
    fn links_tasks_and_removals_change_only_what_they_name() {
        let linked = plan_input(&status(), &PlanChange::Link { dependency: 2, task: 1, add: true, soft: true }).unwrap();
        assert_eq!(linked["tasks"][0]["deps"], json!([{"task": 2, "kind": "soft"}]));
        assert!(plan_input(&status(), &PlanChange::Link { dependency: 9, task: 1, add: true, soft: false }).unwrap_err().contains("#9"));
        let added = plan_input(&status(), &PlanChange::Task { task: json!({"title": "Docs", "assignee": "dev", "phase": 2}) }).unwrap();
        let docs = &added["tasks"][2];
        assert_eq!((docs["id"].clone(), docs["title"].clone(), docs["phase"].clone()), (json!(4), json!("Docs"), json!(2)), "the next number after the cancelled #3");
        let edited = plan_input(&status(), &PlanChange::Task { task: json!({"id": 1, "title": "REST API", "criteria": ["Green"]}) }).unwrap();
        assert_eq!((edited["tasks"][0]["title"].clone(), edited["tasks"][0]["criteria"].clone()), (json!("REST API"), json!(["Green"])));
        assert_eq!(edited["tasks"][0]["description"], "The REST API", "an edit keeps the fields it does not name");
        let removed = plan_input(&status(), &PlanChange::RemoveTask { task: 1 }).unwrap();
        assert_eq!(removed["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(removed["tasks"][0]["deps"], json!([]), "the dependency on it goes too");
        let mut running = status();
        running["status"] = json!("running");
        assert!(plan_input(&running, &PlanChange::RemoveTask { task: 1 }).unwrap_err().contains("no longer plans"));
    }

    #[test]
    fn a_change_is_checked_before_anything_runs() {
        assert!(PlanChange::Link { dependency: 1, task: 1, add: true, soft: false }.check().is_err());
        assert!(PlanChange::Task { task: json!({"assignee": "dev"}) }.check().unwrap_err().contains("needs a title"));
        assert!(PlanChange::Task { task: json!({"id": 2, "status": "accepted"}) }.check().unwrap_err().contains("status"));
        assert!(PlanChange::Task { task: json!({"id": 2, "assignee": "qa"}) }.check().is_ok());
        assert!(PlanChange::RemoveTask { task: 0 }.check().is_err());
    }
}
