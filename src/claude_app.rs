//! WhiteWh fork: sessions of the Claude desktop app (its Code tab), continued
//! in Zerus.
//!
//! The app keeps one JSON file per session under
//! `<app data>/Claude/claude-code-sessions/<account>/<organization>/local_<id>.json`
//! with the native conversation (`cliSessionId`), folder, title and model, and
//! Claude Code keeps that conversation's history in
//! `~/.claude/projects/<folder>/<conversation>.jsonl`. `ls` lists the newest
//! sessions; `resume` continues one as a new tracked Zerus session with
//! `claude --resume`. Zerus never writes the app's files. Whether the app still
//! runs a session is read from the Claude processes that resume its conversation,
//! or else guessed from recent changes to its files: two processes
//! writing one conversation corrupt it, so `resume` refuses such a session
//! unless `--force` says otherwise.
use crate::{
    cli::{Error, Result},
    platform,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// At most this many sessions are listed, newest first.
const LIMIT: usize = 200;
/// At most this many session files are looked at.
const SCAN: usize = 5000;
/// A session whose files changed this recently may still run in the app.
const RUNNING_SECS: u64 = 120;

pub enum Outcome {
    Done(i32),
    /// Arguments for `hgs claude …`.
    Launch(Vec<String>),
}

pub fn dispatch(args: &[String]) -> Result<Outcome> {
    let usage = "usage: hgs claude-app ls | resume ID [-n NAME] [--launch-id UUID] [-d] [--force]";
    match args.first().map(String::as_str) {
        Some("ls") => {
            println!("{}", list(LIMIT));
            Ok(Outcome::Done(0))
        }
        Some("resume") => resume(&args[1..]).map(Outcome::Launch),
        _ => Err(Error::new(1, usage)),
    }
}

/// The app's session folder on this computer.
fn sessions_root() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("HGS_CLAUDE_APP_SESSIONS").filter(|v| !v.is_empty()) {
        return Some(dir.into());
    }
    let base = if cfg!(windows) {
        PathBuf::from(env::var_os("APPDATA")?)
    } else if cfg!(target_os = "macos") {
        platform::home_dir()?.join("Library/Application Support")
    } else {
        env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| platform::home_dir().map(|h| h.join(".config")))?
    };
    Some(base.join("Claude").join("claude-code-sessions"))
}

/// Claude Code's own home, where the app's sessions keep their history.
fn claude_home() -> PathBuf {
    env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()).map(PathBuf::from)
        .unwrap_or_else(|| platform::home_dir().unwrap_or_default().join(".claude"))
}

/// Claude Code's project folder name: every character but ASCII letters and
/// digits becomes '-'.
fn project_dir(cwd: &str) -> String {
    cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn modified(path: &Path) -> Option<u64> {
    fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Session files, newest first: the app's account and organization folders,
/// two levels deep, and at most SCAN of them.
fn files(root: &Path) -> (Vec<(u64, PathBuf)>, bool) {
    let mut found = Vec::new();
    let mut truncated = false;
    let mut folders = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = folders.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && depth < 2 {
                folders.push((path, depth + 1));
            } else if kind.is_file() && valid_id(path.file_stem().and_then(|s| s.to_str()).unwrap_or(""))
                && path.extension().is_some_and(|e| e == "json")
            {
                if found.len() >= SCAN {
                    truncated = true;
                    continue;
                }
                found.push((modified(&path).unwrap_or(0), path));
            }
        }
    }
    found.sort_by_key(|(changed, _)| std::cmp::Reverse(*changed));
    (found, truncated)
}

