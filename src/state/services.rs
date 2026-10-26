//! WhiteWh fork (G7): MCP servers are session services, not Background work.
//! A process hierarchy shows a service only when the whole configured argv
//! (program and arguments) matches a server the agent started from its own MCP
//! config - Claude's `.mcp.json` and settings `mcpServers`, Codex's
//! `config.toml` `[mcp_servers]` at the user or the project level - or when it
//! is the agent's own MCP child on stdio. A plain `node`, `python`, `cmd` or
//! `npx` job stays Background. Fixtures per agent live in the tests below.
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

/// One configured MCP server: its name and the command that starts it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Service {
    pub name: String,
    pub command: Vec<String>,
}

impl Service {
    fn new(name: &str, command: &[String]) -> Self {
        Self { name: name.to_owned(), command: command.to_vec() }
    }
}

/// How many agent+folder configs a long-lived dispatcher keeps.
const CACHE_LIMIT: usize = 64;
/// How long one config read is reused.
const CACHE_SECONDS: f64 = 60.0;

#[derive(Default)]
struct Cache {
    entries: HashMap<String, (f64, Vec<Service>)>,
    order: VecDeque<String>,
}
impl Cache {
    fn get(&mut self, key: &str, now: f64) -> Option<Vec<Service>> {
        self.entries.get(key).filter(|(at, _)| now - at < CACHE_SECONDS).map(|(_, services)| services.clone())
    }

    /// Keeps the map bounded: the oldest keys leave first.
    fn insert(&mut self, key: String, at: f64, services: Vec<Service>) {
        if !self.entries.contains_key(&key) {
            self.order.push_back(key.clone());
        }
        self.entries.insert(key, (at, services));
        while self.entries.len() > CACHE_LIMIT {
            match self.order.pop_front() {
                Some(oldest) => {
                    self.entries.remove(&oldest);
                }
                None => break,
            }
        }
    }
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

/// The services the agent would start from its own configuration (G7). The
/// configs are read at most once a minute per agent and folder, and the cache
/// is bounded: a listing polls this for every session.
pub(crate) fn configured(agent: &str, cwd: &str) -> Vec<Service> {
    let key = format!("{agent}\n{cwd}");
    let now = super::now();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(mut cache) = cache.lock() {
        if let Some(services) = cache.get(&key, now) {
            return services;
        }
    }
    let home = crate::platform::home_dir();
    let services = match agent {
        "claude" => claude(cwd, home.as_deref()),
        "codex" => codex(cwd, home.as_deref()),
        _ => Vec::new(),
    };
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, now, services.clone());
    }
    services
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The `mcpServers` object of a JSON config: name -> the command it starts.
fn servers(value: &Value, services: &mut Vec<Service>) {
    let Some(servers) = value.get("mcpServers").and_then(Value::as_object) else { return };
    for (name, server) in servers {
        let mut command = Vec::new();
        if let Some(program) = server.get("command").and_then(Value::as_str).filter(|program| !program.is_empty()) {
            command.push(program.to_owned());
        }
        for arg in server.get("args").and_then(Value::as_array).into_iter().flatten() {
            if let Some(arg) = arg.as_str() {
                command.push(arg.to_owned());
            }
        }
        if !command.is_empty() {
            services.push(Service::new(name, &command));
        }
    }
}

/// Claude reads `.mcp.json` in the session's folder, then the user's settings
/// and `.claude.json` (its own and its per-project entries).
fn claude(cwd: &str, home: Option<&Path>) -> Vec<Service> {
    let mut services = Vec::new();
    if !cwd.is_empty() {
        if let Some(value) = read_json(&Path::new(cwd).join(".mcp.json")) {
            servers(&value, &mut services);
        }
    }
    let Some(home) = home else { return services };
    if let Some(value) = read_json(&home.join(".claude").join("settings.json")) {
        servers(&value, &mut services);
    }
    if let Some(value) = read_json(&home.join(".claude.json")) {
        servers(&value, &mut services);
        if let Some(projects) = value.get("projects").and_then(Value::as_object) {
            for (folder, project) in projects {
                if !cwd.is_empty() && (Path::new(cwd).starts_with(folder) || folder == cwd) {
                    servers(project, &mut services);
                }
            }
        }
    }
    services
}

/// Codex reads `[mcp_servers.<name>] command` and `args` from its user config
/// and from the project's own `.codex/config.toml` (L7).
fn codex(cwd: &str, home: Option<&Path>) -> Vec<Service> {
    let mut services = Vec::new();
    // The project's own config is more specific and wins over the user's when
    // both name the same server.
    if !cwd.is_empty() {
        read_codex_config(&Path::new(cwd).join(".codex").join("config.toml"), &mut services);
    }
    if let Some(home) = home {
        read_codex_config(&home.join(".codex").join("config.toml"), &mut services);
    }
    services
}

fn read_codex_config(path: &Path, services: &mut Vec<Service>) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return };
    let Some(servers) = document.get("mcp_servers").and_then(|item| item.as_table_like()) else { return };
    for (name, server) in servers.iter() {
        let Some(table) = server.as_table_like() else { continue };
        let mut command = Vec::new();
        if let Some(program) = table.get("command").and_then(|item| item.as_str()).filter(|program| !program.is_empty()) {
            command.push(program.to_owned());
        }
        for arg in table.get("args").and_then(|item| item.as_array()).into_iter().flatten() {
            if let Some(arg) = arg.as_str() {
                command.push(arg.to_owned());
            }
        }
        if !command.is_empty() && !services.iter().any(|service| service.name == *name) {
            services.push(Service::new(name, &command));
        }
    }
}

fn file_name(value: &str) -> String {
    Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| value.to_owned())
}

