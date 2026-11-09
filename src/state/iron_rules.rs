//! Owner-managed iron rules. Project rows are indexed; session rules travel
//! with their exact native binding, and team/member rules with the team.
use super::*;
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

pub(crate) const MAX_RULES: usize = 20;
pub(crate) const MAX_LENGTH: usize = 300;
// Claude Code persists strings over 10,000 UTF-16 units as a file/preview.
// Keep mandatory startup rules far below that native cap, with room for hints
// and task commands. https://code.claude.com/docs/en/hooks#json-output
pub(crate) const MAX_START_RULES: usize = 6000;
pub(crate) const MAX_START_CONTEXT: usize = 8000;

pub(crate) fn start_proof_matches(record: &Value, effective: &str) -> bool {
    record["iron_rules_start_supported"] == true
        && string(record,"iron_rules_start_run") == string(record,"run_id")
        && !string(record,"run_id").is_empty()
        && string(record,"iron_rules_start_conversation") == string(record,"conversation_id")
        && string(record,"iron_rules_start_digest") == teams::store::digest(&json!(effective))
        && effective.encode_utf16().count() <= MAX_START_RULES
}


pub(crate) fn validate(value: &Value) -> anyhow::Result<Vec<String>> {
    let list = value.as_array().context("Rules must be a list")?;
    ensure!(
        list.len() <= MAX_RULES,
        "At most {MAX_RULES} rules per level"
    );
    list.iter()
        .map(|value| {
            let text = value.as_str().context("Each rule must be text")?;
            ensure!(text.chars().count() <= MAX_LENGTH, "A rule is too long");
            ensure!(
                !text
                    .chars()
                    .any(|ch| ch.is_control() && !ch.is_whitespace()),
                "A rule contains control characters"
            );
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            ensure!(
                !text.is_empty() && text.chars().count() <= MAX_LENGTH,
                "A rule must have 1 to {MAX_LENGTH} characters"
            );
            Ok(text)
        })
        .collect()
}

