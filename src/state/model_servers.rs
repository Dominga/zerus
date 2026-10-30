//! U60: model servers - LLM inference on this computer or the LAN.
//!
//! A model server is any computer that serves an OpenAI- or Anthropic-
//! compatible API. The configuration is local to this computer (`model-servers.json`
//! in the private config folder), never committed and never shared; the API key
//! lives in the same private file. Control commands are argv templates: they are
//! run with an argv list, never through a shell, locally or over hgs's own SSH
//! transport. Without them a server is read only: its models come from
//! `GET /v1/models`.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
    state::storage,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    process::{Command, Stdio},
    time::Duration,
};

/// The private per-computer file: servers and their keys, never committed.
const FILE: &str = "model-servers.json";
const MAX_FILE: u64 = 256 * 1024;
const STATUS_SECS: u64 = 20;
const LOGS_SECS: u64 = 20;
const UP_SECS: u64 = 120;
const MAX_OUTPUT: usize = 256 * 1024;
const MAX_MODELS: usize = 500;
const MAX_LINES: usize = 2000;
const MAX_MODEL_CHARS: usize = 200;
const MAX_KEY: usize = 8192;

/// One configured server. The key is never shown by `list`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Server {
    pub name: String,
    pub base_url: String,
    /// openai or anthropic: the API the server speaks.
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// One of this computer's Machines, or an SSH alias; empty means this computer.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub ssh: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub status: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub up: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub down: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
    /// The time limit of a detached up: 30 minutes when unset.
    #[serde(skip_serializing_if = "is_zero")]
    pub up_minutes: u32,
}

#[derive(Default, Deserialize, Serialize)]
struct Store {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    servers: Vec<Server>,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

fn path(config: &Config) -> std::path::PathBuf {
    config.dir.join(FILE)
}

/// The configured servers; a missing file is no server at all.
pub(crate) fn load(config: &Config) -> Result<Vec<Server>> {
    let path = path(config);
    let opened = std::fs::OpenOptions::new().read(true).open(&path);
    let file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(Error::new(1, "Cannot read the model servers configuration")),
    };
    #[cfg(windows)]
    if !crate::private_stores::ensure_file(&path, &file).unwrap_or(false) {
        return Err(Error::new(1, "The model servers configuration must be private to this Windows user"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes).map_err(|_| Error::new(1, "Cannot read the model servers configuration"))?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(Error::new(1, "The model servers configuration is too large"));
    }
    let store: Store = serde_json::from_slice(&bytes)
        .map_err(|error| Error::new(1, format!("Invalid model servers configuration: {error}")))?;
    let mut servers = Vec::new();
    for server in store.servers {
        servers.push(checked(server)?);
    }
    Ok(servers)
}

/// Writes the servers to the private file; the key stays only there. Settings >
/// Model servers writes the configuration this way (`save --json`).
pub(crate) fn save(config: &Config, servers: &[Server]) -> Result<()> {
    for server in servers {
        checked(server.clone())?;
    }
    std::fs::create_dir_all(&config.dir).map_err(|_| Error::new(1, "Cannot create the configuration folder"))?;
    platform::private_dir(&config.dir).map_err(|error| Error::new(1, error.to_string()))?;
    let store = Store { version: 1, servers: servers.to_vec() };
    let text = serde_json::to_string_pretty(&store).map_err(|error| Error::new(1, error.to_string()))?;
    storage::atomic_private(&path(config), &text).map_err(|error| Error::new(1, error.to_string()))
}

/// A name for a server, a model or an SSH alias: no spaces, quotes or shell
/// syntax. A model may name a provider (`provider/model`).
fn safe_name(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= max
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_.:/@+".contains(ch))
}

/// A server name or an SSH alias: no spaces, colons or shell syntax.
fn safe_word(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.chars().count() <= max
        && value.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch))
}

/// One model name as a server or a user may write it (no spaces, quotes or
/// shell syntax), bounded.
pub(crate) fn checked_model(value: &str) -> Result<String> {
    let value = value.trim();
    if !safe_name(value, MAX_MODEL_CHARS) {
        return Err(Error::new(1, "A model name uses letters, digits and -_.:/@+ only"));
    }
    Ok(value.to_owned())
}

/// `http://` or `https://` followed by `HOST[:PORT][/path]`, no credentials and
/// no query.
fn checked_base_url(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("http://").or_else(|| value.strip_prefix("https://")) else { return false };
    if rest.is_empty() || rest.contains(['?', '#', '@', ' ', '\t', '\n']) || value.len() > 2000 {
        return false;
    }
    let host = rest.split('/').next().unwrap_or_default();
    let host = host.rsplit_once(':').map_or(host, |(host, port)| {
        if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
            ""
        } else {
            host
        }
    });
    !host.is_empty() && !host.starts_with(':')
}

/// Validates one server: its name, API, SSH alias, key, model names and argv
/// templates (argv lists, never shell lines; `{model}`/`{server}` only).
pub(crate) fn checked(server: Server) -> Result<Server> {
    if !safe_word(&server.name, 64) || server.name.starts_with(['-', '.']) {
        return Err(Error::new(1, "A server name uses letters, digits and -_. only"));
    }
    if !checked_base_url(&server.base_url) {
        return Err(Error::new(1, "The API URL must be http:// or https://HOST[:PORT][/path], without credentials or a query"));
    }
    if !matches!(server.kind.as_str(), "openai" | "anthropic") {
        return Err(Error::new(1, "A server kind is openai or anthropic"));
    }
    if !server.ssh.is_empty() && (!safe_word(&server.ssh, 255) || server.ssh.starts_with('-')) {
        return Err(Error::new(1, "The SSH host is one of this computer's Machines or an SSH alias"));
    }
    if server.key.chars().count() > MAX_KEY || server.key.chars().any(char::is_control) {
        return Err(Error::new(1, "The API key is at most 8192 characters without control characters"));
    }
    for (field, argv) in [("status", &server.status), ("up", &server.up), ("down", &server.down), ("logs", &server.logs)] {
        if argv.len() > 64 || argv.iter().any(|arg| arg.len() > 2000 || arg.chars().any(|ch| ch.is_control())) {
            return Err(Error::new(1, format!("The {field} command is an argv list of at most 64 arguments")));
        }
        if argv.iter().any(|arg| arg.contains('{') || arg.contains('}')) {
            for arg in argv {
                let mut rest = arg.as_str();
                while let Some(at) = rest.find('{') {
                    let tail = &rest[at + 1..];
                    let Some(end) = tail.find('}') else { return Err(Error::new(1, format!("The {field} command has an unclosed placeholder"))) };
                    if !matches!(&tail[..end], "model" | "server") {
                        return Err(Error::new(1, format!("The {field} command names a placeholder other than {{model}} or {{server}}")));
                    }
                    rest = &tail[end + 1..];
                }
            }
        }
        if !argv.is_empty() && !argv.iter().any(|arg| arg.contains("{model}")) && matches!(field, "up" | "down" | "logs") {
            return Err(Error::new(1, format!("The {field} command names the model with {{model}}")));
        }
    }
    let minutes = if server.up_minutes == 0 { 30 } else { server.up_minutes };
    if !(1..=240).contains(&minutes) {
        return Err(Error::new(1, "The up time limit is 1 to 240 minutes"));
    }
    Ok(Server { up_minutes: minutes, ..server })
}

