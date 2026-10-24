//! The team catalog — roles, templates and routing rules that New
//! team, Settings > Teams and the Team designer use. The catalog is versioned;
//! the designer only proposes changes into a draft, and a human keeps or
//! rejects each one and applies them as the next version.
use super::super::*;
use anyhow::{bail, ensure, Context};
use serde::{Deserialize, Serialize};
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
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Seat {
    pub role: String,
    #[serde(default = "one")]
    pub count: u32,
    /// The seat is filled only when the goal needs it (shown as 0–1).
    #[serde(default)]
    pub optional: bool,
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
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Template {
    pub id: String,
    pub name: String,
    #[serde(default)]
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
    #[serde(default)]
    pub max_members: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Rule {
    pub id: String,
    /// Words that, all present in the goal, pick this rule.
    pub when: String,
    pub template: String,
    #[serde(default)]
    pub approval: String,
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
    let seat = |role: &str, count: u32, optional: bool| Seat { role: role.into(), count, optional };
    let task = |title: &str, role: &str, deps: &[usize]| SkeletonTask { title: title.into(), role: role.into(), deps: deps.to_vec() };
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
    let routing = vec![Rule { id: "bug".into(), when: "bug fix".into(), template: "bugfix".into(), approval: "review".into() }];
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
    atomic(&path(), &serde_json::to_string_pretty(catalog)?).map_err(anyhow::Error::msg)
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
    }
    for rule in &catalog.routing {
        clean_id(&rule.id)?;
        ensure!(templates.contains_key(&rule.template), "Rule {} names unknown template {}", rule.id, rule.template);
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
pub(crate) fn route(catalog: &Catalog, goal: &str) -> Value {
    let goal = goal.to_lowercase();
    for rule in &catalog.routing {
        let words: Vec<String> = rule.when.to_lowercase().split_whitespace().map(str::to_owned).collect();
        if !words.is_empty() && words.iter().all(|w| goal.contains(w.as_str())) {
            if let Some(template) = catalog.templates.iter().find(|t| t.id == rule.template) {
                return json!({"rule":rule.id,"template":template,"approval":rule.approval});
            }
        }
    }
    let fallback = catalog.templates.first();
    json!({"rule":null,"template":fallback,"approval":fallback.map(|t| t.approval.clone())})
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
    fn the_builtin_catalog_is_valid_and_routes_bugs() {
        let catalog = builtin();
        validate(&catalog).unwrap();
        assert_eq!(route(&catalog, "Fix the login bug")["template"]["id"], "bugfix");
        assert_eq!(route(&catalog, "New checkout")["template"]["id"], "feature");
    }

    #[test]
    fn a_proposal_needs_no_id_of_its_own() {
        // hgs gives the id; the guide's example has none.
        let proposal: Proposal = serde_json::from_value(json!({"kind":"role","action":"remove","target":"tester"})).unwrap();
        assert!(proposal.id.is_empty());
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
}
