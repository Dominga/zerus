//! The team catalog's field editors (task #44, gap 8 of task #14), as Qt's
//! `TeamCatalogEditor` edits it: roles, templates, routing rules, pipelines
//! and the planning rules, with Import, Export, Revert and Save catalog.
//!
//! The editor keeps the saved catalog as hgs returned it and a working copy
//! the owner edits. The webview never parses a catalog: it renders the
//! [`schema`] and the [`CatalogEditorView`] and sends typed [`CatalogEdit`]s,
//! which are checked here against the schema before they change anything.
//! Every field the schema does not name (and everything a newer hgs adds)
//! stays as it was, so Save never drops what this client does not show.
//!
//! As in Qt:
//! - Save writes the working copy as the next version (`team catalog set` with
//!   the version it was read at, [`super::TeamAction::CatalogSet`]); a newer
//!   saved version refuses it.
//! - Revert drops the unsaved edits.
//! - Export writes the working copy; Import replaces the working copy with a
//!   file's catalog (an export, or a `catalog get` reply), unsaved until Save.
//! - A newer saved catalog under unsaved edits keeps each edited item and takes
//!   every other item from the newer one (Qt's rebase).
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// The largest catalog file Import reads.
pub const IMPORT_BYTES: usize = 1024 * 1024;

/// One kind of catalog item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Role,
    Template,
    Rule,
    Pipeline,
}

impl Kind {
    /// The catalog's list of this kind.
    pub fn list(self) -> &'static str {
        match self {
            Self::Role => "roles",
            Self::Template => "templates",
            Self::Rule => "routing",
            Self::Pipeline => "pipelines",
        }
    }
}

/// How the webview edits a field.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Multiline,
    Number { min: u64, max: u64 },
    Bool,
    /// One of these values; each with its label.
    Choice { options: Vec<(String, String)> },
    /// A list of short words (skills, computers, globs, block names).
    Words,
    /// A list of rows, each with these fields (roster, tasks, stages).
    List { fields: Vec<FieldSpec> },
}

/// One editable field of a kind.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldSpec {
    pub name: String,
    /// Qt's label, translated by the webview.
    pub label: String,
    #[serde(flatten)]
    pub ty: FieldType,
    /// Qt's hint under the field, if any.
    pub hint: String,
}

fn field(name: &str, label: &str, ty: FieldType, hint: &str) -> FieldSpec {
    FieldSpec { name: name.into(), label: label.into(), ty, hint: hint.into() }
}

fn choice(options: &[(&str, &str)]) -> FieldType {
    FieldType::Choice { options: options.iter().map(|(value, label)| ((*value).into(), (*label).into())).collect() }
}

const AGENTS: &[(&str, &str)] =
    &[("claude", "Claude"), ("codex", "Codex"), ("kimi", "Kimi"), ("dsh", "DeepSeek"), ("opencode", "OpenCode")];