/// The public row: the key is never included, only whether one is set.
pub(crate) fn public(server: &Server) -> Value {
    json!({
        "name": server.name,
        "base_url": server.base_url,
        "kind": server.kind,
        "ssh": server.ssh,
        "key": !server.key.is_empty(),
        "command": {
            "status": !server.status.is_empty(),
            "up": !server.up.is_empty(),
            "down": !server.down.is_empty(),
            "logs": !server.logs.is_empty(),
        },
        "up_minutes": server.up_minutes,
        // U60 (#656): the argv templates Settings edits; never the key.
        "argv": {"status": server.status, "up": server.up, "down": server.down, "logs": server.logs},
    })
}

/// U60 (#656): one server as Settings sends it. `was` is its previous name; a
/// request without `key` keeps the key stored under `was`, `"key": null`
/// removes it. Every error names the server and the field.
fn requested(mut entry: Value, stored: &[Server]) -> Result<(Server, String)> {
    let Some(object) = entry.as_object_mut() else { return Err(Error::new(1, "Each model server is a JSON object")) };
    let label = object.get("name").and_then(Value::as_str).filter(|name| !name.is_empty()).unwrap_or("(unnamed)").to_owned();
    let was = match object.remove("was") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(was)) => was,
        Some(_) => return Err(Error::new(1, format!("Server {label}: was is the server's previous name"))),
    };
    let key = match object.remove("key") {
        None => stored.iter().find(|server| !was.is_empty() && server.name == was).map(|server| server.key.clone()).unwrap_or_default(),
        Some(Value::Null) => String::new(),
        Some(Value::String(key)) => key,
        Some(_) => return Err(Error::new(1, format!("Server {label}: key is a string, or null to remove it"))),
    };
    let mut server: Server = serde_json::from_value(entry).map_err(|error| Error::new(1, format!("Server {label}: {error}")))?;
    server.key = key;
    let server = checked(server).map_err(|error| Error::new(1, format!("Server {label}: {}", error.message)))?;
    Ok((server, was))
}

/// U60 (#656): `hgs model-server save --json` - Settings replaces the whole
/// list; only you change it (the commands run on the server).
pub(crate) fn save_request(config: &Config, input: &Value) -> Result<Value> {
    super::owner_request(input, "the model servers").map_err(|error| Error::new(1, error))?;
    let stored = load(config)?;
    let entries = input["servers"].as_array().ok_or_else(|| Error::new(1, "The request carries \"servers\": a list"))?;
    if entries.len() > 64 {
        return Err(Error::new(1, "At most 64 model servers"));
    }
    let mut servers: Vec<Server> = Vec::new();
    for entry in entries {
        let (server, _) = requested(entry.clone(), &stored)?;
        if servers.iter().any(|other| other.name == server.name) {
            return Err(Error::new(1, format!("Server {}: name is already used by another server", server.name)));
        }
        servers.push(server);
    }
    save(config, &servers)?;
    Ok(json!({"status": "saved", "servers": servers.iter().map(public).collect::<Vec<_>>()}))
}

/// U60 (#656): `hgs model-server test --json` - status once for one server as
/// Settings shows it, saved or not; nothing is written.
pub(crate) fn test_request(config: &Config, input: &Value) -> Result<std::result::Result<Value, Failure>> {
    let stored = load(config)?;
    let (server, _) = requested(input.clone(), &stored)?;
    Ok(status(config, &server))
}

/// Substitutes `{model}` and `{server}` in every argument; the values are
/// validated names, so the argv reaches the server as an argv, never a shell.
fn filled(argv: &[String], model: &str, server: &str) -> Vec<String> {
    argv.iter()
        .map(|arg| arg.replace("{model}", model).replace("{server}", server))
        .collect()
}

/// How a command or a request failed. The kinds are distinct and nothing here
/// retries.
#[derive(Debug, PartialEq)]
pub(crate) enum Failure {
    Unreachable(String),
    Refused(String),
    TimedOut(String),
    Unauthorized(String),
}

impl Failure {
    fn kind(&self) -> &'static str {
        match self {
            Self::Unreachable(_) => "unreachable",
            Self::Refused(_) => "refused",
            Self::TimedOut(_) => "timed_out",
            Self::Unauthorized(_) => "unauthorized",
        }
    }
    fn detail(&self) -> &str {
        match self {
            Self::Unreachable(detail) | Self::Refused(detail) | Self::TimedOut(detail) | Self::Unauthorized(detail) => detail,
        }
    }
    pub(crate) fn value(&self) -> Value {
        json!({"status": "error", "error": self.kind(), "detail": self.detail()})
    }
}

#[derive(Clone)]
pub(crate) struct Run {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

#[cfg(test)]
type FakeCommand = Box<dyn Fn(&Server, &[String], Duration) -> std::result::Result<Run, Failure>>;

#[cfg(test)]
thread_local! {
    static FAKE: std::cell::RefCell<Option<FakeCommand>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_fake_command(fake: Option<FakeCommand>) {
    FAKE.with(|slot| *slot.borrow_mut() = fake);
}

fn execute(mut command: Command, timeout: Duration, ssh: bool) -> std::result::Result<Run, Failure> {
    command.stdin(Stdio::null());
    match platform::output_until(&mut command, timeout) {
        Ok(out) => {
            let stdout: String = String::from_utf8_lossy(&out.stdout).chars().take(MAX_OUTPUT).collect();
            let stderr: String = String::from_utf8_lossy(&out.stderr).chars().take(2000).collect();
            Ok(Run { code: out.status.code(), stdout, stderr })
        }
        Err(platform::RunError::Stopped) => {
            let where_ = if ssh { "over SSH" } else { "on this computer" };
            Err(Failure::TimedOut(format!("The command did not finish in {} s {where_}", timeout.as_secs())))
        }
        Err(platform::RunError::TooLarge) => Err(Failure::Refused("The command wrote more output than allowed".into())),
        Err(platform::RunError::Spawn(error)) | Err(platform::RunError::Io(error)) => {
            if ssh {
                Err(Failure::Unreachable(format!("SSH did not start: {error}")))
            } else {
                Err(Failure::Refused(format!("The command did not start: {error}")))
            }
        }
        Err(platform::RunError::Failed) | Err(platform::RunError::Unsupported) => Err(Failure::Refused("The command could not run".into())),
    }
}

/// The ssh command that runs one argv on the server, with BatchMode and the
/// peer's login-shell quoting. No argument passes through a local shell.
fn ssh_argv(config: &Config, server: &Server, argv: &[String]) -> Vec<String> {
    let mut ssh = vec![platform::ssh_program().to_string_lossy().into_owned()];
    ssh.extend(crate::machines::ssh_options(config, &server.ssh));
    ssh.extend(["-o".to_owned(), "BatchMode=yes".to_owned(), "-o".to_owned(), "ConnectTimeout=3".to_owned()]);
    ssh.push(server.ssh.clone());
    ssh.push(platform::remote_join(argv));
    ssh
}

/// Runs one control template: an argv locally, or the same argv over hgs's own
/// SSH transport (BatchMode, no shell on this side). A non-zero exit is a
/// refusal; SSH's own 255 is an unreachable host.
fn run_template(config: &Config, server: &Server, argv: &[String], timeout: Duration) -> std::result::Result<Run, Failure> {
    if argv.is_empty() {
        return Err(Failure::Refused("The command is empty".into()));
    }
    #[cfg(test)]
    if let Some(fake) = FAKE.with(|slot| slot.borrow().as_ref().map(|fake| fake(server, argv, timeout))) {
        return fake;
    }
    let ssh = !server.ssh.is_empty();
    let command = if ssh {
        let argv = ssh_argv(config, server, argv);
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        command.env("GIT_TERMINAL_PROMPT", "0");
        command
    } else {
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        command
    };
    let run = execute(command, timeout, ssh)?;
    if run.code == Some(0) {
        return Ok(run);
    }
    let detail = first_line(&run.stderr).or_else(|| first_line(&run.stdout)).unwrap_or_else(|| format!("The command failed ({})", run.code.map_or_else(|| "no exit code".into(), |code| code.to_string())));
    if ssh && run.code == Some(255) {
        return Err(Failure::Unreachable(detail));
    }
    Err(Failure::Refused(detail))
}

fn first_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|line| !line.is_empty()).map(|line| line.chars().take(300).collect())
}

