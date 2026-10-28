//! WhiteWh fork (owner, 2026-10-28): safe actions. A lead that allowed a
//! member's command once records its shape, and Zerus allows the next request
//! of that shape itself, once per question, instead of asking the lead again.
//! The shapes live in Zerus state (`teams/safe-actions.json`), so the lead adds
//! one without a new build.
//!
//! A shape is the command's literal text with typed slots:
//!   {path}   a path inside the member's own folder (absolute or relative)
//!   {word}   a name: letters, digits, `_ . -`
//!   {words}  names separated by spaces, commas or single quotes
//!   {guid}   a GUID
//!   {number} up to nine digits
//! Literal text matches exactly (any run of spaces as one). No slot ever takes
//! a shell metacharacter (`; | & < > $ ( ) { } ` " @ %` or a line break), so a
//! matched command is the recorded one with other names and paths, never a
//! command with something appended. A shape starts with literal text and has
//! at least twelve literal characters, so a slot alone never allows anything.
use super::super::*;
use super::model::{Member, Team};
use anyhow::{bail, ensure, Context};
use regex::Regex;
use std::path::{Component, Path, PathBuf};

const MAX_ACTIONS: usize = 200;
const MIN_LITERAL: usize = 12;

#[cfg(test)]
thread_local! {
    static PATH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_test_path(path: Option<PathBuf>) {
    PATH.with(|value| *value.borrow_mut() = path);
}

pub(crate) fn path() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = PATH.with(|path| path.borrow().clone()) {
        return path;
    }
    root().join("teams/safe-actions.json")
}

pub(crate) fn list() -> Vec<Value> {
    std::fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value["actions"].as_array().cloned())
        .unwrap_or_default()
}

fn write(actions: &[Value]) -> anyhow::Result<()> {
    let target = path();
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = target.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(&json!({"actions":actions}))?)?;
    std::fs::rename(&temp, &target)?;
    Ok(())
}

const SLOTS: &[&str] = &["{path}", "{word}", "{words}", "{guid}", "{number}"];

/// A slot's regex. A name starts with a letter, digit or `_`, so it is never
/// an option (`-x`) or `..`; a path never starts with `-`. Only `{path}`
/// captures, so the captures are the paths to check.
fn slot(slot: &str) -> &'static str {
    match slot {
        "{path}" => r#"((?:[A-Za-z]:[\\/]|[^\s\-;|&<>$(){}`"'@%!*?])[^\s;|&<>$(){}`"'@%!*?]*)"#,
        "{word}" => r"[A-Za-z0-9_][A-Za-z0-9_.\-]*",
        "{words}" => r"'?[A-Za-z0-9_][A-Za-z0-9_.\-]*(?:[ ,']+[A-Za-z0-9_][A-Za-z0-9_.\-]*)*'?",
        "{guid}" => r"[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}",
        _ => r"[0-9]{1,9}",
    }
}

/// The shape as a whole-command regex: literal characters exactly, a run of
/// spaces as any run of spaces, each slot by its type.
fn compile(shape: &str) -> anyhow::Result<Regex> {
    let shape = shape.trim();
    ensure!(!shape.is_empty() && shape.chars().count() <= 4000, "A safe action's shape is 1-4000 characters");
    ensure!(!shape.contains(['\n', '\r', '\0']), "A safe action's shape is one line");
    let mut pattern = String::from("^");
    let (mut literal, mut first, mut rest) = (0usize, true, shape);
    while let Some(next) = rest.chars().next() {
        if let Some(found) = SLOTS.iter().find(|found| rest.starts_with(**found)) {
            ensure!(!first, "A safe action's shape starts with literal text, not {found}");
            pattern.push_str(slot(found));
            rest = &rest[found.len()..];
        } else if next.is_whitespace() {
            pattern.push_str(r"\s+");
            rest = rest.trim_start();
        } else {
            literal += 1;
            pattern.push_str(&regex::escape(next.encode_utf8(&mut [0; 4])));
            rest = &rest[next.len_utf8()..];
        }
        first = false;
    }
    ensure!(literal >= MIN_LITERAL, "A safe action's shape keeps at least {MIN_LITERAL} literal characters");
    pattern.push('$');
    Regex::new(&pattern).context("The safe action's shape does not compile")
}

