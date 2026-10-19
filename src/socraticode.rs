//! WhiteWh fork: the SocratiCode index of a folder (github.com/giancarloerra/socraticode).
//!
//! `hgs socraticode status PATH` reads what SocratiCode itself records: the
//! Qdrant collection and metadata point of the folder's project id, and the
//! lock of the process that watches it. `start` keeps one SocratiCode MCP
//! server alive for the folder (it indexes a new folder, then watches it),
//! `stop` ends that server, and `install` adds the Claude Code plugin.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const RUNNER: &str = include_str!("socraticode/runner.mjs");
const PLUGIN: &str = "socraticode@socraticode";
const MARKETPLACE: &str = "giancarloerra/socraticode";
/// SocratiCode's own lock is stale when its file is not touched for this long.
const LOCK_STALE_SECS: u64 = 120;

pub fn dispatch(config: &Config, args: &[String], dry: bool) -> Result<i32> {
    let usage = "usage: hgs socraticode status|start|stop PATH | install";
    let action = args.first().map(String::as_str).ok_or_else(|| Error::new(1, usage))?;
    if action == "install" {
        return install(dry);
    }
    let folder = args.get(1).ok_or_else(|| Error::new(1, usage))?;
    let folder = resolve(folder)?;
    match action {
        "status" => {
            println!("{}", status(config, &folder));
            Ok(0)
        }
        "start" => start(config, &folder, dry),
        "stop" => stop(config, &folder, dry),
        _ => Err(Error::new(1, usage)),
    }
}

/// The folder as Node's path.resolve() spells it, which SocratiCode hashes.
fn resolve(folder: &str) -> Result<String> {
    let path = Path::new(folder);
    if !path.is_absolute() || !path.is_dir() {
        return Err(Error::new(1, format!("{folder} is not a folder.")));
    }
    let mut text = folder.to_owned();
    if cfg!(windows) {
        text = text.replace('/', "\\");
        while text.len() > 3 && text.ends_with('\\') {
            text.pop();
        }
    } else {
        while text.len() > 1 && text.ends_with('/') {
            text.pop();
        }
    }
    Ok(text)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Project ids the folder may have: its .socraticode.json projectId, else the
/// first 12 hex digits of SHA-256 of the resolved path. Windows drive letters
/// keep the case they were typed with, so both spellings are candidates.
fn project_ids(folder: &str) -> Vec<String> {
    let named = fs::read(Path::new(folder).join(".socraticode.json")).ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|data| data["projectId"].as_str().map(str::to_owned))
        .filter(|id| !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    if let Some(id) = named {
        return vec![id];
    }
    let mut spellings = vec![folder.to_owned()];
    if cfg!(windows) && folder.as_bytes().get(1) == Some(&b':') {
        for drive in [folder[..1].to_ascii_uppercase(), folder[..1].to_ascii_lowercase()] {
            let other = drive + &folder[1..];
            if !spellings.contains(&other) {
                spellings.push(other);
            }
        }
    }
    spellings.iter().map(|path| hex(&Sha256::digest(path.as_bytes()))[..12].to_owned()).collect()
}

/// SocratiCode's Qdrant: QDRANT_URL in external mode, else its managed container.
fn qdrant() -> (String, u16, String) {
    let external = env::var("QDRANT_MODE").is_ok_and(|mode| mode == "external");
    let url = env::var("QDRANT_URL").ok().filter(|_| external).unwrap_or_else(|| "http://127.0.0.1:16333".into());
    let (host, port) = address(&url).unwrap_or(("127.0.0.1".into(), 16333));
    (host, port, env::var("QDRANT_API_KEY").unwrap_or_default())
}

fn address(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?;
    let (host, port) = authority.rsplit_once(':').map(|(h, p)| (h, p.parse().ok())).unwrap_or((authority, Some(80)));
    let host = if host == "localhost" { "127.0.0.1" } else { host };
    Some((host.to_owned(), port?))
}

/// One small HTTP/1.0 JSON exchange; None when nothing answers.
fn http(host: &str, port: u16, method: &str, path: &str, body: Option<&Value>, key: &str) -> Option<(u16, Value)> {
    let addr = (host, port).to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(700)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(4))).ok()?;
    let body = body.map(Value::to_string).unwrap_or_default();
    let auth = if key.is_empty() { String::new() } else { format!("api-key: {key}\r\n") };
    write!(stream, "{method} {path} HTTP/1.0\r\nHost: {host}:{port}\r\nAccept: application/json\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).ok()?;
    let mut response = Vec::new();
    stream.take(2 * 1024 * 1024).read_to_end(&mut response).ok()?;
    let text = String::from_utf8_lossy(&response);
    let (head, body) = text.split_once("\r\n\r\n")?;
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, serde_json::from_str(body).unwrap_or(Value::Null)))
}