/// The server's status: the control command when configured, else the API's
/// model list. A command may print the Zerus JSON contract or `NAME: STATE`
/// lines with an optional memory line; other lines are kept raw.
pub(crate) fn status(config: &Config, server: &Server) -> std::result::Result<Value, Failure> {
    let value = if server.status.is_empty() {
        let models = api_models(config, server)?;
        json!({"server": server.name, "source": "api", "models": models, "memory": Value::Null, "raw": []})
    } else {
        let argv = filled(&server.status, "", &server.name);
        let run = run_template(config, server, &argv, Duration::from_secs(STATUS_SECS))?;
        let parsed = parse_status(&run.stdout);
        json!({"server": server.name, "source": "command", "models": parsed.models, "memory": parsed.memory, "raw": parsed.raw})
    };
    Ok(value)
}

#[derive(Default)]
struct Parsed {
    models: Vec<Value>,
    memory: Value,
    raw: Vec<String>,
}

fn number(text: &str) -> Option<f64> {
    text.trim().trim_end_matches(|ch: char| ch.is_ascii_alphabetic()).parse::<f64>().ok()
}

fn parse_status(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            if let Some(models) = value["models"].as_array() {
                for model in models.iter().take(MAX_MODELS) {
                    let Some(name) = model["name"].as_str().filter(|name| !name.is_empty()) else { continue };
                    let state = model["state"].as_str().unwrap_or("unknown").to_ascii_lowercase();
                    parsed.models.push(json!({"name": name, "state": state}));
                }
                if let Some(memory) = value["memory"].as_object() {
                    if let (Some(free), Some(total)) = (memory["free"].as_f64(), memory["total"].as_f64()) {
                        parsed.memory = json!({"free": free, "total": total});
                    }
                }
                return parsed;
            }
        }
    }
    for line in text.lines().take(MAX_LINES) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if parsed.memory.is_null() && (lower.starts_with("memory") || lower.starts_with("free memory") || lower.starts_with("ram")) {
            let numbers: Vec<f64> = trimmed
                .split(|ch: char| !ch.is_ascii_digit() && ch != '.' && ch != '-')
                .filter(|part| !part.is_empty())
                .filter_map(number)
                .collect();
            if numbers.len() >= 2 {
                parsed.memory = json!({"free": numbers[0], "total": numbers[1]});
                continue;
            }
        }
        if let Some((name, state)) = trimmed.split_once(':') {
            let (name, state) = (name.trim(), state.trim());
            if !name.is_empty() && !name.contains(char::is_whitespace) && !state.is_empty() && state.split_whitespace().count() <= 3 {
                parsed.models.push(json!({"name": name, "state": state.to_ascii_lowercase()}));
                continue;
            }
        }
        parsed.raw.push(line.to_owned());
    }
    parsed
}

/// The API's model list, `GET /v1/models`, with the server's key. http uses the
/// small client below; https runs the system curl, whose TLS is the system's,
/// so hgs needs no TLS crate and the agents speak TLS themselves anyway.
fn api_models(config: &Config, server: &Server) -> std::result::Result<Vec<Value>, Failure> {
    let url = format!("{}/v1/models", server.base_url.trim_end_matches('/'));
    let (code, body) = if url.starts_with("https://") {
        curl_get(config, server, &url)?
    } else {
        http_get(&url, server, http_timeout())?
    };
    if !(200..300).contains(&code) {
        return Err(http_failure(code, &body));
    }
    let value: Value = serde_json::from_str(&body).map_err(|_| Failure::Refused("The model list is not JSON".into()))?;
    let data = value["data"].as_array().or_else(|| value["models"].as_array()).ok_or_else(|| Failure::Refused("The model list has no data array".into()))?;
    let mut models = Vec::new();
    for model in data.iter().take(MAX_MODELS) {
        let Some(name) = model["id"].as_str().or_else(|| model["name"].as_str()).filter(|name| !name.is_empty()) else { continue };
        models.push(json!({"name": name, "state": "available"}));
    }
    Ok(models)
}

/// What the system curl did. `Missing` (no curl on this computer) and
/// `TimedOut` are not exit codes; they are reported as their own errors.
pub(crate) enum CurlRun {
    Ran(Run),
    Missing,
    TimedOut,
    Failed(String),
}

const CURL_MISSING: &str = "curl was not found: install curl to read an https model server";

/// The system curl: `curl.exe` on Windows 10+, `curl` elsewhere.
fn curl_program() -> &'static str {
    if cfg!(windows) { "curl.exe" } else { "curl" }
}

#[cfg(test)]
pub(crate) type FakeCurl = Box<dyn Fn(&[String], Option<&str>) -> CurlRun>;

#[cfg(test)]
thread_local! {
    static FAKE_CURL: std::cell::RefCell<Option<FakeCurl>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_fake_curl(fake: Option<FakeCurl>) {
    FAKE_CURL.with(|slot| *slot.borrow_mut() = fake);
}

fn curl_run(argv: &[String], config: Option<&str>, timeout: Duration) -> CurlRun {
    #[cfg(test)]
    if let Some(fake) = FAKE_CURL.with(|slot| slot.borrow().as_ref().map(|fake| fake(argv, config))) {
        return fake;
    }
    // The header text is only read by the test fake; curl reads it from its
    // --config file.
    let _ = config;
    if platform::which(curl_program()).is_none() {
        return CurlRun::Missing;
    }
    let mut command = Command::new(curl_program());
    command.args(argv);
    match platform::output_until(&mut command, timeout) {
        Ok(out) => CurlRun::Ran(Run {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).chars().take(MAX_OUTPUT).collect(),
            stderr: String::from_utf8_lossy(&out.stderr).chars().take(2000).collect(),
        }),
        Err(platform::RunError::Stopped) => CurlRun::TimedOut,
        Err(platform::RunError::Spawn(error)) | Err(platform::RunError::Io(error)) => CurlRun::Failed(error.to_string()),
        Err(_) => CurlRun::Failed("curl could not run".into()),
    }
}

/// The key reaches curl in a private config file (`--config`), never on the
/// command line: a process listing or a log keeps only the file path.
fn curl_header(server: &Server) -> std::result::Result<Option<String>, Failure> {
    if server.key.is_empty() {
        return Ok(None);
    }
    let key = header_safe(&server.key)?.replace('\\', "\\\\").replace('"', "\\\"");
    let lines = match server.kind.as_str() {
        "anthropic" => vec![format!("header = \"x-api-key: {key}\""), "header = \"anthropic-version: 2023-06-01\"".to_owned()],
        _ => vec![format!("header = \"Authorization: Bearer {key}\"")],
    };
    Ok(Some(lines.join("\n") + "\n"))
}

/// `--write-out` appends the status code after the body; split it off.
fn split_status(stdout: &str) -> (String, Option<u16>) {
    match stdout.rsplit_once('\n') {
        Some((body, code)) if code.trim().len() == 3 && code.trim().bytes().all(|b| b.is_ascii_digit()) => {
            (body.to_owned(), code.trim().parse().ok())
        }
        _ => (stdout.to_owned(), None),
    }
}

