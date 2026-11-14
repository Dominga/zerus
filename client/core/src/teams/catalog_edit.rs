//! Phase-one catalog fields, adapted from WhiteWh's catalog editor. The plan
//! kernel is the save/import validator; this schema describes only our types.
use serde_json::{json, Value};
use zerus_team_plan::catalog::{Catalog, TemplateBundle};

pub const BYTES: usize = 240_000; // room for the guarded CLI envelope
pub const ITEMS: usize = 200;

fn text(name: &str, label: &str) -> Value {
    json!({"name":name,"label":label,"type":"text","default":""})
}
fn choice(name: &str, label: &str, options: &[&str]) -> Value {
    json!({"name":name,"label":label,"type":"choice","options":options,"default":options[0]})
}
fn number(name: &str, label: &str, default: u32) -> Value {
    json!({"name":name,"label":label,"type":"number","default":default})
}
fn boolean(name: &str, label: &str, default: bool) -> Value {
    json!({"name":name,"label":label,"type":"bool","default":default})
}
fn group(name: &str, label: &str, fields: Vec<Value>) -> Value {
    json!({"name":name,"label":label,"type":"group","fields":fields})
}
fn account() -> Value {
    group(
        "account",
        "Account",
        vec![
            choice("mode", "Account choice", &["freest", "named"]),
            text("account", "Account ID"),
        ],
    )
}
fn agent() -> Value {
    choice("agent", "Agent", &["claude", "codex", "kimi", "dsh"])
}
fn multiline(name: &str, label: &str) -> Value {
    json!({"name":name,"label":label,"type":"multiline","default":""})
}
fn list(name: &str, label: &str, fields: Vec<Value>) -> Value {
    json!({"name":name,"label":label,"type":"list","fields":fields,"default":[]})
}

pub fn schema() -> Value {
    json!({
        "roles":[text("id","ID"),text("name","Name"),multiline("prompt","Prompt"),agent(),account(),text("model","Model"),text("effort","Effort"),text("pipeline","Pipeline"),choice("context","Context",&["keep","fresh"])],
        "pipelines":[text("id","ID"),text("name","Name"),text("implementer","Implementer"),list("stages","Stages",vec![choice("kind","Kind",&["review","test","land"]),text("role","Role")]),number("max_rounds","Maximum rounds",0),choice("accept","Final acceptance",&["lead","person","pipeline"])],
        "templates":[text("id","ID"),text("name","Name"),text("description","Description"),group("lead","Lead",vec![agent(),account(),text("model","Model"),text("effort","Effort"),multiline("rules","Lead rules")]),list("roster","Roster",vec![text("role","Role"),number("count","Count",1),boolean("optional","Optional",false)]),text("default_pipeline","Default pipeline"),settings()]
    })
}

fn settings() -> Value {
    group(
        "settings",
        "Settings",
        vec![
            group(
                "delegations",
                "Delegations",
                vec![
                    boolean("accept_results", "Accept results", true),
                    boolean("answer_members", "Answer members", true),
                    boolean("confirm_trust", "Confirm folder trust", true),
                    boolean("resume_exited", "Resume exited members", true),
                    boolean("merge_accepted", "Merge accepted work", true),
                ],
            ),
            group(
                "caps",
                "Limits",
                vec![
                    number("max_members", "Maximum members", 6),
                    number("max_phase_tasks", "Maximum phase tasks", 50),
                    number("max_open_tasks", "Maximum open tasks", 500),
                    number("max_open_questions", "Maximum open questions", 3),
                ],
            ),
            group(
                "timers",
                "Timers",
                vec![
                    number("report_timeout_seconds", "Report timeout (seconds)", 120),
                    number("start_timeout_seconds", "Start timeout (seconds)", 180),
                    number("inbox_interval_seconds", "Inbox interval (seconds)", 120),
                    number("remind_lead_minutes", "Lead reminder (minutes)", 15),
                    number("remind_person_minutes", "Person reminder (minutes)", 60),
                    number("lead_stall_minutes", "Lead stall (minutes)", 20),
                    number("answer_escalate_minutes", "Answer escalation (minutes)", 10),
                    number("resume_limit_per_hour", "Resumes per hour", 3),
                ],
            ),
            boolean("queue", "Queue", true),
            choice("routing", "Routing", &["direct", "lead"]),
            choice("plan_approval", "Plan approval", &["review", "auto"]),
            choice("phase_acceptance", "Phase acceptance", &["lead", "person"]),
            choice("integration", "Integration", &["branch", "none"]),
            boolean("land_allowed", "Allow landing", false),
        ],
    )
}

