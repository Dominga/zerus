//! WhiteWh fork: the team catalog — roles, templates and routing rules that New
//! team, Settings > Teams and the Team designer use. The catalog is versioned;
//! the designer only proposes changes into a draft, and a human keeps or
//! rejects each one and applies them as the next version.
use super::super::*;
use super::model::Task;
use anyhow::{bail, ensure, Context};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

pub(crate) fn path() -> PathBuf {
    root().join("teams/catalog.json")
}
fn draft_path() -> PathBuf {
    root().join("teams/catalog-draft.json")
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Role {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub permissions: String,
    #[serde(default)]
    pub computer: String,
    /// worktree, shared or read_only.
    #[serde(default)]
    pub folder: String,
    /// lead or team.
    #[serde(default)]
    pub can_message: String,
    #[serde(default)]
    pub dangerous: bool,
    /// Section 9.8: this role may lead a sub-team.
    #[serde(default)]
    pub can_lead_subteam: bool,
    // U25: the editable role from the owner's designer.
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub machines: Vec<String>,
    /// lead, some or team.
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub message_to: Vec<String>,
    #[serde(default)]
    pub joins: String,
    #[serde(default)]
    pub danger: Value,
    #[serde(default)]
    pub protocol_extra: String,
    #[serde(default)]
    pub task_template: String,
    #[serde(default)]
    pub prompt_order: Vec<String>,
    #[serde(default)]
    pub prompt_off: Vec<String>,
    #[serde(default)]
    pub notes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Seat {
    pub role: String,
    #[serde(default = "one")]
    pub count: u32,
    /// The seat is filled only when the goal needs it (shown as 0–1).
    #[serde(default)]
    pub optional: bool,
    /// Section 9.8: the seat's member leads a sub-team from this template.
    #[serde(default)]
    pub subteam: String,
}
fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct SkeletonTask {
    pub title: String,
    pub role: String,
    /// Indexes of earlier skeleton tasks.
    #[serde(default)]
    pub deps: Vec<usize>,
    /// U25: the phase this skeleton task belongs to.
    #[serde(default = "one_phase")]
    pub phase: u32,
}
fn one_phase() -> u32 {
    1
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Template {
    pub id: String,
    pub name: String,
    #[serde(default, alias = "desc")]
    pub description: String,
    #[serde(default)]
    pub lead_protocol: String,
    #[serde(default)]
    pub roster: Vec<Seat>,
    #[serde(default)]
    pub tasks: Vec<SkeletonTask>,
    /// lead (the lead plans) or template (start from the skeleton).
    #[serde(default)]
    pub planning: String,
    /// review or auto.
    #[serde(default)]
    pub approval: String,
    /// "" (the plan's approval), review or auto: how new phases are approved.
    #[serde(default)]
    pub phase_approval: String,
    #[serde(default)]
    pub max_members: usize,
    /// U25: worktree, clone, same or cloud.
    #[serde(default)]
    pub work: String,
    /// U25: same, owner or lead (new phases).
    #[serde(default)]
    pub phases: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Rule {
    pub id: String,
    /// Words that, all present in the goal, pick this rule.
    pub when: String,
    pub template: String,
    #[serde(default)]
    pub approval: String,
    /// U25: a template id (stops the search) or "+role" (adds a member and
    /// keeps looking). Empty uses `template`.
    #[serde(default, rename = "use")]
    pub use_: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Catalog {
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub roles: Vec<Role>,
    #[serde(default)]
    pub templates: Vec<Template>,
    #[serde(default)]
    pub routing: Vec<Rule>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Decision {
    #[default]
    Proposed,
    Kept,
    Rejected,
}

/// F3: the designer's role corrections: each names its role, the change and
/// why, at most 20 and 500 characters each.
fn check_notes(proposal: &Proposal) -> anyhow::Result<()> {
    ensure!(proposal.notes.len() <= 20, "A proposal has at most 20 role notes");
    for note in &proposal.notes {
        ensure!(!note["role"].as_str().unwrap_or_default().trim().is_empty(), "A role note names its role");
        for field in ["change", "why"] {
            ensure!(
                note[field].as_str().unwrap_or_default().trim().chars().count() <= 500,
                "A role note {field} has at most 500 characters"
            );
        }
    }
    Ok(())
}

/// One change the designer proposes: a whole role, template or rule to add or
/// replace (by id), or an id to remove.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Proposal {
    /// Given by hgs when the proposal is made.
    #[serde(default)]
    pub id: String,
    /// role, template or rule.
    pub kind: String,
    /// add, change or remove.
    pub action: String,
    pub target: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub reason: String,
    /// F3: the designer's role corrections, [{role, change, why}], at most 20.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Value>,
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub state: Decision,
    #[serde(default)]
    pub at: f64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Draft {
    /// The catalog version the proposals were made against.
    #[serde(default)]
    pub base: u64,
    #[serde(default)]
    pub proposals: Vec<Proposal>,
}

fn role(id: &str, name: &str, color: &str, agent: &str, model: &str, folder: &str, prompt: &str) -> Role {
    Role {
        id: id.into(),
        name: name.into(),
        color: color.into(),
        prompt: prompt.into(),
        agent: agent.into(),
        model: model.into(),
        folder: folder.into(),
        can_message: "lead".into(),
        ..Default::default()
    }
}

/// The catalog before anyone changed it.
pub(crate) fn builtin() -> Catalog {
    let finish = "Finish every task with `hgs team report` (hgs guide team).";
    let roles = vec![
        role("lead", "Lead", "#b79cff", "claude", "", "shared",
            "You lead this team: plan tasks with dependencies, hand nothing out yourself, and accept or reject each report against its criteria."),
        role("architect", "Architect", "#8e9bff", "claude", "", "read_only", &format!("You are the Architect: design interfaces and boundaries; change code only when a task asks. {finish}")),
        role("backend", "Backend", "#6fa8ff", "codex", "", "worktree", &format!("You are the Backend engineer of this team. Work only inside your folder. Keep APIs backward compatible unless the task says otherwise. {finish}")),
        role("frontend", "Frontend", "#f08bb4", "codex", "", "worktree", &format!("You are the Frontend engineer of this team. Work only inside your folder. {finish}")),
        role("tester", "Tester", "#56c8e0", "claude", "", "worktree", &format!("You are the Tester of this team. Write and run tests; report what passed and what failed. {finish}")),
        role("reviewer", "Reviewer", "#d9b38c", "claude", "", "read_only", &format!("You are the Reviewer: read the changes, report findings by severity, change nothing. {finish}")),
        role("security", "Security", "#e48de0", "claude", "", "read_only", &format!("You are the Security reviewer: look for secrets, injection, authorization and data exposure. {finish}")),
        role("integrator", "Integrator", "#a3b1c6", "codex", "", "shared", &format!("You are the Integrator: merge the members' branches, resolve conflicts, run the full checks. {finish}")),
        role("researcher", "Researcher", "#b9d36b", "claude", "", "read_only", &format!("You are the Researcher: find facts and options with sources; change no code. {finish}")),
    ];
    let seat = |role: &str, count: u32, optional: bool| Seat { role: role.into(), count, optional, subteam: String::new() };
    let task = |title: &str, role: &str, deps: &[usize]| SkeletonTask { title: title.into(), role: role.into(), deps: deps.to_vec(), phase: 1 };
    let templates = vec![
        Template {
            id: "feature".into(),
            name: "Feature delivery".into(),
            description: "Design, build, test and review a feature.".into(),
            roster: vec![seat("backend", 1, false), seat("frontend", 1, true), seat("tester", 1, false), seat("reviewer", 1, false)],
            tasks: vec![
                task("Requirements and acceptance", "lead", &[]),
                task("Implementation", "backend", &[0]),
                task("Tests", "tester", &[1]),
                task("Code review", "reviewer", &[1, 2]),
            ],
            planning: "lead".into(),
            approval: "review".into(),
            max_members: 6,
            ..Default::default()
        },
        Template {
            id: "bugfix".into(),
            name: "Bugfix".into(),
            description: "Reproduce, fix and test one bug.".into(),
            roster: vec![seat("backend", 1, false), seat("tester", 1, false)],
            tasks: vec![task("Reproduce", "tester", &[]), task("Fix", "backend", &[0]), task("Test the fix", "tester", &[1])],
            planning: "template".into(),
            approval: "review".into(),
            max_members: 3,
            ..Default::default()
        },
    ];
    let routing = vec![Rule { id: "bug".into(), when: "bug fix".into(), template: "bugfix".into(), approval: "review".into(), use_: String::new() }];
    Catalog { version: 1, roles, templates, routing }
}

pub(crate) fn load() -> anyhow::Result<Catalog> {
    match fs::read_to_string(path()) {
        Ok(text) => serde_json::from_str(&text).context("Invalid team catalog"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(builtin()),
        Err(e) => Err(e.into()),
    }
}

fn load_draft() -> anyhow::Result<Draft> {
    match fs::read_to_string(draft_path()) {
        Ok(text) => serde_json::from_str(&text).context("Invalid catalog draft"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Draft::default()),
        Err(e) => Err(e.into()),
    }
}

fn save(catalog: &Catalog) -> anyhow::Result<()> {
    private_dir(&root().join("teams")).map_err(anyhow::Error::msg)?;
    // U25 Revert: keep the previous version before the new one lands.
    if path().exists() {
        let _ = fs::copy(path(), prev_path());
    }
    atomic(&path(), &serde_json::to_string_pretty(catalog)?).map_err(anyhow::Error::msg)
}

/// U25: the role's prompt blocks in the owner's order, skipping the ones turned
/// off. None when the role has no order (an old catalog keeps today's text).
pub(crate) fn prompt_order_text(role: &Role, member_prompt: &str, task: Option<&Task>, project_rules: &str) -> Option<String> {
    if role.prompt_order.is_empty() {
        return None;
    }
    let mut text = String::new();
    for key in &role.prompt_order {
        if role.prompt_off.contains(key) {
            continue;
        }
        let block = match key.as_str() {
            "rules" => (!project_rules.trim().is_empty()).then(|| format!("Project rules:\n{}\n", project_rules.trim())),
            "role" => (!member_prompt.trim().is_empty()).then(|| format!("Role: {}\n", member_prompt.trim())),
            "protocol" => (!role.protocol_extra.trim().is_empty()).then(|| format!("Team protocol extra:\n{}\n", role.protocol_extra.trim())),
            "task" => task.map(|task| {
                let criteria = task.criteria.join("; ");
                let rendered = role
                    .task_template
                    .replace("{id}", &task.id.to_string())
                    .replace("{title}", &task.title)
                    .replace("{description}", &task.description)
                    .replace("{criteria}", &criteria);
                format!("Task:\n{rendered}\n")
            }),
            note => role.notes.get(note).map(|value| format!("Note:\n{}\n", value.trim())),
        };
        if let Some(block) = block {
            text += &block;
        }
    }
    Some(text)
}

fn prev_path() -> PathBuf {
    root().join("teams/catalog.prev.json")
}

fn save_draft(draft: &Draft) -> anyhow::Result<()> {
    private_dir(&root().join("teams")).map_err(anyhow::Error::msg)?;
    if draft.proposals.is_empty() {
        let _ = fs::remove_file(draft_path());
        return Ok(());
    }
    atomic(&draft_path(), &serde_json::to_string_pretty(draft)?).map_err(anyhow::Error::msg)
}

fn clean_id(id: &str) -> anyhow::Result<String> {
    ensure!(
        !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
        "Catalog ids use letters, digits, - and _"
    );
    Ok(id.to_owned())
}

/// Checks a whole catalog: unique ids, known roles and templates.
pub(crate) fn validate(catalog: &Catalog) -> anyhow::Result<()> {
    let mut roles = BTreeMap::new();
    for role in &catalog.roles {
        clean_id(&role.id)?;
        ensure!(!role.name.trim().is_empty(), "Role {} needs a name", role.id);
        ensure!(roles.insert(role.id.clone(), ()).is_none(), "Role {} is listed twice", role.id);
        ensure!(role.prompt.len() <= 8000, "The prompt of role {} is too long", role.id);
        // U25: the role's editable prompt order and notes.
        ensure!(role.about.chars().count() <= 500, "The about of role {} is too long", role.id);
        ensure!(role.skills.len() <= 20, "Role {} has at most 20 skills", role.id);
        ensure!(matches!(role.message.as_str(), "" | "lead" | "some" | "team"), "Role {}: message is lead, some or team", role.id);
        for key in &role.prompt_off {
            ensure!(role.prompt_order.contains(key) || role.notes.contains_key(key), "Role {} turns off unknown block {key}", role.id);
        }
        for key in &role.prompt_order {
            ensure!(
                matches!(key.as_str(), "rules" | "role" | "protocol" | "task") || role.notes.contains_key(key),
                "Role {}: unknown prompt block {key}",
                role.id
            );
        }
    }
    let mut templates = BTreeMap::new();
    for template in &catalog.templates {
        clean_id(&template.id)?;
        ensure!(templates.insert(template.id.clone(), ()).is_none(), "Template {} is listed twice", template.id);
        for seat in &template.roster {
            ensure!(roles.contains_key(&seat.role), "Template {} names unknown role {}", template.name, seat.role);
        }
        for (i, task) in template.tasks.iter().enumerate() {
            ensure!(task.role == "lead" || roles.contains_key(&task.role), "Template {} task {} names unknown role {}", template.name, i + 1, task.role);
            ensure!(task.deps.iter().all(|d| *d < i), "Template {} task {} depends on a later task", template.name, i + 1);
        }
        ensure!(matches!(template.phase_approval.as_str(), "" | "review" | "auto"),
            "Template {}: phase approval is \"\", review or auto", template.id);
        ensure!(matches!(template.work.as_str(), "" | "worktree" | "clone" | "same" | "cloud"),
            "Template {}: work is worktree, clone, same or cloud", template.id);
        ensure!(matches!(template.phases.as_str(), "" | "same" | "owner" | "lead"),
            "Template {}: phases is same, owner or lead", template.id);
        for task in &template.tasks {
            ensure!(task.phase >= 1, "Template {}: a task phase starts at 1", template.id);
        }
    }
    for rule in &catalog.routing {
        clean_id(&rule.id)?;
        let use_ = if rule.use_.is_empty() { &rule.template } else { &rule.use_ };
        if let Some(role) = use_.strip_prefix('+') {
            ensure!(roles.contains_key(role), "Rule {} names unknown role {}", rule.id, role);
        } else {
            ensure!(templates.contains_key(use_), "Rule {} names unknown template {}", rule.id, use_);
        }
    }
    // Section 9.8: a seat may delegate to a template that exists, and no
    // template may lead back to itself.
    for template in &catalog.templates {
        for seat in template.roster.iter().filter(|seat| !seat.subteam.is_empty()) {
            ensure!(
                templates.contains_key(&seat.subteam),
                "Template {} names unknown sub-team template {}",
                template.id,
                seat.subteam
            );
        }
    }
    fn visit(catalog: &Catalog, id: &str, visiting: &mut BTreeSet<String>, done: &mut BTreeSet<String>) -> anyhow::Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        ensure!(visiting.insert(id.to_owned()), "Template {} has a sub-team cycle", id);
        if let Some(template) = catalog.templates.iter().find(|template| template.id == id) {
            for seat in template.roster.iter().filter(|seat| !seat.subteam.is_empty()) {
                visit(catalog, &seat.subteam, visiting, done)?;
            }
        }
        visiting.remove(id);
        done.insert(id.to_owned());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for template in &catalog.templates {
        visit(catalog, &template.id, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

fn apply_one(catalog: &mut Catalog, proposal: &Proposal) -> anyhow::Result<()> {
    fn upsert<T: Clone>(items: &mut Vec<T>, id: &str, value: Option<T>, get: impl Fn(&T) -> &str) {
        let at = items.iter().position(|item| get(item) == id);
        match (at, value) {
            (Some(i), Some(v)) => items[i] = v,
            (None, Some(v)) => items.push(v),
            (Some(i), None) => {
                items.remove(i);
            }
            (None, None) => {}
        }
    }
    let remove = proposal.action == "remove";
    match proposal.kind.as_str() {
        "role" => {
            let value: Option<Role> = if remove { None } else { Some(serde_json::from_value(proposal.value.clone()).context("Invalid role")?) };
            upsert(&mut catalog.roles, &proposal.target, value, |r| r.id.as_str());
        }
        "template" => {
            let value: Option<Template> = if remove { None } else { Some(serde_json::from_value(proposal.value.clone()).context("Invalid template")?) };
            upsert(&mut catalog.templates, &proposal.target, value, |t| t.id.as_str());
        }
        "rule" => {
            let value: Option<Rule> = if remove { None } else { Some(serde_json::from_value(proposal.value.clone()).context("Invalid rule")?) };
            upsert(&mut catalog.routing, &proposal.target, value, |r| r.id.as_str());
        }
        kind => bail!("Unknown catalog kind {kind}"),
    }
    Ok(())
}

/// The template a goal would use: the first rule whose words all appear.
/// N2: the prototype's routeMatch: an empty `when` never matches; "everything"
/// or "*" always matches; otherwise any word of 3+ letters (stop words out)
/// that appears in the goal matches.
fn route_match(when: &str, goal_words: &BTreeSet<String>) -> bool {
    const STOP: [&str; 16] = [
        "the", "and", "for", "with", "any", "all", "else", "when", "use", "to", "of", "in", "on", "a", "an", "is",
    ];
    let lowered = when.to_lowercase();
    let tokens: Vec<String> = lowered
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect();
    if tokens.iter().any(|token| token == "everything" || token == "*") {
        return true;
    }
    tokens
        .iter()
        .filter(|token| token.chars().count() >= 3 && !STOP.contains(&token.as_str()))
        .any(|token| goal_words.contains(token))
}

pub(crate) fn route(catalog: &Catalog, goal: &str) -> Value {
    let goal_words: BTreeSet<String> = goal
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .collect();
    let mut added: Vec<String> = Vec::new();
    for rule in &catalog.routing {
        if !route_match(&rule.when, &goal_words) {
            continue;
        }
        let use_ = if rule.use_.is_empty() { &rule.template } else { &rule.use_ };
        if let Some(role) = use_.strip_prefix('+') {
            if !added.iter().any(|found| found == role) {
                added.push(role.to_owned());
            }
            continue;
        }
        if let Some(template) = catalog.templates.iter().find(|t| t.id == *use_) {
            return json!({"rule":rule.id,"template":template,"approval":rule.approval,"added":added});
        }
    }
    let fallback = catalog.templates.first();
    json!({"rule":null,"template":fallback,"approval":fallback.map(|t| t.approval.clone()),"added":added})
}

fn locked<T>(f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    private_dir(&root().join("teams")).map_err(anyhow::Error::msg)?;
    let file = lock_file(&root().join("teams/catalog.lock")).map_err(anyhow::Error::msg)?;
    fs2::FileExt::lock_exclusive(&file)?;
    let result = f();
    let _ = fs2::FileExt::unlock(&file);
    result
}

/// The Team designer's first prompt: what it may do and how it proposes.
pub(crate) fn designer_brief(catalog: &Catalog, goal: &str, project: &str) -> String {
    let mut text = format!(
        "You are the Zerus Team designer. You configure the team catalog: roles, templates (which roles a kind of work \
         needs, with a task skeleton) and routing rules. Read `hgs guide designer` first. You may read the owner's \
         projects to learn how they work, but change no file: your only output is proposals with `hgs team catalog propose --json`. \
         The owner keeps or rejects each proposal and applies them; running teams keep their roster. The catalog is v{}: \
         read it with `echo '{{}}' | hgs team catalog get --json`. Ask the owner what they need, then propose.",
        catalog.version
    );
    // Section 9.8: the designer explains sub-leads: a role may lead one, and a
    // seat may delegate to another template.
    text += "\n\nA role with can_lead_subteam may lead a sub-team; a roster seat may name subteam: <template> so that member delegates that template. Unknown templates and template cycles are refused, so keep the catalog acyclic.";
    if !project.trim().is_empty() {
        text += &format!("\n\nThe owner works in the project folder {}.", project.trim());
    }
    if !goal.trim().is_empty() {
        text += &format!("\n\nThe owner asks: {}", goal.trim());
    }
    text
}

fn designer(input: &Value) -> anyhow::Result<Value> {
    // The catalog belongs to no project: the designer runs in its own folder,
    // so the desktop lists it outside the projects. The project it may read is
    // named in its brief.
    let project = input["folder"].as_str().unwrap_or_default();
    let home = super::super::root().join("team-designer");
    std::fs::create_dir_all(&home).context("Could not create the Team designer's folder")?;
    let folder = home.to_string_lossy().into_owned();
    let agent = input["agent"].as_str().filter(|a| !a.is_empty()).unwrap_or("claude");
    ensure!(["claude", "codex", "kimi", "dsh", "opencode"].contains(&agent), "Unknown agent {agent}");
    // A model or effort is one word, as a provider's command line takes it.
    let word = |key: &str| -> anyhow::Result<String> {
        let value = input[key].as_str().unwrap_or_default().trim().to_owned();
        ensure!(value.len() <= 120 && !value.contains(char::is_whitespace) && !value.starts_with('-'), "Invalid {key}");
        Ok(value)
    };
    let (model, effort) = (word("model")?, word("effort")?);
    let launch_id = uuid::Uuid::new_v4().to_string();
    let brief = designer_brief(&load()?, input["goal"].as_str().unwrap_or_default(), project);
    let mut args: Vec<String> = [agent, folder.as_str(), "--new", "-n", "team-designer", "--launch-id", launch_id.as_str()].map(String::from).to_vec();
    // As a new session from the desktop: a terminal agent runs detached and
    // takes the model on its own command line; OpenCode takes it from hgs.
    let terminal = agent != "dsh" && agent != "opencode";
    if terminal {
        args.push("-d".into());
    }
    if agent == "opencode" && !model.is_empty() {
        args.extend(["--model".into(), model.clone()]);
        if !effort.is_empty() {
            args.extend(["--effort".into(), effort.clone()]);
        }
    }
    args.push("--".into());
    if terminal {
        args.extend(super::dispatcher::agent_args(agent, &model, &effort));
    }
    args.push(brief);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = super::dispatcher::hgs("", &args, None, std::time::Duration::from_secs(120))?;
    ensure!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr).trim());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let name = stdout.lines().chain(String::from_utf8_lossy(&output.stderr).lines()).find_map(|line| line.trim().strip_prefix("hgs: started ").map(str::to_owned));
    Ok(json!({"status":"started","launch_id":launch_id,"name":name}))
}

/// `hgs team catalog get|set|propose|decide|apply|discard|route|designer --json`.
/// `human` is true when the caller said "by":"human" from no session.
pub(crate) fn run(op: &str, input: &Value, human: bool) -> anyhow::Result<Value> {
    let text = |key: &str| input[key].as_str().unwrap_or_default().to_owned();
    match op {
        "get" => {
            let catalog = load()?;
            let draft = load_draft()?;
            Ok(json!({"catalog":catalog,"draft":draft,"builtin":!path().exists()}))
        }
        "route" => Ok(route(&load()?, &text("goal"))),
        "set" => locked(|| {
            ensure!(human, "Only you change the catalog directly; a designer proposes");
            let current = load()?;
            let expected = input["version"].as_u64().context("The catalog version you edited is required")?;
            ensure!(expected == current.version, "The catalog changed (now v{}); refresh and try again", current.version);
            let mut catalog: Catalog = serde_json::from_value(input["catalog"].clone()).context("Invalid catalog")?;
            validate(&catalog)?;
            catalog.version = current.version + 1;
            save(&catalog)?;
            Ok(json!({"status":"saved","version":catalog.version}))
        }),
        "propose" => locked(|| {
            let current = load()?;
            let mut draft = load_draft()?;
            if draft.proposals.is_empty() {
                draft.base = current.version;
            }
            let mut added = Vec::new();
            for value in input["proposals"].as_array().context("A list of proposals is required")? {
                let mut proposal: Proposal = serde_json::from_value(value.clone()).context("Invalid proposal")?;
                ensure!(matches!(proposal.kind.as_str(), "role" | "template" | "rule"), "A proposal changes a role, template or rule");
                ensure!(matches!(proposal.action.as_str(), "add" | "change" | "remove"), "A proposal adds, changes or removes");
                proposal.target = clean_id(&proposal.target)?;
                // F3: the designer names each role correction with its reason.
                check_notes(&proposal)?;
                // A proposal must apply on its own to the current catalog.
                let mut trial = current.clone();
                apply_one(&mut trial, &proposal)?;
                proposal.id = uuid::Uuid::new_v4().to_string();
                proposal.state = Decision::Proposed;
                proposal.at = now();
                added.push(proposal.id.clone());
                draft.proposals.push(proposal);
            }
            ensure!(draft.proposals.len() <= 200, "The draft holds at most 200 proposals");
            save_draft(&draft)?;
            Ok(json!({"status":"proposed","ids":added,"base":draft.base}))
        }),
        "decide" => locked(|| {
            ensure!(human, "Only you keep or reject a proposal");
            let mut draft = load_draft()?;
            let decision = match text("decision").as_str() {
                "keep" => Decision::Kept,
                "reject" => Decision::Rejected,
                "undo" => Decision::Proposed,
                _ => bail!("The decision is keep, reject or undo"),
            };
            let id = text("proposal");
            let proposal = draft.proposals.iter_mut().find(|p| p.id == id).context("Unknown proposal")?;
            proposal.state = decision;
            save_draft(&draft)?;
            Ok(json!({"status":"decided","proposal":id}))
        }),
        "apply" => locked(|| {
            ensure!(human, "Only you apply the draft");
            let current = load()?;
            let draft = load_draft()?;
            ensure!(draft.base == current.version, "The catalog changed since this draft (v{} → v{}); discard it and ask again", draft.base, current.version);
            let kept: Vec<&Proposal> = draft.proposals.iter().filter(|p| p.state == Decision::Kept).collect();
            ensure!(!kept.is_empty(), "Keep at least one change to apply");
            let mut catalog = current.clone();
            for proposal in &kept {
                apply_one(&mut catalog, proposal)?;
            }
            validate(&catalog)?;
            catalog.version = current.version + 1;
            save(&catalog)?;
            save_draft(&Draft::default())?;
            Ok(json!({"status":"applied","version":catalog.version,"changes":kept.len()}))
        }),
        "export" => Ok(json!({"catalog":load()?})),
        "revert" => locked(|| {
            ensure!(human, "Only you revert the catalog");
            let current = load()?;
            let text = fs::read_to_string(prev_path()).context("There is no previous catalog version to revert to")?;
            let mut previous: Catalog = serde_json::from_str(&text).context("Invalid previous catalog")?;
            validate(&previous)?;
            previous.version = current.version + 1;
            save(&previous)?;
            Ok(json!({"status":"reverted","version":previous.version}))
        }),
        "import" => locked(|| {
            ensure!(human, "Only you import a catalog");
            let current = load()?;
            let expected = input["version"].as_u64().context("The catalog version you edited is required")?;
            ensure!(expected == current.version, "The catalog changed (now v{}); refresh and try again", current.version);
            let mut catalog: Catalog = serde_json::from_value(input["catalog"].clone()).context("Invalid catalog")?;
            validate(&catalog)?;
            catalog.version = current.version + 1;
            save(&catalog)?;
            Ok(json!({"status":"imported","version":catalog.version}))
        }),
        "designer" => {
            ensure!(human, "Only you start the Team designer");
            designer(input)
        }
        "discard" => locked(|| {
            ensure!(human, "Only you discard the draft");
            save_draft(&Draft::default())?;
            Ok(json!({"status":"discarded"}))
        }),
        _ => bail!("Unknown catalog operation {op}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_matches_like_the_prototype() {
        let mut catalog = builtin();
        catalog.routing = vec![
            Rule { id: "review".into(), when: "review · release".into(), template: "bugfix".into(), approval: String::new(), use_: String::new() },
            Rule { id: "all".into(), when: "everything else".into(), template: "feature".into(), approval: String::new(), use_: String::new() },
        ];
        validate(&catalog).unwrap();
        // Either word of "review · release" matches; the dot is not a token.
        assert_eq!(route(&catalog, "please review this")["template"]["id"], "bugfix");
        assert_eq!(route(&catalog, "make a release")["template"]["id"], "bugfix");
        // "everything else" always matches, so it is the catch-all, not the
        // first template fallback.
        assert_eq!(route(&catalog, "new checkout")["template"]["id"], "feature");
        // An empty when never matches.
        catalog.routing[0].when.clear();
        assert_eq!(route(&catalog, "new checkout")["template"]["id"], "feature");
    }

    #[test]
    fn the_builtin_catalog_is_valid_and_routes_bugs() {
        let catalog = builtin();
        validate(&catalog).unwrap();
        assert_eq!(route(&catalog, "Fix the login bug")["template"]["id"], "bugfix");
        assert_eq!(route(&catalog, "New checkout")["template"]["id"], "feature");
    }

    #[test]
    fn a_route_adds_a_role_and_the_next_template_rule_stops() {
        let mut catalog = builtin();
        catalog.routing.insert(0, Rule {
            id: "security".into(), when: "secure".into(), template: String::new(), approval: String::new(),
            use_: "+security".into(),
        });
        if !catalog.roles.iter().any(|role| role.id == "security") {
            catalog.roles.push(Role { id: "security".into(), name: "Security".into(), ..Default::default() });
        }
        validate(&catalog).unwrap();
        let found = route(&catalog, "secure login bug fix");
        assert_eq!(found["added"], json!(["security"]));
        assert_eq!(found["template"]["id"], "bugfix");
        // A rule naming an unknown role or template is refused.
        catalog.routing[0].use_ = "+nobody".into();
        assert!(validate(&catalog).unwrap_err().to_string().contains("unknown role"));
    }

    #[test]
    fn export_import_and_revert_move_versions() {
        static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        let result = (|| -> anyhow::Result<()> {
            let mut start = builtin();
            start.version = 1;
            save(&start)?;
            let exported = run("export", &json!({}), true)?;
            assert_eq!(exported["catalog"]["version"], 1);
            // A bad catalog is refused and changes nothing.
            let error = run("import", &json!({"version":1,"catalog":{"roles":[{"id":"x","name":"","prompt":""}]}}), true)
                .unwrap_err()
                .to_string();
            assert!(error.contains("needs a name"), "{error}");
            // A good one becomes v2.
            let mut imported = start.clone();
            imported.roles[0].name = "Imported Lead".into();
            let value = run("import", &json!({"version":1,"catalog":imported}), true)?;
            assert_eq!(value["version"], 2);
            assert_eq!(load()?.roles[0].name, "Imported Lead");
            // Revert brings the previous version back as v3.
            let value = run("revert", &json!({}), true)?;
            assert_eq!(value["version"], 3);
            assert_eq!(load()?.roles[0].name, "Lead");
            Ok(())
        })();
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
        result.unwrap();
    }

    #[test]
    fn the_prompt_order_sends_each_block_and_skips_the_off_ones() {
        let mut role = Role {
            id: "dev".into(), name: "Dev".into(), prompt: "Write the code".into(), protocol_extra: "Report each step".into(),
            prompt_order: vec!["rules".into(), "role".into(), "protocol".into(), "task".into(), "note1".into()],
            task_template: "Do #{id} {title}: {description}. Criteria: {criteria}".into(),
            ..Default::default()
        };
        role.notes.insert("note1".into(), "Read the docs first".into());
        let task = Task { id: 7, title: "API".into(), description: "The endpoint".into(), criteria: vec!["Fast".into(), "Safe".into()], ..Default::default() };
        let text = prompt_order_text(&role, &role.prompt, Some(&task), "Use tabs").unwrap();
        assert!(text.starts_with("Project rules:\nUse tabs\nRole: Write the code\n"), "{text}");
        assert!(text.contains("Team protocol extra:\nReport each step"), "{text}");
        assert!(text.contains("Do #7 API: The endpoint. Criteria: Fast; Safe"), "{text}");
        assert!(text.contains("Note:\nRead the docs first"), "{text}");
        // Off blocks are skipped; an old role (no order) keeps today's text.
        role.prompt_off = vec!["protocol".into(), "task".into()];
        let text = prompt_order_text(&role, &role.prompt, Some(&task), "").unwrap();
        assert!(!text.contains("Team protocol extra") && !text.contains("Do #7"), "{text}");
        role.prompt_order.clear();
        assert!(prompt_order_text(&role, &role.prompt, Some(&task), "").is_none());
    }

    #[test]
    fn an_old_catalog_still_loads_and_prompt_order_is_checked() {
        // A catalog from before U25: no new fields anywhere.
        let old: Catalog = serde_json::from_value(json!({
            "version": 3,
            "roles": [{"id":"dev","name":"Dev","prompt":"Work","agent":"codex"}],
            "templates": [{"id":"t","name":"T","description":"D","roster":[{"role":"dev"}],
                "tasks":[{"title":"A","role":"dev"}]}],
            "routing": [{"id":"r","when":"x","template":"t"}]
        })).unwrap();
        validate(&old).unwrap();
        assert!(old.roles[0].prompt_order.is_empty());
        assert!(old.templates[0].work.is_empty());
        // A prompt block must be a known block or a note, and prompt_off may not
        // turn off an unknown one.
        let mut bad = old.clone();
        bad.roles[0].prompt_order = vec!["unknown".into()];
        assert!(validate(&bad).unwrap_err().to_string().contains("unknown prompt block"));
        bad.roles[0].prompt_order = vec!["rules".into(), "note1".into()];
        bad.roles[0].notes.insert("note1".into(), "Read the docs first".into());
        bad.roles[0].prompt_off = vec!["rules".into()];
        validate(&bad).unwrap();
    }

    #[test]
    fn a_proposals_role_notes_are_bounded_and_explained() {
        let mut proposal = Proposal { kind: "role".into(), action: "add".into(), target: "security".into(), ..Default::default() };
        proposal.notes = vec![json!({"role":"backend","change":"merge the tests role","why":"the goal has no tests owner"})];
        check_notes(&proposal).unwrap();
        proposal.notes = (0..21).map(|_| json!({"role":"r","change":"c","why":"w"})).collect();
        assert_eq!(check_notes(&proposal).unwrap_err().to_string(), "A proposal has at most 20 role notes");
        proposal.notes = vec![json!({"role":"","change":"c","why":"w"})];
        assert!(check_notes(&proposal).unwrap_err().to_string().contains("names its role"));
        proposal.notes = vec![json!({"role":"r","change":"c","why":"x".repeat(501)})];
        assert!(check_notes(&proposal).unwrap_err().to_string().contains("A role note why"));
    }

    #[test]
    fn a_proposal_needs_no_id_of_its_own() {
        // hgs gives the id; the guide's example has none.
        let proposal: Proposal = serde_json::from_value(json!({"kind":"role","action":"remove","target":"tester"})).unwrap();
        assert!(proposal.id.is_empty());
    }

    #[test]
    fn a_template_carries_its_phase_approval() {
        let template: Template = serde_json::from_value(json!({"id":"feature","name":"Feature","phase_approval":"auto"})).unwrap();
        assert_eq!(template.phase_approval, "auto");
        let plain: Template = serde_json::from_value(json!({"id":"feature","name":"Feature"})).unwrap();
        assert_eq!(plain.phase_approval, "");
        let mut catalog = builtin();
        catalog.templates[0].phase_approval = "sometimes".into();
        assert!(validate(&catalog).is_err());
    }

    #[test]
    fn proposals_apply_in_order_and_keep_the_catalog_valid() {
        let mut catalog = builtin();
        let change = Proposal {
            kind: "role".into(),
            action: "change".into(),
            target: "tester".into(),
            value: serde_json::to_value(Role { id: "tester".into(), name: "Tester".into(), model: "Sonnet".into(), ..Default::default() }).unwrap(),
            ..Default::default()
        };
        apply_one(&mut catalog, &change).unwrap();
        assert_eq!(catalog.roles.iter().find(|r| r.id == "tester").unwrap().model, "Sonnet");
        let remove = Proposal { kind: "role".into(), action: "remove".into(), target: "tester".into(), ..Default::default() };
        apply_one(&mut catalog, &remove).unwrap();
        // Templates still name the tester: the result is refused.
        assert!(validate(&catalog).is_err());
    }

    #[test]
    fn a_sub_team_template_must_exist_and_may_not_cycle() {
        let mut catalog = builtin();
        let id = catalog.templates[0].id.clone();
        let role = catalog.templates[0].roster[0].role.clone();
        // A seat may name a template that exists.
        catalog.templates[0].roster[0].subteam = id.clone();
        assert!(validate(&catalog).is_err(), "a template leading to itself is a cycle");
        catalog.templates.push(Template { id: "sub".into(), name: "Sub".into(), roster: vec![Seat { role: role.clone(), subteam: id.clone(), ..Default::default() }], ..Default::default() });
        // A two-template cycle is refused.
        assert!(validate(&catalog).is_err());
        // A missing template is refused before any cycle check.
        catalog.templates[0].roster[0].subteam = "missing".into();
        let error = validate(&catalog).unwrap_err().to_string();
        assert!(error.contains("unknown sub-team template"), "{error}");
        // A valid delegation loads, and the role may say it leads sub-teams.
        catalog.templates[0].roster[0].subteam = "sub".into();
        catalog.templates.iter_mut().find(|template| template.id == "sub").unwrap().roster[0].subteam =
            String::new();
        assert!(validate(&catalog).is_ok());
        catalog.roles[0].can_lead_subteam = true;
        let value = serde_json::to_value(&catalog.roles[0]).unwrap();
        assert_eq!(value["can_lead_subteam"], true);
    }
}