/// A path a slot took: inside the member's folder, no `..`, and an existing
/// path still inside once links are resolved.
fn inside(path: &str, folder: &Path) -> bool {
    if path.is_empty() || !folder.is_absolute() {
        return false;
    }
    // These slots are unquoted shell text. On Unix a backslash can disguise
    // a parent component (for example `\../other`) after shell unescaping.
    #[cfg(not(windows))]
    if path.contains('\\') {
        return false;
    }
    let candidate = Path::new(path);
    // A drive-relative Windows path depends on process state, not this folder.
    #[cfg(windows)]
    if candidate.components().any(|part| matches!(part, Component::Prefix(_))) && !candidate.is_absolute() {
        return false;
    }
    let full = if candidate.is_absolute() { candidate.to_path_buf() } else { folder.join(candidate) };
    if full.components().any(|part| matches!(part, Component::ParentDir)) {
        return false;
    }
    #[cfg(windows)]
    let normal = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_start_matches(r"\\?\").trim_end_matches('\\').to_lowercase();
    #[cfg(windows)]
    let within = |full: &Path, folder: &Path| {
        let (full, folder) = (normal(full), normal(folder));
        !folder.is_empty() && (full == folder || full.starts_with(&format!("{folder}\\")))
    };
    // Unix paths are case sensitive; a backslash is a filename character.
    #[cfg(not(windows))]
    let within = |full: &Path, folder: &Path| full.starts_with(folder);
    if !within(&full, folder) {
        return false;
    }
    let Ok(base) = std::fs::canonicalize(folder) else { return false };
    // New output paths may not exist yet, but their nearest existing ancestor
    // must not be a link outside the member's folder. Other I/O errors deny.
    let mut existing = full.as_path();
    loop {
        match std::fs::canonicalize(existing) {
            Ok(real) => return within(&real, &base),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let Some(parent) = existing.parent().filter(|parent| within(parent, folder)) else { return false };
                // A dangling link must not become an allowed missing directory.
                match std::fs::symlink_metadata(existing) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => return false,
                }
                existing = parent;
            }
            Err(_) => return false,
        }
    }
}

/// Whether one recorded action allows this command for this member.
fn allows(action: &Value, team: &Team, member: &Member, tool: &str, command: &str, folder: &Path) -> bool {
    if action["team"].as_str() != Some(team.id.as_str()) || member.id == team.lead {
        return false;
    }
    if let Some(only) = action["member"].as_str().filter(|only| !only.is_empty()) {
        if only != member.id {
            return false;
        }
    }
    if let Some(role) = action["role"].as_str().filter(|role| !role.is_empty()) {
        if !role.eq_ignore_ascii_case(&member.role) {
            return false;
        }
    }
    if let Some(wanted) = action["tool"].as_str().filter(|wanted| !wanted.is_empty()) {
        if !tool.to_lowercase().contains(&wanted.to_lowercase()) {
            return false;
        }
    }
    if command.contains(['\n', '\r', '\0']) || command.chars().count() > 8000 {
        return false;
    }
    let Ok(shape) = compile(action["shape"].as_str().unwrap_or_default()) else { return false };
    let command = command.trim();
    let Some(found) = shape.captures(command) else { return false };
    // Every path slot stays inside the member's own folder.
    found.iter().skip(1).flatten().all(|path| inside(path.as_str(), folder))
}

/// The recorded action that allows this member's command, if any.
pub(crate) fn find(team: &Team, member: &Member, tool: &str, command: &str, folder: &Path) -> Option<Value> {
    list().into_iter().find(|action| allows(action, team, member, tool, command, folder))
}

/// Adds a shape for a team (and a role or member of it), by the lead or you.
pub(crate) fn add(team: &Team, input: &Value, by: &str) -> anyhow::Result<Value> {
    let shape = input["shape"].as_str().unwrap_or_default().trim().to_owned();
    compile(&shape)?;
    let member = input["member"].as_str().unwrap_or_default();
    if !member.is_empty() {
        ensure!(team.member(member).is_some(), "Unknown member {member}");
        ensure!(member != team.lead, "The lead's own requests are not the lead's to allow");
    }
    let role = input["role"].as_str().unwrap_or_default();
    ensure!(!member.is_empty() || !role.is_empty(), "A safe action names the member or the role it allows");
    let reason = input["reason"].as_str().unwrap_or_default().trim();
    ensure!(!reason.is_empty() && reason.chars().count() <= 400, "A safe action says why it is safe (1-400 characters)");
    let mut actions = list();
    ensure!(actions.len() < MAX_ACTIONS, "Zerus keeps at most {MAX_ACTIONS} safe actions: remove one first");
    if let Some(found) = actions.iter().find(|action| action["team"] == team.id.as_str() && action["shape"] == shape.as_str()
        && action["member"] == member && action["role"] == role) {
        return Ok(found.clone());
    }
    let action = json!({"id":uuid::Uuid::new_v4().to_string(),"team":team.id,"member":member,"role":role,
        "tool":input["tool"].as_str().unwrap_or_default(),"shape":shape,"reason":reason,"by":by,"at":now()});
    actions.push(action.clone());
    write(&actions)?;
    Ok(action)
}