fn curl_failure(code: Option<u16>, body: &str, stderr: &str) -> Failure {
    match code {
        Some(401 | 403) => Failure::Unauthorized(format!("The server refused the API key (HTTP {})", code.unwrap_or_default())),
        Some(code) => http_failure(code, body),
        None => Failure::Refused(first_line(stderr).or_else(|| first_line(body)).unwrap_or_else(|| "curl printed no HTTP status".into())),
    }
}

/// The https model list through the system curl: `--fail-with-body` keeps the
/// error body and `--write-out` the status code. Curl's exit codes map to the
/// same failures as the http client: 6/7 unreachable, 28 timed out, 22 an HTTP
/// error (401/403 unauthorized), anything else refused.
fn curl_get(config: &Config, server: &Server, url: &str) -> std::result::Result<(u16, String), Failure> {
    let timeout = http_timeout();
    let mut argv = vec![
        "-sS".to_owned(),
        "--max-time".to_owned(),
        timeout.as_secs().to_string(),
        "--fail-with-body".to_owned(),
        "--write-out".to_owned(),
        "\n%{http_code}".to_owned(),
    ];
    let header = curl_header(server)?;
    let mut header_file = None;
    if let Some(header) = &header {
        platform::private_dir(&config.dir).map_err(|error| Failure::Refused(error.to_string()))?;
        let mut file = tempfile::Builder::new()
            .prefix(".model-server-")
            .suffix(".conf")
            .tempfile_in(&config.dir)
            .map_err(|error| Failure::Refused(format!("The private header file could not be created: {error}")))?;
        file.write_all(header.as_bytes())
            .map_err(|error| Failure::Refused(format!("The private header file could not be written: {error}")))?;
        file.flush().ok();
        argv.push("--config".into());
        argv.push(file.path().display().to_string());
        header_file = Some(file);
    }
    argv.push(url.to_owned());
    let run = curl_run(&argv, header.as_deref(), timeout + Duration::from_secs(5));
    drop(header_file);
    match run {
        CurlRun::Missing => Err(Failure::Refused(CURL_MISSING.into())),
        CurlRun::TimedOut => Err(Failure::TimedOut(format!("The server did not answer in {} s", timeout.as_secs()))),
        CurlRun::Failed(detail) => Err(Failure::Refused(format!("curl could not run: {detail}"))),
        CurlRun::Ran(run) => {
            let (body, status) = split_status(&run.stdout);
            match run.code {
                Some(0) => status
                    .map(|code| (code, body))
                    .ok_or_else(|| Failure::Refused("curl printed no HTTP status".into())),
                Some(22) => Err(curl_failure(status, &body, &run.stderr)),
                Some(6 | 7) => Err(Failure::Unreachable(first_line(&run.stderr).unwrap_or_else(|| "The server did not answer".into()))),
                Some(28) => Err(Failure::TimedOut(format!("The server did not answer in {} s", timeout.as_secs()))),
                _ => Err(Failure::Refused(
                    first_line(&run.stderr)
                        .or_else(|| first_line(&body))
                        .unwrap_or_else(|| format!("curl failed ({})", run.code.map_or_else(|| "no exit code".into(), |code| code.to_string()))),
                )),
            }
        }
    }
}

/// The status read's time limit; tests shorten it.
fn http_timeout() -> Duration {
    #[cfg(test)]
    if let Some(timeout) = TEST_TIMEOUT.with(std::cell::Cell::get) {
        return timeout;
    }
    Duration::from_secs(STATUS_SECS)
}

#[cfg(test)]
thread_local! {
    static TEST_TIMEOUT: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_http_timeout(timeout: Option<Duration>) {
    TEST_TIMEOUT.with(|slot| slot.set(timeout));
}

/// A minimal HTTP/1.1 GET with a deadline for an http server (an https server
/// is read through the system curl, `curl_get`). The status line decides:
/// 401/403 is unauthorized, another failure is refused, a connect error
/// unreachable and a read timeout timed_out.
fn http_get(url: &str, server: &Server, timeout: Duration) -> std::result::Result<(u16, String), Failure> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(Failure::Refused("The API URL must be http:// or https://".into()));
    };
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().map_err(|_| Failure::Refused("The API port is not a number".into()))?),
        None => (authority, 80),
    };
    if host.is_empty() {
        return Err(Failure::Refused("The API URL has no host".into()));
    }
    let addresses = (host, port).to_socket_addrs().map_err(|error| Failure::Unreachable(format!("The server address did not resolve: {error}")))?;
    let mut stream = None;
    let mut last = String::new();
    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout.min(Duration::from_secs(10))) {
            Ok(found) => {
                stream = Some(found);
                break;
            }
            Err(error) => last = error.to_string(),
        }
    }
    let mut stream = stream.ok_or_else(|| Failure::Unreachable(format!("The server did not answer: {last}")))?;
    stream.set_read_timeout(Some(timeout)).map_err(|error| Failure::Refused(error.to_string()))?;
    stream.set_write_timeout(Some(timeout)).map_err(|error| Failure::Refused(error.to_string()))?;
    let mut request = format!(
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\nUser-Agent: hgs model-server\r\n"
    );
    if !server.key.is_empty() {
        match server.kind.as_str() {
            "anthropic" => {
                request.push_str(&format!("x-api-key: {}\r\nanthropic-version: 2023-06-01\r\n", header_safe(&server.key)?));
            }
            _ => request.push_str(&format!("Authorization: Bearer {}\r\n", header_safe(&server.key)?)),
        }
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).map_err(|error| Failure::Refused(error.to_string()))?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                bytes.extend_from_slice(&buffer[..n]);
                if bytes.len() > MAX_OUTPUT {
                    return Err(Failure::Refused("The model list is larger than allowed".into()));
                }
            }
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                return Err(Failure::TimedOut(format!("The server did not answer in {} s", timeout.as_secs())));
            }
            Err(error) => return Err(Failure::Refused(format!("The server closed the connection: {error}"))),
        }
    }
    parse_http(&bytes)
}

/// A key is quoted as a header value: no CR/LF or other control characters.
fn header_safe(key: &str) -> std::result::Result<String, Failure> {
    if key.chars().any(char::is_control) {
        return Err(Failure::Refused("The API key has a control character".into()));
    }
    Ok(key.to_owned())
}

fn parse_http(bytes: &[u8]) -> std::result::Result<(u16, String), Failure> {
    let text = String::from_utf8_lossy(bytes);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.split_whitespace();
    let _version = parts.next();
    let code: u16 = parts
        .next()
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| Failure::Refused("The server's answer is not HTTP".into()))?;
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        let Some((key, value)) = line.split_once(':') else { continue };
        let key = key.trim().to_ascii_lowercase();
        if key == "transfer-encoding" && value.trim().eq_ignore_ascii_case("chunked") {
            chunked = true;
        }
        if key == "content-length" {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let body = if chunked {
        decoded_chunks(body).map_err(Failure::Refused)?
    } else if let Some(length) = length {
        body[..body.len().min(length)].to_owned()
    } else {
        body.to_owned()
    };
    Ok((code, body))
}

fn decoded_chunks(mut body: &str) -> std::result::Result<String, String> {
    let mut out = String::new();
    loop {
        let Some((size, rest)) = body.split_once("\r\n") else { return Err("Invalid chunked answer".into()) };
        let size = usize::from_str_radix(size.split(';').next().unwrap_or_default().trim(), 16).map_err(|_| "Invalid chunk size".to_string())?;
        if size == 0 {
            return Ok(out);
        }
        if rest.len() < size + 2 {
            return Err("A chunk is shorter than its size".into());
        }
        out.push_str(&rest[..size]);
        body = &rest[size + 2..];
    }
}

