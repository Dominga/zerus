//! WhiteWh fork: the SocratiCode index of a folder (github.com/giancarloerra/socraticode).
//!
//! `hgs socraticode status PATH` reads what SocratiCode itself records: the
//! Qdrant collection and metadata point of the folder's project id, and the
//! lock of the process that watches it. SocratiCode is configured the way
//! Claude Code configures its plugin: the `env` of ~/.claude/settings.json,
//! then of the folder's .claude/settings.json and settings.local.json.
//!
//! With `--qdrant native` (off by default: SocratiCode's own Docker Qdrant),
//! Zerus runs SocratiCode on a native Qdrant
//! (%LOCALAPPDATA%\Programs\qdrant\qdrant.exe, its data in
//! %LOCALAPPDATA%\qdrant, on 127.0.0.1:6333) and a native Ollama:
//! `install-qdrant` fetches the pinned Qdrant release, `start` brings up what
//! is down and keeps one SocratiCode MCP server alive for the folder (it
//! indexes a new folder, then watches it), `stop` ends that server, `connect`
//! points the folder's agents at the same Qdrant, and `install` adds the
//! Claude Code plugin.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const RUNNER: &str = include_str!("socraticode/runner.mjs");
const PLUGIN: &str = "socraticode@socraticode";
const MARKETPLACE: &str = "giancarloerra/socraticode";
/// SocratiCode's own lock is stale when its file is not touched for this long.
const LOCK_STALE_SECS: u64 = 120;
const MANAGED_URL: &str = "http://127.0.0.1:16333";
const NATIVE_URL: &str = "http://127.0.0.1:6333";
const OLLAMA_URL: &str = "http://127.0.0.1:11434";

// The native Qdrant release, pinned: its storage is written by this version,
// and an upgrade is a decision with a re-index behind it.
const QDRANT_VERSION: &str = "1.19.1";
const QDRANT_RELEASE: &str = "https://github.com/qdrant/qdrant/releases/download/v1.19.1/qdrant-x86_64-pc-windows-msvc.zip";
const QDRANT_ZIP_BYTES: u64 = 29_671_153;
const QDRANT_ZIP_SHA256: &str = "9b6f69bd85f6abed4bc13f943099f55c6ffd55f5dd90388635320d8fbb569eb0";
const QDRANT_EXE_SHA256: &str = "b5354e3c8f9d13d92294f38d4988d4ddedc04acaa2ce51f0a2d65b30e2d4cd69";
const QDRANT_CONFIG: &str = "# Qdrant for SocratiCode, native, no Docker. Written by Zerus.\r
log_level: INFO\r
\r
storage:\r
  storage_path: ./storage\r
  snapshots_path: ./snapshots\r
  on_disk_payload: true\r
\r
service:\r
  host: 127.0.0.1\r
  http_port: 6333\r
  grpc_port: 6334\r
  enable_cors: true\r
\r
telemetry_disabled: true\r
";