fn database(write: bool) -> anyhow::Result<Option<Connection>> {
    let path = root().join("iron-rules.sqlite3");
    if !write && !path.exists() {
        return Ok(None);
    }
    if write {
        std::fs::create_dir_all(root())?;
    }
    let flags = if write {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE
    } else {
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let db = Connection::open_with_flags(path, flags)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    if write {
        db.execute_batch("CREATE TABLE IF NOT EXISTS projects (id TEXT PRIMARY KEY, rules TEXT NOT NULL, revision INTEGER NOT NULL, migrated INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS receipts (id TEXT PRIMARY KEY, input TEXT NOT NULL, result TEXT NOT NULL);")?;
    }
    Ok(Some(db))
}

pub(crate) fn project(id: &str) -> anyhow::Result<Value> {
    ensure!(
        id.len() <= 200 && !id.chars().any(char::is_control),
        "Invalid project identity"
    );
    let row: Option<(String, u64, bool)> = match database(false)? {
        Some(db) => db
            .query_row(
                "SELECT rules,revision,migrated FROM projects WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?,
        None => None,
    };
    let (rules, revision, migrated) = match row {
        Some((text, revision, migrated)) => (
            validate(&serde_json::from_str::<Value>(&text)?)?,
            revision,
            migrated,
        ),
        None => (Vec::new(), 0, false),
    };
    Ok(
        json!({"scope":"project","project":id,"rules":rules,"revision":revision,"migrated":migrated}),
    )
}

fn project_write(input: &Value, migrate: bool) -> anyhow::Result<Value> {
    let id = string(input, "project");
    ensure!(!id.is_empty(), "Name the exact project identity");
    let rules = validate(&input["rules"])?;
    let request = string(input, "request_id");
    ensure!(
        teams::store::canonical_uuid(request),
        "A canonical request_id is required"
    );
    let before = project(id)?;
    let expected = input["expected_revision"]
        .as_u64()
        .context("Supply expected_revision from rules get")?;
    let mut db = database(true)?.unwrap();
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let payload = json!({"migrate":migrate,"input":input}).to_string();
    if let Some((previous, result)) = tx
        .query_row(
            "SELECT input,result FROM receipts WHERE id=?1",
            [request],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        ensure!(
            previous == payload,
            "Request identity was used for different rules"
        );
        return Ok(serde_json::from_str(&result)?);
    }
    let row: Option<(String, u64)> = tx
        .query_row(
            "SELECT rules,revision FROM projects WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let current = row.as_ref().map(|row| row.1).unwrap_or(0);
    if !migrate || row.is_none() {
        ensure!(current == expected, "Project rules changed; read them again");
    }
    // Validate identity/revision before spending a verified owner statement.
    teams::commands::authorize_external_rules(input)?;
    // Migration never replaces an authoritative value, including an owner's
    // deliberately empty list. A replay therefore cannot resurrect old rules.
    let result = if let Some((stored_rules, _)) = row.as_ref().filter(|_| migrate) {
        json!({"status":"kept","scope":"project","project":id,"rules":serde_json::from_str::<Value>(stored_rules)?,"revision":current,"migrated":true})
    } else {
        ensure!(
            current == expected,
            "Project rules changed; read them again"
        );
        tx.execute("INSERT INTO projects(id,rules,revision,migrated) VALUES(?1,?2,?3,?4)
            ON CONFLICT(id) DO UPDATE SET rules=excluded.rules,revision=excluded.revision,migrated=MAX(projects.migrated,excluded.migrated)",
            params![id, serde_json::to_string(&rules)?, current + 1, migrate])?;
        json!({"status":"saved","scope":"project","project":id,"rules":rules,"revision":current+1,"migrated":migrate || before["migrated"]==true})
    };
    if migrate {
        tx.execute("UPDATE projects SET migrated=1 WHERE id=?1", [id])?;
    }
    tx.execute(
        "INSERT INTO receipts(id,input,result) VALUES(?1,?2,?3)",
        params![request, payload, result.to_string()],
    )?;
    tx.commit()?;
    Ok(result)
}

pub(crate) fn render(levels: &[(&str, &[String])]) -> String {
    let mut text = if levels.iter().any(|(_, rules)| !rules.is_empty()) {
        "Iron rules are cumulative: lower levels add to, never cancel, higher levels.\n".to_owned()
    } else {
        String::new()
    };
    for (name, rules) in levels {
        if rules.is_empty() {
            continue;
        }
        text += &format!("{name} rules:\n");
        for (n, rule) in rules.iter().enumerate() {
            text += &format!("{}. {rule}\n", n + 1);
        }
    }
    text
}

fn session_rules(session: &teams::model::SessionRef) -> anyhow::Result<Vec<String>> {
    if session.host.is_empty() {
        let path = if session.name.starts_with("dsh/") {
            root()
                .join("dsh/bindings")
                .join(legacy_record_path(&session.name).file_name().unwrap())
        } else if session.name.starts_with("opencode/") {
            root()
                .join("opencode/bindings")
                .join(legacy_record_path(&session.name).file_name().unwrap())
        } else {
            record_path(&session.name)
        };
        // Untracked/legacy sessions have no session-level list to inherit.
        if !path.exists() {
            return Ok(Vec::new());
        }
        return super::with_session_project_binding(&session.name, None, &session.run_id, &session.conversation_id, |record| {
            ensure!(
                string(record, "run_id") == session.run_id
                    && string(record, "conversation_id") == session.conversation_id,
                "Session identity changed"
            );
            validate(record.get("iron_rules").unwrap_or(&json!([])))
        });
    }
    validate(&remote_session(session)?["rules"])
}

fn remote_session(session: &teams::model::SessionRef) -> anyhow::Result<Value> {
    let help = teams::dispatcher::hgs(
        &session.host,
        &["--help"],
        None,
        std::time::Duration::from_secs(15),
    )?;
    ensure!(
        help.status.success(),
        "Could not check the remote rules capability"
    );
    // An older binding cannot have Rust session rules. Its project fallback
    // remains on the GUI until the selected host supports this contract.
    if !String::from_utf8_lossy(&help.stdout).contains("iron-rules-v1") {
        return Ok(
            json!({"unsupported":true,"rules":[],"effective_rules":"","start_delivery_supported":false}),
        );
    }
    let output = teams::dispatcher::hgs(
        &session.host,
        &["rules", "get", "--json"],
        Some(&json!({"scope":"session","name":session.name,
        "expected_run_id":session.run_id,"expected_conversation_id":session.conversation_id})),
        std::time::Duration::from_secs(30),
    )?;
    ensure!(
        output.status.success(),
        "Could not read the exact remote session's iron rules"
    );
    let value: Value = serde_json::from_slice(&output.stdout)?;
    ensure!(
        string(&value, "name") == session.name
            && string(&value, "run_id") == session.run_id
            && string(&value, "conversation_id") == session.conversation_id,
        "Remote session rules identity changed"
    );
    validate(&value["rules"])?;
    Ok(value)
}

pub(crate) fn project_identity(team: &teams::model::Team) -> anyhow::Result<String> {
    if !team.project_id.is_empty() {
        return Ok(team.project_id.clone());
    }
    if team.project.is_empty() || !root().join("iron-rules.sqlite3").exists() {
        return Ok(String::new());
    }
    let config = crate::config::Config::load().map_err(|error| anyhow::anyhow!("{error}"))?;
    let snapshot = crate::swarm::read_catalog_snapshot(&config)?;
    let found: Vec<_> = snapshot["organization"]["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|value| string(value, "name") == team.project)
        .map(|value| string(value, "id").to_owned())
        .collect();
    ensure!(
        found.len() <= 1,
        "Project label is ambiguous; select its stable identity"
    );
    Ok(found.first().cloned().unwrap_or_default())
}

pub(crate) fn for_team(
    team: &teams::model::Team,
    member: &teams::model::Member,
) -> anyhow::Result<String> {
    let project = validate(&project(&project_identity(team)?)?["rules"])?;
    let own = match &member.session {
        Some(session) if !session.name.is_empty() => session_rules(session)?,
        _ => Vec::new(),
    };
    Ok(render(&[
        ("Project", &project),
        ("Team", &team.settings.iron_rules),
        ("Member", &member.iron_rules),
        ("Session", &own),
    ]))
}

/// Only Claude's current adapter consumes SessionStart additionalContext.
/// Other adapters retain their per-send fallback until covered by a native
/// startup/resume/clear/compact delivery contract.
pub(crate) fn record_context(record: &Value) -> anyhow::Result<(String, String)> {
    let own = validate(record.get("iron_rules").unwrap_or(&json!([])))?;
    let team = if teams::store::path().exists() {
        teams::store::Store::open(false)?.rules_member(record)?
    } else {
        None
    };
    if let Some((team, member)) = team {
        let member = team.member(&member).context("Unknown member")?;
        let id = project_identity(&team)?;
        let project = validate(&project(&id)?["rules"])?;
        return Ok((
            id,
            render(&[
                ("Project", &project),
                ("Team", &team.settings.iron_rules),
                ("Member", &member.iron_rules),
                ("Session", &own),
            ]),
        ));
    }
    let mut id = String::new();
    if root().join("iron-rules.sqlite3").exists() {
        let config = crate::config::Config::load().map_err(|error| anyhow::anyhow!("{error}"))?;
        let snapshot = crate::swarm::read_catalog_snapshot(&config)?;
        let key = format!("{}\n{}", config.host(), string(record, "name"));
        if let Some(project) = snapshot["organization"]["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|project| {
                project["sessions"].as_array().is_some_and(|sessions| {
                    sessions
                        .iter()
                        .any(|session| session.as_str() == Some(key.as_str()))
                })
            })
        {
            id = string(project, "id").to_owned();
        } else if let Some(project) =
            crate::swarm::directory_project(&config, Path::new(string(record, "launch_dir")))
        {
            id = string(&project, "project_id").to_owned();
        }
    }
    let project = validate(&project(&id)?["rules"])?;
    Ok((id, render(&[("Project", &project), ("Session", &own)])))
}

pub(crate) fn run(op: &str, input: &Value) -> anyhow::Result<Value> {
    ensure!(input.is_object(), "Rules input must be an object");
    if op != "get" {
        ensure!(
            teams::store::canonical_uuid(string(input, "request_id")),
            "A canonical request_id is required"
        );
    }
    let scope = string(input, "scope");
    if scope == "session" {
        let name = string(input, "name");
        ensure!(
            !name.is_empty() && name.len() <= 2048 && !name.chars().any(char::is_control),
            "Name the exact session"
        );
        let run = input["expected_run_id"]
            .as_str()
            .context("Supply expected_run_id")?;
        let conversation = input["expected_conversation_id"]
            .as_str()
            .context("Supply expected_conversation_id")?;
        ensure!(!run.is_empty() && run.len() <= 2048 && !run.chars().any(char::is_control), "Supply the exact nonempty run identity");
        ensure!(conversation.len() <= 2048 && !conversation.chars().any(char::is_control), "Invalid conversation identity");
        let host = string(input, "host");
        ensure!(
            host.len() <= 200 && !host.chars().any(char::is_control),
            "Invalid host identity"
        );
        if !host.is_empty() {
            ensure!(
                op == "get",
                "Remote resolution is read-only; write on the selected host"
            );
            let session = teams::model::SessionRef {
                host: host.to_owned(),
                name: name.to_owned(),
                run_id: run.to_owned(),
                conversation_id: conversation.to_owned(),
            };
            let mut value = remote_session(&session)?;
            let team = if teams::store::path().exists() {
                teams::store::Store::open(false)?.rules_session(
                    host,
                    name,
                    run,
                    conversation,
                    "",
                )?
            } else {
                None
            };
            if let Some((team, member)) = team {
                let member = team.member(&member).context("Unknown member")?;
                let project_id = project_identity(&team)?;
                let project = validate(&project(&project_id)?["rules"])?;
                let own = validate(&value["rules"])?;
                value["effective_rules"] = json!(render(&[
                    ("Project", &project),
                    ("Team", &team.settings.iron_rules),
                    ("Member", &member.iron_rules),
                    ("Session", &own)
                ]));
                value["project"] = json!(project_id);
                value["start_delivery_supported"] = json!(false);
            }
            return Ok(value);
        }
        let record = super::with_session_project_binding(name, None, run, conversation, |record| {
            ensure!(
                string(record, "run_id") == run
                    && string(record, "conversation_id") == conversation,
                "Session identity changed"
            );
            Ok(record.clone())
        })?;
        if op == "get" {
            let rules = validate(record.get("iron_rules").unwrap_or(&json!([])))?;
            let (project, effective) = record_context(&record)?;
            return Ok(
                json!({"scope":"session","name":name,"rules":rules,"revision":record["iron_rules_revision"].as_u64().unwrap_or(0),
                "run_id":run,"conversation_id":conversation,"effective_rules":effective,"project":project,
                "start_delivery_supported":start_proof_matches(&record, &effective),
                "start_project":record["iron_rules_start_project"].as_str().unwrap_or_default()}),
            );
        }
        ensure!(op == "set", "Session rules support get or set");
        let rules = validate(&input["rules"])?;
        let expected = input["expected_revision"].as_u64().context("Supply expected_revision")?;
        let replay = string(&record,"iron_rules_request") == string(input,"request_id")
            && string(&record,"iron_rules_digest") == teams::store::digest(input);
        ensure!(replay || expected == record["iron_rules_revision"].as_u64().unwrap_or(0), "Session rules changed; read them again");
        teams::commands::authorize_external_rules(input)?;
        return super::session_project::update_iron_rules(
            name,
            run,
            conversation,
            input["expected_revision"]
                .as_u64()
                .context("Supply expected_revision")?,
            &rules,
            string(input, "request_id"),
            &teams::store::digest(input),
        );
    }
    ensure!(scope == "project", "Rules scope must be project or session");
    match op {
        "get" => project(string(input, "project")),
        "set" | "migrate" => project_write(input, op == "migrate"),
        _ => anyhow::bail!("Rules operation must be get, set or migrate"),
    }
}

pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() != 2 || args[1] != "--json" {
        return Err("usage: hgs rules get|set|migrate --json (JSON stdin)".into());
    }
    let result = run(&args[0], &stdin_json()?).map_err(|error| error.to_string())?;
    println!("{result}");
    Ok(0)
}