/// `HTTP` maps the code to the failure the caller reports.
fn http_failure(code: u16, body: &str) -> Failure {
    let detail = body.chars().take(300).collect::<String>();
    if matches!(code, 401 | 403) {
        return Failure::Unauthorized(format!("The server refused the API key (HTTP {code})"));
    }
    Failure::Refused(if detail.trim().is_empty() { format!("The server answered HTTP {code}") } else { format!("HTTP {code}: {detail}") })
}

/// The receipt of `up`: it runs the template now, or starts it as a detached
/// hgs run with the server's time limit.
pub(crate) fn up(config: &Config, server: &Server, model: &str, detach: bool) -> std::result::Result<Value, Failure> {
    let model = checked_model(model).map_err(|error| Failure::Refused(error.message))?;
    if server.up.is_empty() {
        return Err(Failure::Refused(format!("{} has no up command; it is read only", server.name)));
    }
    let argv = filled(&server.up, &model, &server.name);
    if detach {
        let request = uuid::Uuid::new_v4().to_string();
        let label = format!("up {} {model}", server.name);
        let command = detached_command(config, server, &argv);
        crate::state::builds::start_for(&request, &label, &command, &std::env::current_dir().map_err(|error| Failure::Refused(error.to_string()))?, json!({"name":"","run_id":"","conversation_id":""}), server.up_minutes, &[])
            .map_err(Failure::Refused)?;
        return Ok(json!({"status":"started","server":server.name,"model":model,"request_id":request,
            "run":"hgs run","timeout_minutes":server.up_minutes}));
    }
    let run = run_template(config, server, &argv, Duration::from_secs(UP_SECS))?;
    Ok(json!({"status":"ran","server":server.name,"model":model,"exit":run.code}))
}

/// The shell line of a detached up, quoted with hgs's own quoting; for an SSH
/// server the line is the ssh command itself, with the argv quoted for the
/// peer's login shell.
fn detached_command(config: &Config, server: &Server, argv: &[String]) -> String {
    if server.ssh.is_empty() {
        return platform::join(argv);
    }
    platform::join(&ssh_argv(config, server, argv))
}

/// `down` asks first: a server may be shared, so it runs only with `--yes`
/// (the receipt says what it would do).
pub(crate) fn down(config: &Config, server: &Server, model: &str, yes: bool) -> std::result::Result<Value, Failure> {
    let model = checked_model(model).map_err(|error| Failure::Refused(error.message))?;
    if server.down.is_empty() {
        return Err(Failure::Refused(format!("{} has no down command; it is read only", server.name)));
    }
    if !yes {
        return Ok(json!({"status":"confirm","server":server.name,"model":model,
            "note":format!("Down stops a model on {}; it may be shared. Pass --yes to run the down command.", server.name)}));
    }
    let argv = filled(&server.down, &model, &server.name);
    let run = run_template(config, server, &argv, Duration::from_secs(UP_SECS))?;
    Ok(json!({"status":"ran","server":server.name,"model":model,"exit":run.code}))
}

/// `logs` is bounded: at most `lines` lines and a fixed output and time bound.
pub(crate) fn logs(config: &Config, server: &Server, model: Option<&str>, lines: usize) -> std::result::Result<Value, Failure> {
    let model = match model {
        Some(model) => checked_model(model).map_err(|error| Failure::Refused(error.message))?,
        None => String::new(),
    };
    if server.logs.is_empty() {
        return Err(Failure::Refused(format!("{} has no logs command", server.name)));
    }
    let argv = filled(&server.logs, &model, &server.name);
    let run = run_template(config, server, &argv, Duration::from_secs(LOGS_SECS))?;
    let lines = lines.clamp(1, MAX_LINES);
    let collected: Vec<String> = run.stdout.lines().map(str::to_owned).collect();
    let truncated = collected.len() > lines;
    let taken: Vec<String> = collected.into_iter().rev().take(lines).collect::<Vec<_>>().into_iter().rev().collect();
    Ok(json!({"status":"ran","server":server.name,"model":model,"lines":taken,"truncated":truncated}))
}

/// The session-scoped provider settings of an agent on a server's model: the
/// environment and extra launch arguments, never a global agent configuration.
pub(crate) fn session_settings(config: &Config, server_name: &str, agent: &str, model: &str) -> Result<(Vec<String>, Vec<String>)> {
    let server = load(config)?.into_iter().find(|server| server.name == server_name)
        .ok_or_else(|| Error::new(1, format!("Unknown model server {server_name}")))?;
    let model = checked_model(model)?;
    let base = server.base_url.trim_end_matches('/').to_owned();
    let key = if server.key.is_empty() { "local".to_owned() } else { server.key.clone() };
    let mut env = Vec::new();
    let mut args = Vec::new();
    match agent {
        "claude" => {
            if server.kind != "anthropic" {
                return Err(Error::new(1, "Claude Code needs an Anthropic-compatible server"));
            }
            env.push(format!("ANTHROPIC_BASE_URL={base}"));
            env.push(format!("ANTHROPIC_AUTH_TOKEN={key}"));
            for slot in ["ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL", "ANTHROPIC_SMALL_FAST_MODEL"] {
                env.push(format!("{slot}={model}"));
            }
            env.push("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".into());
            env.push("API_TIMEOUT_MS=1800000".into());
        }
        "codex" => {
            if server.kind != "openai" {
                return Err(Error::new(1, "Codex needs an OpenAI-compatible server"));
            }
            env.push(format!("OPENAI_BASE_URL={base}"));
            env.push(format!("OPENAI_API_KEY={key}"));
            args.extend(["--model".to_owned(), model.clone()]);
        }
        "opencode" => {
            let provider: String = format!("hgs-{}", server.name)
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '-' })
                .collect::<String>()
                .to_lowercase();
            let npm = if server.kind == "anthropic" { "@ai-sdk/anthropic" } else { "@ai-sdk/openai-compatible" };
            let content = json!({"provider": {provider.clone(): {
                "npm": npm,
                "name": server.name,
                "options": {"baseURL": base, "apiKey": key},
                "models": {model.clone(): {}},
            }}});
            env.push(format!("OPENCODE_CONFIG_CONTENT={content}"));
            args.extend(["--model".to_owned(), format!("{provider}/{model}")]);
        }
        other => return Err(Error::new(1, format!("A model server session starts claude, codex or opencode, not {other}"))),
    }
    Ok((env, args))
}

