//! `lore.toml`: finding, reading and refusing what it may not say.
use crate::LoreError;
use std::path::{Path, PathBuf};

pub const CONFIG_NAME: &str = "lore.toml";
pub const MODES: &[&str] = &["none", "plain", "shared-tree"];

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub root: PathBuf,
    pub dir: String,
    pub areas: Vec<String>,
    pub commit_mode: String,
    pub branch: String,
    pub remote: String,
    pub push: bool,
    pub trailers: Vec<String>,
    pub embed_provider: String,
    pub embed_url: String,
    pub embed_model: String,
    pub embed_timeout: f64,
    pub doc_prefix: Option<String>,
    pub query_prefix: Option<String>,
    pub limits: std::collections::BTreeMap<String, u64>,
    pub dedupe_cosine: f64,
    pub dedupe_jaccard: f64,
}

impl Config {
    pub fn lore_dir(&self) -> PathBuf { self.root.join(&self.dir) }
    pub fn prefixes(&self) -> (String, String) {
        let nomic = self.embed_model.contains("nomic");
        (
            self.doc_prefix.clone().unwrap_or_else(|| if nomic { "search_document: ".into() } else { String::new() }),
            self.query_prefix.clone().unwrap_or_else(|| if nomic { "search_query: ".into() } else { String::new() }),
        )
    }
}

