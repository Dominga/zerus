//! Dynamic native permission catalogs. Each adapter reads only the installed
//! executable's own help/version, config or composed profile. There is no
//! version table and no probe starts an agent, a host or a session.
use super::*;
use crate::platform::{self, RunError};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::time::{Duration, Instant};

const NATIVE_PREFIX: &str = "native:";
const MAX_NATIVE: usize = 1024;
const MAX_OUTPUT: usize = 256 * 1024;
const MAX_CATALOG: usize = 1536;
const MAX_CONTROLS: usize = 8;
const MAX_OPTIONS: usize = 32;
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
const VERSION_TIMEOUT: Duration = Duration::from_secs(1);

/// One list call shares provider probes; current native settings are still read
/// per account home.
#[derive(Default)]
pub(super) struct Cache {
    providers: BTreeMap<String, Value>,
}

impl Cache {
    /// Probe every provider once, in parallel. Each native probe has its own
    /// bounded deadline rather than repeating discovery for every profile.
    pub(super) fn prefetch(config: &Config, providers: &BTreeSet<String>) -> Self {
        let mut cache = Self::default();
        std::thread::scope(|scope| {
            let jobs: Vec<_> = providers
                .iter()
                .filter(|provider| ["claude", "codex", "kimi", "dsh"].contains(&provider.as_str()))
                .map(|provider| {
                    let provider = provider.clone();
                    let worker = provider.clone();
                    let home = native_home(config, &provider);
                    let job = scope.spawn(move || discover(config, &worker, &home));
                    (provider, job)
                })
                .collect();
            for (provider, job) in jobs {
                match job.join() {
                    Ok(value) => {
                        let value = bounded(&provider, value);
                        cache.providers.insert(provider, value);
                    }
                    Err(_) => {
                        // A panicking adapter must not take the account list with it.
                        cache.providers.insert(provider.clone(), unavailable(&provider, "Permission discovery stopped unexpectedly."));
                    }
                }
            }
        });
        cache
    }
}

pub(super) fn is_native(mode: &str) -> bool {
    mode.starts_with(NATIVE_PREFIX)
}

/// Decode only the stored syntax. Missing native keys mean "inherit"; an empty
/// object normalizes to provider. Values are validated against fresh options
/// before a write or a launch, never here.
pub(super) fn parse_native(mode: &str) -> Result<BTreeMap<String, String>> {
    let Some(body) = mode.strip_prefix(NATIVE_PREFIX) else {
        return Err(Error::new(1, "Choose provider, bypass or a native permission selection."));
    };
    if body.len() > MAX_NATIVE {
        return Err(Error::new(1, "Native permission selection is too long."));
    }
    let value: Value = serde_json::from_str(body)
        .map_err(|_| Error::new(1, "Native permission selection is not valid JSON."))?;
    let object = value
        .as_object()
        .ok_or_else(|| Error::new(1, "Native permission selection must be a JSON object."))?;
    let mut selection = BTreeMap::new();
    for (key, value) in object {
        if !valid_key(key) {
            return Err(Error::new(1, "Native permission control names use lowercase letters, digits and underscores."));
        }
        let value = value
            .as_str()
            .ok_or_else(|| Error::new(1, "Native permission values must be strings."))?;
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            return Err(Error::new(1, "Native permission value is invalid."));
        }
        selection.insert(key.clone(), value.to_owned());
    }
    Ok(selection)
}

pub(super) fn encode_native(selection: &BTreeMap<String, String>) -> String {
    format!(
        "{NATIVE_PREFIX}{}",
        serde_json::to_string(selection).unwrap_or_else(|_| "{}".into())
    )
}

/// Canonical persisted form. Syntax-only decode remains permissive so a saved
/// choice from another version stays readable; writes normalize here.
pub(super) fn normalize_mode(mode: &str) -> Result<String> {
    match mode {
        "provider" | "bypass" | "auto" => Ok(mode.to_owned()),
        _ if is_native(mode) => {
            let selection = parse_native(mode)?;
            if selection.is_empty() {
                Ok("provider".into())
            } else {
                Ok(encode_native(&selection))
            }
        }
        _ => Err(Error::new(1, "Choose provider, bypass or a native permission selection.")),
    }
}

pub(super) fn check_stored_mode(mode: &str) -> Result<()> {
    match mode {
        "provider" | "bypass" | "auto" => Ok(()),
        _ if is_native(mode) => {
            parse_native(mode)?;
            Ok(())
        }
        _ => Err(Error::new(1, "Invalid account permission mode.")),
    }
}

/// Validate a requested override without probing: provider, bypass and Claude's
/// legacy auto are static. `native:` syntax is checked here; fresh options are
/// checked by `args` at launch or by `validate_for_account` on a write.
pub(super) fn check_mode(provider: &str, mode: &str) -> Result<()> {
    if mode == "provider" {
        return Ok(());
    }
    if !["claude", "codex", "kimi", "dsh"].contains(&provider) {
        return Err(Error::new(1, "Permissions are managed in this provider's native configuration."));
    }
    match mode {
        "bypass" => Ok(()),
        "auto" if provider == "claude" => Ok(()),
        "auto" => Err(Error::new(1, "Auto is available only for Claude accounts.")),
        _ if is_native(mode) => {
            parse_native(mode)?;
            Ok(())
        }
        _ => Err(Error::new(1, "Choose provider, bypass or a native permission selection.")),
    }
}