/// `hgs model-server list|status|up|down|logs`.
pub(crate) fn dispatch(config: &Config, args: &[String]) -> Result<i32> {
    let command = args.first().map(String::as_str).unwrap_or("list");
    let servers = load(config)?;
    match command {
        "" | "list" => {
            let rows: Vec<Value> = servers.iter().map(public).collect();
            if args.iter().any(|arg| arg == "--json") {
                println!("{}", json!({"servers": rows}));
            } else if rows.is_empty() {
                println!("No model servers on this computer.");
            } else {
                for row in rows {
                    let control = ["status", "up", "down", "logs"].into_iter().filter(|key| row["command"][*key] == true).collect::<Vec<_>>().join(",");
                    println!("{}\t{}\t{}{}{}", row["name"].as_str().unwrap_or_default(), row["base_url"].as_str().unwrap_or_default(), row["kind"].as_str().unwrap_or_default(),
                        if row["ssh"].as_str().unwrap_or_default().is_empty() { String::new() } else { format!(" via {}", row["ssh"].as_str().unwrap_or_default()) },
                        if control.is_empty() { String::new() } else { format!(" [{control}]") });
                }
            }
            Ok(0)
        }
        "status" | "up" | "down" | "logs" => {
            let name = args.get(1).filter(|arg| !arg.starts_with('-')).ok_or_else(|| Error::new(2, format!("hgs model-server {command} SERVER {}", if command == "status" { "" } else { "MODEL" })))?;
            let server = servers.iter().find(|server| &server.name == name).ok_or_else(|| Error::new(1, format!("Unknown model server {name}")))?;
            let json_out = args.iter().any(|arg| arg == "--json");
            let flag = |name: &str| args.iter().position(|arg| arg == name).and_then(|at| args.get(at + 1)).map(String::as_str);
            match command {
                "status" => match status(config, server) {
                    Ok(value) => {
                        if json_out { println!("{value}"); } else { print_status(&value); }
                        Ok(0)
                    }
                    Err(failure) => fail(&failure),
                },
                "up" => {
                    let model = args.get(2).filter(|arg| !arg.starts_with('-')).ok_or_else(|| Error::new(2, "hgs model-server up SERVER MODEL"))?;
                    match up(config, server, model, args.iter().any(|arg| arg == "--detach")) {
                        Ok(value) => { println!("{value}"); Ok(0) }
                        Err(failure) => fail(&failure),
                    }
                }
                "down" => {
                    let model = args.get(2).filter(|arg| !arg.starts_with('-')).ok_or_else(|| Error::new(2, "hgs model-server down SERVER MODEL [--yes]"))?;
                    let yes = args.iter().any(|arg| arg == "--yes");
                    match down(config, server, model, yes) {
                        Ok(value) => { println!("{value}"); Ok(0) }
                        Err(failure) => fail(&failure),
                    }
                }
                _ => {
                    let model = args.get(2).filter(|arg| !arg.starts_with('-'));
                    let lines = flag("--lines").and_then(|lines| lines.parse::<usize>().ok()).unwrap_or(200);
                    match logs(config, server, model.map(String::as_str), lines) {
                        Ok(value) => { println!("{value}"); Ok(0) }
                        Err(failure) => fail(&failure),
                    }
                }
            }
        }
        "save" | "test" => {
            let input = super::stdin_json().map_err(|error| Error::new(1, error))?;
            if command == "save" {
                println!("{}", save_request(config, &input)?);
                return Ok(0);
            }
            match test_request(config, &input)? {
                Ok(value) => {
                    println!("{value}");
                    Ok(0)
                }
                Err(failure) => fail(&failure),
            }
        }
        "set" => {
            let _ = servers;
            Err(Error::new(1, "The model servers are configured in Settings > Model servers; the file is model-servers.json in the private config folder"))
        }
        other => Err(Error::new(2, format!("Unknown model-server command {other}; use list, status, up, down, logs, save or test"))),
    }
}

fn fail(failure: &Failure) -> Result<i32> {
    println!("{}", failure.value());
    Ok(1)
}

