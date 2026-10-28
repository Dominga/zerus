//! WhiteWh fork (G7): MCP servers are session services, not Background work.
//! A process hierarchy shows a service only when the whole configured argv
//! (program and arguments) matches a server the agent started from its own MCP
//! config - Claude's `.mcp.json`, settings and `.claude.json` `mcpServers` in
//! its profile folder, and the servers of its enabled plugins (U11); Codex's
//! `config.toml` `[mcp_servers]` at the user or the project level - or when it
//! is the agent's own MCP child on stdio. Claude Code on Windows starts an
//! `npx` server as `cmd.exe /d /s /c npx ^-y^ ...`: that wrapper and its caret
//! escapes are taken off before matching (U11). A plain `node`, `python`,
//! `cmd` or `npx` job stays Background. Fixtures per agent live in the tests
//! below.
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
/// configs are read at most once a minute per agent, folder and profile, and
/// the cache is bounded: a listing polls this for every session. `agent_home`
/// is the session's profile folder (Claude's `CLAUDE_CONFIG_DIR`); empty means
/// the agent's default.
pub(crate) fn configured(agent: &str, cwd: &str, agent_home: &str) -> Vec<Service> {
    let key = format!("{agent}\n{cwd}\n{agent_home}");
    let now = super::now();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(mut cache) = cache.lock() {
        if let Some(services) = cache.get(&key, now) {
            return services;
        }
    }
    let home = crate::platform::home_dir();
    let services = match agent {
        "claude" => claude(cwd, home.as_deref(), agent_home),
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
    servers_in(value.get("mcpServers"), None, services);
}

/// An `mcpServers` object; a plugin's commands expand `${CLAUDE_PLUGIN_ROOT}`
/// and `${VAR:-default}` as Claude does.
fn servers_in(servers: Option<&Value>, plugin_root: Option<&Path>, services: &mut Vec<Service>) {
    let Some(servers) = servers.and_then(Value::as_object) else { return };
    let expand = |text: &str| match plugin_root {
        Some(root) => expand_variables(text, root),
        None => text.to_owned(),
    };
    for (name, server) in servers {
        let mut command = Vec::new();
        if let Some(program) = server.get("command").and_then(Value::as_str).filter(|program| !program.is_empty()) {
            command.push(expand(program));
        }
        for arg in server.get("args").and_then(Value::as_array).into_iter().flatten() {
            if let Some(arg) = arg.as_str() {
                command.push(expand(arg));
            }
        }
        if !command.is_empty() && !services.iter().any(|service| service.name == *name && service.command == command) {
            services.push(Service::new(name, &command));
        }
    }
}

/// `${CLAUDE_PLUGIN_ROOT}`, `${VAR}` and `${VAR:-default}` in a plugin's
/// server command: the plugin's folder, the variable, or its default.
fn expand_variables(text: &str, plugin_root: &Path) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let Some(length) = rest[start + 2..].find('}') else { break };
        out.push_str(&rest[..start]);
        let inner = &rest[start + 2..start + 2 + length];
        let (variable, default) = match inner.split_once(":-") {
            Some((variable, default)) => (variable, Some(default)),
            None => (inner, None),
        };
        let value = if variable == "CLAUDE_PLUGIN_ROOT" {
            Some(plugin_root.to_string_lossy().into_owned())
        } else {
            std::env::var(variable).ok().filter(|value| !value.is_empty())
        };
        out.push_str(&value.unwrap_or_else(|| default.unwrap_or_default().to_owned()));
        rest = &rest[start + 3 + length..];
    }
    out.push_str(rest);
    out
}

/// Claude reads `.mcp.json` in the session's folder, then its profile's
/// settings and `.claude.json` (its own and its per-project entries), then the
/// servers of its enabled plugins. The profile folder is `agent_home` (a
/// managed profile's `CLAUDE_CONFIG_DIR`, holding its own `.claude.json`), or
/// `~/.claude` with `~/.claude.json` beside it.
fn claude(cwd: &str, home: Option<&Path>, agent_home: &str) -> Vec<Service> {
    let mut services = Vec::new();
    if !cwd.is_empty() {
        if let Some(value) = read_json(&Path::new(cwd).join(".mcp.json")) {
            servers(&value, &mut services);
        }
    }
    let (profile, state) = if agent_home.is_empty() {
        let Some(home) = home else { return services };
        (home.join(".claude"), home.join(".claude.json"))
    } else {
        (Path::new(agent_home).to_path_buf(), Path::new(agent_home).join(".claude.json"))
    };
    let settings = read_json(&profile.join("settings.json"));
    if let Some(value) = &settings {
        servers(value, &mut services);
    }
    if let Some(value) = read_json(&state) {
        servers(&value, &mut services);
        if let Some(projects) = value.get("projects").and_then(Value::as_object) {
            for (folder, project) in projects {
                if !cwd.is_empty() && (Path::new(cwd).starts_with(folder) || folder == cwd) {
                    servers(project, &mut services);
                }
            }
        }
    }
    plugins(&profile, settings.as_ref(), cwd, &mut services);
    services
}