/// The fields Qt's editors show for each kind, in Qt's order and words.
pub fn schema(kind: Kind) -> Vec<FieldSpec> {
    match kind {
        Kind::Role => vec![
            field("name", "Name", FieldType::Text, ""),
            field("about", "About", FieldType::Text, "What this role does, in a few words"),
            field("icon", "Icon", FieldType::Text, ""),
            field("color", "Color", FieldType::Text, ""),
            field("skills", "Specialization", FieldType::Words, "+ skill, Enter"),
            field("agent", "Default agent", choice(AGENTS), ""),
            field("account", "Account", FieldType::Text, ""),
            field("model", "Default model", FieldType::Text, "Only the default model. A task that names other models goes to a member that allows one"),
            field("effort", "Effort", FieldType::Text, ""),
            field(
                "models",
                "Models",
                FieldType::List { fields: vec![field("model", "Model", FieldType::Text, ""), field("effort", "Effort", FieldType::Text, "")] },
                "Before a task that names one of these, Zerus switches the member to it and waits for the switch",
            ),
            field("machines", "Computer", FieldType::Words, "Only these computers; the first free one wins"),
            // Qt's one choice over `permissions` and `permissions_profile`.
            field(
                "permission_mode",
                "Permissions",
                choice(&[
                    ("ask", "Ask every time"),
                    ("auto", "Auto (workspace)"),
                    ("account", "Account’s mode"),
                    ("read_only", "Read only"),
                    ("bypass", "Bypass (dangerous)"),
                ]),
                "",
            ),
            field("folder", "Folder policy", choice(&[("worktree", "Own worktree"), ("shared", "Shared folder"), ("read_only", "Read only")]), ""),
            field("message", "Can message", choice(&[("lead", "Lead only"), ("some", "Lead + selected roles"), ("team", "Whole team")]), ""),
            field("message_to", "Selected roles", FieldType::Words, ""),
            field("joins", "Joins when", FieldType::Text, "file globs, e.g. services/payments/** — empty: only when a template asks"),
            field("dangerous", "Dangerous actions", FieldType::Bool, "Runs every tool without asking"),
            field("can_lead_subteam", "Sub-lead", FieldType::Bool, ""),
            field(
                "context_before_task",
                "Context before task",
                choice(&[("none", "Keep"), ("compact", "Compact"), ("clear", "Clear")]),
                "Each task continues the member's conversation",
            ),
            field("pipeline", "Pipeline", FieldType::Text, ""),
            field("prompt", "Prompt", FieldType::Multiline, ""),
            field("protocol_extra", "Team protocol", FieldType::Multiline, "Extra lines for this role on top of the template’s Lead protocol (optional)"),
            field("task_template", "Task (per attempt)", FieldType::Multiline, "{id} {title} {description} {criteria} are filled per attempt"),
            field("prompt_order", "Prompt order", FieldType::Words, "Blocks are sent top to bottom; switched-off blocks are skipped."),
            field("prompt_off", "Switched off", FieldType::Words, ""),
        ],
        Kind::Template => vec![
            field("name", "Name", FieldType::Text, ""),
            field("description", "Description", FieldType::Text, "When to use this template"),
            field("lead_protocol", "Lead protocol", FieldType::Multiline, "Extra instructions for the coordinator of this kind of team"),
            field(
                "roster",
                "Roster",
                FieldType::List {
                    fields: vec![
                        field("role", "Role", FieldType::Text, ""),
                        field("count", "Members", FieldType::Number { min: 0, max: 20 }, ""),
                        field("optional", "only if needed", FieldType::Bool, "The lead adds this member only when the plan needs it"),
                    ],
                },
                "",
            ),
            field(
                "tasks",
                "Task skeleton",
                FieldType::List {
                    fields: vec![
                        field("title", "Task", FieldType::Text, ""),
                        field("role", "Role", FieldType::Text, ""),
                        field("deps", "After #", FieldType::Words, ""),
                        field("phase", "Phase", FieldType::Number { min: 1, max: 20 }, ""),
                    ],
                },
                "No tasks: the lead plans",
            ),
            field("planning", "Planning", choice(&[("lead", "Lead plans"), ("template", "Use template tasks")]), ""),
            field("approval", "Approval", choice(&[("review", "Review plan first"), ("auto", "Run when the plan is valid")]), ""),
            // Qt's one choice over `phase_approval` and `phases`.
            field("phase_approval", "New phases", choice(&[("", "Same as plan"), ("review", "Owner approves"), ("auto", "Lead runs")]), ""),
            field("max_members", "Max members", FieldType::Number { min: 1, max: 30 }, ""),
            field(
                "work",
                "Work in",
                choice(&[
                    ("worktree", "Worktree per member"),
                    ("clone", "Own clone"),
                    ("same", "Same folder"),
                    ("cloud", "Cloud sandbox"),
                ]),
                "",
            ),
            field("pipeline", "Pipeline", FieldType::Text, ""),
        ],
        Kind::Rule => vec![
            field("when", "When the goal / issue has", FieldType::Text, ""),
            field("template", "Template", FieldType::Text, ""),
            field("use", "Use", FieldType::Text, "A template id, or +role to add a member and keep looking"),
            field("approval", "Approval", choice(&[("", "Template default"), ("review", "Review plan first"), ("auto", "Run when the plan is valid")]), ""),
        ],
        Kind::Pipeline => vec![
            field("name", "Name", FieldType::Text, ""),
            field(
                "stages",
                "Stages",
                FieldType::List {
                    fields: vec![
                        field("kind", "Kind", choice(&[("review", "Review"), ("test", "Test"), ("command", "Command"), ("land", "Land")]), ""),
                        field("role", "Who takes it", FieldType::Text, ""),
                        field("member", "Pinned member…", FieldType::Text, ""),
                        field("command", "Command", FieldType::Text, "Zerus runs it, with no model"),
                    ],
                },
                "A pipeline has at most 8 stages",
            ),
            field("max_rounds", "After the rounds", FieldType::Number { min: 0, max: 10 }, ""),
            field("folder", "Works in", choice(&[("hold", "Task's folder"), ("checkout", "Detached checkout")]), ""),
            field("accept", "Final acceptance", choice(&[("lead", "Lead"), ("sub_lead", "Sub-lead"), ("auto", "Auto")]), ""),
        ],
    }
}

/// One edit the owner makes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum CatalogEdit {
    /// One field of one item.
    Set { kind: Kind, id: String, field: String, value: Value },
    /// A new item with Qt's defaults, named "New role" (and so on).
    Add { kind: Kind },
    /// A copy of an item under a new id.
    Duplicate { kind: Kind, id: String },
    Remove { kind: Kind, id: String },
    /// One step up (`-1`) or down (`1`) in its list: routing rules run in order.
    Move { kind: Kind, id: String, by: i64 },
    /// The lead's planning rules.
    PlanningRules { text: String },
}

/// What the editors show: the working copy's items with every schema field,
/// the version, and how many items differ from the saved catalog.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct CatalogEditorView {
    pub version: u64,
    pub builtin: bool,
    pub roles: Vec<Value>,
    pub templates: Vec<Value>,
    pub routing: Vec<Value>,
    pub pipelines: Vec<Value>,
    pub planning_rules: String,
    /// The changed items and fields, as Qt counts "unsaved changes".
    pub unsaved: usize,
    /// The ids edited since the last load, by kind ("roles": [...]).
    pub edited: Map<String, Value>,
}

/// What a deletion takes with it ([`CatalogEditor::remove_impact`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct RemoveImpact {
    pub name: String,
    /// A role: the templates that lose its seats. A pipeline: the templates
    /// and roles that lose it as their default.
    pub templates: Vec<String>,
    /// A role: the pipelines that lose its stages.
    pub pipelines: Vec<String>,
    /// A role: the pipelines left with no stage.
    pub emptied: Vec<String>,
}

/// The working copy and the saved catalog it started from.
#[derive(Clone, Debug, Default)]
pub struct CatalogEditor {
    saved: Value,
    working: Value,
    builtin: bool,
    edited: BTreeSet<(Kind, String)>,
    rules_edited: bool,
}

const LIST_ITEMS: usize = 200;
const TEXT_CHARS: usize = 20_000;

