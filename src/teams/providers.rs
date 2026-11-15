//! Read-only provider discovery. No repository command or script is executed.
use crate::teams::store::{decode, encode, storage};
use crate::{config::Config, state::repositories::directory::Directory};
use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use zerus_team_plan::validation::{self, Diagnostic, Inspection, Selection};
use zerus_team_plan::{model::Team, problem::code, Problem};

type Result<T> = std::result::Result<T, Problem>;
fn invalid(text: impl Into<String>) -> Problem {
    Problem::new(code::INVALID_STATE, text)
}
fn digest(value: &impl serde::Serialize) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(storage)?)
    ))
}

pub(crate) fn saved(db: &Connection, team: &str) -> Result<Option<Selection>> {
    let value: Option<String> = db.query_row("SELECT CASE WHEN length(CAST(data AS BLOB))<=65536 THEN data END FROM validation_settings WHERE team=?1",[team],|r|r.get(0)).optional().map_err(storage)?;
    value.map(|s| decode(&s)).transpose()
}

fn project(config: &Config, team: &Team) -> Result<Directory> {
    let snapshot = crate::swarm::organization(config, false)
        .map_err(|_| invalid("Project catalog unavailable."))?;
    if snapshot["node_id"] != team.home {
        return Err(invalid("Team home is not this project computer."));
    }
    let projects = snapshot["organization"]["projects"]
        .as_array()
        .ok_or_else(|| invalid("Project catalog unavailable."))?;
    let mut paths = BTreeSet::new();
    for p in projects.iter().filter(|p| p["id"] == team.project) {
        for f in p["folders"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|f| f["machine_id"] == team.home)
        {
            if let Some(path) = f["path"].as_str() {
                paths.insert(path.to_owned());
            }
        }
    }
    if paths.len() != 1 {
        return Err(invalid(
            "Select exactly one local folder for the team project.",
        ));
    }
    // Only the trusted configured root is resolved. Descendants stay no-follow.
    let root = crate::platform::canonical(paths.into_iter().next().expect("one folder"))
        .map_err(|_| invalid("Project checkout unavailable."))?;
    Directory::open(&root).map_err(invalid)
}

fn optional(dir: &Directory, name: &str) -> Result<Option<Vec<u8>>> {
    dir.verify().map_err(invalid)?;
    match std::fs::symlink_metadata(dir.path.join(name)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            dir.verify().map_err(invalid)?;
            return Ok(None);
        }
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => (),
        _ => {
            return Err(invalid(
                "Configuration input must be a regular file, never a link.",
            ))
        }
    }
    #[cfg(unix)]
    let bytes = dir
        .read_file(name, validation::CONFIG_BYTES)
        .map_err(invalid)?;
    #[cfg(windows)]
    let bytes = crate::swarm::read_regular(&dir.path.join(name), validation::CONFIG_BYTES as u64)
        .map_err(|_| invalid("Configuration input is unreadable or too large."))?;
    dir.verify().map_err(invalid)?;
    Ok(Some(bytes))
}

fn toml_value(item: &toml_edit::Item, depth: usize) -> Result<Value> {
    if depth > 8 {
        return Err(invalid("Configuration nesting exceeds 8."));
    }
    if let Some(table) = item.as_table_like() {
        let mut obj = serde_json::Map::new();
        for (key, value) in table.iter() {
            obj.insert(key.into(), toml_value(value, depth + 1)?);
        }
        return Ok(Value::Object(obj));
    }
    if let Some(tables) = item.as_array_of_tables() {
        return tables
            .iter()
            .map(|t| toml_value(&toml_edit::Item::Table(t.clone()), depth + 1))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array);
    }
    if let Some(value) = item.as_value() {
        if let Some(s) = value.as_str() {
            return Ok(json!(s));
        }
        if let Some(n) = value.as_integer() {
            return Ok(json!(n));
        }
        if let Some(b) = value.as_bool() {
            return Ok(json!(b));
        }
        if let Some(a) = value.as_array() {
            return a
                .iter()
                .map(|v| toml_value(&toml_edit::Item::Value(v.clone()), depth + 1))
                .collect::<Result<Vec<_>>>()
                .map(Value::Array);
        }
    }
    Err(invalid("Unsupported configuration value."))
}

fn parse(raw: &[u8]) -> Result<validation::Config> {
    let text = std::str::from_utf8(raw).map_err(|_| invalid("Configuration is not UTF-8."))?;
    let doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| invalid("Malformed .zerus/tests.toml."))?;
    let config: validation::Config = serde_json::from_value(toml_value(doc.as_item(), 0)?)
        .map_err(|_| invalid("Unknown or missing configuration fields."))?;
    config.check().map_err(invalid)?;
    Ok(config)
}