/// The metadata point id of a collection: SHA-256 of its name as a UUID.
fn metadata_id(collection: &str) -> String {
    let h = hex(&Sha256::digest(collection.as_bytes()));
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Whether a live SocratiCode process holds the folder's `kind` lock.
fn locked(id: &str, kind: &str) -> bool {
    let lock = env::temp_dir().join("socraticode-locks").join(format!("{id}-{kind}.lock"));
    fs::metadata(lock).and_then(|m| m.modified()).ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .is_some_and(|time| now().saturating_sub(time.as_secs()) < LOCK_STALE_SECS)
}

fn plugin() -> Value {
    let file = platform::home_dir().map(|home| home.join(".claude/plugins/installed_plugins.json"));
    let data = file.and_then(|file| fs::read(file).ok()).and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).unwrap_or(Value::Null);
    let entry = &data["plugins"][PLUGIN];
    let version = entry.as_array().and_then(|list| list.first()).map(|e| e["version"].clone()).unwrap_or(Value::Null);
    json!({"installed": !entry.is_null(), "version": version})
}

fn ollama() -> Value {
    let url = env::var("OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".into());
    let model = env::var("EMBEDDING_MODEL").unwrap_or_else(|_| "nomic-embed-text".into());
    let (host, port) = address(&url).unwrap_or(("127.0.0.1".into(), 11434));
    let tags = http(&host, port, "GET", "/api/tags", None, "").filter(|(code, _)| *code == 200).map(|(_, data)| data);
    let present = tags.as_ref().is_some_and(|data| data["models"].as_array().into_iter().flatten()
        .any(|m| m["name"].as_str().is_some_and(|name| name == model || name.strip_suffix(":latest") == Some(&model))));
    json!({"reachable": tags.is_some(), "model": model, "model_present": present})
}

fn runner_file(config: &Config, id: &str) -> PathBuf {
    config.state.join("socraticode").join(format!("{id}.json"))
}

/// The runner this hgs started for the folder, if it still runs.
fn runner(config: &Config, id: &str) -> Value {
    let data = fs::read(runner_file(config, id)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).unwrap_or(Value::Null);
    if data.is_null() {
        return json!({"running": false});
    }
    let pid = data["pid"].as_u64().unwrap_or(0) as u32;
    let running = platform::process_alive(pid) && !matches!(data["phase"].as_str(), Some("failed" | "stopped"));
    json!({"running": running, "phase": data["phase"], "detail": data["detail"], "error": data["error"], "updated_at": data["updated_at"]})
}

pub fn status(config: &Config, folder: &str) -> Value {
    let ids = project_ids(folder);
    let (host, port, key) = qdrant();
    let reachable = http(&host, port, "GET", "/collections", None, &key).is_some_and(|(code, _)| code == 200);
    let mut chosen = ids[0].clone();
    let mut index = json!({"state": "none"});
    if reachable {
        for id in &ids {
            let collection = format!("codebase_{id}");
            let Some((200, info)) = http(&host, port, "GET", &format!("/collections/{collection}"), None, &key) else { continue };
            chosen = id.clone();
            let body = json!({"ids": [metadata_id(&collection)], "with_payload": true, "with_vector": false});
            let meta = http(&host, port, "POST", "/collections/socraticode_metadata/points", Some(&body), &key)
                .map(|(_, data)| data["result"][0]["payload"].clone()).unwrap_or(Value::Null);
            let state = match meta["indexingStatus"].as_str() { Some("in-progress") => "indexing", Some("completed") => "indexed", _ => "indexed" };
            index = json!({"state": state, "collection": collection, "chunks": info["result"]["points_count"],
                           "files_total": meta["filesTotal"], "files_indexed": meta["filesIndexed"], "last_indexed_at": meta["lastIndexedAt"]});
            break;
        }
    }
    let watching = ids.iter().any(|id| locked(id, "watch"));
    let indexing = ids.iter().any(|id| locked(id, "index"));
    if indexing && index["state"] != "indexed" {
        index["state"] = json!("indexing");
    }
    json!({
        "path": folder, "project_id": chosen, "plugin": plugin(),
        "docker": platform::which("docker").is_some(),
        "qdrant": {"reachable": reachable, "url": format!("http://{host}:{port}")},
        "ollama": ollama(), "node": platform::which("node").is_some() && platform::which("npx").is_some(),
        "index": index, "watching": watching, "runner": runner(config, &chosen),
    })
}