impl CatalogEditor {
    /// The editor of a `team catalog get` reply (`{"catalog":..,"draft":..,
    /// "builtin":..}`), or of a newer one under unsaved edits (Qt's rebase).
    pub fn load(&mut self, reply: &Value) -> Result<(), String> {
        let catalog = reply.get("catalog").filter(|catalog| catalog.is_object()).ok_or("team catalog gives no catalog")?.clone();
        shape(&catalog)?;
        self.builtin = reply.get("builtin").and_then(Value::as_bool).unwrap_or(false);
        if self.edited.is_empty() && !self.rules_edited {
            self.saved = catalog.clone();
            self.working = catalog;
            return Ok(());
        }
        // Rebase: the edited items keep the edit, every other item is the
        // newer catalog's; an item deleted here stays deleted.
        let mut rebased = catalog.clone();
        for kind in [Kind::Role, Kind::Template, Kind::Rule, Kind::Pipeline] {
            let list = kind.list();
            let mine = items(&self.working, list);
            let mut out: Vec<Value> = Vec::new();
            for item in items(&catalog, list) {
                let id = string(&item, "id");
                if self.edited.contains(&(kind, id.clone())) {
                    if let Some(edit) = mine.iter().find(|entry| string(entry, "id") == id) {
                        out.push(edit.clone());
                    }
                } else {
                    out.push(item);
                }
            }
            for item in mine {
                let id = string(&item, "id");
                if self.edited.contains(&(kind, id.clone())) && !out.iter().any(|entry| string(entry, "id") == id) {
                    out.push(item);
                }
            }
            rebased[list] = Value::Array(out);
        }
        if self.rules_edited {
            rebased["planning_rules"] = self.working["planning_rules"].clone();
        }
        self.saved = catalog;
        self.working = rebased;
        Ok(())
    }

    /// The version Save writes over.
    pub fn version(&self) -> u64 {
        self.saved.get("version").and_then(Value::as_u64).unwrap_or(0)
    }

    /// The working copy, as Save and Export send it.
    pub fn working(&self) -> &Value {
        &self.working
    }

    pub fn is_dirty(&self) -> bool {
        self.working != self.saved
    }

    /// Apply one edit, checked against the schema; nothing changes on an error.
    pub fn edit(&mut self, edit: &CatalogEdit) -> Result<(), String> {
        let mut next = self.working.clone();
        match edit {
            CatalogEdit::Set { kind, id, field: name, value } => {
                let spec = schema(*kind).into_iter().find(|spec| spec.name == *name).ok_or(format!("{name} is not a field of a {kind:?}"))?;
                let checked = checked(&spec, value)?;
                let index = position(&next, *kind, id)?;
                let item = &mut next[kind.list()][index];
                if !derived_set(*kind, name, &checked, item) {
                    let previous = item.get(name.as_str()).cloned();
                    item[name.as_str()] = merge_list(name, checked, previous.as_ref());
                }
                self.edited.insert((*kind, id.clone()));
            }
            CatalogEdit::Add { kind } => {
                let list = next[kind.list()].as_array().map(Vec::len).unwrap_or(0);
                if list >= LIST_ITEMS {
                    return Err(format!("The catalog keeps at most {LIST_ITEMS} {}", kind.list()));
                }
                let item = fresh(&next, *kind);
                self.edited.insert((*kind, string(&item, "id")));
                push(&mut next, *kind, item);
            }
            CatalogEdit::Duplicate { kind, id } => {
                let index = position(&next, *kind, id)?;
                let mut copy = next[kind.list()][index].clone();
                let new_id = unique(&next, *kind, &format!("{id}-copy"));
                copy["id"] = json!(new_id);
                if let Some(name) = copy.get("name").and_then(Value::as_str).map(str::to_owned) {
                    copy["name"] = json!(format!("{name} copy"));
                }
                self.edited.insert((*kind, new_id));
                push(&mut next, *kind, copy);
            }
            CatalogEdit::Remove { kind, id } => {
                let index = position(&next, *kind, id)?;
                next[kind.list()].as_array_mut().map(|list| list.remove(index));
                for (other, changed) in cascade(&mut next, *kind, id) {
                    self.edited.insert((other, changed));
                }
                self.edited.insert((*kind, id.clone()));
            }
            CatalogEdit::Move { kind, id, by } => {
                let index = position(&next, *kind, id)?;
                let list = next[kind.list()].as_array_mut().ok_or("no list")?;
                let target = index as i64 + by.signum();
                if target < 0 || target as usize >= list.len() {
                    return Ok(());
                }
                list.swap(index, target as usize);
                self.edited.insert((*kind, id.clone()));
            }
            CatalogEdit::PlanningRules { text } => {
                if text.chars().count() > TEXT_CHARS {
                    return Err("The planning rules are too long".into());
                }
                next["planning_rules"] = json!(text);
                self.rules_edited = true;
            }
        }
        self.working = next;
        Ok(())
    }

    /// What deleting an item takes with it, for the owner's confirmation, as
    /// Qt names it: the templates that lose a role's seats and the pipelines
    /// that lose its stages; the templates and roles that lose a pipeline as
    /// their default.
    pub fn remove_impact(&self, kind: Kind, id: &str) -> RemoveImpact {
        let name = items(&self.working, kind.list())
            .iter()
            .find(|item| string(item, "id") == id)
            .map(|item| {
                let name = string(item, "name");
                if name.is_empty() {
                    id.to_owned()
                } else {
                    name
                }
            })
            .unwrap_or_else(|| id.to_owned());
        let mut impact = RemoveImpact { name, ..RemoveImpact::default() };
        match kind {
            Kind::Role => {
                for template in items(&self.working, "templates") {
                    if items(&template, "roster").iter().any(|seat| string(seat, "role") == id) {
                        impact.templates.push(string(&template, "name"));
                    }
                }
                for pipeline in items(&self.working, "pipelines") {
                    let stages = items(&pipeline, "stages");
                    let taken = stages.iter().filter(|stage| string(stage, "role") == id).count();
                    if taken > 0 {
                        impact.pipelines.push(string(&pipeline, "name"));
                        if taken == stages.len() {
                            impact.emptied.push(string(&pipeline, "name"));
                        }
                    }
                }
            }
            Kind::Pipeline => {
                for list in ["templates", "roles"] {
                    for item in items(&self.working, list) {
                        if string(&item, "pipeline") == id {
                            impact.templates.push(string(&item, "name"));
                        }
                    }
                }
            }
            Kind::Template | Kind::Rule => {}
        }
        impact
    }