fn tokens(command: &str) -> Vec<String> {
    command.split_whitespace().map(str::to_owned).collect()
}

/// Whether a live process is one of the configured services (N1): its whole
/// configured argv (program and arguments) matches, or it is the agent's own
/// MCP child on stdio - a direct child of the agent whose command starts with
/// the configured argv. A plain `node`/`python` job with other arguments is
/// Background even when the program name is the same.
pub(crate) fn is_service(services: &[Service], executable: &str, command: &str, agent_pid: u32, parent: u32) -> bool {
    services.iter().any(|service| matches_service(service, executable, command, agent_pid, parent))
}

fn matches_service(service: &Service, executable: &str, command: &str, agent_pid: u32, parent: u32) -> bool {
    let Some(program) = service.command.first() else { return false };
    let live = tokens(command);
    if live.is_empty() {
        return false;
    }
    let program_matches = file_name(program).eq_ignore_ascii_case(&file_name(executable))
        || file_name(program).eq_ignore_ascii_case(&file_name(&live[0]));
    if !program_matches {
        return false;
    }
    let exact = live.len() == service.command.len()
        && service
            .command
            .iter()
            .zip(&live)
            .all(|(configured, running)| configured == running || file_name(configured).eq_ignore_ascii_case(&file_name(running)));
    if exact {
        return true;
    }
    // The agent starts MCP servers itself, with stdio pipes: its direct child
    // may add flags after the configured argv. A job the member's shell starts
    // has a shell parent and stays Background.
    agent_pid != 0
        && parent == agent_pid
        && live.len() >= service.command.len()
        && service
            .command
            .iter()
            .zip(&live)
            .all(|(configured, running)| configured == running || file_name(configured).eq_ignore_ascii_case(&file_name(running)))
}

#[cfg(test)]
pub(crate) fn cache_len() -> usize {
    CACHE.get_or_init(Default::default).lock().map(|cache| cache.entries.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_mcp_servers_come_from_the_projects_mcp_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".mcp.json"),
            serde_json::json!({"mcpServers":{
                "socraticode":{"command":"npx","args":["-y","@socraticode/server"]},
                "server-pdf":{"command":"node","args":["server-pdf.js"]}}}).to_string(),
        )
        .unwrap();
        let services = claude(dir.path().to_str().unwrap(), Some(dir.path()));
        assert!(
            services.contains(&Service::new("socraticode", &["npx".into(), "-y".into(), "@socraticode/server".into()])),
            "{services:?}"
        );
        assert!(services.iter().any(|service| service.name == "server-pdf"));
        // The agent's own MCP child on stdio: the whole configured argv, with
        // an extra flag.
        assert!(is_service(&services, "node", "node server-pdf.js --stdio", 100, 100));
        assert!(is_service(&services, "npx", "npx -y @socraticode/server", 100, 100));
        // A plain Background command is not a service.
        assert!(!is_service(&services, "cargo", "cargo test --locked", 100, 100));
        assert!(!is_service(&services, "bash", "npm install", 100, 101));
    }

    #[test]
    fn a_node_job_beside_a_node_mcp_server_stays_background() {
        let services = vec![Service::new("server-pdf", &["node".into(), "server-pdf.js".into()])];
        // The MCP server itself: a direct child of the agent with its argv.
        assert!(is_service(&services, "node", "node server-pdf.js", 100, 100));
        // Any plain node job with other arguments is Background, next to the
        // server or inside the member's shell.
        assert!(!is_service(&services, "node", "node build.js", 100, 100));
        assert!(!is_service(&services, "node", "node build.js", 100, 101));
        assert!(!is_service(&services, "python", "python tool.py", 100, 100));
        assert!(!is_service(&services, "cmd", "cmd /c npx build", 100, 100));
        // A server started with extra flags is still the service.
        assert!(is_service(&services, "node", "node server-pdf.js --port 0", 100, 100));
    }

    #[test]
    fn codex_mcp_servers_come_from_its_user_and_project_configs() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("project");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(project.join(".codex")).unwrap();
        std::fs::write(
            home.join(".codex").join("config.toml"),
            r#"[mcp_servers.files]
command = "python"
args = ["-m", "files_server"]

[mcp_servers.docs]
command = "docs-server"
args = []
"#,
        )
        .unwrap();
        // L7: the project's own config, with a server named as the user's.
        std::fs::write(
            project.join(".codex").join("config.toml"),
            r#"[mcp_servers.files]
command = "python"
args = ["-m", "files_server", "--project"]

[mcp_servers.local]
command = "local-server"
args = []
"#,
        )
        .unwrap();
        let services = codex(project.to_str().unwrap(), Some(&home));
        assert!(services.iter().any(|service| service.name == "docs"), "{services:?}");
        assert!(services.iter().any(|service| service.name == "local"), "{services:?}");
        let files = services.iter().find(|service| service.name == "files").unwrap();
        assert_eq!(files.command.last().map(String::as_str), Some("--project"));
        assert!(is_service(&services, "local-server", "local-server", 100, 100));
        assert!(!is_service(&services, "bash", "make test", 100, 101));
    }

    #[test]
    fn the_service_config_cache_is_bounded() {
        let before = cache_len();
        for index in 0..(CACHE_LIMIT + 20) {
            let _ = configured("dsh", &format!("/nowhere/{index}"));
        }
        let after = cache_len();
        assert!(after <= CACHE_LIMIT, "cache grew to {after} (limit {CACHE_LIMIT})");
        assert!(after <= before + CACHE_LIMIT + 1, "{before} -> {after}");
    }

    #[test]
    fn an_agent_without_mcp_config_has_no_services() {
        assert!(super::configured("dsh", "/nowhere").is_empty());
        assert!(super::configured("kimi", "/nowhere").is_empty());
    }
}