pub(crate) fn remove(team: &Team, id: &str) -> anyhow::Result<bool> {
    let mut actions = list();
    let before = actions.len();
    actions.retain(|action| !(action["team"] == team.id.as_str() && action["id"] == id));
    if actions.len() == before {
        bail!("No safe action {id} in this team");
    }
    write(&actions)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shape_takes_names_and_paths_but_nothing_appended() {
        let shape = compile(r"cmd /c {path}\.ci-build\qa-qtbuild.bat {words}").unwrap();
        assert!(shape.is_match(r"cmd /c D:\work\team-qa\.ci-build\qa-qtbuild.bat test_teams test_teamsui"));
        assert!(shape.is_match(r"cmd  /c D:\work\team-qa\.ci-build\qa-qtbuild.bat test_teams"));
        assert!(!shape.is_match(r"cmd /c D:\work\team-qa\.ci-build\qa-qtbuild.bat test_teams; rm -rf C:\"));
        assert!(!shape.is_match(r"cmd /c D:\work\team-qa\.ci-build\qa-qtbuild.bat test_teams | evil"));
        assert!(!shape.is_match(r"cmd /c D:\work\team-qa\.ci-build\qa-qtbuild.bat $(evil)"));
        assert!(!shape.is_match(r"cmd /c D:\work\team-qa\.ci-build\qa-qtbuild.bat test_teams --evil"));
        assert!(!shape.is_match(r"cmd /c D:\work\team-qa\.ci-build\qa-qtbuild.bat .."));
        let remove = compile("cmake -E rm -rf {path}").unwrap();
        assert!(remove.is_match(r"cmake -E rm -rf .ci-build\gui"));
        assert!(!remove.is_match(r"cmake -E rm -rf --force"));
        assert!(!remove.is_match(r"cmake -E rm -rf *"));
        assert!(!remove.is_match(r"cmake -E rm -rf %USERPROFILE%"));
        // A slot never starts a shape; a short shape allows nothing.
        assert!(compile("{path} run").is_err());
        assert!(compile("ls {path}").is_err());
    }

    #[test]
    fn paths_stay_inside_the_members_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("team-qa");
        std::fs::create_dir(&folder).unwrap();
        assert!(inside(folder.join(".ci-build").to_str().unwrap(), &folder));
        assert!(inside(".ci-build/gui", &folder));
        assert!(!inside(dir.path().join("other").to_str().unwrap(), &folder));
        assert!(!inside(folder.join("../other").to_str().unwrap(), &folder));
        assert!(!inside(dir.path().join("team-qa-evil/x").to_str().unwrap(), &folder));
        assert!(!inside("../other", &folder));
        assert!(!inside("", &folder));
        #[cfg(not(windows))]
        assert!(!inside(r"\../other", &folder));
        #[cfg(windows)]
        {
            assert!(!inside(r"D:relative", &folder));
            assert!(!inside(&format!(r"{}\..\other", folder.display()), &folder));
        }
    }

    #[cfg(unix)]
    #[test]
    fn native_case_and_links_cannot_escape_the_members_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("team-qa");
        let other = dir.path().join("other");
        std::fs::create_dir(&folder).unwrap();
        std::fs::create_dir(&other).unwrap();
        assert!(!inside(dir.path().join("TEAM-QA/new").to_str().unwrap(), &folder));
        std::os::unix::fs::symlink(&other, folder.join("escape")).unwrap();
        assert!(!inside("escape", &folder));
        assert!(!inside("escape/missing/output", &folder));
        std::os::unix::fs::symlink(dir.path().join("missing"), folder.join("dangling")).unwrap();
        assert!(!inside("dangling/output", &folder));
        std::os::unix::fs::symlink(&folder, folder.join("internal")).unwrap();
        assert!(inside("internal/new-output", &folder));
    }

    #[test]
    fn a_recorded_action_allows_its_member_and_shape_only() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("team-qa");
        std::fs::create_dir(&folder).unwrap();
        set_test_path(Some(dir.path().join("safe-actions.json")));
        let mut team = Team { id: "t1".into(), lead: "lead".into(), ..Default::default() };
        team.members.push(Member { id: "lead".into(), role: "QA tester".into(), ..Default::default() });
        team.members.push(Member { id: "qa".into(), role: "QA tester".into(), ..Default::default() });
        team.members.push(Member { id: "dev".into(), role: "Backend".into(), ..Default::default() });
        let added = add(&team, &json!({"role":"QA tester","shape":"cmake --build {path} --target {words}","reason":"builds its own test programs"}), "Lead").unwrap();
        assert!(added["id"].is_string());
        let qa = team.member("qa").unwrap().clone();
        let dev = team.member("dev").unwrap().clone();
        let command = "cmake --build .ci-build --target test_teams";
        assert!(find(&team, &qa, "shell", command, &folder).is_some());
        // Another role, another folder, another team: no.
        assert!(find(&team, &dev, "shell", command, &folder).is_none());
        assert!(find(&team, &qa, "shell", "cmake --build ../other --target test_teams", &folder).is_none());
        let other = Team { id: "t2".into(), ..team.clone() };
        assert!(find(&other, &qa, "shell", command, &folder).is_none());
        // A reason is required; the lead's own requests are never allowed this way.
        assert!(add(&team, &json!({"role":"QA tester","shape":"cargo test --locked {word}"}), "Lead").is_err());
        assert!(add(&team, &json!({"member":"lead","shape":"cargo test --locked {word}","reason":"x"}), "Lead").is_err());
        assert!(remove(&team, added["id"].as_str().unwrap()).unwrap());
        assert!(find(&team, &qa, "shell", command, &folder).is_none());
        set_test_path(None);
    }
}