    /// Reset planning rules (U28): the saved catalog without its planning
    /// rules, so hgs fills in its built-in ones; only without unsaved edits.
    pub fn reset_rules_action(&self) -> Option<super::TeamAction> {
        if self.is_dirty() || self.builtin {
            return None;
        }
        let mut catalog = self.saved.clone();
        catalog.as_object_mut()?.remove("planning_rules");
        Some(super::TeamAction::CatalogSet { version: self.version(), catalog })
    }

    /// Drop the unsaved edits.
    pub fn revert(&mut self) {
        self.working = self.saved.clone();
        self.edited.clear();
        self.rules_edited = false;
    }

    /// The working copy as a file Import reads back and Qt writes.
    pub fn export_text(&self) -> String {
        serde_json::to_string_pretty(&self.working).unwrap_or_default()
    }

    /// Replace the working copy with a file's catalog: an export (the catalog
    /// itself) or a reply that wraps it in `catalog`. Unsaved until Save; it
    /// keeps the version it was read at, so Save refuses it on a newer one.
    pub fn import_text(&mut self, text: &str) -> Result<(), String> {
        if text.len() > IMPORT_BYTES {
            return Err(format!("A team catalog file is at most {} KiB", IMPORT_BYTES / 1024));
        }
        let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| "This file is not a team catalog".to_owned())?;
        let mut catalog = match value.get("catalog") {
            Some(inner) if inner.is_object() => inner.clone(),
            _ => value,
        };
        shape(&catalog).map_err(|_| "This file is not a team catalog".to_owned())?;
        catalog["version"] = json!(self.version());
        for kind in [Kind::Role, Kind::Template, Kind::Rule, Kind::Pipeline] {
            for item in items(&catalog, kind.list()) {
                self.edited.insert((kind, string(&item, "id")));
            }
        }
        self.rules_edited = true;
        self.working = catalog;
        Ok(())
    }

    /// The `team catalog set` request of Save: the working copy over the
    /// version it was read at.
    pub fn save_action(&self) -> super::TeamAction {
        super::TeamAction::CatalogSet { version: self.version(), catalog: self.working.clone() }
    }

    /// After a saved catalog came back: the edits are the saved catalog now.
    pub fn saved(&mut self) {
        self.saved = self.working.clone();
        self.edited.clear();
        self.rules_edited = false;
    }

    pub fn view(&self) -> CatalogEditorView {
        let fields = |kind: Kind| -> Vec<Value> {
            let specs = schema(kind);
            items(&self.working, kind.list())
                .iter()
                .map(|item| {
                    let mut out = Map::new();
                    out.insert("id".into(), item.get("id").cloned().unwrap_or(json!("")));
                    for spec in &specs {
                        let value = derived_shown(kind, &spec.name, item).unwrap_or_else(|| shown(&spec.name, item.get(&spec.name)));
                        out.insert(spec.name.clone(), value);
                    }
                    Value::Object(out)
                })
                .collect()
        };
        let mut edited = Map::new();
        for (kind, id) in &self.edited {
            if let Some(list) = edited.entry(kind.list()).or_insert_with(|| json!([])).as_array_mut() {
                list.push(json!(id));
            }
        }
        CatalogEditorView {
            version: self.version(),
            builtin: self.builtin,
            roles: fields(Kind::Role),
            templates: fields(Kind::Template),
            routing: fields(Kind::Rule),
            pipelines: fields(Kind::Pipeline),
            planning_rules: string(&self.working, "planning_rules"),
            unsaved: self.unsaved(),
            edited,
        }
    }

    /// How many items (and the planning rules) differ from the saved catalog.
    pub fn unsaved(&self) -> usize {
        let mut count = 0;
        for kind in [Kind::Role, Kind::Template, Kind::Rule, Kind::Pipeline] {
            let saved = items(&self.saved, kind.list());
            let working = items(&self.working, kind.list());
            for item in &working {
                if !saved.contains(item) {
                    count += 1;
                }
            }
            count += saved.iter().filter(|item| !working.iter().any(|entry| string(entry, "id") == string(item, "id"))).count();
        }
        if self.saved.get("planning_rules") != self.working.get("planning_rules") {
            count += 1;
        }
        count
    }
}

/// What a deletion changes elsewhere, as Qt's editor does it, so the catalog
/// stays valid: the (kind, id) of every item it changed.
fn cascade(catalog: &mut Value, kind: Kind, id: &str) -> Vec<(Kind, String)> {
    let mut changed = Vec::new();
    let mut each = |catalog: &mut Value, list: &str, of: Kind, change: &mut dyn FnMut(&mut Value) -> bool| {
        if let Some(entries) = catalog.get_mut(list).and_then(Value::as_array_mut) {
            for item in entries {
                if change(item) {
                    changed.push((of, string(item, "id")));
                }
            }
        }
    };
    let retain = |item: &mut Value, key: &str, keep: &dyn Fn(&Value) -> bool| -> bool {
        let Some(entries) = item.get_mut(key).and_then(Value::as_array_mut) else { return false };
        let before = entries.len();
        entries.retain(|entry| keep(entry));
        entries.len() != before
    };
    match kind {
        Kind::Role => {
            each(catalog, "templates", Kind::Template, &mut |template: &mut Value| {
                let mut moved = retain(template, "roster", &|seat: &Value| string(seat, "role") != id);
                if let Some(tasks) = template.get_mut("tasks").and_then(Value::as_array_mut) {
                    for task in tasks {
                        if string(task, "role") == id {
                            task["role"] = json!("lead");
                            moved = true;
                        }
                    }
                }
                moved
            });
            let plus = format!("+{id}");
            if let Some(rules) = catalog.get_mut("routing").and_then(Value::as_array_mut) {
                rules.retain(|rule| string(rule, "use") != plus);
            }
            each(catalog, "roles", Kind::Role, &mut |role: &mut Value| retain(role, "message_to", &|other: &Value| other.as_str() != Some(id)));
            each(catalog, "pipelines", Kind::Pipeline, &mut |pipeline: &mut Value| retain(pipeline, "stages", &|stage: &Value| string(stage, "role") != id));
        }
        Kind::Template => {
            if let Some(rules) = catalog.get_mut("routing").and_then(Value::as_array_mut) {
                rules.retain(|rule| string(rule, "use").starts_with('+') || string(rule, "template") != id);
            }
        }
        Kind::Pipeline => {
            for (list, of) in [("templates", Kind::Template), ("roles", Kind::Role)] {
                each(catalog, list, of, &mut |item: &mut Value| {
                    if string(item, "pipeline") == id {
                        item.as_object_mut().map(|item| item.remove("pipeline"));
                        true
                    } else {
                        false
                    }
                });
            }
        }
        Kind::Rule => {}
    }
    changed
}