const DETECTED: [(&str, &str, &[&str]); 4] = [
    ("Cargo.toml", "cargo", &["test", "build"]),
    ("package.json", "npm", &["test", "build"]),
    ("pytest.ini", "pytest", &["test"]),
    ("CMakeLists.txt", "cmake", &["test", "build"]),
];

pub(crate) fn inspect(
    config: &Config,
    db: &Connection,
    team: &Team,
    candidate: Option<Option<validation::Config>>,
) -> Result<Inspection> {
    let saved = saved(db, &team.id)?;
    let override_config =
        candidate.unwrap_or_else(|| saved.as_ref().and_then(|s| s.override_config.clone()));
    let mut view = Inspection {
        schema: 1,
        team: team.id.clone(),
        revision: team.revision,
        project: team.project.clone(),
        home: team.home.clone(),
        checkout: String::new(),
        source: "none".into(),
        source_digest: String::new(),
        digest: String::new(),
        valid: false,
        providers: vec![],
        diagnostics: vec![],
        override_config,
        saved,
        saved_current: false,
        unsupported: vec![
            "direct_runner".into(),
            "executable_templates".into(),
            "validation_requests".into(),
            "result_import".into(),
            "mandatory_gates".into(),
        ],
    };
    let root = match project(config, team) {
        Ok(root) => root,
        Err(p) => {
            view.diagnostics.push(Diagnostic {
                code: "project_unavailable".into(),
                message: p.message,
            });
            return Ok(view);
        }
    };
    view.checkout = root.path.display().to_string();
    let checked = (|| -> Result<()> {
        let mut inputs: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
        let file = match std::fs::symlink_metadata(root.path.join(".zerus")) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {
                let child = root.child(".zerus").map_err(invalid)?;
                let file = optional(&child, "tests.toml")?;
                child.verify().map_err(invalid)?;
                file
            }
            _ => return Err(invalid(".zerus must be a directory without symlinks.")),
        };
        inputs.insert(".zerus/tests.toml".into(), file.clone());
        let mut detected = vec![];
        for (path, provider, kinds) in DETECTED {
            let bytes = optional(&root, path)?;
            if bytes.is_some() {
                for kind in kinds {
                    detected.push(validation::Provider {
                        name: format!("{provider}-{kind}"),
                        provider: provider.into(),
                        kind: (*kind).into(),
                        scopes: vec!["full".into()],
                        reports: vec![],
                        disabled: false,
                    });
                }
            }
            inputs.insert(path.into(), bytes);
        }
        root.verify().map_err(invalid)?;
        let object = root.object().map_err(invalid)?;
        view.source_digest = digest(&(
            1,
            &team.project,
            &team.home,
            &view.checkout,
            object,
            &inputs,
        ))?;
        let providers = if let Some(raw) = file {
            view.source = "file".into();
            parse(&raw)?.provider
        } else {
            view.source = if detected.is_empty() {
                "none"
            } else {
                "detected"
            }
            .into();
            detected
        };
        let mut merged: BTreeMap<_, _> =
            providers.into_iter().map(|p| (p.name.clone(), p)).collect();
        if let Some(override_config) = &view.override_config {
            override_config.check().map_err(invalid)?;
            for p in &override_config.provider {
                merged.insert(p.name.clone(), p.clone());
            }
        }
        if merged.len() > validation::PROVIDERS {
            return Err(invalid("Effective configuration exceeds 16 providers."));
        }
        view.providers = merged.into_values().collect();
        view.digest = digest(&(1, &view.source_digest, &view.providers))?;
        view.valid = true;
        view.saved_current = view
            .saved
            .as_ref()
            .is_some_and(|s| s.source_digest == view.source_digest && s.digest == view.digest);
        Ok(())
    })();
    if let Err(p) = checked {
        view.diagnostics.push(Diagnostic {
            code: "invalid_config".into(),
            message: p.message.chars().take(1000).collect(),
        });
    }
    Ok(view)
}

pub(crate) fn write(db: &Connection, team: &str, selection: &Selection) -> Result<()> {
    db.execute("INSERT INTO validation_settings(team,data) VALUES(?1,?2) ON CONFLICT(team) DO UPDATE SET data=excluded.data",rusqlite::params![team,encode(selection)?]).map_err(storage)?;
    Ok(())
}