/// The MCP servers of the enabled Claude plugins (U11): each plugin
/// `enabledPlugins` turns on in the profile's settings (or the project's), at
/// the install path `plugins/installed_plugins.json` names. A plugin declares
/// its servers in `.mcp.json` at its root, or with `mcpServers` in
/// `.claude-plugin/plugin.json`: inline, or a path (or paths) to such a file.
fn plugins(profile: &Path, settings: Option<&Value>, cwd: &str, services: &mut Vec<Service>) {
    let mut enabled = std::collections::BTreeMap::<String, bool>::new();
    let mut layers = vec![settings.cloned()];
    if !cwd.is_empty() {
        for file in ["settings.json", "settings.local.json"] {
            layers.push(read_json(&Path::new(cwd).join(".claude").join(file)));
        }
    }
    for layer in layers.into_iter().flatten() {
        for (key, on) in layer.get("enabledPlugins").and_then(Value::as_object).into_iter().flatten() {
            if let Some(on) = on.as_bool() {
                enabled.insert(key.clone(), on);
            }
        }
    }
    if !enabled.values().any(|on| *on) {
        return;
    }
    let Some(installed) = read_json(&profile.join("plugins").join("installed_plugins.json")) else { return };
    let Some(list) = installed.get("plugins").and_then(Value::as_object) else { return };
    for (key, installs) in list {
        if enabled.get(key) != Some(&true) {
            continue;
        }
        // Version 2 lists the installs (per scope); version 1 has one.
        let installs: Vec<&Value> = match installs {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        };
        for install in installs {
            let Some(path) = install.get("installPath").and_then(Value::as_str).filter(|path| !path.is_empty()) else { continue };
            let project = install.get("projectPath").and_then(Value::as_str).unwrap_or_default();
            if !project.is_empty() && !(cwd == project || Path::new(cwd).starts_with(project)) {
                continue;
            }
            plugin_servers(Path::new(path), services);
        }
    }
}

fn plugin_servers(root: &Path, services: &mut Vec<Service>) {
    let manifest = read_json(&root.join(".claude-plugin").join("plugin.json"));
    match manifest.as_ref().and_then(|manifest| manifest.get("mcpServers")) {
        Some(inline @ Value::Object(_)) => servers_in(Some(inline), Some(root), services),
        Some(Value::String(file)) => plugin_file(root, file, services),
        Some(Value::Array(files)) => {
            for file in files.iter().filter_map(Value::as_str) {
                plugin_file(root, file, services);
            }
        }
        _ => {
            if let Some(value) = read_json(&root.join(".mcp.json")) {
                servers_in(value.get("mcpServers"), Some(root), services);
            }
        }
    }
}

/// A server file a plugin manifest names, inside the plugin's own folder.
fn plugin_file(root: &Path, file: &str, services: &mut Vec<Service>) {
    let path = root.join(file);
    if path.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
        return;
    }
    if let Some(value) = read_json(&path) {
        servers_in(value.get("mcpServers").or(Some(&value)), Some(root), services);
    }
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

/// A program name without its folder and a Windows `.exe`/`.cmd`/`.bat`
/// suffix: `npx` and `...\npx.cmd` are the same program.
fn program_name(value: &str) -> String {
    let name = file_name(value);
    let lower = name.to_ascii_lowercase();
    for suffix in [".exe", ".cmd", ".bat"] {
        if lower.ends_with(suffix) {
            return name[..name.len() - suffix.len()].to_owned();
        }
    }
    name
}

fn tokens(command: &str) -> Vec<String> {
    command.split_whitespace().map(str::to_owned).collect()
}