fn bounded(value: &Value) -> Result<(), String> {
    if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > BYTES {
        return Err("The catalog exceeds the editor's size limit.".into());
    }
    Ok(())
}

/// Strict parsing: a newer field is refused, never silently dropped on save.
/// `base` is the captured built-in catalog; no fleet or account I/O occurs.
pub fn validate(value: Value, base: Value) -> Result<Value, String> {
    bounded(&value)?;
    bounded(&base)?;
    let person: Catalog = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let base: Catalog = serde_json::from_value(base).map_err(|e| e.to_string())?;
    for count in [
        person.roles.len(),
        person.pipelines.len(),
        person.templates.len(),
    ] {
        if count > ITEMS {
            return Err("The catalog has too many entries.".into());
        }
    }
    let mut merged = base;
    macro_rules! over {
        ($field:ident) => {
            for row in &person.$field {
                if let Some(at) = merged.$field.iter().position(|old| old.id == row.id) {
                    merged.$field[at] = row.clone();
                } else {
                    merged.$field.push(row.clone());
                }
            }
        };
    }
    over!(roles);
    over!(pipelines);
    over!(templates);
    if !person.planning_rules.is_empty() {
        merged.planning_rules.clone_from(&person.planning_rules);
    }
    let mut problems = zerus_team_plan::validate_ids(&person);
    problems.extend(zerus_team_plan::validate_catalog(&merged));
    problems.extend(zerus_team_plan::admission_problems(&merged));
    for role in &merged.roles {
        if !["claude", "codex", "kimi", "dsh"].contains(&role.agent.as_str()) {
            return Err(format!("Role {} has an unsupported agent.", role.id));
        }
    }
    for template in &merged.templates {
        if !["claude", "codex", "kimi", "dsh"].contains(&template.lead.agent.as_str()) {
            return Err(format!(
                "Template {} has an unsupported lead agent.",
                template.id
            ));
        }
        let bundle = zerus_team_plan::bundle(&merged, &template.id).map_err(|p| p.message)?;
        problems.extend(zerus_team_plan::validate_bundle(
            &bundle,
            &template.settings.caps,
        ));
    }
    if let Some(problem) = problems.first() {
        return Err(problem.message.clone());
    }
    serde_json::to_value(person).map_err(|e| e.to_string())
}