/// A catalog: an object whose lists are lists of objects with ids.
fn shape(catalog: &Value) -> Result<(), String> {
    if !catalog.is_object() {
        return Err("not an object".into());
    }
    if catalog.get("roles").is_none() && catalog.get("templates").is_none() {
        return Err("no roles or templates".into());
    }
    for list in ["roles", "templates", "routing", "pipelines"] {
        match catalog.get(list) {
            None | Some(Value::Null) => {}
            Some(Value::Array(entries)) if entries.len() <= LIST_ITEMS && entries.iter().all(Value::is_object) => {}
            Some(_) => return Err(format!("{list} is not a list of items")),
        }
    }
    Ok(())
}

fn items(catalog: &Value, list: &str) -> Vec<Value> {
    catalog.get(list).and_then(Value::as_array).cloned().unwrap_or_default()
}

fn string(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn position(catalog: &Value, kind: Kind, id: &str) -> Result<usize, String> {
    catalog
        .get(kind.list())
        .and_then(Value::as_array)
        .and_then(|list| list.iter().position(|item| item.get("id").and_then(Value::as_str) == Some(id)))
        .ok_or(format!("No {kind:?} {id} in the catalog"))
}

fn push(catalog: &mut Value, kind: Kind, item: Value) {
    if !catalog[kind.list()].is_array() {
        catalog[kind.list()] = json!([]);
    }
    if let Some(list) = catalog[kind.list()].as_array_mut() {
        list.push(item);
    }
}

/// A slug of `name`, made unique in the kind's list.
fn unique(catalog: &Value, kind: Kind, name: &str) -> String {
    let base: String = name
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() { "item".to_owned() } else { base };
    let taken = |id: &str| items(catalog, kind.list()).iter().any(|item| string(item, "id") == id);
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| !taken(id)).unwrap_or(base)
}

/// Qt's new item of each kind.
fn fresh(catalog: &Value, kind: Kind) -> Value {
    match kind {
        Kind::Role => {
            let id = unique(catalog, kind, "New role");
            json!({"id":id,"name":"New role","agent":"claude","folder":"worktree","message":"lead","prompt":""})
        }
        Kind::Template => {
            let id = unique(catalog, kind, "New template");
            json!({"id":id,"name":"New template","planning":"lead","approval":"review","work":"worktree","roster":[],"tasks":[]})
        }
        Kind::Rule => {
            let id = unique(catalog, kind, "rule");
            json!({"id":id,"when":"","template":"","approval":""})
        }
        Kind::Pipeline => {
            let id = unique(catalog, kind, "New pipeline");
            // U36: a new preset starts with a review by the reviewer (or the
            // first role that is not the lead); the lead accepts.
            let roles = items(catalog, "roles");
            let role = roles
                .iter()
                .map(|role| string(role, "id"))
                .find(|id| id == "reviewer")
                .or_else(|| roles.iter().map(|role| string(role, "id")).find(|id| id != "lead"));
            let stages = role.map(|role| json!([{"kind":"review","role":role}])).unwrap_or(json!([]));
            json!({"id":id,"name":"New pipeline","stages":stages,"max_rounds":3,"folder":"hold","accept":"lead"})
        }
    }
}

fn text(item: &Value, key: &str) -> String {
    string(item, key)
}

/// The fields Qt edits as one choice over two stored fields, as it shows
/// them; None: the stored value itself.
fn derived_shown(kind: Kind, name: &str, item: &Value) -> Option<Value> {
    let value = match (kind, name) {
        (Kind::Role, "permission_mode") => {
            let permissions = text(item, "permissions");
            let profile = text(item, "permissions_profile");
            let mode = match (permissions.as_str(), profile.as_str()) {
                ("bypass", _) => "bypass",
                ("auto", _) => "auto",
                (_, "read") => "read_only",
                (_, "write") => "ask",
                _ => "account",
            };
            mode.to_owned()
        }
        (Kind::Role, "message") => {
            let message = text(item, "message");
            let can = text(item, "can_message");
            if !message.is_empty() {
                message
            } else if can.is_empty() {
                "lead".into()
            } else {
                can
            }
        }
        (Kind::Role, "folder") if text(item, "folder").is_empty() => "worktree".into(),
        (Kind::Role, "context_before_task") if text(item, "context_before_task").is_empty() => "none".into(),
        (Kind::Template, "phase_approval") => {
            let approval = text(item, "phase_approval");
            if !approval.is_empty() {
                approval
            } else {
                match text(item, "phases").as_str() {
                    "owner" => "review".into(),
                    "lead" => "auto".into(),
                    _ => String::new(),
                }
            }
        }
        (Kind::Template, "work") if text(item, "work").is_empty() => "worktree".into(),
        (Kind::Template, "approval") if text(item, "approval").is_empty() => "review".into(),
        (Kind::Pipeline, "folder") if text(item, "folder").is_empty() => "hold".into(),
        (Kind::Pipeline, "accept") if text(item, "accept").is_empty() => "lead".into(),
        _ => return None,
    };
    Some(json!(value))
}