fn print_status(value: &Value) {
    for model in value["models"].as_array().into_iter().flatten() {
        println!("{}: {}", model["name"].as_str().unwrap_or_default(), model["state"].as_str().unwrap_or_default());
    }
    if value["memory"].is_object() {
        println!("memory: {}/{}", value["memory"]["free"], value["memory"]["total"]);
    }
    for line in value["raw"].as_array().into_iter().flatten() {
        println!("{}", line.as_str().unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn config(dir: &std::path::Path) -> Config {
        Config {
            home: dir.display().to_string(),
            dir: dir.to_path_buf(),
            state: dir.join("state"),
            selves: Vec::new(),
            peers: Vec::new(),
            base_peers: Vec::new(),
            peers_from_environment: false,
            machines: std::collections::BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        }
    }

    fn server() -> Server {
        checked(Server {
            name: "lab".into(),
            base_url: "http://127.0.0.1:11434".into(),
            kind: "openai".into(),
            key: "sk-test".into(),
            ssh: String::new(),
            ..Default::default()
        })
        .unwrap()
    }

    /// U60 (#656): Settings saves the whole list through `save --json`: a
    /// server without `key` keeps the one stored under its previous name,
    /// `null` removes it, errors name the server and field, nothing returns a
    /// key, and only you save it.
    #[test]
    fn settings_save_keeps_or_removes_the_key_and_never_returns_it() {
        let dir = tempfile::tempdir().unwrap();
        // The test process may run inside an agent session; save is the owner's.
        crate::state::with_test_state_root(dir.path(), || {
            let config = config(dir.path());
            save(&config, &[server()]).unwrap();
            let human = |servers: Value| json!({"by": "human", "servers": servers});
            // Renamed, its key kept through `was`; a second one added without a key.
            let saved = save_request(&config, &human(json!([
                {"was": "lab", "name": "lab-a", "base_url": "http://127.0.0.1:11434", "kind": "openai",
                 "status": ["serve", "status"], "down": ["serve", "down", "{model}"]},
                {"name": "gate", "base_url": "https://gate.example/v1", "kind": "anthropic"}
            ]))).unwrap();
            assert_eq!(saved["status"], "saved");
            assert!(!saved.to_string().contains("sk-test"), "{saved}");
            assert_eq!(saved["servers"][0]["key"], true);
            assert_eq!(saved["servers"][0]["argv"]["down"], json!(["serve", "down", "{model}"]));
            let found = load(&config).unwrap();
            assert_eq!((found[0].name.as_str(), found[0].key.as_str()), ("lab-a", "sk-test"));
            assert_eq!((found[1].name.as_str(), found[1].key.as_str()), ("gate", ""));
            // A new key replaces it; null removes it.
            save_request(&config, &human(json!([{"was": "lab-a", "name": "lab-a", "base_url": "http://127.0.0.1:11434", "kind": "openai", "key": "sk-new"}]))).unwrap();
            assert_eq!(load(&config).unwrap()[0].key, "sk-new");
            save_request(&config, &human(json!([{"was": "lab-a", "name": "lab-a", "base_url": "http://127.0.0.1:11434", "kind": "openai", "key": null}]))).unwrap();
            assert_eq!(load(&config).unwrap()[0].key, "");
            // Errors name the server and the field; nothing is written.
            let refused = |servers: Value| save_request(&config, &human(servers)).unwrap_err().message;
            let error = refused(json!([{"name": "bad", "base_url": "ftp://x", "kind": "openai"}]));
            assert!(error.starts_with("Server bad: The API URL"), "{error}");
            let error = refused(json!([{"name": "bad", "base_url": "http://x", "kind": "openai", "colour": "red"}]));
            assert!(error.starts_with("Server bad: unknown field `colour`"), "{error}");
            let error = refused(json!([{"name": "x", "base_url": "http://x", "kind": "openai"}, {"name": "x", "base_url": "http://y", "kind": "openai"}]));
            assert!(error.contains("Server x: name is already used"), "{error}");
            let error = refused(json!([{"name": "x", "base_url": "http://x", "kind": "openai", "up": ["serve", "up"]}]));
            assert!(error.starts_with("Server x: The up command"), "{error}");
            assert_eq!(load(&config).unwrap().len(), 1);
            // Only you: no "by":"human", nothing saved.
            let error = save_request(&config, &json!({"servers": []})).unwrap_err().message;
            assert!(error.contains("by"), "{error}");
            assert_eq!(load(&config).unwrap().len(), 1);
        });
    }

    /// U60 (#656): `test --json` reads status once for an unsaved server,
    /// with the stored key of `was` when it sends none, and writes nothing.
    #[test]
    fn settings_test_reads_status_for_an_unsaved_server_and_saves_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let heard = seen.clone();
        let answer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap();
            *heard.lock().unwrap() = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let body = r#"{"data":[{"id":"small"},{"id":"large"}]}"#;
            write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        save(&config, &[server()]).unwrap();
        let before = std::fs::read(path(&config)).unwrap();
        let value = test_request(&config, &json!({"was": "lab", "name": "lab-edit", "base_url": format!("http://127.0.0.1:{port}"), "kind": "openai"}))
            .unwrap()
            .unwrap();
        answer.join().unwrap();
        assert_eq!(value["source"], "api");
        assert_eq!(value["models"].as_array().unwrap().len(), 2);
        assert!(seen.lock().unwrap().contains("Bearer sk-test"), "the stored key of `was` is used");
        assert_eq!(std::fs::read(path(&config)).unwrap(), before, "a test writes nothing");
        // A refused field is an error, not a status.
        assert!(test_request(&config, &json!({"name": "x", "base_url": "nope", "kind": "openai"})).unwrap_err().message.starts_with("Server x:"));
    }

    #[test]
    fn servers_come_from_the_private_file_and_list_hides_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        assert!(load(&config).unwrap().is_empty(), "a missing configuration lists none");
        save(&config, &[server()]).unwrap();
        let found = load(&config).unwrap();
        assert_eq!(found, [server()]);
        let row = public(&found[0]);
        assert_eq!(row["name"], "lab");
        assert_eq!(row["key"], true, "list says a key is set");
        assert!(!row.to_string().contains("sk-test"), "the key is never shown: {row}");
        #[cfg(windows)]
        assert!(platform::verify_private(&path(&config), platform::Private::File).unwrap());
        // https is accepted and saved: many LAN gateways serve TLS only.
        let mut https = server();
        https.base_url = "https://192.0.2.10:8443/api".into();
        assert_eq!(checked(https.clone()).unwrap(), https);
        save(&config, &[https.clone()]).unwrap();
        assert_eq!(load(&config).unwrap(), [https]);
        save(&config, &[server()]).unwrap();
        // Invalid fields are refused before anything is written.
        let broken = |change: &dyn Fn(&mut Server)| {
            let mut server = server();
            change(&mut server);
            checked(server).unwrap_err().to_string()
        };
        assert!(broken(&|s| s.name = "a b".into()).contains("server name"));
        assert!(broken(&|s| s.base_url = "ftp://lab".into()).contains("http://"));
        assert!(broken(&|s| s.base_url = "https://user:key@lab".into()).contains("http://"));
        assert!(broken(&|s| s.kind = "grpc".into()).contains("openai or anthropic"));
        assert!(broken(&|s| s.ssh = "-oProxyCommand".into()).contains("SSH host"));
        assert!(broken(&|s| s.up = vec!["serve".into(), "up".into()]).contains("{model}"));
        assert!(broken(&|s| s.up = vec!["serve".into(), "{host}".into()]).contains("placeholder"));
    }

    /// The rejected gap: https servers are read through the system curl. The key
    /// travels in the private `--config` file, never on the command line; curl's
    /// exit codes keep the four failures distinct, and a missing curl is refused
    /// by name (sessions still work because the agents speak TLS themselves).
    #[test]
    fn an_https_server_is_read_with_curl_and_the_key_stays_off_the_argv() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        std::fs::create_dir_all(&config.dir).unwrap();
        let mut server = server();
        server.base_url = "https://192.0.2.10:8443".into();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(Vec<String>, Option<String>)>::new()));
        let record = seen.clone();
        set_fake_curl(Some(Box::new(move |argv: &[String], header: Option<&str>| {
            let at = argv.iter().position(|arg| arg == "--config").expect("a key needs --config");
            let path = std::path::Path::new(&argv[at + 1]);
            assert!(path.is_file(), "the private header file exists while curl runs: {path:?}");
            assert_eq!(std::fs::read_to_string(path).unwrap(), header.unwrap(), "curl reads the key from the private file");
            record.borrow_mut().push((argv.to_vec(), header.map(str::to_owned)));
            CurlRun::Ran(Run { code: Some(0), stdout: "{\"data\":[{\"id\":\"small\"}]}\n200".into(), stderr: String::new() })
        })));
        let value = status(&config, &server).unwrap();
        assert_eq!(value["source"], "api");
        assert_eq!(value["models"], json!([{"name":"small","state":"available"}]));
        let (argv, header) = seen.borrow()[0].clone();
        let line = argv.join(" ");
        assert!(line.contains("--fail-with-body") && line.contains("--max-time"), "{argv:?}");
        assert!(argv.last().unwrap().ends_with("/v1/models"), "{argv:?}");
        assert!(!line.contains("sk-test"), "the key never reaches the argv: {argv:?}");
        assert!(header.as_deref().unwrap().contains("Authorization: Bearer sk-test"), "{header:?}");
        // 401 is unauthorized; a timeout, a connect failure and a missing curl
        // are their own errors.
        set_fake_curl(Some(Box::new(|_: &[String], _: Option<&str>| {
            CurlRun::Ran(Run { code: Some(22), stdout: "{\"error\":\"no key\"}\n401".into(), stderr: String::new() })
        })));
        assert_eq!(status(&config, &server).unwrap_err().kind(), "unauthorized");
        set_fake_curl(Some(Box::new(|_: &[String], _: Option<&str>| CurlRun::TimedOut)));
        assert_eq!(status(&config, &server).unwrap_err().kind(), "timed_out");
        set_fake_curl(Some(Box::new(|_: &[String], _: Option<&str>| {
            CurlRun::Ran(Run { code: Some(7), stdout: String::new(), stderr: "Failed to connect".into() })
        })));
        assert_eq!(status(&config, &server).unwrap_err().kind(), "unreachable");
        set_fake_curl(Some(Box::new(|_: &[String], _: Option<&str>| CurlRun::Missing)));
        let missing = status(&config, &server).unwrap_err();
        assert_eq!(missing.kind(), "refused");
        assert!(missing.detail().contains("curl was not found"), "{missing:?}");
        // A server without a key passes no --config.
        set_fake_curl(Some(Box::new(|argv: &[String], header: Option<&str>| {
            assert!(header.is_none() && !argv.iter().any(|arg| arg == "--config"), "{argv:?}");
            CurlRun::Ran(Run { code: Some(0), stdout: "{\"data\":[]}\n200".into(), stderr: String::new() })
        })));
        let mut open = server.clone();
        open.key.clear();
        assert_eq!(status(&config, &open).unwrap()["models"], json!([]));
        set_fake_curl(None);
    }

    #[test]
    fn a_status_command_runs_with_argv_and_reads_the_contract_or_the_lines() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let mut server = server();
        server.ssh = "labhost".into();
        server.status = vec!["serve".into(), "status".into()];
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let record = seen.clone();
        set_fake_command(Some(Box::new(move |server: &Server, argv: &[String], _| {
            record.borrow_mut().push((server.name.clone(), server.ssh.clone(), argv.to_vec()));
            Ok(Run { code: Some(0), stdout: r#"{"models":[{"name":"small","state":"RUNNING"}],"memory":{"free":12.5,"total":64}}"#.into(), stderr: String::new() })
        })));
        let value = status(&config, &server).unwrap();
        assert_eq!(value["source"], "command");
        assert_eq!(value["models"], json!([{"name":"small","state":"running"}]));
        assert_eq!(value["memory"], json!({"free":12.5,"total":64.0}));
        assert_eq!(*seen.borrow(), [("lab".to_owned(), "labhost".to_owned(), vec!["serve".to_owned(), "status".to_owned()])]);
        // The command reaches the server over hgs's SSH transport: BatchMode,
        // the host, and the argv as one quoted remote command.
        let ssh = ssh_argv(&config, &server, &["serve".to_owned(), "status".to_owned()]);
        assert!(ssh.iter().any(|arg| arg == "BatchMode=yes"), "{ssh:?}");
        assert!(ssh.contains(&"labhost".to_owned()), "{ssh:?}");
        assert_eq!(ssh.last().unwrap(), "serve status", "{ssh:?}");
        // The line format, an unknown line kept raw, and a memory line.
        set_fake_command(Some(Box::new(|_: &Server, _: &[String], _| {
            Ok(Run { code: Some(0), stdout: "small: ready\nbig: STOPPED\nqueue depth 3\nmemory: 8/32 GB\n".into(), stderr: String::new() })
        })));
        let value = status(&config, &server).unwrap();
        assert_eq!(value["models"], json!([{"name":"small","state":"ready"},{"name":"big","state":"stopped"}]));
        assert_eq!(value["memory"], json!({"free":8.0,"total":32.0}));
        assert_eq!(value["raw"], json!(["queue depth 3"]));
        // A refusal and an unreachable host are distinct.
        set_fake_command(Some(Box::new(|_: &Server, _: &[String], _| Err(Failure::Refused("no such model".into())))));
        assert_eq!(status(&config, &server).unwrap_err().kind(), "refused");
        set_fake_command(Some(Box::new(|_: &Server, _: &[String], _| Err(Failure::Unreachable("ssh: connect refused".into())))));
        assert_eq!(status(&config, &server).unwrap_err().kind(), "unreachable");
        set_fake_command(None);
    }

    #[test]
    fn a_read_only_server_lists_its_models_over_http() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{\"data\":[{\"id\":\"small\"},{\"id\":\"big\"}]}").unwrap();
        });
        let mut server = server();
        server.base_url = format!("http://127.0.0.1:{port}");
        let value = status(&config, &server).unwrap();
        handle.join().unwrap();
        assert_eq!(value["source"], "api");
        assert_eq!(value["models"], json!([{"name":"small","state":"available"},{"name":"big","state":"available"}]));
    }

    #[test]
    fn http_failures_are_distinct_and_never_retried() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        // Unauthorized.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n").unwrap();
        });
        let mut server = server();
        server.base_url = format!("http://127.0.0.1:{port}");
        assert_eq!(status(&config, &server).unwrap_err().kind(), "unauthorized");
        handle.join().unwrap();
        // Refused (HTTP 500).
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\n\r\nboom").unwrap();
        });
        server.base_url = format!("http://127.0.0.1:{port}");
        assert_eq!(status(&config, &server).unwrap_err().kind(), "refused");
        handle.join().unwrap();
        // Timed out: the port is open but never answers.
        set_http_timeout(Some(Duration::from_millis(300)));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(2));
        });
        server.base_url = format!("http://127.0.0.1:{port}");
        let failure = status(&config, &server).unwrap_err();
        assert_eq!(failure.kind(), "timed_out");
        handle.join().unwrap();
        set_http_timeout(None);
        // Unreachable: a port nothing listens on.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        server.base_url = format!("http://127.0.0.1:{port}");
        assert_eq!(status(&config, &server).unwrap_err().kind(), "unreachable");
    }

    #[test]
    fn up_down_and_logs_are_bounded_and_ask_first() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let mut server = server();
        server.up = vec!["serve".into(), "up".into(), "--name".into(), "{model}".into()];
        server.down = vec!["serve".into(), "down".into(), "{model}".into()];
        server.logs = vec!["serve".into(), "logs".into(), "{model}".into()];
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let record = seen.clone();
        set_fake_command(Some(Box::new(move |_: &Server, argv: &[String], _| {
            record.borrow_mut().push(argv.to_vec());
            Ok(Run { code: Some(0), stdout: (0..100).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n"), stderr: String::new() })
        })));
        // A model name with shell syntax never reaches the command.
        assert!(up(&config, &server, "small; rm -rf /", false).unwrap_err().kind() == "refused");
        let started = up(&config, &server, "small", false).unwrap();
        assert_eq!((started["status"].as_str(), started["model"].as_str()), (Some("ran"), Some("small")));
        assert_eq!(seen.borrow()[0], ["serve", "up", "--name", "small"]);
        // down asks first and says so, and only --yes runs it.
        let asked = down(&config, &server, "small", false).unwrap();
        assert_eq!(asked["status"], "confirm");
        assert!(asked["note"].as_str().unwrap().contains("shared"));
        assert_eq!(seen.borrow().len(), 1, "nothing ran on the question");
        let ran = down(&config, &server, "small", true).unwrap();
        assert_eq!((ran["status"].as_str(), seen.borrow().len()), (Some("ran"), 2));
        // logs keeps the last lines only, and says it was truncated.
        let logs_value = logs(&config, &server, Some("small"), 3).unwrap();
        assert_eq!(logs_value["lines"], json!(["line 97", "line 98", "line 99"]));
        assert_eq!(logs_value["truncated"], true);
        set_fake_command(None);
    }

    #[test]
    fn a_detached_up_runs_as_an_hgs_run_with_the_time_limit() {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let config = config(&dir.path().join("config"));
            std::fs::create_dir_all(&config.dir).unwrap();
            let mut server = server();
            server.up = vec!["serve".into(), "up".into(), "{model}".into()];
            server.up_minutes = 7;
            crate::state::builds::INLINE_RUNNER.with(|flag| flag.set(true));
            let started = up(&config, &server, "small", true).unwrap();
            assert_eq!(started["status"], "started");
            assert_eq!(started["timeout_minutes"], 7);
            let id = started["request_id"].as_str().unwrap();
            assert!(uuid::Uuid::parse_str(id).is_ok(), "{id}");
            let record = crate::state::builds::record(id, crate::state::now()).unwrap();
            assert_eq!(record["label"], "up lab small");
            assert_eq!(record["timeout_secs"], 7 * 60);
            crate::state::builds::INLINE_RUNNER.with(|flag| flag.set(false));
        });
    }

    #[test]
    fn a_session_gets_its_own_provider_settings_and_writes_no_global_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = config(dir.path());
        let mut server = server();
        server.kind = "openai".into();
        save(&config, &[server.clone()]).unwrap();
        let before = std::fs::read_dir(dir.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>();
        let (env, args) = session_settings(&config, "lab", "codex", "small").unwrap();
        assert!(env.iter().any(|line| line == "OPENAI_BASE_URL=http://127.0.0.1:11434"), "{env:?}");
        assert!(env.iter().any(|line| line == "OPENAI_API_KEY=sk-test"), "{env:?}");
        assert_eq!(args, ["--model", "small"]);
        let (env, args) = session_settings(&config, "lab", "opencode", "small").unwrap();
        assert!(env.iter().any(|line| line.starts_with("OPENCODE_CONFIG_CONTENT=") && line.contains("\"baseURL\":\"http://127.0.0.1:11434\"")), "{env:?}");
        assert_eq!(args, ["--model", "hgs-lab/small"]);
        // Anthropic-compatible settings for Claude Code.
        let mut anthropic = server.clone();
        anthropic.kind = "anthropic".into();
        save(&config, &[anthropic]).unwrap();
        let (env, args) = session_settings(&config, "lab", "claude", "small").unwrap();
        assert!(env.iter().any(|line| line == "ANTHROPIC_BASE_URL=http://127.0.0.1:11434"), "{env:?}");
        assert!(env.iter().any(|line| line == "ANTHROPIC_AUTH_TOKEN=sk-test"), "{env:?}");
        assert!(env.iter().any(|line| line == "ANTHROPIC_MODEL=small"), "{env:?}");
        assert!(args.is_empty());
        // The agents' kinds must match the server's.
        assert!(session_settings(&config, "lab", "codex", "small").unwrap_err().to_string().contains("OpenAI"));
        let after = std::fs::read_dir(dir.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect::<Vec<_>>();
        assert_eq!(before, after, "only the configuration file exists: no global agent settings");
    }
}