fn valid_id(id: &str) -> bool {
    id.strip_prefix("local_").is_some_and(|rest| !rest.is_empty() && rest.len() <= 64 && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

fn clean(value: &Value, limit: usize) -> String {
    value.as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(limit).collect()
}

/// A short session name from the title: ASCII letters and digits joined by
/// '-', else app- and the conversation's first characters.
fn suggested_name(title: &str, conversation: &str) -> String {
    let mut name = String::new();
    for word in title.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()) {
        if name.len() + word.len() + 1 > 40 {
            break;
        }
        if !name.is_empty() {
            name.push('-');
        }
        name.push_str(&word.to_ascii_lowercase());
    }
    if name.len() < 3 {
        name = format!("app-{}", &conversation[..8]);
    }
    name
}

/// Conversations that a running Claude Code process continues: the app starts
/// its sessions as `claude --resume=ID`, and Zerus as `claude --resume ID`.
fn live_conversations() -> BTreeSet<String> {
    let mut pids = Vec::new();
    #[cfg(windows)]
    if let Ok(names) = platform::process_names() {
        pids.extend(names.into_iter().filter(|(_, name)| name.eq_ignore_ascii_case("claude.exe") || name.eq_ignore_ascii_case("node.exe")).map(|(pid, _)| pid));
    }
    #[cfg(target_os = "linux")]
    if let Ok(entries) = fs::read_dir("/proc") {
        pids.extend(entries.flatten().take(65536).filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok()));
    }
    let mut found = BTreeSet::new();
    for pid in pids.into_iter().take(4096) {
        let Some(image) = platform::describe(pid) else { continue };
        let program = Path::new(&image.executable).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        if program != "claude" && program != "node" {
            continue;
        }
        let argv = image.argv.unwrap_or_default();
        for (at, arg) in argv.iter().enumerate() {
            let value = match arg.split_once('=') {
                Some(("--resume" | "--session-id", value)) => Some(value),
                _ if matches!(arg.as_str(), "--resume" | "-r" | "--session-id") => argv.get(at + 1).map(String::as_str),
                _ => None,
            };
            if let Some(id) = value.and_then(|v| uuid::Uuid::parse_str(v).ok()) {
                found.insert(id.to_string());
            }
        }
    }
    found
}

/// One listed session, or None for a file that is not one.
fn entry(path: &Path, changed: u64, live: &BTreeSet<String>) -> Option<Value> {
    if fs::metadata(path).ok()?.len() > 1024 * 1024 {
        return None;
    }
    let data: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let id = data["sessionId"].as_str().filter(|id| valid_id(id))?;
    let conversation = data["cliSessionId"].as_str().and_then(|v| uuid::Uuid::parse_str(v).ok())?.to_string();
    let cwd = data["cwd"].as_str().filter(|cwd| Path::new(cwd).is_absolute())?;
    let transcript = claude_home().join("projects").join(project_dir(cwd)).join(format!("{conversation}.jsonl"));
    let history = modified(&transcript);
    let last = changed.max(history.unwrap_or(0));
    let title = clean(&data["title"], 200);
    Some(json!({
        "id": id, "conversation": conversation, "cwd": cwd, "title": title,
        "suggested_name": suggested_name(&title, &conversation),
        "model": clean(&data["model"], 64), "archived": data["isArchived"].as_bool().unwrap_or(false),
        "last_activity": data["lastActivityAt"].as_f64().map(|ms| (ms / 1000.0) as u64).unwrap_or(last),
        "history": history.is_some(), "folder_exists": Path::new(cwd).is_dir(),
        "running": live.contains(&conversation) || now().saturating_sub(last) < RUNNING_SECS,
        "running_process": live.contains(&conversation),
    }))
}

fn list(limit: usize) -> Value {
    let Some(root) = sessions_root().filter(|root| root.is_dir()) else {
        return json!({"sessions": [], "truncated": false, "available": false});
    };
    let (found, mut truncated) = files(&root);
    let live = live_conversations();
    let mut sessions = Vec::new();
    for (changed, path) in &found {
        if sessions.len() >= limit {
            truncated = true;
            break;
        }
        if let Some(entry) = entry(path, *changed, &live) {
            sessions.push(entry);
        }
    }
    json!({"sessions": sessions, "truncated": truncated, "available": true,
           "running_rule": format!("a Claude process continues the conversation, or its files changed in the last {RUNNING_SECS} seconds")})
}

fn find(id: &str) -> Result<Value> {
    if !valid_id(id) {
        return Err(Error::new(1, "a Claude app session ID looks like local_UUID"));
    }
    let root = sessions_root().filter(|root| root.is_dir()).ok_or_else(|| Error::new(1, "The Claude app keeps no sessions on this computer."))?;
    let (found, _) = files(&root);
    let (changed, path) = found.iter().find(|(_, path)| path.file_stem().is_some_and(|s| s == id))
        .ok_or_else(|| Error::new(1, format!("No Claude app session {id} on this computer.")))?;
    entry(path, *changed, &live_conversations()).ok_or_else(|| Error::new(1, format!("The Claude app session {id} names no conversation and folder.")))
}