pub fn dispatch(config: &Config, args: &[String], dry: bool) -> Result<i32> {
    let usage = "usage: hgs socraticode status|start|stop|connect PATH [--qdrant native|docker] | install | install-qdrant";
    let mut args = args.to_vec();
    let native_qdrant = match args.iter().position(|a| a == "--qdrant") {
        Some(at) => {
            let choice = args.get(at + 1).cloned().ok_or_else(|| Error::new(1, "--qdrant needs native or docker"))?;
            args.drain(at..at + 2);
            match choice.as_str() {
                "native" => true,
                "docker" => false,
                _ => return Err(Error::new(1, "--qdrant needs native or docker")),
            }
        }
        None => false,
    };
    let action = args.first().map(String::as_str).ok_or_else(|| Error::new(1, usage))?;
    match action {
        "install" => return install(dry),
        "install-qdrant" => return install_qdrant(dry),
        _ => {}
    }
    let folder = args.get(1).ok_or_else(|| Error::new(1, usage))?;
    let folder = resolve(folder)?;
    match action {
        "status" => {
            println!("{}", status(config, &folder, native_qdrant));
            Ok(0)
        }
        "start" => start(config, &folder, native_qdrant, dry),
        "stop" => stop(config, &folder, native_qdrant, dry),
        "connect" => connect(&folder, dry),
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

fn read_json(path: &Path) -> Value {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null)
}

/// Whether an environment name configures SocratiCode or what it reaches.
fn relevant(key: &str) -> bool {
    ["QDRANT_", "OLLAMA_", "EMBEDDING_", "SOCRATICODE_", "OPENAI_", "GOOGLE_", "LMSTUDIO_", "LITELLM_"].iter().any(|p| key.starts_with(p))
        || key == "EXTRA_EXTENSIONS"
}

/// SocratiCode's environment for the folder, as Claude Code would give it to
/// the plugin: this process, then the user's, the project's and the local
/// settings, each overriding the one before.
fn settings(folder: &str) -> BTreeMap<String, String> {
    let mut values: BTreeMap<String, String> = env::vars().filter(|(key, _)| relevant(key)).collect();
    values.extend(settings_files(folder));
    values
}

/// What the Claude Code settings files alone give SocratiCode in the folder.
fn settings_files(folder: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let mut files = Vec::new();
    if let Some(home) = platform::home_dir() {
        files.push(home.join(".claude/settings.json"));
    }
    files.push(Path::new(folder).join(".claude/settings.json"));
    files.push(Path::new(folder).join(".claude/settings.local.json"));
    for file in files {
        if let Some(env) = read_json(&file)["env"].as_object() {
            for (key, value) in env {
                if let Some(value) = value.as_str().filter(|_| relevant(key)) {
                    values.insert(key.clone(), value.to_owned());
                }
            }
        }
    }
    values
}

/// Project ids the folder may have: SOCRATICODE_PROJECT_ID, its
/// .socraticode.json projectId, else the first 12 hex digits of SHA-256 of the
/// resolved path. Windows drive letters keep the case they were typed with,
/// so both spellings are candidates.
fn project_ids(folder: &str, values: &BTreeMap<String, String>) -> Vec<String> {
    let valid = |id: &String| !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let named = values.get("SOCRATICODE_PROJECT_ID").cloned().filter(valid).or_else(|| {
        read_json(&Path::new(folder).join(".socraticode.json"))["projectId"].as_str().map(str::to_owned).filter(valid)
    });
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

fn address(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?;
    let (host, port) = authority.rsplit_once(':').map(|(h, p)| (h, p.parse().ok())).unwrap_or((authority, Some(80)));
    let host = if host == "localhost" { "127.0.0.1" } else { host };
    Some((host.to_owned(), port?))
}

/// One small HTTP/1.0 JSON exchange; None when nothing answers.
fn http(url: &str, method: &str, path: &str, body: Option<&Value>, key: &str) -> Option<(u16, Value)> {
    let (host, port) = address(url)?;
    let addr = (host.as_str(), port).to_socket_addrs().ok()?.next()?;
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

fn local(url: &str) -> bool {
    address(url).is_some_and(|(host, _)| host == "127.0.0.1")
}

/// The native Qdrant: where its program and its data live.
struct Native {
    exe: Option<PathBuf>,
    home: PathBuf,
}

fn native() -> Native {
    if cfg!(windows) {
        let root = env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
        let exe = root.join("Programs").join("qdrant").join("qdrant.exe");
        return Native { exe: exe.is_file().then_some(exe).or_else(|| platform::which("qdrant")), home: root.join("qdrant") };
    }
    let home = platform::home_dir().unwrap_or_default().join(".local/share/qdrant");
    Native { exe: platform::which("qdrant"), home }
}

/// Where SocratiCode keeps this folder's index, and how Zerus reaches it.
struct Target {
    mode: &'static str, // external (the settings name it), native (--qdrant native) or managed (Docker)
    url: String,
    key: String,
    agents: bool, // agents in the folder reach the same Qdrant
}

fn target(values: &BTreeMap<String, String>, docker: bool, native_qdrant: bool) -> Target {
    let key = values.get("QDRANT_API_KEY").cloned().unwrap_or_default();
    if values.get("QDRANT_MODE").is_some_and(|mode| mode == "external") {
        let url = values.get("QDRANT_URL").cloned().unwrap_or_else(|| NATIVE_URL.into());
        return Target { mode: "external", url, key, agents: true };
    }
    if native_qdrant {
        return Target { mode: "native", url: NATIVE_URL.into(), key, agents: false };
    }
    Target { mode: "managed", url: MANAGED_URL.into(), key, agents: docker }
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
    let data = platform::home_dir().map(|home| read_json(&home.join(".claude/plugins/installed_plugins.json"))).unwrap_or(Value::Null);
    let entry = &data["plugins"][PLUGIN];
    let version = entry.as_array().and_then(|list| list.first()).map(|e| e["version"].clone()).unwrap_or(Value::Null);
    json!({"installed": !entry.is_null(), "version": version})
}

fn ollama_url(values: &BTreeMap<String, String>) -> String {
    values.get("OLLAMA_URL").cloned().unwrap_or_else(|| OLLAMA_URL.into())
}

fn ollama(values: &BTreeMap<String, String>) -> Value {
    let provider = values.get("EMBEDDING_PROVIDER").map(String::as_str).unwrap_or("ollama");
    let url = ollama_url(values);
    let model = values.get("EMBEDDING_MODEL").cloned().unwrap_or_else(|| "nomic-embed-text".into());
    let tags = (provider == "ollama").then(|| http(&url, "GET", "/api/tags", None, "")).flatten()
        .filter(|(code, _)| *code == 200).map(|(_, data)| data);
    let present = tags.as_ref().is_some_and(|data| data["models"].as_array().into_iter().flatten()
        .any(|m| m["name"].as_str().is_some_and(|name| name == model || name.strip_suffix(":latest") == Some(&model))));
    json!({"provider": provider, "url": url, "reachable": tags.is_some(), "model": model, "model_present": present,
           "installed": platform::which("ollama").is_some()})
}

fn runner_file(config: &Config, id: &str) -> PathBuf {
    config.state.join("socraticode").join(format!("{id}.json"))
}

/// The runner this hgs started for the folder, if it still runs.
fn runner(config: &Config, id: &str) -> Value {
    let data = read_json(&runner_file(config, id));
    if data.is_null() {
        return json!({"running": false});
    }
    let pid = data["pid"].as_u64().unwrap_or(0) as u32;
    let running = platform::process_alive(pid) && !matches!(data["phase"].as_str(), Some("failed" | "stopped"));
    json!({"running": running, "phase": data["phase"], "detail": data["detail"], "error": data["error"], "updated_at": data["updated_at"]})
}

pub fn status(config: &Config, folder: &str, native_qdrant: bool) -> Value {
    let values = settings(folder);
    let ids = project_ids(folder, &values);
    let docker = platform::which("docker").is_some();
    let native = native();
    let target = target(&values, docker, native_qdrant);
    let prefix = values.get("QDRANT_COLLECTION_PREFIX").cloned().unwrap_or_default();
    let reachable = http(&target.url, "GET", "/collections", None, &target.key).is_some_and(|(code, _)| code == 200);
    let mut chosen = ids[0].clone();
    let mut index = json!({"state": "none"});
    if reachable {
        for id in &ids {
            let collection = format!("{prefix}codebase_{id}");
            let Some((200, info)) = http(&target.url, "GET", &format!("/collections/{collection}"), None, &target.key) else { continue };
            chosen = id.clone();
            let body = json!({"ids": [metadata_id(&collection)], "with_payload": ["indexingStatus", "filesTotal", "filesIndexed", "lastIndexedAt"], "with_vector": false});
            let meta = http(&target.url, "POST", &format!("/collections/{prefix}socraticode_metadata/points"), Some(&body), &target.key)
                .map(|(_, data)| data["result"][0]["payload"].clone()).unwrap_or(Value::Null);
            let state = if meta["indexingStatus"] == "in-progress" { "indexing" } else { "indexed" };
            index = json!({"state": state, "collection": collection, "chunks": info["result"]["points_count"],
                           "files_total": meta["filesTotal"], "files_indexed": meta["filesIndexed"], "last_indexed_at": meta["lastIndexedAt"]});
            break;
        }
    }
    if ids.iter().any(|id| locked(id, "index")) && index["state"] != "indexed" {
        index["state"] = json!("indexing");
    }
    json!({
        "path": folder, "project_id": chosen, "plugin": plugin(), "docker": docker,
        "qdrant": {"mode": target.mode, "url": target.url, "reachable": reachable, "agents": target.agents,
                   "native": {"installed": native.exe.is_some(), "version": QDRANT_VERSION, "installable": cfg!(all(windows, target_arch = "x86_64"))}},
        "ollama": ollama(&values), "node": platform::which("node").is_some() && platform::which("npx").is_some(),
        "index": index, "watching": ids.iter().any(|id| locked(id, "watch")), "runner": runner(config, &chosen),
    })
}

fn wait_for(probe: impl Fn() -> bool, seconds: u64) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        if probe() {
            return true;
        }
        thread::sleep(Duration::from_millis(500));
    }
    false
}

/// Starts the native Qdrant: the user's own launcher when there is one,
/// else the program in its data folder, where it reads config/config.yaml.
fn start_native(native: &Native) -> Result<()> {
    let exe = native.exe.as_ref().ok_or_else(|| Error::new(1, "Qdrant is not installed; run hgs socraticode install-qdrant."))?;
    ensure_qdrant_files(&native.home)?;
    let launcher = native.home.join("qdrant-start.cmd");
    let mut command = if cfg!(windows) && launcher.is_file() {
        let mut command = Command::new("cmd");
        command.arg("/c").arg(&launcher);
        command
    } else {
        let log = fs::OpenOptions::new().create(true).append(true).open(native.home.join("logs").join("qdrant.log"))?;
        let mut command = Command::new(exe);
        command.stdout(log.try_clone()?).stderr(log);
        command
    };
    command.current_dir(&native.home).stdin(Stdio::null());
    platform::spawn_detached(&mut command).map_err(|e| Error::new(1, format!("Qdrant did not start: {e}")))?;
    if !wait_for(|| http(NATIVE_URL, "GET", "/collections", None, "").is_some(), 30) {
        return Err(Error::new(1, format!("Qdrant did not answer at {NATIVE_URL}; see {}", native.home.join("logs").join("qdrant.log").display())));
    }
    Ok(())
}

/// Writes the native Qdrant's config and folders when they are missing; a
/// tuned config is left alone.
fn ensure_qdrant_files(home: &Path) -> Result<()> {
    for folder in ["config", "logs", "storage", "snapshots"] {
        fs::create_dir_all(home.join(folder))?;
    }
    let config = home.join("config").join("config.yaml");
    if !config.is_file() {
        fs::write(config, QDRANT_CONFIG)?;
    }
    Ok(())
}

fn start(config: &Config, folder: &str, native_qdrant: bool, dry: bool) -> Result<i32> {
    let current = status(config, folder, native_qdrant);
    let id = current["project_id"].as_str().unwrap_or_default().to_owned();
    if current["runner"]["running"] == true || current["watching"] == true {
        println!("{}", json!({"started": false, "reason": "already running", "status": current}));
        return Ok(0);
    }
    if current["node"] != true {
        return Err(Error::new(1, "SocratiCode needs Node.js with npx on this computer."));
    }
    let values = settings(folder);
    let native = native();
    let target = target(&values, current["docker"] == true, native_qdrant);
    let qdrant_down = current["qdrant"]["reachable"] != true;
    let ollama = &current["ollama"];
    let ollama_down = ollama["provider"] == "ollama" && ollama["reachable"] != true;
    if qdrant_down && target.mode == "managed" && current["docker"] != true {
        return Err(Error::new(1, "SocratiCode keeps its index in Qdrant: install Docker, or use a native Qdrant (--qdrant native)."));
    }
    if qdrant_down && target.mode == "external" && !(local(&target.url) && native.exe.is_some()) {
        return Err(Error::new(1, format!("Qdrant does not answer at {}.", target.url)));
    }
    let file = runner_file(config, &id);
    if dry {
        println!("{}", json!({"started": false, "dry_run": true, "start_qdrant": qdrant_down && target.mode != "managed", "start_ollama": ollama_down, "state": file}));
        return Ok(0);
    }
    if qdrant_down && target.mode != "managed" {
        start_native(&native)?;
    }
    // A native Ollama comes up on demand; SocratiCode would start a Docker one.
    if ollama_down && local(&ollama_url(&values)) {
        if let Some(program) = platform::which("ollama") {
            let mut command = Command::new(program);
            command.arg("serve").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            platform::spawn_detached(&mut command).map_err(|e| Error::new(1, format!("Ollama did not start: {e}")))?;
            let url = ollama_url(&values);
            if !wait_for(|| http(&url, "GET", "/api/tags", None, "").is_some(), 30) {
                return Err(Error::new(1, format!("Ollama did not answer at {url}.")));
            }
        }
    }
    fs::create_dir_all(file.parent().unwrap())?;
    let mut command = Command::new(platform::which("node").unwrap());
    command.args(["--input-type=module", "-e", RUNNER]).envs(&values);
    if target.mode == "native" {
        command.env("QDRANT_MODE", "external").env("QDRANT_URL", &target.url);
    }
    if !values.contains_key("OLLAMA_MODE") && ollama["provider"] == "ollama" && platform::which("ollama").is_some() {
        command.env("OLLAMA_MODE", "external").env("OLLAMA_URL", ollama_url(&values));
    }
    command.env("HGS_SC_PATH", folder).env("HGS_SC_STATE", &file)
        .current_dir(folder)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let child = platform::spawn_detached(&mut command).map_err(|e| Error::new(1, format!("Could not start SocratiCode: {e}")))?;
    println!("{}", json!({"started": true, "pid": child.id(), "project_id": id, "qdrant": target.mode}));
    Ok(0)
}

fn stop(config: &Config, folder: &str, native_qdrant: bool, dry: bool) -> Result<i32> {
    let current = status(config, folder, native_qdrant);
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

/// Points the folder's agents at the native Qdrant and Ollama: the variables
/// the plugin reads, in .claude/settings.local.json, which stays out of Git.
/// Variables the folder already sets are kept.
fn connect(folder: &str, dry: bool) -> Result<i32> {
    let file = Path::new(folder).join(".claude").join("settings.local.json");
    let mut data = if file.is_file() {
        serde_json::from_slice::<Value>(&fs::read(&file)?).map_err(|_| Error::new(1, format!("{} is not valid JSON.", file.display())))?
    } else {
        json!({})
    };
    // Agents get the settings files, not this process's environment.
    let values = settings_files(folder);
    let wanted = [("QDRANT_MODE", "external"), ("QDRANT_URL", NATIVE_URL), ("EMBEDDING_PROVIDER", "ollama"), ("OLLAMA_MODE", "external"),
                  ("OLLAMA_URL", OLLAMA_URL), ("EMBEDDING_MODEL", "nomic-embed-text"), ("EMBEDDING_DIMENSIONS", "768")];
    let root = data.as_object_mut().ok_or_else(|| Error::new(1, format!("{} is not a JSON object.", file.display())))?;
    let env = root.entry("env").or_insert_with(|| Value::Object(Map::new()));
    let env = env.as_object_mut().ok_or_else(|| Error::new(1, format!("env in {} is not an object.", file.display())))?;
    let mut added = Vec::new();
    for (key, value) in wanted {
        if !values.contains_key(key) && !env.contains_key(key) {
            env.insert(key.into(), json!(value));
            added.push(key);
        }
    }
    if !dry && !added.is_empty() {
        fs::create_dir_all(file.parent().unwrap())?;
        let temporary = file.with_extension("json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&data).unwrap())?;
        fs::rename(&temporary, &file)?;
    }
    println!("{}", json!({"connected": !dry, "dry_run": dry, "file": file, "added": added}));
    Ok(0)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hex(&digest.finalize()));
        }
        digest.update(&buffer[..read]);
    }
}