/// Stores a field Qt edits as one choice over two stored fields, as Qt
/// does. False: an ordinary field.
fn derived_set(kind: Kind, name: &str, value: &Value, item: &mut Value) -> bool {
    let chosen = value.as_str().unwrap_or_default();
    match (kind, name) {
        (Kind::Role, "permission_mode") => {
            let permissions = if matches!(chosen, "auto" | "bypass") { chosen } else { "" };
            let profile = match chosen {
                "read_only" => "read",
                "ask" => "write",
                _ => "",
            };
            set_or_remove(item, "permissions", permissions);
            set_or_remove(item, "permissions_profile", profile);
            true
        }
        (Kind::Role, "message") => {
            item["message"] = json!(chosen);
            item["can_message"] = json!(if chosen == "team" { "team" } else { "lead" });
            true
        }
        (Kind::Template, "phase_approval") => {
            item["phase_approval"] = json!(chosen);
            let phases = match chosen {
                "review" => "owner",
                "auto" => "lead",
                _ => "same",
            };
            item["phases"] = json!(phases);
            true
        }
        _ => false,
    }
}

fn set_or_remove(item: &mut Value, key: &str, value: &str) {
    if value.is_empty() {
        if let Some(object) = item.as_object_mut() {
            object.remove(key);
        }
    } else {
        item[key] = json!(value);
    }
}

/// A field's value as the webview shows it: a stage's command as its text.
fn shown(name: &str, value: Option<&Value>) -> Value {
    let value = value.cloned().unwrap_or(Value::Null);
    if name == "stages" {
        if let Value::Array(stages) = &value {
            return Value::Array(
                stages
                    .iter()
                    .map(|stage| {
                        let mut stage = stage.clone();
                        if let Some(command) = stage.get("command").and_then(|command| command.get("command")).cloned() {
                            stage["command"] = command;
                        }
                        stage
                    })
                    .collect(),
            );
        }
    }
    value
}

/// A stage's command text goes back into its check object (its timeout
/// kept); other fields are stored as checked.
fn merge_list(name: &str, value: Value, previous: Option<&Value>) -> Value {
    if name != "stages" {
        return value;
    }
    let Value::Array(stages) = value else { return value };
    Value::Array(
        stages
            .into_iter()
            .enumerate()
            .map(|(index, mut stage)| {
                let old = previous.and_then(|previous| previous.get(index)).and_then(|stage| stage.get("command")).cloned();
                match stage.get("command").cloned() {
                    Some(Value::String(text)) if text.trim().is_empty() => {
                        stage.as_object_mut().map(|stage| stage.remove("command"));
                    }
                    Some(Value::String(text)) => {
                        let mut check = old.filter(Value::is_object).unwrap_or(json!({}));
                        check["command"] = json!(text);
                        stage["command"] = check;
                    }
                    _ => {}
                }
                stage
            })
            .collect(),
    )
}