fn slug(text: &str) -> bool {
    let count = text.chars().count();
    (1..=48).contains(&count) && text.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn find_root(start: &Path) -> Result<PathBuf, LoreError> {
    let mut here = start.to_path_buf();
    loop {
        if here.join(CONFIG_NAME).is_file() { return Ok(here); }
        if !here.pop() { break; }
    }
    Err(LoreError(format!("no {CONFIG_NAME} in {} or above - run `lore init` at the repository root", start.display())))
}

pub fn load(start: &Path) -> Result<Config, LoreError> {
    let root = find_root(start)?;
    let path = root.join(CONFIG_NAME);
    let text = std::fs::read_to_string(&path).map_err(|error| LoreError(format!("{}: {error}", path.display())))?;
    let data: toml::Value = toml::from_str(&text).map_err(|error| LoreError(format!("{}: not valid TOML: {error}", path.display())))?;
    parse(&data, &root, &path.display().to_string())
}

pub fn parse(data: &toml::Value, root: &Path, where_: &str) -> Result<Config, LoreError> {
    let mut bad = Vec::new();
    let table = data.as_table().ok_or_else(|| LoreError(format!("{where_} is not a table")))?;
    let known = ["version", "dir", "areas", "commit", "embed", "limits", "dedupe"];
    for key in table.keys() { if !known.contains(&key.as_str()) { bad.push(format!("unknown key `{key}`")); } }
    if table.get("version").and_then(toml::Value::as_integer) != Some(1) { bad.push("`version` must be 1".into()); }
    let mut config = Config {
        root: root.to_path_buf(), dir: "lore".into(), areas: Vec::new(), commit_mode: "plain".into(),
        branch: "main".into(), remote: "origin".into(), push: false, trailers: Vec::new(),
        embed_provider: "ollama".into(), embed_url: "http://127.0.0.1:11434".into(), embed_model: "nomic-embed-text".into(),
        embed_timeout: 2.0, doc_prefix: None, query_prefix: None, limits: Default::default(),
        dedupe_cosine: 0.95, dedupe_jaccard: 0.5,
    };
    if let Some(dir) = table.get("dir") {
        match dir.as_str() {
            Some(dir) if !dir.is_empty() && !dir.starts_with('/') && !dir.split(['/', '\\']).any(|part| part == "..") => {
                config.dir = dir.replace('\\', "/").trim_matches('/').to_owned();
            }
            _ => bad.push("`dir` must be a relative path inside the repository".into()),
        }
    }
    if let Some(areas) = table.get("areas").and_then(toml::Value::as_array) {
        let mut seen = std::collections::BTreeSet::new();
        for area in areas {
            let Some(area) = area.as_str() else { bad.push("`areas` must be a list of strings".into()); continue };
            if !slug(area) { bad.push(format!("area `{area}` must match ^[a-z0-9][a-z0-9-]{{0,47}}$")); }
            if area == "plans" || area == "sources" { bad.push(format!("`{area}` is reserved for {} records and is not an area", &area[..area.len() - 1])); }
            if !seen.insert(area) { bad.push("`areas` has duplicates".into()); }
            config.areas.push(area.to_owned());
        }
    } else if table.contains_key("areas") {
        bad.push("`areas` must be a list of strings".into());
    }
    if let Some(commit) = table.get("commit").and_then(toml::Value::as_table) {
        for key in commit.keys() {
            if !["mode", "branch", "remote", "push", "trailers"].contains(&key.as_str()) { bad.push(format!("unknown key `commit.{key}`")); }
        }
        if let Some(mode) = commit.get("mode").and_then(toml::Value::as_str) {
            if !MODES.contains(&mode) { bad.push(format!("`commit.mode` must be one of {}", MODES.join(", "))); }
            config.commit_mode = mode.into();
        }
        if let Some(value) = commit.get("branch").and_then(toml::Value::as_str) { config.branch = value.into(); }
        if let Some(value) = commit.get("remote").and_then(toml::Value::as_str) { config.remote = value.into(); }
        if let Some(value) = commit.get("push") { config.push = value.as_bool().unwrap_or_else(|| { bad.push("`commit.push` must be true or false".into()); false }); }
        if let Some(values) = commit.get("trailers").and_then(toml::Value::as_array) {
            config.trailers = values.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect();
        }
    } else if table.contains_key("commit") {
        bad.push("`[commit]` must be a table".into());
    }
    if let Some(embed) = table.get("embed").and_then(toml::Value::as_table) {
        for key in embed.keys() {
            if !["provider", "url", "model", "timeout", "doc_prefix", "query_prefix"].contains(&key.as_str()) { bad.push(format!("unknown key `embed.{key}`")); }
        }
        if let Some(provider) = embed.get("provider").and_then(toml::Value::as_str) {
            if provider != "ollama" && provider != "none" { bad.push("`embed.provider` must be one of ollama, none".into()); }
            config.embed_provider = provider.into();
        }
        if let Some(url) = embed.get("url").and_then(toml::Value::as_str) { config.embed_url = url.trim_end_matches('/').into(); }
        if let Some(model) = embed.get("model").and_then(toml::Value::as_str) { config.embed_model = model.into(); }
        if let Some(timeout) = embed.get("timeout").and_then(toml::Value::as_float) { config.embed_timeout = timeout; }
        if let Some(prefix) = embed.get("doc_prefix").and_then(toml::Value::as_str) { config.doc_prefix = Some(prefix.into()); }
        if let Some(prefix) = embed.get("query_prefix").and_then(toml::Value::as_str) { config.query_prefix = Some(prefix.into()); }
    }
    if let Some(limits) = table.get("limits").and_then(toml::Value::as_table) {
        for (key, value) in limits {
            match value.as_integer() {
                Some(value) if value > 0 => { config.limits.insert(key.clone(), value as u64); }
                _ => bad.push("`[limits]` must map `kind.field` to positive integers".into()),
            }
        }
    }
    if let Some(dedupe) = table.get("dedupe").and_then(toml::Value::as_table) {
        if let Some(value) = dedupe.get("cosine").and_then(toml::Value::as_float) { config.dedupe_cosine = value; }
        if let Some(value) = dedupe.get("jaccard").and_then(toml::Value::as_float) { config.dedupe_jaccard = value; }
    }
    if bad.is_empty() { Ok(config) } else { Err(LoreError(format!("{where_} is refused\n{}", bad.join("\n")))) }
}