fn start(config: &Config, folder: &str, dry: bool) -> Result<i32> {
    let current = status(config, folder);
    let id = current["project_id"].as_str().unwrap_or_default().to_owned();
    if current["runner"]["running"] == true || current["watching"] == true {
        println!("{}", json!({"started": false, "reason": "already running", "status": current}));
        return Ok(0);
    }
    if current["node"] != true {
        return Err(Error::new(1, "SocratiCode needs Node.js with npx on this computer."));
    }
    if current["docker"] != true && current["qdrant"]["reachable"] != true {
        return Err(Error::new(1, "SocratiCode keeps its index in Qdrant, which needs Docker. Install Docker and start it first."));
    }
    let file = runner_file(config, &id);
    if dry {
        println!("{}", json!({"started": false, "dry_run": true, "state": file}));
        return Ok(0);
    }
    fs::create_dir_all(file.parent().unwrap())?;
    let mut command = Command::new(platform::which("node").unwrap());
    command.args(["--input-type=module", "-e", RUNNER])
        .env("HGS_SC_PATH", folder).env("HGS_SC_STATE", &file)
        .current_dir(folder)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let child = platform::spawn_detached(&mut command).map_err(|e| Error::new(1, format!("Could not start SocratiCode: {e}")))?;
    println!("{}", json!({"started": true, "pid": child.id(), "project_id": id}));
    Ok(0)
}

fn stop(config: &Config, folder: &str, dry: bool) -> Result<i32> {
    let current = status(config, folder);
    let id = current["project_id"].as_str().unwrap_or_default();
    if current["runner"]["running"] != true {
        // A watcher of an agent session belongs to that session.
        println!("{}", json!({"stopped": false, "reason": "no SocratiCode server that Zerus started runs for this folder"}));
        return Ok(0);
    }
    if !dry {
        let file = runner_file(config, id);
        fs::write(PathBuf::from(format!("{}.stop", file.display())), b"stop")?;
    }
    println!("{}", json!({"stopped": !dry, "dry_run": dry}));
    Ok(0)
}

/// Adds the SocratiCode marketplace and installs its Claude Code plugin.
fn install(dry: bool) -> Result<i32> {
    let steps: [&[&str]; 2] = [&["plugin", "marketplace", "add", MARKETPLACE], &["plugin", "install", "--scope", "user", PLUGIN]];
    if dry {
        println!("{}", json!({"dry_run": true, "commands": steps.iter().map(|s| format!("claude {}", s.join(" "))).collect::<Vec<_>>()}));
        return Ok(0);
    }
    let mut log = Vec::new();
    for step in steps {
        let argv: Vec<String> = std::iter::once("claude").chain(step.iter().copied()).map(str::to_owned).collect();
        let mut command = platform::agent_command(&argv).map_err(|_| Error::new(1, "Claude Code (claude) is not on PATH."))?;
        let output = command.stdin(Stdio::null()).output().map_err(|e| Error::new(1, format!("claude did not run: {e}")))?;
        let text = String::from_utf8_lossy(&output.stdout).trim().chars().take(2000).collect::<String>();
        log.push(json!({"command": argv.join(" "), "ok": output.status.success(), "output": text}));
        // An existing marketplace is not an error for the install that follows.
        if !output.status.success() && step[1] != "marketplace" {
            println!("{}", json!({"installed": false, "steps": log}));
            return Ok(1);
        }
    }
    println!("{}", json!({"installed": true, "steps": log, "plugin": plugin()}));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_ids_hash_the_resolved_path_like_socraticode() {
        let dir = tempfile::tempdir().unwrap();
        let folder = resolve(dir.path().to_str().unwrap()).unwrap();
        let ids = project_ids(&folder);
        assert_eq!(ids[0], hex(&Sha256::digest(folder.as_bytes()))[..12]);
        fs::write(dir.path().join(".socraticode.json"), br#"{"projectId":"my-app"}"#).unwrap();
        assert_eq!(project_ids(&folder), vec!["my-app".to_owned()]);
        fs::write(dir.path().join(".socraticode.json"), br#"{"projectId":"../bad"}"#).unwrap();
        assert_eq!(project_ids(&folder)[0], ids[0]);
    }

    #[test]
    fn metadata_points_are_named_like_socraticode() {
        let id = metadata_id("codebase_abc");
        assert_eq!(id.len(), 36);
        assert_eq!(id.replace('-', ""), hex(&Sha256::digest(b"codebase_abc"))[..32]);
    }

    #[test]
    fn addresses_take_the_port_and_local_host() {
        assert_eq!(address("http://localhost:16333"), Some(("127.0.0.1".into(), 16333)));
        assert_eq!(address("http://qdrant.example.invalid/x"), Some(("qdrant.example.invalid".into(), 80)));
        assert_eq!(address("https://example.invalid"), None);
    }
}