/// The value checked against its field: the webview's value or an error.
fn checked(spec: &FieldSpec, value: &Value) -> Result<Value, String> {
    let label = &spec.label;
    match &spec.ty {
        FieldType::Text | FieldType::Multiline => {
            let text = value.as_str().ok_or(format!("{label} is text"))?;
            if text.chars().count() > TEXT_CHARS {
                return Err(format!("{label} is too long"));
            }
            if matches!(spec.ty, FieldType::Text) && text.contains(['\n', '\r']) {
                return Err(format!("{label} is one line"));
            }
            Ok(json!(text))
        }
        FieldType::Number { min, max } => {
            let number = value.as_u64().ok_or(format!("{label} is a whole number"))?;
            if number < *min || number > *max {
                return Err(format!("{label} is {min} to {max}"));
            }
            Ok(json!(number))
        }
        FieldType::Bool => value.as_bool().map(Value::Bool).ok_or(format!("{label} is on or off")),
        FieldType::Choice { options } => {
            let chosen = value.as_str().ok_or(format!("{label} is one of its choices"))?;
            if !options.iter().any(|(option, _)| option == chosen) {
                return Err(format!("{label} has no choice {chosen}"));
            }
            Ok(json!(chosen))
        }
        FieldType::Words => {
            let words = value.as_array().ok_or(format!("{label} is a list"))?;
            if words.len() > 50 {
                return Err(format!("{label} has at most 50 entries"));
            }
            let mut out = Vec::new();
            for word in words {
                // Task numbers ("after #") are kept as numbers.
                if let Some(number) = word.as_u64() {
                    out.push(json!(number));
                    continue;
                }
                let word = word.as_str().ok_or(format!("{label} lists words"))?.trim();
                if word.is_empty() {
                    continue;
                }
                if word.chars().count() > 200 || word.chars().any(char::is_control) {
                    return Err(format!("{label}: {word} is not a short word"));
                }
                out.push(json!(word));
            }
            Ok(Value::Array(out))
        }
        FieldType::List { fields } => {
            let rows = value.as_array().ok_or(format!("{label} is a list"))?;
            if rows.len() > 50 {
                return Err(format!("{label} has at most 50 rows"));
            }
            let mut out = Vec::new();
            for row in rows {
                let row = row.as_object().ok_or(format!("{label} lists rows"))?;
                let mut checked_row = Map::new();
                for (key, cell) in row {
                    let Some(spec) = fields.iter().find(|spec| spec.name == *key) else {
                        // A field this client does not edit stays as it was.
                        checked_row.insert(key.clone(), cell.clone());
                        continue;
                    };
                    if cell.is_null() {
                        continue;
                    }
                    let mut value = checked(spec, cell)?;
                    // A task's deps are task numbers.
                    if key == "deps" {
                        value = Value::Array(
                            value
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|dep| dep.as_u64().or_else(|| dep.as_str().and_then(|text| text.trim_start_matches('#').parse().ok())))
                                .map(|dep| json!(dep))
                                .collect(),
                        );
                    }
                    checked_row.insert(key.clone(), value);
                }
                out.push(Value::Object(checked_row));
            }
            Ok(Value::Array(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(version: u64) -> Value {
        json!({"builtin":false,"draft":{"base":version,"proposals":[]},"catalog":{"version":version,
            "roles":[{"id":"backend","name":"Backend","agent":"codex","prompt":"Build it.","future_field":{"kept":true}},
                     {"id":"reviewer","name":"Reviewer","agent":"claude","prompt":"Read it."}],
            "templates":[{"id":"feature","name":"Feature","planning":"lead","approval":"review","roster":[{"role":"backend","count":1}],"tasks":[]}],
            "routing":[{"id":"bug","when":"bug","template":"feature"},{"id":"docs","when":"docs","template":"feature"}],
            "pipelines":[{"id":"reviewed","name":"Reviewed","stages":[{"kind":"command","role":"backend","command":{"command":"cargo test","timeout_minutes":20}}]}],
            "planning_rules":"(1) small phases"}})
    }

    fn editor(version: u64) -> CatalogEditor {
        let mut editor = CatalogEditor::default();
        editor.load(&reply(version)).unwrap();
        editor
    }

    /// Save sends the working copy over the version it read, with every field
    /// the editors do not show (a newer hgs's) as it was.
    #[test]
    fn save_sends_the_edited_catalog_over_its_version_and_keeps_unknown_fields() {
        let mut editor = editor(7);
        editor.edit(&CatalogEdit::Set { kind: Kind::Role, id: "backend".into(), field: "name".into(), value: json!("API") }).unwrap();
        editor.edit(&CatalogEdit::Set { kind: Kind::Role, id: "backend".into(), field: "skills".into(), value: json!(["rust", " ", "sql"]) }).unwrap();
        assert_eq!(editor.unsaved(), 1);
        let super::super::TeamAction::CatalogSet { version, catalog } = editor.save_action() else { panic!() };
        assert_eq!(version, 7);
        assert_eq!(catalog["roles"][0]["name"], "API");
        assert_eq!(catalog["roles"][0]["skills"], json!(["rust", "sql"]));
        assert_eq!(catalog["roles"][0]["future_field"], json!({"kept":true}), "unknown fields survive");
        editor.saved();
        assert!(!editor.is_dirty());
    }

    /// Edits are checked against the schema: nothing changes on a wrong one.
    #[test]
    fn a_wrong_edit_changes_nothing() {
        let mut editor = editor(1);
        let before = editor.working().clone();
        for edit in [
            CatalogEdit::Set { kind: Kind::Role, id: "backend".into(), field: "secret".into(), value: json!("x") },
            CatalogEdit::Set { kind: Kind::Role, id: "backend".into(), field: "agent".into(), value: json!("gpt") },
            CatalogEdit::Set { kind: Kind::Role, id: "backend".into(), field: "name".into(), value: json!("two\nlines") },
            CatalogEdit::Set { kind: Kind::Template, id: "feature".into(), field: "max_members".into(), value: json!(99) },
            CatalogEdit::Set { kind: Kind::Role, id: "nobody".into(), field: "name".into(), value: json!("x") },
        ] {
            assert!(editor.edit(&edit).is_err(), "{edit:?}");
        }
        assert_eq!(editor.working(), &before);
        assert_eq!(editor.unsaved(), 0);
    }

    /// Qt's list editors: roster rows, a task's "after #", a stage's command
    /// text kept in its check object with its timeout; new, copied, moved and
    /// removed items.
    #[test]
    fn list_fields_and_items_edit_as_in_qt() {
        let mut editor = editor(3);
        editor
            .edit(&CatalogEdit::Set {
                kind: Kind::Template,
                id: "feature".into(),
                field: "tasks".into(),
                value: json!([{"title":"Plan","role":"lead","deps":[],"phase":1},{"title":"Build","role":"backend","deps":["#1"],"phase":1}]),
            })
            .unwrap();
        assert_eq!(editor.working()["templates"][0]["tasks"][1]["deps"], json!([1]));
        editor
            .edit(&CatalogEdit::Set {
                kind: Kind::Pipeline,
                id: "reviewed".into(),
                field: "stages".into(),
                value: json!([{"kind":"command","role":"backend","command":"cargo test --locked"}]),
            })
            .unwrap();
        assert_eq!(editor.working()["pipelines"][0]["stages"][0]["command"], json!({"command":"cargo test --locked","timeout_minutes":20}));
        assert_eq!(editor.view().pipelines[0]["stages"][0]["command"], "cargo test --locked", "the webview sees the text");
        editor.edit(&CatalogEdit::Add { kind: Kind::Role }).unwrap();
        editor.edit(&CatalogEdit::Add { kind: Kind::Role }).unwrap();
        let ids: Vec<String> = items(editor.working(), "roles").iter().map(|role| string(role, "id")).collect();
        assert_eq!(ids[2..], ["new-role", "new-role-2"]);
        editor.edit(&CatalogEdit::Duplicate { kind: Kind::Role, id: "backend".into() }).unwrap();
        assert_eq!(items(editor.working(), "roles").last().unwrap()["name"], "Backend copy");
        editor.edit(&CatalogEdit::Move { kind: Kind::Rule, id: "docs".into(), by: -1 }).unwrap();
        assert_eq!(editor.working()["routing"][0]["id"], "docs", "rules run top to bottom");
        editor.edit(&CatalogEdit::Remove { kind: Kind::Role, id: "new-role-2".into() }).unwrap();
        editor.edit(&CatalogEdit::Add { kind: Kind::Pipeline }).unwrap();
        assert_eq!(items(editor.working(), "pipelines").last().unwrap()["stages"], json!([{"kind":"review","role":"reviewer"}]));
        editor.revert();
        assert_eq!(editor.working(), editor.saved_catalog());
    }

    /// Export writes the working copy; Import reads it back (or a `catalog
    /// get` reply) as unsaved edits on the version it was read at, and
    /// refuses anything that is no catalog.
    #[test]
    fn export_and_import_round_trip_as_unsaved_edits() {
        let mut editor = editor(5);
        editor.edit(&CatalogEdit::PlanningRules { text: "(1) one phase".into() }).unwrap();
        let file = editor.export_text();
        let mut other = self::editor(5);
        other.import_text(&file).unwrap();
        assert_eq!(other.working()["planning_rules"], "(1) one phase");
        assert!(other.is_dirty() && other.unsaved() >= 1);
        let wrapped = serde_json::to_string(&json!({"catalog": editor.working()})).unwrap();
        let mut third = self::editor(9);
        third.import_text(&wrapped).unwrap();
        assert_eq!(third.working()["version"], 9, "Save writes over the version it read");
        for bad in ["", "[]", "{\"name\":\"x\"}", "{\"roles\":[1,2]}", "not json"] {
            assert_eq!(third.clone().import_text(bad).unwrap_err(), "This file is not a team catalog", "{bad}");
        }
        assert!(third.import_text(&"x".repeat(IMPORT_BYTES + 1)).is_err());
    }

    /// Deleting a role takes its seats, task roles (to the lead), "+role"
    /// rules, message lists and pipeline stages with it, as Qt does; a
    /// template takes its rules; a pipeline leaves no default naming it.
    #[test]
    fn deletions_cascade_as_in_qt_and_name_what_they_take() {
        let mut editor = editor(1);
        editor.edit(&CatalogEdit::Set { kind: Kind::Template, id: "feature".into(), field: "tasks".into(), value: json!([{"title":"Build","role":"backend"}]) }).unwrap();
        editor.edit(&CatalogEdit::Set { kind: Kind::Rule, id: "docs".into(), field: "use".into(), value: json!("+backend") }).unwrap();
        editor.edit(&CatalogEdit::Set { kind: Kind::Role, id: "reviewer".into(), field: "message_to".into(), value: json!(["backend"]) }).unwrap();
        editor.edit(&CatalogEdit::Set { kind: Kind::Role, id: "reviewer".into(), field: "pipeline".into(), value: json!("reviewed") }).unwrap();
        let impact = editor.remove_impact(Kind::Role, "backend");
        assert_eq!((impact.name.as_str(), impact.templates.clone(), impact.emptied.clone()), ("Backend", vec!["Feature".to_owned()], vec!["Reviewed".to_owned()]));
        editor.edit(&CatalogEdit::Remove { kind: Kind::Role, id: "backend".into() }).unwrap();
        let catalog = editor.working();
        assert_eq!(catalog["templates"][0]["roster"], json!([]));
        assert_eq!(catalog["templates"][0]["tasks"][0]["role"], "lead");
        assert_eq!(items(catalog, "routing").len(), 1, "the +backend rule went with it");
        assert_eq!(catalog["roles"][0]["message_to"], json!([]));
        assert_eq!(catalog["pipelines"][0]["stages"], json!([]));
        assert_eq!(editor.remove_impact(Kind::Pipeline, "reviewed").templates, vec!["Reviewer".to_owned()]);
        editor.edit(&CatalogEdit::Remove { kind: Kind::Pipeline, id: "reviewed".into() }).unwrap();
        assert!(editor.working()["roles"][0].get("pipeline").is_none());
        editor.edit(&CatalogEdit::Remove { kind: Kind::Template, id: "feature".into() }).unwrap();
        assert!(items(editor.working(), "routing").is_empty(), "its rules went with it");
        // Reset planning rules only from a saved catalog, without the rules.
        assert!(editor.reset_rules_action().is_none(), "unsaved edits come first");
        editor.revert();
        let Some(super::super::TeamAction::CatalogSet { version, catalog }) = editor.reset_rules_action() else { panic!() };
        assert_eq!(version, 1);
        assert!(catalog.get("planning_rules").is_none());
    }

    /// Qt's choices over two stored fields: permissions, who a role may
    /// message, and who approves new phases.
    #[test]
    fn combined_choices_store_both_fields_as_qt() {
        let mut editor = editor(1);
        let set = |editor: &mut CatalogEditor, kind, id: &str, field: &str, value: &str| {
            editor.edit(&CatalogEdit::Set { kind, id: id.into(), field: field.into(), value: json!(value) }).unwrap();
        };
        assert_eq!(editor.view().roles[0]["permission_mode"], "account");
        set(&mut editor, Kind::Role, "backend", "permission_mode", "read_only");
        assert_eq!(editor.working()["roles"][0]["permissions_profile"], "read");
        assert!(editor.working()["roles"][0].get("permissions").is_none());
        set(&mut editor, Kind::Role, "backend", "permission_mode", "bypass");
        assert_eq!(editor.working()["roles"][0]["permissions"], "bypass");
        assert!(editor.working()["roles"][0].get("permissions_profile").is_none());
        assert_eq!(editor.view().roles[0]["permission_mode"], "bypass");
        set(&mut editor, Kind::Role, "backend", "message", "team");
        assert_eq!((editor.working()["roles"][0]["message"].as_str(), editor.working()["roles"][0]["can_message"].as_str()), (Some("team"), Some("team")));
        set(&mut editor, Kind::Template, "feature", "phase_approval", "auto");
        assert_eq!(editor.working()["templates"][0]["phases"], "lead");
        assert_eq!(editor.view().templates[0]["phase_approval"], "auto");
    }

    /// A newer saved catalog under unsaved edits: the edited item keeps the
    /// edit, every other item is the newer one's (Qt's rebase).
    #[test]
    fn a_newer_catalog_keeps_the_items_edited_here() {
        let mut editor = editor(2);
        editor.edit(&CatalogEdit::Set { kind: Kind::Role, id: "backend".into(), field: "about".into(), value: json!("Mine") }).unwrap();
        let mut newer = reply(3);
        newer["catalog"]["roles"][1]["name"] = json!("Reviewer v3");
        newer["catalog"]["roles"][0]["about"] = json!("Theirs");
        editor.load(&newer).unwrap();
        assert_eq!(editor.version(), 3);
        assert_eq!(editor.working()["roles"][0]["about"], "Mine");
        assert_eq!(editor.working()["roles"][1]["name"], "Reviewer v3");
        assert_eq!(editor.unsaved(), 1);
    }

    impl CatalogEditor {
        fn saved_catalog(&self) -> &Value {
            &self.saved
        }
    }
}
