//! WhiteWh fork (G7): MCP servers are session services, not Background work.
//! A process hierarchy shows a service when its command line matches a server
//! the agent started from its own MCP config: Claude's `.mcp.json` and settings
//! `mcpServers`, Codex's `config.toml` `[mcp_servers]`. A child of such a
//! server belongs to it too. Fixtures per agent live in the tests below.
use serde_json::Value;
use std::path::Path;

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

/// The services the agent would start from its own configuration (G7). The
/// configs are read at most once a minute per agent and folder: a listing
/// polls this for every session.
pub(crate) fn configured(agent: &str, cwd: &str) -> Vec<Service> {
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<std::collections::HashMap<String, (f64, Vec<Service>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = format!("{agent}\n{cwd}");
    let now = super::now();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(cache) = cache.lock() {
        if let Some((at, services)) = cache.get(&key) {
            if now - at < 60.0 {
                return services.clone();
            }
        }
    }
    let home = crate::platform::home_dir();
    let services = match agent {
        "claude" => claude(cwd, home.as_deref()),
        "codex" => codex(home.as_deref()),
        _ => Vec::new(),
    };
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key, (now, services.clone()));
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

/// Codex reads `[mcp_servers.<name>] command` and `args` from its config.
fn codex(home: Option<&Path>) -> Vec<Service> {
    let mut services = Vec::new();
    let Some(home) = home else { return services };
    let Ok(text) = std::fs::read_to_string(home.join(".codex").join("config.toml")) else { return services };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else { return services };
    let Some(servers) = document.get("mcp_servers").and_then(|item| item.as_table_like()) else { return services };
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
        if !command.is_empty() {
            services.push(Service::new(name, &command));
        }
    }
    services
}

fn file_name(value: &str) -> String {
    Path::new(value)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| value.to_owned())
}

/// Whether a live process is one of the configured services (G7): its
/// executable is the server's program, or its command line carries the
/// package/script the server's command names (`npx pkg` starts a `node pkg`).
pub(crate) fn is_service(services: &[Service], executable: &str, command: &str) -> bool {
    services.iter().any(|service| matches_service(service, executable, command))
}

fn matches_service(service: &Service, executable: &str, command: &str) -> bool {
    let Some(program) = service.command.first() else { return false };
    let program = file_name(program);
    let executable_name = file_name(executable);
    if !program.is_empty()
        && (program.eq_ignore_ascii_case(&executable_name)
            || command.split_whitespace().next().is_some_and(|first| program.eq_ignore_ascii_case(&file_name(first))))
    {
        return true;
    }
    // The launcher may differ (npx becomes node); the package or script stays.
    service.command.iter().skip(1).any(|arg| arg.len() > 3 && command.contains(arg.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let _ = name;
        dir
    }

    #[test]
    fn claude_mcp_servers_come_from_the_projects_mcp_json() {
        let dir = fixture("claude");
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
        // The launcher and the executable need not match when the package does.
        assert!(is_service(&services, "node.exe", "node -y @socraticode/server"));
        assert!(is_service(&services, "node", "node server-pdf.js --stdio"));
        // A plain Background command is not a service.
        assert!(!is_service(&services, "cargo", "cargo test --locked"));
        assert!(!is_service(&services, "bash", "npm install"));
    }

    #[test]
    fn codex_mcp_servers_come_from_its_config_toml() {
        let dir = fixture("codex");
        let codex_dir = dir.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(
            codex_dir.join("config.toml"),
            r#"[mcp_servers.files]
command = "python"
args = ["-m", "files_server"]

[mcp_servers.docs]
command = "docs-server"
args = []
"#,
        )
        .unwrap();
        let services = codex(Some(dir.path()));
        assert_eq!(services.len(), 2, "{services:?}");
        assert!(is_service(&services, "python.exe", "python -m files_server --port 0"));
        assert!(is_service(&services, "docs-server", "docs-server"));
        assert!(!is_service(&services, "bash", "make test"));
    }

    #[test]
    fn an_agent_without_mcp_config_has_no_services() {
        assert!(super::configured("dsh", "/nowhere").is_empty());
        assert!(super::configured("kimi", "/nowhere").is_empty());
    }
}