/// Fetches the pinned Qdrant release, checks the archive's size and both
/// SHA-256 hashes, and puts qdrant.exe where `start` looks for it.
fn install_qdrant(dry: bool) -> Result<i32> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err(Error::new(1, "Zerus installs Qdrant on Windows x86_64; elsewhere install qdrant on PATH or use Docker."));
    }
    let native = native();
    let root = env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or_else(|| Error::new(1, "LOCALAPPDATA is not set."))?;
    let destination = root.join("Programs").join("qdrant").join("qdrant.exe");
    if destination.is_file() {
        ensure_qdrant_files(&native.home)?;
        let verified = sha256_file(&destination)? == QDRANT_EXE_SHA256;
        println!("{}", json!({"installed": true, "already": true, "path": destination, "pinned": verified}));
        return Ok(0);
    }
    if dry {
        println!("{}", json!({"installed": false, "dry_run": true, "release": QDRANT_RELEASE, "path": destination}));
        return Ok(0);
    }
    let system = PathBuf::from(env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32");
    let work = tempfile::tempdir()?;
    let archive = work.path().join("qdrant.zip");
    let fetched = Command::new(system.join("curl.exe"))
        .args(["-fsSL", "--proto", "=https", "--max-filesize", "40000000", "-o"]).arg(&archive).arg(QDRANT_RELEASE)
        .stdin(Stdio::null()).status().map_err(|e| Error::new(1, format!("curl did not run: {e}")))?;
    if !fetched.success() {
        return Err(Error::new(1, format!("Could not download {QDRANT_RELEASE}.")));
    }
    let size = fs::metadata(&archive)?.len();
    let digest = sha256_file(&archive)?;
    if size != QDRANT_ZIP_BYTES || digest != QDRANT_ZIP_SHA256 {
        return Err(Error::new(1, format!("The Qdrant archive does not match its pin ({size} bytes, sha256 {digest}).")));
    }
    let unpacked = work.path().join("unpacked");
    fs::create_dir_all(&unpacked)?;
    let extracted = Command::new(system.join("tar.exe")).arg("-xf").arg(&archive).arg("-C").arg(&unpacked)
        .stdin(Stdio::null()).stdout(Stdio::null()).status().map_err(|e| Error::new(1, format!("tar did not run: {e}")))?;
    let exe = unpacked.join("qdrant.exe");
    if !extracted.success() || !exe.is_file() {
        return Err(Error::new(1, "The Qdrant archive has no qdrant.exe."));
    }
    let digest = sha256_file(&exe)?;
    if digest != QDRANT_EXE_SHA256 {
        return Err(Error::new(1, format!("qdrant.exe does not match its pin (sha256 {digest}).")));
    }
    fs::create_dir_all(destination.parent().unwrap())?;
    fs::copy(&exe, &destination)?;
    ensure_qdrant_files(&native.home)?;
    println!("{}", json!({"installed": true, "path": destination, "version": QDRANT_VERSION}));
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
        let ids = project_ids(&folder, &BTreeMap::new());
        assert_eq!(ids[0], hex(&Sha256::digest(folder.as_bytes()))[..12]);
        fs::write(dir.path().join(".socraticode.json"), br#"{"projectId":"my-app"}"#).unwrap();
        assert_eq!(project_ids(&folder, &BTreeMap::new()), vec!["my-app".to_owned()]);
        let named = BTreeMap::from([("SOCRATICODE_PROJECT_ID".to_owned(), "other".to_owned())]);
        assert_eq!(project_ids(&folder, &named), vec!["other".to_owned()]);
        fs::write(dir.path().join(".socraticode.json"), br#"{"projectId":"../bad"}"#).unwrap();
        assert_eq!(project_ids(&folder, &BTreeMap::new())[0], ids[0]);
    }

    #[test]
    fn folder_settings_override_and_name_an_external_qdrant() {
        let dir = tempfile::tempdir().unwrap();
        let folder = resolve(dir.path().to_str().unwrap()).unwrap();
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::write(dir.path().join(".claude/settings.json"), br#"{"env":{"QDRANT_MODE":"external","QDRANT_URL":"http://127.0.0.1:7333","PATH":"x"}}"#).unwrap();
        fs::write(dir.path().join(".claude/settings.local.json"), br#"{"env":{"QDRANT_URL":"http://127.0.0.1:6333"}}"#).unwrap();
        let values = settings(&folder);
        assert_eq!(values["QDRANT_URL"], "http://127.0.0.1:6333");
        assert!(!values.contains_key("PATH"));
        let named = target(&values, false, false);
        assert_eq!((named.mode, named.url.as_str(), named.agents), ("external", "http://127.0.0.1:6333", true));
        // Without a Qdrant in the settings, Docker is the default and native the choice.
        let none = BTreeMap::new();
        assert_eq!(target(&none, false, false).mode, "managed");
        assert_eq!(target(&none, false, false).url, MANAGED_URL);
        let native = target(&none, false, true);
        assert_eq!((native.mode, native.url.as_str(), native.agents), ("native", NATIVE_URL, false));
    }

    #[test]
    fn connect_adds_only_missing_variables_locally() {
        let dir = tempfile::tempdir().unwrap();
        let folder = resolve(dir.path().to_str().unwrap()).unwrap();
        fs::create_dir_all(dir.path().join(".claude")).unwrap();
        fs::write(dir.path().join(".claude/settings.local.json"), br#"{"permissions":{"allow":["Bash"]},"env":{"EMBEDDING_MODEL":"mine"}}"#).unwrap();
        connect(&folder, false).unwrap();
        let data = read_json(&dir.path().join(".claude/settings.local.json"));
        assert_eq!(data["permissions"]["allow"][0], "Bash");
        assert_eq!(data["env"]["EMBEDDING_MODEL"], "mine");
        assert_eq!(data["env"]["QDRANT_MODE"], "external");
        assert_eq!(data["env"]["QDRANT_URL"], NATIVE_URL);
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