fn toml(path: &Path) -> Option<toml_edit::DocumentMut> {
    let body = text(path)?;
    if body.trim().is_empty() {
        Some(toml_edit::DocumentMut::default())
    } else {
        body.parse().ok()
    }
}

fn text(path: &Path) -> Option<String> {
    let mut value = String::new();
    match fs::File::open(path) {
        Ok(file) => {
            use std::io::Read;
            file.take(1024 * 1024 + 1).read_to_string(&mut value).ok()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(String::new()),
        Err(_) => return None,
    }
    (value.len() <= 1024 * 1024).then_some(value)
}

fn valid_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn unavailable(provider: &str, reason: impl Into<String>) -> Value {
    let session_supported = ["claude", "codex", "kimi", "dsh"].contains(&provider);
    let reason: String = reason.into();
    json!({
        "status": "unavailable",
        "status_reason": reason,
        "source": provider,
        "version": Value::Null,
        "session_supported": session_supported,
        "selected": {},
        "controls": [],
    })
}

fn available(source: &str, version: Option<String>, controls: Vec<Value>) -> Value {
    json!({
        "status": "available",
        "status_reason": "",
        "source": source,
        "version": version,
        "session_supported": true,
        "selected": {},
        "controls": controls,
    })
}

fn option(value: &str, label: &str, description: Option<&str>) -> Value {
    let mut item = json!({"value": value, "label": label});
    if let Some(description) = description.filter(|text| !text.is_empty()) {
        item["description"] = json!(description);
    }
    item
}

fn control(id: &str, label: &str, description: Option<&str>, options: Vec<Value>) -> Value {
    let mut item = json!({
        "id": id,
        "label": label,
        "kind": "select",
        "scope": "launch",
        "options": options,
    });
    if let Some(description) = description.filter(|text| !text.is_empty()) {
        item["description"] = json!(description);
    }
    item
}

/// Run a bounded, read-only command. The executable is the same one a launch
/// resolves, through the maintained native command resolver (including npm
/// `.cmd` shims on Windows).
fn probe(mut command: Command, timeout: Duration) -> std::result::Result<String, String> {
    let (status, stdout, _stderr) = platform::run_bounded(
        &mut command,
        [MAX_OUTPUT, 8192],
        Instant::now() + timeout,
        &|| false,
    )
    .map_err(|error| match error {
        RunError::Stopped => "timed out".to_owned(),
        RunError::TooLarge => "wrote too much output".to_owned(),
        RunError::Spawn(error) if error.kind() == std::io::ErrorKind::NotFound => {
            "was not found on PATH".to_owned()
        }
        RunError::Spawn(error) | RunError::Io(error) => error.to_string(),
        RunError::Failed => "exited unsuccessfully".to_owned(),
        RunError::Unsupported => "is not supported here".to_owned(),
    })?;
    if !status.success() {
        return Err(format!("exited with {status}"));
    }
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}

fn provider_command(provider: &str, home: &Path, argument: &str) -> Command {
    // Same resolver as a launch, including npm .cmd shims on Windows. Help and
    // version need no credential mutation beyond the provider's home.
    let mut command = platform::agent_command(&[provider.to_owned()])
        .unwrap_or_else(|_| Command::new(provider));
    match provider {
        "claude" => command.env("CLAUDE_CONFIG_DIR", home),
        "codex" => command.env("CODEX_HOME", home),
        "kimi" => command.env("KIMI_CODE_HOME", home),
        _ => &mut command,
    };
    command.arg(argument);
    command
}

fn version(provider: &str, home: &Path) -> Option<String> {
    let text = probe(provider_command(provider, home, "--version"), VERSION_TIMEOUT).ok()?;
    let first = text.lines().next()?.trim();
    (!first.is_empty()).then(|| first.to_owned())
}

fn quote_values(text: &str) -> Vec<String> {
    Regex::new(r#""([^"]+)""#)
        .expect("valid quoted-value regex")
        .captures_iter(text)
        .filter_map(|capture| capture.get(1).map(|value| value.as_str().to_owned()))
        .collect()
}

fn option_section<'a>(help: &'a str, marker: &str) -> Option<&'a str> {
    let start = help.find(marker)?;
    let rest = &help[start..];
    // Stop before the next long option. Value bullets (`- never:`) and wrapped
    // descriptions do not start with `--`, so a missing own section cannot
    // borrow a later flag's choices.
    let after = rest.find('\n')? + 1;
    let end = rest[after..]
        .char_indices()
        .find_map(|(offset, _)| {
            let line = rest[after + offset..].lines().next()?;
            line.trim_start().starts_with("--").then_some(after + offset)
        })
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

fn claude_options(help: &str) -> Option<Vec<String>> {
    let tail = option_section(help, "--permission-mode")?;
    let choices = tail.find("choices:")?;
    let end = tail[choices..].find(')')? + choices;
    let values = quote_values(&tail[choices..end]);
    (!values.is_empty()).then_some(values)
}

fn codex_sandbox_options(help: &str) -> Option<Vec<String>> {
    let tail = option_section(help, "--sandbox <SANDBOX_MODE>")?;
    let start = tail.find("[possible values:")? + "[possible values:".len();
    let end = tail[start..].find(']')? + start;
    let values: Vec<String> = tail[start..end]
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    (!values.is_empty()).then_some(values)
}

fn codex_approval_options(help: &str) -> Option<Vec<String>> {
    let tail = option_section(help, "--ask-for-approval <APPROVAL_POLICY>")?;
    let start = tail.find("Possible values:")? + "Possible values:".len();
    let mut values = Vec::new();
    for line in tail[start..].lines() {
        let line = line.trim();
        if line.is_empty() {
            if values.is_empty() {
                continue;
            }
            break;
        }
        let Some(rest) = line.strip_prefix("- ") else {
            if values.is_empty() {
                continue;
            }
            break;
        };
        let value = rest.split(':').next().unwrap_or("").trim();
        if !value.is_empty() && value.chars().all(|c| c.is_ascii_lowercase() || c == '-' || c == '_') {
            values.push(value.to_owned());
        }
    }
    (!values.is_empty()).then_some(values)
}

fn claude_catalog(config: &Config, home: &Path) -> Value {
    let help = match probe(provider_command("claude", home, "--help"), PROBE_TIMEOUT) {
        Ok(help) => help,
        Err(error) => return unavailable("claude", format!("Claude help could not be read: {error}.")),
    };
    let Some(values) = claude_options(&help) else {
        return unavailable("claude", "Claude help did not publish --permission-mode choices.");
    };
    let options = values
        .iter()
        .map(|value| option(value, value, None))
        .collect();
    let _ = config;
    available(
        "claude-help",
        version("claude", home),
        vec![control("mode", "Permission mode", None, options)],
    )
}

fn codex_catalog(_config: &Config, home: &Path) -> Value {
    let help = match probe(provider_command("codex", home, "--help"), PROBE_TIMEOUT) {
        Ok(help) => help,
        Err(error) => return unavailable("codex", format!("Codex help could not be read: {error}.")),
    };
    let (Some(sandboxes), Some(approvals)) = (codex_sandbox_options(&help), codex_approval_options(&help)) else {
        return unavailable("codex", "Codex help did not publish approval and sandbox choices.");
    };
    let mut controls = vec![
        control(
            "approval",
            "Approval policy",
            Some("When Codex asks for human approval before running a command."),
            approvals
                .iter()
                .map(|value| {
                    let description = if value == "on-request" {
                        Some("The model decides when to ask.")
                    } else if value == "never" {
                        Some("Never ask for approval.")
                    } else {
                        None
                    };
                    option(value, value, description)
                })
                .collect(),
        ),
        control(
            "sandbox",
            "Sandbox",
            Some("The filesystem and network policy for model-generated commands."),
            sandboxes.iter().map(|value| option(value, value, None)).collect(),
        ),
    ];
    if help.contains("--approve-for-me") {
        // Only the enabling switch is real. Leaving the control out inherits
        // the native setting; there is no fabricated disable flag.
        let mut automatic = control(
            "approve_for_me",
            "Approve for me",
            Some("Automatic review with workspace-write. Leave Approval policy and Sandbox inherited; their explicit switches conflict with this option."),
            vec![option("on", "Approve for me", Some("Pass --approve-for-me."))],
        );
        automatic["conflicts_with"] = json!(["approval", "sandbox"]);
        controls.push(automatic);
    }
    available("codex-help", version("codex", home), controls)
}

fn kimi_catalog(_config: &Config, home: &Path) -> Value {
    let help = match probe(provider_command("kimi", home, "--help"), PROBE_TIMEOUT) {
        Ok(help) => help,
        Err(error) => return unavailable("kimi", format!("Kimi help could not be read: {error}.")),
    };
    // Only flags that force a native permission mode. Always Ask has no launch
    // switch, so it is not synthesized; Plan is separate below.
    let mut options = Vec::new();
    for (flag, value) in [("--yolo", "yolo"), ("--auto", "auto")] {
        let Some(tail) = option_section(&help, flag) else {
            continue;
        };
        let label = tail
            .find("Start in ")
            .and_then(|start| {
                let rest = &tail[start + "Start in ".len()..];
                let end = rest.find(" mode")?;
                Some(rest[..end].trim())
            })
            .filter(|label| !label.is_empty())
            .unwrap_or(value);
        let item = if label == value { option(value, value, None) } else { option(value, label, None) };
        options.push(item);
    }
    if options.is_empty() {
        return unavailable("kimi", "Kimi help did not publish a launch permission mode.");
    }
    let mut controls = vec![control("mode", "Permission mode", None, options)];
    // Plan is a separate native setting and can combine with a permission mode.
    if help.contains("--plan") {
        controls.push(control(
            "plan",
            "Plan mode",
            Some("Start this session in Plan mode. Leave unset to inherit the native setting."),
            vec![option("on", "Plan", Some("Pass --plan."))],
        ));
    }
    available("kimi-help", version("kimi", home), controls)
}

fn dsh_catalog(config: &Config) -> Value {
    match crate::state::dsh::permission_catalog(config) {
        Ok(value) if value["status"] == "available" && value["controls"].as_array().is_some_and(|controls| !controls.is_empty()) => {
            let mut value = value;
            value["session_supported"] = json!(true);
            if !value["status_reason"].is_string() {
                value["status_reason"] = json!("");
            }
            if !value["source"].is_string() {
                value["source"] = json!("dsh-profile");
            }
            value
        }
        Ok(value) => unavailable(
            "dsh",
            value["status_reason"]
                .as_str()
                .filter(|reason| !reason.is_empty())
                .unwrap_or("DeepSeek profile did not publish a permission preset."),
        ),
        Err(error) => unavailable("dsh", format!("DeepSeek permission catalog could not be read: {error}.")),
    }
}

fn discover(config: &Config, provider: &str, home: &Path) -> Value {
    let value = match provider {
        "claude" => claude_catalog(config, home),
        "codex" => codex_catalog(config, home),
        "kimi" => kimi_catalog(config, home),
        "dsh" => dsh_catalog(config),
        _ => unavailable(provider, "Permissions are managed in this provider's native configuration."),
    };
    bounded(provider, value)
}

/// Prefer a complete catalog or an explicit unavailable status; never truncate
/// choices silently. This also keeps account-list payloads within their budget.
fn bounded(provider: &str, value: Value) -> Value {
    if value["status"] != "available" {
        return value;
    }
    let Some(controls) = value["controls"].as_array() else {
        return unavailable(provider, "Native permission catalog has no controls.");
    };
    if controls.len() > MAX_CONTROLS {
        return unavailable(provider, "Native permission catalog has too many controls.");
    }
    for control in controls {
        if control["label"].as_str().map(str::len).unwrap_or(0) > 160
            || control["description"].as_str().map(str::len).unwrap_or(0) > 512
        {
            return unavailable(provider, "Native permission catalog has oversized labels.");
        }
        let Some(options) = control["options"].as_array() else {
            return unavailable(provider, "Native permission catalog control has no options.");
        };
        if options.is_empty() || options.len() > MAX_OPTIONS {
            return unavailable(provider, "Native permission catalog has too many options.");
        }
        for option in options {
            if option["label"].as_str().map(str::len).unwrap_or(0) > 160
                || option["description"].as_str().map(str::len).unwrap_or(0) > 512
                || option["value"].as_str().map(str::len).unwrap_or(0) > 128
            {
                return unavailable(provider, "Native permission catalog has an oversized option.");
            }
        }
    }
    if serde_json::to_vec(&value).map(|bytes| bytes.len()).unwrap_or(usize::MAX) > MAX_CATALOG {
        return unavailable(provider, "Native permission catalog is too large to list completely.");
    }
    value
}

pub(super) fn catalog_for(config: &Config, provider: &str, home: &Path, cache: &mut Cache) -> Value {
    if let Some(value) = cache.providers.get(provider) {
        return value.clone();
    }
    let value = discover(config, provider, home);
    cache.providers.insert(provider.to_owned(), value.clone());
    value
}

fn string_map(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// The native launch setting this account inherits. Absent keys are never
/// guessed: they simply do not appear in the map.
fn native_values(
    config: &Config,
    provider: &str,
    home: &Path,
    native_account: bool,
    catalog: &Value,
) -> (BTreeMap<String, String>, String) {
    match provider {
        "claude" => {
            let native = if native_account {
                home.to_path_buf()
            } else {
                Path::new(&config.home).join(".claude")
            };
            match claude_settings::preview(&native, home) {
                Ok(data) => match data["permissions"]["defaultMode"].as_str().filter(|mode| !mode.is_empty()) {
                    Some(mode) => (
                        BTreeMap::from([("mode".to_owned(), mode.to_owned())]),
                        format!("From Claude settings: permissions.defaultMode={mode}."),
                    ),
                    None => (BTreeMap::new(), "Claude settings do not pin permissions.defaultMode.".into()),
                },
                Err(_) => (BTreeMap::new(), "Claude settings could not be read.".into()),
            }
        }
        "codex" => {
            let path = home.join("config.toml");
            let Some(base) = toml(&path) else {
                return (BTreeMap::new(), "Codex settings could not be read.".into());
            };
            // 0.166 layers `$CODEX_HOME/<name>.config.toml` over the base file.
            // A selected profile whose layered file is absent is unverified;
            // never fall back to a stale inline [profiles.<name>] table.
            let mut layers = vec![base];
            let mut profile_name = None;
            if let Some(profile) = layers[0].get("profile").and_then(|value| value.as_str()) {
                profile_name = Some(profile.to_owned());
                let layer_path = home.join(format!("{profile}.config.toml"));
                if !layer_path.is_file() {
                    return (
                        BTreeMap::new(),
                        format!(
                            "Codex profile \"{profile}\" selects a layered file that was not found; effective settings are unverified."
                        ),
                    );
                }
                let Some(layer) = toml(&layer_path) else {
                    return (
                        BTreeMap::new(),
                        format!("Codex profile \"{profile}\" could not be read; effective settings are unverified."),
                    );
                };
                layers.push(layer);
            }
            let get = |key: &str| {
                layers
                    .iter()
                    .rev()
                    .find_map(|document| document.get(key).and_then(|value| value.as_str()))
            };
            let mut values = BTreeMap::new();
            if let Some(approval) = get("approval_policy").filter(|value| !value.is_empty()) {
                values.insert("approval".to_owned(), approval.to_owned());
            }
            if let Some(sandbox) = get("sandbox_mode").filter(|value| !value.is_empty()) {
                values.insert("sandbox".to_owned(), sandbox.to_owned());
            }
            let detail = if values.is_empty() {
                "Codex settings do not pin approval_policy or sandbox_mode.".into()
            } else {
                format!(
                    "From Codex settings{}: {}.",
                    profile_name
                        .as_deref()
                        .map(|profile| format!(" with layered profile \"{profile}\""))
                        .unwrap_or_default(),
                    values
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            };
            (values, detail)
        }
        "kimi" => {
            let path = home.join("config.toml");
            match toml(&path) {
                Some(document) => {
                    let mut values = BTreeMap::new();
                    if let Some(mode) = document
                        .get("permission_mode")
                        .and_then(|value| value.as_str())
                        .filter(|mode| !mode.is_empty())
                    {
                        values.insert("mode".to_owned(), mode.to_owned());
                    }
                    if document
                        .get("default_plan_mode")
                        .and_then(|value| value.as_bool())
                        == Some(true)
                    {
                        values.insert("plan".to_owned(), "on".to_owned());
                    }
                    let detail = if values.is_empty() {
                        "Kimi settings do not pin permission_mode or default_plan_mode.".into()
                    } else {
                        format!(
                            "From Kimi settings: {}.",
                            values
                                .iter()
                                .map(|(key, value)| format!("{key}={value}"))
                                .collect::<Vec<_>>()
                                .join("; ")
                        )
                    };
                    (values, detail)
                }
                None => (BTreeMap::new(), "Kimi settings could not be read.".into()),
            }
        }
        "dsh" => {
            let values = string_map(&catalog["selected"]);
            let detail = if values.is_empty() {
                "DeepSeek settings do not pin a permission preset.".into()
            } else {
                "From DeepSeek's configured permission preset.".into()
            };
            (values, detail)
        }
        _ => (BTreeMap::new(), "Permissions are managed in this provider's native configuration.".into()),
    }
}

fn bypass_selection(provider: &str) -> BTreeMap<String, String> {
    match provider {
        "claude" => BTreeMap::from([("mode".to_owned(), "bypassPermissions".to_owned())]),
        "codex" => BTreeMap::from([
            ("approval".to_owned(), "never".to_owned()),
            ("sandbox".to_owned(), "danger-full-access".to_owned()),
        ]),
        "kimi" => BTreeMap::from([("mode".to_owned(), "auto".to_owned())]),
        "dsh" => BTreeMap::from([("preset".to_owned(), "danger-full-access".to_owned())]),
        _ => BTreeMap::new(),
    }
}

/// Account launch settings for display. Never live state.
pub(super) fn describe_account(
    config: &Config,
    provider: &str,
    home: &Path,
    native_account: bool,
    mode: &str,
    base: &Value,
) -> (Value, Value) {
    let mut catalog = base.clone();
    let (values, source, mut detail) = if mode == "provider" {
        let (values, detail) = native_values(config, provider, home, native_account, &catalog);
        (values, "provider", detail)
    } else if mode == "bypass" {
        (bypass_selection(provider), "zerus", "Set in Zerus: bypass; the native flags are chosen at launch.".into())
    } else if mode == "auto" {
        if provider == "claude" {
            (BTreeMap::from([("mode".to_owned(), "auto".to_owned())]), "zerus", "Set in Zerus: Claude Auto.".into())
        } else {
            (BTreeMap::new(), "zerus", "Legacy Auto is available only for Claude.".into())
        }
    } else if is_native(mode) {
        match parse_native(mode) {
            Ok(values) if values.is_empty() => {
                let (values, detail) = native_values(config, provider, home, native_account, &catalog);
                (values, "provider", detail)
            }
            Ok(values) => {
                let note = if catalog["status"] == "available" {
                    if let Err(error) = validate_selection(provider, &catalog, &values) {
                        let note = format!("Saved native selection is not available: {}.", error.message);
                        catalog["status_reason"] = json!(note.clone());
                        note
                    } else {
                        "Set in Zerus: native permission selection.".into()
                    }
                } else {
                    format!(
                        "Saved native selection kept: fresh options are unavailable ({}).",
                        catalog["status_reason"].as_str().unwrap_or("not discovered")
                    )
                };
                (values, "zerus", note)
            }
            Err(error) => (BTreeMap::new(), "zerus", format!("Saved native selection is invalid: {}.", error.message)),
        }
    } else {
        (BTreeMap::new(), "zerus", "The saved permission mode is not recognized.".into())
    };
    if detail.is_empty() {
        detail = "No native permission is pinned.".into();
    }
    catalog["selected"] = json!(values.clone());
    let current = json!({"source": source, "values": values, "detail": detail});
    (catalog, current)
}

fn legacy_mode(provider: &str, values: &BTreeMap<String, String>) -> String {
    match provider {
        "claude" => match values.get("mode").map(String::as_str) {
            Some("auto") => "auto",
            Some("plan") => "plan",
            Some("bypassPermissions") => "bypass",
            _ => "default",
        },
        "codex" => {
            if values.get("approval").map(String::as_str) == Some("never")
                && values.get("sandbox").map(String::as_str) == Some("danger-full-access")
            {
                "bypass"
            } else {
                "default"
            }
        }
        "kimi" => match values.get("mode").map(String::as_str) {
            Some("auto") => "bypass",
            _ => "default",
        },
        "dsh" => match values.get("preset").map(String::as_str) {
            Some("danger-full-access") => "bypass",
            _ => "default",
        },
        _ => "default",
    }
    .to_owned()
}

pub(super) fn effective_mode(provider: &str, source: &str, stored: &str, values: &Value) -> String {
    if source == "zerus" && (stored == "bypass" || stored == "auto") {
        return stored.to_owned();
    }
    legacy_mode(provider, &string_map(values))
}

fn find_option<'a>(catalog: &'a Value, control: &str, value: &str) -> Option<&'a Value> {
    catalog["controls"]
        .as_array()?
        .iter()
        .find(|item| item["id"].as_str() == Some(control))?
        .get("options")?
        .as_array()?
        .iter()
        .find(|item| item["value"].as_str() == Some(value))
}

/// Fresh options are required for a native selection. A missing key means the
/// native setting is inherited.
pub(super) fn validate_selection(provider: &str, catalog: &Value, selection: &BTreeMap<String, String>) -> Result<()> {
    if catalog["status"] != "available" {
        return Err(Error::new(
            1,
            format!(
                "Native permission options for {provider} could not be discovered: {}.",
                catalog["status_reason"].as_str().unwrap_or("unavailable")
            ),
        ));
    }
    let controls = catalog["controls"]
        .as_array()
        .ok_or_else(|| Error::new(1, "Native permission catalog has no controls."))?;
    for (id, value) in selection {
        if !controls.iter().any(|control| control["id"].as_str() == Some(id.as_str())) {
            return Err(Error::new(1, format!("Unknown native permission control: {id}.")));
        }
        if find_option(catalog, id, value).is_none() {
            return Err(Error::new(
                1,
                format!("{value} is not an available value for the native permission control {id} on this computer."),
            ));
        }
    }
    if provider == "codex"
        && selection.get("approve_for_me").map(String::as_str) == Some("on")
        && (selection.contains_key("approval") || selection.contains_key("sandbox"))
    {
        return Err(Error::new(
            1,
            "Codex --approve-for-me cannot be combined with explicit Approval policy or Sandbox switches. Choose Use Codex settings for both controls, or leave Approve for me inherited.",
        ));
    }
    Ok(())
}

fn args_for(provider: &str, selection: &BTreeMap<String, String>) -> Vec<String> {
    match provider {
        "claude" => selection
            .get("mode")
            .map(|mode| vec!["--permission-mode".to_owned(), mode.clone()])
            .unwrap_or_default(),
        "codex" => {
            let mut args = Vec::new();
            if let Some(approval) = selection.get("approval") {
                args.extend(["-a".to_owned(), approval.clone()]);
            }
            if let Some(sandbox) = selection.get("sandbox") {
                args.extend(["-s".to_owned(), sandbox.clone()]);
            }
            if selection.get("approve_for_me").map(String::as_str) == Some("on") {
                args.push("--approve-for-me".into());
            }
            args
        }
        "kimi" => {
            let mut args = Vec::new();
            match selection.get("mode").map(String::as_str) {
                Some("yolo") => args.push("-y".into()),
                Some("auto") => args.push("--auto".into()),
                _ => {}
            }
            if selection.get("plan").map(String::as_str) == Some("on") {
                args.push("--plan".into());
            }
            args
        }
        _ => Vec::new(),
    }
}

pub(super) fn bypass_args(provider: &str) -> Vec<String> {
    match provider {
        "claude" => vec!["--permission-mode".into(), "bypassPermissions".into()],
        "codex" => vec!["--dangerously-bypass-approvals-and-sandbox".into()],
        "kimi" => vec!["--auto".into()],
        _ => Vec::new(),
    }
}

/// Build the native flags for an account or launch. `provider` and `bypass`
/// never require discovery; a native selection does.
pub(super) fn args(config: &Config, provider: &str, mode: &str, home: &Path) -> Result<Vec<String>> {
    match mode {
        "provider" => Ok(Vec::new()),
        "bypass" => Ok(bypass_args(provider)),
        "auto" if provider == "claude" => Ok(vec!["--permission-mode".into(), "auto".into()]),
        "auto" => Err(Error::new(1, "Auto is available only for Claude accounts.")),
        _ if is_native(mode) => {
            let selection = parse_native(mode)?;
            if selection.is_empty() {
                return Ok(Vec::new());
            }
            let catalog = discover(config, provider, home);
            validate_selection(provider, &catalog, &selection)?;
            Ok(args_for(provider, &selection))
        }
        _ => Err(Error::new(1, "Choose provider, bypass or a native permission selection.")),
    }
}

/// Validate a write against fresh options without holding the account lock.
pub(super) fn validate_for_account(config: &Config, provider: &str, home: &Path, mode: &str) -> Result<()> {
    check_mode(provider, mode)?;
    if !is_native(mode) {
        return Ok(());
    }
    let selection = parse_native(mode)?;
    if selection.is_empty() {
        return Ok(());
    }
    let catalog = discover(config, provider, home);
    validate_selection(provider, &catalog, &selection)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE_HELP: &str = r#"
  --permission-mode <mode>              Permission mode to use for the session
                                        (choices: "acceptEdits", "auto",
                                        "bypassPermissions", "manual",
                                        "dontAsk", "plan")
"#;

    const CODEX_HELP: &str = r#"
  -s, --sandbox <SANDBOX_MODE>
          Select the sandbox policy to use when executing model-generated shell commands

          [possible values: read-only, workspace-write, danger-full-access]

      --approve-for-me
          Route approval requests through automatic review using the workspace-write sandbox

  -a, --ask-for-approval <APPROVAL_POLICY>
          Configure when the model requires human approval before executing a command

          Possible values:
          - on-request: The model decides when to ask the user for approval
          - never:      Never ask for user approval Execution failures are immediately returned to
            the model
"#;

    const KIMI_HELP: &str = r#"
  -y, --yolo                    Start in Ask When Needed mode: routine edits and commands run
                                automatically; risky actions, questions, and plans still ask.
  --auto                        Start in Never Ask mode: everything runs and is decided
                                automatically; only plans in Plan mode wait for your review.
  --plan                        Start in plan mode. (default: false)
"#;

    #[test]
    fn claude_help_publishes_native_choices() {
        assert_eq!(
            claude_options(CLAUDE_HELP).unwrap(),
            ["acceptEdits", "auto", "bypassPermissions", "manual", "dontAsk", "plan"]
        );
    }

    #[test]
    fn codex_help_publishes_both_controls() {
        assert_eq!(
            codex_sandbox_options(CODEX_HELP).unwrap(),
            ["read-only", "workspace-write", "danger-full-access"]
        );
        assert_eq!(codex_approval_options(CODEX_HELP).unwrap(), ["on-request", "never"]);
        assert!(CODEX_HELP.contains("--approve-for-me"));
    }

    #[test]
    fn kimi_help_publishes_only_forceable_modes_and_a_separate_plan() {
        let options = [("--yolo", "yolo"), ("--auto", "auto")];
        let mut values = Vec::new();
        for (flag, value) in options {
            let tail = option_section(KIMI_HELP, flag).unwrap();
            let label = tail
                .find("Start in ")
                .map(|start| {
                    let rest = &tail[start + "Start in ".len()..];
                    rest[..rest.find(" mode").unwrap()].trim()
                })
                .unwrap();
            values.push((value, label.to_owned()));
        }
        assert_eq!(values, [("yolo", "Ask When Needed".into()), ("auto", "Never Ask".into())]);
        assert!(!KIMI_HELP.contains("Always Ask"));
        assert!(KIMI_HELP.contains("--plan"));
    }

    #[test]
    fn native_selection_is_canonical_and_bounded() {
        let mut selection = BTreeMap::new();
        selection.insert("sandbox".into(), "workspace-write".into());
        selection.insert("approval".into(), "on-request".into());
        assert_eq!(encode_native(&selection), r#"native:{"approval":"on-request","sandbox":"workspace-write"}"#);
        assert_eq!(parse_native(&encode_native(&selection)).unwrap(), selection);
        assert!(parse_native("native:{\"Mode\":\"auto\"}").is_err());
        assert!(parse_native("native:[]").is_err());
        assert!(parse_native("native:{\"mode\":1}").is_err());
        assert!(parse_native(&format!("native:{{\"mode\":\"{}\"}}", "x".repeat(MAX_NATIVE))).is_err());
    }

    #[test]
    fn a_single_published_choice_is_still_a_successful_discovery() {
        let claude = r#"
  --permission-mode <mode>              Permission mode
                                        (choices: "manual")
"#;
        assert_eq!(claude_options(claude).unwrap(), ["manual"]);
        let codex_sandbox = r#"
  -s, --sandbox <SANDBOX_MODE>
          [possible values: workspace-write]
"#;
        assert_eq!(codex_sandbox_options(codex_sandbox).unwrap(), ["workspace-write"]);
        let codex_approval = r#"
  -a, --ask-for-approval <APPROVAL_POLICY>
          Possible values:
          - on-request: ask when needed
"#;
        assert_eq!(codex_approval_options(codex_approval).unwrap(), ["on-request"]);
    }

    #[test]
    fn native_write_normalizes_whitespace_and_key_order() {
        let raw = "native:{ \"sandbox\" : \"workspace-write\", \"approval\": \"on-request\" }";
        assert_eq!(
            normalize_mode(raw).unwrap(),
            r#"native:{"approval":"on-request","sandbox":"workspace-write"}"#
        );
        assert_eq!(normalize_mode("native:{}").unwrap(), "provider");
        assert_eq!(normalize_mode("native: { } ").unwrap(), "provider");
    }

    #[test]
    fn malformed_or_missing_help_falls_back_to_unavailable() {
        assert!(claude_options("no permission choice here").is_none());
        assert!(codex_sandbox_options("no sandbox values").is_none());
        assert!(codex_approval_options("no approvals").is_none());
        let later_choices = "--permission-mode <mode>\n  No choices here\n\n--other <x>\n  (choices: \"wrong\")\n";
        assert!(claude_options(later_choices).is_none());
    }

    #[test]
    fn native_validation_rejects_unknown_and_withdrawn_values() {
        let catalog = available(
            "codex-help",
            Some("0.166.0".into()),
            vec![
                control("approval", "Approval policy", None, vec![option("on-request", "on-request", None)]),
                control("sandbox", "Sandbox", None, vec![option("workspace-write", "workspace-write", None)]),
            ],
        );
        let mut selection = BTreeMap::new();
        selection.insert("approval".into(), "on-request".into());
        assert!(validate_selection("codex", &catalog, &selection).is_ok());

        selection.insert("approval".into(), "never".into());
        assert!(validate_selection("codex", &catalog, &selection).is_err());

        selection.insert("approval".into(), "on-request".into());
        selection.insert("sandbox".into(), "danger-full-access".into());
        assert!(validate_selection("codex", &catalog, &selection).is_err());

        selection.clear();
        selection.insert("unknown".into(), "x".into());
        assert!(validate_selection("codex", &catalog, &selection).is_err());
    }

    #[test]
    fn codex_approve_for_me_rejects_explicit_approval_and_sandbox() {
        let catalog = available(
            "codex-help",
            None,
            vec![
                control("approval", "Approval", None, vec![option("on-request", "on-request", None), option("never", "never", None)]),
                control("sandbox", "Sandbox", None, vec![option("read-only", "read-only", None), option("workspace-write", "workspace-write", None)]),
                control("approve_for_me", "Approve for me", None, vec![option("on", "on", None)]),
            ],
        );
        let mut selection = BTreeMap::from([("approve_for_me".to_owned(), "on".to_owned())]);
        assert!(validate_selection("codex", &catalog, &selection).is_ok());
        assert_eq!(args_for("codex", &selection), ["--approve-for-me"]);
        for (key, value) in [("approval", "on-request"), ("approval", "never"), ("sandbox", "read-only"), ("sandbox", "workspace-write")] {
            selection.insert(key.into(), value.into());
            let error = validate_selection("codex", &catalog, &selection).unwrap_err();
            assert!(error.message.contains("cannot be combined"), "{}", error.message);
            selection.remove(key);
        }
        selection.insert("approve_for_me".into(), "off".into());
        assert!(validate_selection("codex", &catalog, &selection).is_err());
    }

    #[test]
    fn kimi_plan_is_an_independent_launch_switch() {
        let catalog = available(
            "kimi-help",
            None,
            vec![
                control("mode", "Permission mode", None, vec![option("yolo", "Ask When Needed", None), option("auto", "Never Ask", None)]),
                control("plan", "Plan mode", None, vec![option("on", "Plan", None)]),
            ],
        );
        let selection = BTreeMap::from([
            ("mode".to_owned(), "yolo".to_owned()),
            ("plan".to_owned(), "on".to_owned()),
        ]);
        assert!(validate_selection("kimi", &catalog, &selection).is_ok());
        assert_eq!(args_for("kimi", &selection), ["-y", "--plan"]);
        let mut plan_only = BTreeMap::new();
        plan_only.insert("plan".to_owned(), "on".to_owned());
        assert!(validate_selection("kimi", &catalog, &plan_only).is_ok());
        assert_eq!(args_for("kimi", &plan_only), ["--plan"]);
    }

    #[test]
    fn bypass_and_provider_do_not_need_a_catalog() {
        assert_eq!(
            bypass_args("claude"),
            ["--permission-mode", "bypassPermissions"]
        );
        assert_eq!(bypass_args("codex"), ["--dangerously-bypass-approvals-and-sandbox"]);
        assert_eq!(bypass_args("kimi"), ["--auto"]);
        assert!(bypass_args("dsh").is_empty());
    }

    #[test]
    fn provider_is_a_universal_noop_but_opencode_has_no_native_override() {
        assert!(check_mode("opencode", "provider").is_ok());
        assert!(check_mode("opencode", "bypass").is_err());
        assert!(check_mode("opencode", "native:{}").is_err());
        assert!(check_mode("claude", "provider").is_ok());
        assert!(check_mode("claude", "bypass").is_ok());
    }

    #[test]
    fn stored_modes_validate_syntax_only() {
        assert!(check_stored_mode("provider").is_ok());
        assert!(check_stored_mode("bypass").is_ok());
        assert!(check_stored_mode("auto").is_ok());
        assert!(check_stored_mode("native:{}").is_ok());
        assert!(check_stored_mode("native:{\"mode\":\"auto\"}").is_ok());
        assert!(check_stored_mode("native:{\"mode\":}").is_err());
        assert!(check_stored_mode("full-access").is_err());
    }

    #[test]
    fn legacy_modes_do_not_collapse_unknown_native_values_to_default_label() {
        let mut values: BTreeMap<String, String> = BTreeMap::new();
        values.insert("mode".into(), "acceptEdits".into());
        assert_eq!(effective_mode("claude", "provider", "provider", &json!(values.clone())), "default");
        values.insert("mode".into(), "auto".into());
        assert_eq!(effective_mode("claude", "provider", "provider", &json!(values.clone())), "auto");
    }
}