/// Claude Code on Windows starts a server such as `npx` through
/// `cmd.exe /d /s /c npx ^-y^ ^pkg^`: the argv after `/c`, with the outer
/// quotes `/s` strips and the caret escapes taken off. Any other command
/// comes back as it is.
fn unwrap_cmd(live: Vec<String>) -> Vec<String> {
    if live.is_empty() || !program_name(&live[0]).eq_ignore_ascii_case("cmd") {
        return live;
    }
    let mut at = 1;
    while at < live.len() && ["/d", "/s", "/q"].iter().any(|flag| live[at].eq_ignore_ascii_case(flag)) {
        at += 1;
    }
    if at + 1 >= live.len() || !live[at].eq_ignore_ascii_case("/c") {
        return live;
    }
    let mut inner: Vec<String> = live[at + 1..].to_vec();
    let last = inner.len() - 1;
    if inner[0].starts_with('"') && inner[last].ends_with('"') && (last > 0 || inner[0].len() >= 2) {
        inner[0].remove(0);
        inner[last].pop();
    }
    inner
        .into_iter()
        .map(|arg| {
            let mut out = String::new();
            let mut chars = arg.chars();
            while let Some(c) = chars.next() {
                if c == '^' {
                    out.extend(chars.next());
                } else {
                    out.push(c);
                }
            }
            out
        })
        .filter(|arg| !arg.is_empty())
        .collect()
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
    let raw = tokens(command);
    let wrapped = raw.len() > 1 && program_name(&raw[0]).eq_ignore_ascii_case("cmd");
    let live = unwrap_cmd(raw);
    if live.is_empty() {
        return false;
    }
    // The wrapper's own executable is cmd.exe: then only the unwrapped argv counts.
    let program_matches = (!wrapped && program_name(program).eq_ignore_ascii_case(&program_name(executable)))
        || program_name(program).eq_ignore_ascii_case(&program_name(&live[0]));
    if !program_matches {
        return false;
    }
    let same = |configured: &String, running: &String| {
        configured == running || program_name(configured).eq_ignore_ascii_case(&program_name(running))
    };
    let exact = live.len() == service.command.len() && service.command.iter().zip(&live).all(|(c, r)| same(c, r));
    if exact {
        return true;
    }
    // The agent starts MCP servers itself, with stdio pipes: its direct child
    // may add flags after the configured argv. A job the member's shell starts
    // has a shell parent and stays Background.
    agent_pid != 0
        && parent == agent_pid
        && live.len() >= service.command.len()
        && service.command.iter().zip(&live).all(|(c, r)| same(c, r))
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
        let services = claude(dir.path().to_str().unwrap(), Some(dir.path()), "");
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
            let _ = configured("dsh", &format!("/nowhere/{index}"), "");
        }
        let after = cache_len();
        assert!(after <= CACHE_LIMIT, "cache grew to {after} (limit {CACHE_LIMIT})");
        assert!(after <= before + CACHE_LIMIT + 1, "{before} -> {after}");
    }

    #[test]
    fn an_agent_without_mcp_config_has_no_services() {
        assert!(super::configured("dsh", "/nowhere", "").is_empty());
        assert!(super::configured("kimi", "/nowhere", "").is_empty());
    }

    /// U11: the exact command line Claude Code on Windows ran for the
    /// reviewer's socraticode server (from the enabled plugin, not from any
    /// `.mcp.json` or settings), seen on 17a8db61 as 30 minutes of Background.
    #[cfg(windows)]
    const WINDOWS_SOCRATICODE: &str = r"C:\WINDOWS\system32\cmd.exe /d /s /c npx ^-y^ ^--prefer-online^ ^socraticode@latest^";

    /// A managed Claude profile (its `CLAUDE_CONFIG_DIR`) with the socraticode
    /// plugin installed and enabled, laid out as Claude Code 2 writes it.
    fn plugin_profile(enabled: bool) -> tempfile::TempDir {
        let profile = tempfile::tempdir().unwrap();
        let install = profile.path().join("plugins").join("cache").join("socraticode").join("socraticode").join("1.16.1");
        std::fs::create_dir_all(install.join(".claude-plugin")).unwrap();
        std::fs::write(
            install.join(".claude-plugin").join("plugin.json"),
            serde_json::json!({"name":"socraticode","mcpServers":"./.claude-plugin/mcp.json"}).to_string(),
        )
        .unwrap();
        std::fs::write(
            install.join(".claude-plugin").join("mcp.json"),
            serde_json::json!({"mcpServers":{"socraticode":{"command":"npx",
                "args":["-y","--prefer-online","${SOCRATICODE_SPEC_U11_TEST:-socraticode@latest}"]}}})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            install.join(".mcp.json"),
            serde_json::json!({"mcpServers":{"socraticode":{"command":"npx","args":["-y","--prefer-online","socraticode@latest"]}}}).to_string(),
        )
        .unwrap();
        std::fs::write(
            profile.path().join("plugins").join("installed_plugins.json"),
            serde_json::json!({"version":2,"plugins":{"socraticode@socraticode":[
                {"scope":"user","installPath":install,"version":"1.16.1"}]}})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            profile.path().join("settings.json"),
            serde_json::json!({"enabledPlugins":{"socraticode@socraticode":enabled}}).to_string(),
        )
        .unwrap();
        profile
    }

    #[test]
    fn an_enabled_plugins_mcp_server_is_read_from_the_profile() {
        let profile = plugin_profile(true);
        let project = tempfile::tempdir().unwrap();
        let services = claude(project.path().to_str().unwrap(), None, profile.path().to_str().unwrap());
        let expected = Service::new("socraticode", &["npx".into(), "-y".into(), "--prefer-online".into(), "socraticode@latest".into()]);
        // The manifest's file, with `${VAR:-default}` expanded as Claude does.
        assert_eq!(services, vec![expected], "{services:?}");
        // A disabled plugin's servers are not the agent's.
        let disabled = plugin_profile(false);
        assert!(claude(project.path().to_str().unwrap(), None, disabled.path().to_str().unwrap()).is_empty());
        // Without a manifest entry the plugin's root `.mcp.json` names them.
        let bare = plugin_profile(true);
        let install = bare.path().join("plugins/cache/socraticode/socraticode/1.16.1");
        std::fs::write(install.join(".claude-plugin").join("plugin.json"), r#"{"name":"socraticode"}"#).unwrap();
        let services = claude("", None, bare.path().to_str().unwrap());
        assert_eq!(services.len(), 1, "{services:?}");
        assert_eq!(services[0].command.last().map(String::as_str), Some("socraticode@latest"));
    }

    #[test]
    fn a_managed_profile_reads_its_own_settings_and_claude_json() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let profile = root.path().join("profile");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(home.join(".claude.json"), serde_json::json!({"mcpServers":{"default-only":{"command":"default-server"}}}).to_string()).unwrap();
        std::fs::write(profile.join(".claude.json"), serde_json::json!({"mcpServers":{"vikunja":{"command":"vikunja-mcp","args":["--stdio"]}}}).to_string()).unwrap();
        std::fs::write(profile.join("settings.json"), serde_json::json!({"mcpServers":{"notes":{"command":"notes-server"}}}).to_string()).unwrap();
        let services = claude("", Some(&home), profile.to_str().unwrap());
        let names: Vec<&str> = services.iter().map(|service| service.name.as_str()).collect();
        assert!(names.contains(&"vikunja") && names.contains(&"notes"), "{names:?}");
        // The default profile's `~/.claude.json` is another profile's config.
        assert!(!names.contains(&"default-only"), "{names:?}");
        let default: Vec<String> = claude("", Some(&home), "").into_iter().map(|service| service.name).collect();
        assert_eq!(default, vec!["default-only".to_owned()]);
    }

    #[cfg(windows)] // Native process paths use the host's basename semantics.
    #[test]
    fn the_windows_npx_wrapper_of_a_plugin_server_is_a_service() {
        let profile = plugin_profile(true);
        let services = configured("claude", "", profile.path().to_str().unwrap());
        // The exact command line Claude Code ran, as the agent's child.
        assert!(is_service(&services, r"C:\WINDOWS\system32\cmd.exe", WINDOWS_SOCRATICODE, 100, 100));
        // The same server through its own shell parent: still the whole argv.
        assert!(is_service(&services, "cmd.exe", WINDOWS_SOCRATICODE, 100, 101));
        // `/s` with the quoted form, and npx resolved to npx.cmd.
        assert!(is_service(&services, "cmd.exe", r#"cmd.exe /d /s /c "npx.cmd ^-y^ ^--prefer-online^ ^socraticode@latest^""#, 100, 100));
        assert_eq!(
            unwrap_cmd(tokens(WINDOWS_SOCRATICODE)),
            vec!["npx", "-y", "--prefer-online", "socraticode@latest"]
        );
    }

    #[test]
    fn a_shell_started_npx_job_stays_background_beside_the_plugin_server() {
        let profile = plugin_profile(true);
        let services = configured("claude", "", profile.path().to_str().unwrap());
        assert!(!services.is_empty());
        // npx jobs the member's shell starts, plain or through the same
        // Windows wrapper, with other arguments: Background.
        assert!(!is_service(&services, "npx", "npx -y prettier --write .", 100, 101));
        assert!(!is_service(&services, "cmd.exe", r"C:\WINDOWS\system32\cmd.exe /d /s /c npx ^-y^ ^prettier^ ^--write^ ^.^", 100, 101));
        assert!(!is_service(&services, "node", "node build.js", 100, 101));
        // The cmd wrapper's own name never stands for a server.
        assert!(!is_service(&services, "cmd.exe", "cmd.exe /d /s /c npm test", 100, 100));
        // Even the agent's direct child must carry the whole configured argv.
        assert!(!is_service(&services, "cmd.exe", r"cmd.exe /d /s /c npx ^-y^", 100, 100));
    }
}