/// Import replaces the draft, never the saved catalog. Bundle imports merge
/// their entries by ID into the current personal draft, then validate it.
pub fn import(value: Value, current: Value, base: Value) -> Result<Value, String> {
    bounded(&value)?;
    bounded(&current)?;
    bounded(&base)?;
    let value = if value.get("ok").and_then(Value::as_bool) == Some(true) {
        value
            .get("catalog")
            .or_else(|| value.get("bundle"))
            .cloned()
            .ok_or("The import reply has no catalog or bundle.")?
    } else {
        value
    };
    if value.get("template").is_some() {
        let bundle: TemplateBundle = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let mut current: Catalog = serde_json::from_value(current).map_err(|e| e.to_string())?;
        macro_rules! merge {
            ($field:ident, $rows:expr) => {
                for row in $rows {
                    if let Some(at) = current.$field.iter().position(|old| old.id == row.id) {
                        current.$field[at] = row;
                    } else {
                        current.$field.push(row);
                    }
                }
            };
        }
        merge!(roles, bundle.roles);
        merge!(pipelines, bundle.pipelines);
        merge!(templates, [bundle.template]);
        validate(
            serde_json::to_value(current).map_err(|e| e.to_string())?,
            base,
        )
    } else {
        validate(value, base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Value {
        serde_json::to_value(zerus_team_plan::builtin()).unwrap()
    }
    #[test]
    fn kernel_refuses_missing_roles_unknown_fields_agents_and_oversize() {
        assert!(validate(json!({}), base()).is_ok());
        let mut bad = base();
        bad["templates"][0]["roster"][0]["role"] = json!("missing");
        assert!(validate(bad, base()).is_err());
        assert!(validate(json!({"about":"unsupported"}), base()).is_err());
        assert!(validate(
            json!({"roles":[{"id":"bad","name":"Bad","agent":"fake"}]}),
            base()
        )
        .is_err());
        assert!(validate(json!({"planning_rules":["x".repeat(BYTES)]}), base()).is_err());
    }
    #[test]
    fn bundle_import_uses_kernel_types_and_preserves_unrelated_personal_rows() {
        let bundle = serde_json::to_value(
            zerus_team_plan::bundle(&zerus_team_plan::builtin(), "bugfix").unwrap(),
        )
        .unwrap();
        let own = json!({"roles":[{"id":"extra","name":"Extra","agent":"codex"}]});
        let got = import(bundle, own, base()).unwrap();
        assert_eq!(got["roles"][0]["id"], "extra");
        assert_eq!(got["templates"][0]["id"], "bugfix");
    }
}

/// The guarded import remains opt-in; legacy catalog imports retain their form.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub schema: u32,
    pub request_id: String,
    pub expected_revision: String,
    pub catalog: Value,
}

pub fn valid_request(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
}
fn valid_revision(value: &str) -> bool {
    value.strip_prefix("catalog-v1:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}
pub fn prepare(save: &Save) -> Result<crate::actions::Invocation, String> {
    if save.schema != 1
        || !valid_request(&save.request_id)
        || !valid_revision(&save.expected_revision)
    {
        return Err("Invalid catalog edit identity or revision.".into());
    }
    validate(
        save.catalog.clone(),
        serde_json::to_value(zerus_team_plan::builtin()).map_err(|e| e.to_string())?,
    )?;
    Ok(crate::actions::Invocation {
        args: vec![
            "team".into(),
            "catalog".into(),
            "import".into(),
            "--json".into(),
        ],
        stdin: Some(serde_json::to_vec(save).map_err(|e| e.to_string())?),
        timeout: std::time::Duration::from_secs(15),
        request_id: Some(save.request_id.clone()),
    })
}
pub fn interpret(request: &str, exit: &crate::actions::Exit) -> Value {
    let uncertain = || json!({"outcome":"uncertain","request_id":request,"detail":"The save result is not known."});
    let crate::actions::Exit::Code { code, stdout, .. } = exit else {
        return uncertain();
    };
    let Ok(reply) = serde_json::from_slice::<Value>(stdout) else {
        return uncertain();
    };
    if *code == 0
        && reply["ok"] == true
        && reply["schema"] == 1
        && reply["request_id"] == request
        && reply["actor"]["kind"] == "person"
    {
        if reply["outcome"] == "done" && reply["revision"].as_str().is_some_and(valid_revision) {
            return json!({"outcome":"done","request_id":request,"revision":reply["revision"],"detail":"Catalog saved."});
        }
        return uncertain();
    }
    if reply["ok"] == false {
        let code = reply["error"]["code"].as_str().unwrap_or_default();
        // Storage/transport uncertainty never proves that no write happened.
        if [
            "stale_revision",
            "request_conflict",
            "invalid_state",
            "unknown_field",
            "over_cap",
            "not_allowed",
            "teams_inactive",
            "duplicate",
            "unknown_role",
            "unknown_pipeline",
            "no_seat_for_stage",
        ]
        .contains(&code)
        {
            return json!({"outcome":"failed","request_id":request,"code":code,"detail":reply["error"]["message"]});
        }
    }
    uncertain()
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use crate::actions::Exit;
    #[test]
    fn success_requires_exact_request_revision_schema_and_person_receipt() {
        let request = uuid::Uuid::new_v4().to_string();
        let reply = json!({"ok":true,"schema":1,"outcome":"done","request_id":request,"revision":format!("catalog-v1:{}","a".repeat(64)),"actor":{"kind":"person"}});
        let exit = |reply: Value| Exit::Code {
            code: 0,
            stdout: serde_json::to_vec(&reply).unwrap(),
            stderr: String::new(),
        };
        assert_eq!(interpret(&request, &exit(reply.clone()))["outcome"], "done");
        for (field, value) in [
            ("request_id", json!(uuid::Uuid::new_v4().to_string())),
            ("schema", json!(2)),
            ("revision", json!("bad")),
            ("actor", json!({"kind":"session"})),
        ] {
            let mut wrong = reply.clone();
            wrong[field] = value;
            assert_eq!(interpret(&request, &exit(wrong))["outcome"], "uncertain");
        }
        assert_eq!(interpret(&request, &Exit::TimedOut)["outcome"], "uncertain");
        assert_eq!(
            interpret(
                &request,
                &exit(json!({"ok":false,"error":{"code":"storage_failed","message":"unknown"}}))
            )["outcome"],
            "uncertain"
        );
        assert_eq!(
            interpret(
                &request,
                &exit(json!({"ok":false,"error":{"code":"stale_revision","message":"changed"}}))
            )["outcome"],
            "failed"
        );
    }
}

#[cfg(test)]
mod fixture_export {
    /// Explicit writer for the private screenshot fixture. No hand-written rows.
    #[test]
    #[ignore = "writes private evidence only when explicitly selected"]
    fn write_editor_schema_for_private_preview() {
        let Some(path) = std::env::var_os("WP148_SCHEMA_FIXTURE") else {
            return;
        };
        std::fs::write(path, serde_json::to_vec_pretty(&super::schema()).unwrap()).unwrap();
    }
}

#[cfg(test)]
mod schema_contract {
    use super::*;
    #[test]
    fn schema_fields_are_complete_for_our_catalog_and_exclude_fork_only_fields() {
        let schema = schema();
        let fixture: Value =
            serde_json::from_str(include_str!("fixtures/catalog-editor-schema.json")).unwrap();
        assert_eq!(
            schema, fixture,
            "the frontend fixture is produced by the core schema writer"
        );
        let names = |kind: &str| {
            schema[kind]
                .as_array()
                .unwrap()
                .iter()
                .map(|field| field["name"].as_str().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("roles"),
            [
                "id", "name", "prompt", "agent", "account", "model", "effort", "pipeline",
                "context"
            ]
        );
        assert_eq!(
            names("pipelines"),
            [
                "id",
                "name",
                "implementer",
                "stages",
                "max_rounds",
                "accept"
            ]
        );
        assert_eq!(
            names("templates"),
            [
                "id",
                "name",
                "description",
                "lead",
                "roster",
                "default_pipeline",
                "settings"
            ]
        );
        for name in [
            "about",
            "icon",
            "skills",
            "computers",
            "folder_policy",
            "messaging",
            "routing_rules",
        ] {
            assert!(!schema.to_string().contains(&format!("\"{name}\"")));
        }
    }
    #[test]
    fn validation_retains_prompt_and_unedited_planning_rules_and_refuses_duplicate_ids() {
        let base = serde_json::to_value(zerus_team_plan::builtin()).unwrap();
        let raw = json!({"roles":[{"id":"custom","name":"Custom","agent":"codex","prompt":"Exact instructions"}],"planning_rules":["Keep these"]});
        let checked = validate(raw.clone(), base.clone()).unwrap();
        assert_eq!(checked["roles"][0]["prompt"], raw["roles"][0]["prompt"]);
        assert_eq!(checked["planning_rules"], raw["planning_rules"]);
        let mut duplicate = raw;
        duplicate["roles"] = json!([duplicate["roles"][0], duplicate["roles"][0]]);
        assert!(validate(duplicate, base).is_err());
    }
}