fn resume(args: &[String]) -> Result<Vec<String>> {
    let id = args.first().ok_or_else(|| Error::new(1, "resume needs a Claude app session ID"))?;
    let (mut name, mut launch, mut detached, mut force) = (None, None, false, false);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-n" => { name = Some(args.get(i + 1).cloned().ok_or_else(|| Error::new(1, "-n needs a name"))?); i += 1; }
            "--launch-id" => { launch = Some(args.get(i + 1).cloned().ok_or_else(|| Error::new(1, "--launch-id needs a UUID"))?); i += 1; }
            "-d" => detached = true,
            "--force" => force = true,
            other => return Err(Error::new(1, format!("unexpected argument {other}"))),
        }
        i += 1;
    }
    let session = find(id)?;
    let conversation = session["conversation"].as_str().unwrap_or_default();
    let cwd = session["cwd"].as_str().unwrap_or_default();
    if session["history"] != true {
        return Err(Error::new(1, format!("The conversation {conversation} has no history in {}.", claude_home().join("projects").display())));
    }
    if session["folder_exists"] != true {
        return Err(Error::new(1, format!("{cwd} is not a folder on this computer.")));
    }
    if session["running"] == true && !force {
        return Err(Error::new(1, if session["running_process"] == true {
            "A Claude process still continues this conversation. Stop the session in the Claude app first, or pass --force."
        } else {
            "This session may still run in the Claude app: its history changed in the last two minutes. Stop it there first, or pass --force."
        }));
    }
    // The app's sessions live in Claude Code's own home.
    let mut argv = vec![cwd.to_owned(), "--new".into(), "-n".into(),
                        name.unwrap_or_else(|| session["suggested_name"].as_str().unwrap_or_default().to_owned()),
                        "--account".into(), "native-claude".into(), "-c".into(), conversation.to_owned()];
    if let Some(id) = launch {
        argv.extend(["--launch-id".into(), id]);
    }
    if detached {
        argv.push("-d".into());
    }
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, id: &str, body: Value) {
        let dir = root.join("account").join("org");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{id}.json")), body.to_string()).unwrap();
    }

    #[test]
    fn names_follow_the_title_or_the_conversation() {
        assert_eq!(suggested_name("Fix the DX11 build: shaders", "048ba331-3768"), "fix-the-dx11-build-shaders");
        assert_eq!(suggested_name("Видео просмотр", "048ba331-3768"), "app-048ba331");
        assert_eq!(project_dir("D:\\Work\\GIT\\roguedod_s1"), "D--Work-GIT-roguedod-s1");
    }

    // One test owns the environment it sets.
    #[test]
    fn lists_and_resumes_app_sessions() {
        let apps = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let cwd = folder.path().to_str().unwrap().to_owned();
        env::set_var("HGS_CLAUDE_APP_SESSIONS", apps.path());
        env::set_var("CLAUDE_CONFIG_DIR", home.path());
        // A conversation no process on this computer continues.
        let conversation = &uuid::Uuid::new_v4().to_string();
        write(apps.path(), "local_aaa", json!({"sessionId":"local_aaa","cliSessionId":conversation,"cwd":cwd,"title":"Video view\u{7}","isArchived":false,"lastActivityAt":1_000_000.0}));
        write(apps.path(), "local_bad", json!({"sessionId":"local_bad","cliSessionId":"not-a-uuid","cwd":cwd}));
        fs::write(apps.path().join("account").join("org").join("notes.json"), "{}").unwrap();
        let listed = list(LIMIT);
        let sessions = listed["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["title"], "Video view");
        assert_eq!(sessions[0]["history"], false);
        assert!(resume(&["local_aaa".into()]).unwrap_err().message.contains("no history"));
        let history = home.path().join("projects").join(project_dir(&cwd));
        fs::create_dir_all(&history).unwrap();
        fs::write(history.join(format!("{conversation}.jsonl")), "{}\n").unwrap();
        // Just written: the app may still run it.
        assert!(resume(&["local_aaa".into()]).unwrap_err().message.contains("may still run"));
        let argv = resume(&["local_aaa".into(), "-n".into(), "video".into(), "--launch-id".into(), "x".into(), "-d".into(), "--force".into()]).unwrap();
        assert_eq!(argv, vec![cwd.clone(), "--new".into(), "-n".into(), "video".into(), "--account".into(), "native-claude".into(),
                              "-c".into(), conversation.clone(), "--launch-id".into(), "x".into(), "-d".into()]);
        assert!(resume(&["local_zzz".into()]).is_err());
        assert!(resume(&["../x".into()]).is_err());
        env::remove_var("HGS_CLAUDE_APP_SESSIONS");
        env::remove_var("CLAUDE_CONFIG_DIR");
    }
}
