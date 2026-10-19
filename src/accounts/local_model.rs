//! Claude Code against a local Ollama model (WhiteWh fork). A managed Claude
//! profile with `.hgs-local-model.json` launches with Anthropic-compatible
//! variables that point every model slot at that model; no Anthropic sign-in
//! is involved. Ollama serves the Anthropic API itself, and a proxy in front of
//! it (for a chat template that needs rewriting) can start with the session.
use super::*;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

pub(super) const FILE: &str = ".hgs-local-model.json";
pub(super) const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(super) struct LocalModel {
    pub endpoint: String,
    pub model: String,
    /// The context window Claude Code may fill; 0 keeps its own default.
    #[serde(default)]
    pub context: u64,
    /// Started detached when the endpoint does not answer, such as a proxy.
    #[serde(default)]
    pub prepare: Vec<String>,
}

pub(super) fn read(home: &Path) -> Option<LocalModel> {
    let bytes = fs::read(home.join(FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// `http://host:port`, without a path, user information or another scheme.
fn address(endpoint: &str) -> Option<(String, u16)> {
    let rest = endpoint.strip_prefix("http://")?.trim_end_matches('/');
    let (host, port) = rest.rsplit_once(':')?;
    let valid = !host.is_empty()
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
    Some((host.to_owned(), port.parse().ok().filter(|_| valid)?))
}

pub(super) fn validate(model: &LocalModel) -> Result<()> {
    if address(&model.endpoint).is_none() {
        return Err(Error::new(1, "The local model address must be http://HOST:PORT."));
    }
    if model.model.is_empty()
        || model.model.len() > 256
        || model.model.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(Error::new(1, "Choose a local model name without spaces."));
    }
    if model.prepare.len() > 64 || model.prepare.iter().any(|a| a.chars().any(char::is_control)) {
        return Err(Error::new(1, "The start command is invalid."));
    }
    Ok(())
}

pub(super) fn write(home: &Path, model: &LocalModel) -> Result<()> {
    validate(model)?;
    let path = home.join(FILE);
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&path)?;
    platform::make_private(&file)?;
    file.write_all(serde_json::to_string_pretty(model).map_err(|e| Error::new(1, e.to_string()))?.as_bytes())?;
    file.sync_all()?;
    // A local profile has no claude.ai account to finish onboarding with.
    let settings = home.join(".claude.json");
    if !settings.exists() {
        let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&settings)?;
        platform::make_private(&file)?;
        file.write_all(json!({"hasCompletedOnboarding": true}).to_string().as_bytes())?;
        file.sync_all()?;
    }
    Ok(())
}

/// The variables of `claude-local`-style launches: every model slot, the
/// subagent and auto-mode classifier included, names the local model.
pub(super) fn environment(model: &LocalModel) -> Vec<(String, String)> {
    let mut values = vec![
        ("ANTHROPIC_BASE_URL", model.endpoint.trim_end_matches('/').to_owned()),
        ("ANTHROPIC_AUTH_TOKEN", "ollama".to_owned()),
    ];
    for key in [
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        "ANTHROPIC_SMALL_FAST_MODEL",
        "CLAUDE_CODE_SUBAGENT_MODEL",
        "CLAUDE_CODE_AUTO_MODE_MODEL",
    ] {
        values.push((key, model.model.clone()));
    }
    // Local templates reject Claude's effort levels; slow local generation
    // needs a long request timeout.
    values.push(("CLAUDE_CODE_EFFORT_LEVEL", "unset".to_owned()));
    values.push(("API_TIMEOUT_MS", "1800000".to_owned()));
    values.push(("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".to_owned()));
    if model.context > 0 {
        values.push(("CLAUDE_CODE_MAX_CONTEXT_TOKENS", model.context.to_string()));
    }
    values.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

pub(super) fn reachable(endpoint: &str) -> bool {
    let Some((host, port)) = address(endpoint) else { return false };
    (host.as_str(), port).to_socket_addrs().ok().into_iter().flatten()
        .any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok())
}

/// Starts the profile's start command when its endpoint does not answer and
/// waits for it, so the session's first request finds the endpoint.
pub(super) fn prepare(model: &LocalModel) -> Result<()> {
    if model.prepare.is_empty() || reachable(&model.endpoint) {
        return Ok(());
    }
    #[cfg(windows)]
    let mut command = platform::agent_command(&model.prepare)
        .map_err(|e| Error::new(1, format!("Cannot start {}: {e}", model.prepare[0])))?;
    #[cfg(not(windows))]
    let mut command = {
        let mut command = std::process::Command::new(&model.prepare[0]);
        command.args(&model.prepare[1..]);
        command
    };
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    platform::spawn_detached(&mut command)
        .map_err(|e| Error::new(1, format!("Cannot start {}: {e}", model.prepare[0])))?;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if reachable(&model.endpoint) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err(Error::new(1, format!("{} did not answer after its start command; check that command.", model.endpoint)))
}

/// The start command as typed: POSIX words elsewhere; on Windows words split at
/// spaces outside double quotes, so backslashes in paths stay.
#[cfg(not(windows))]
pub(super) fn split_command(text: &str) -> Result<Vec<String>> {
    shell_words::split(text).map_err(|_| Error::new(1, "The start command has unbalanced quotes."))
}
#[cfg(windows)]
pub(super) fn split_command(text: &str) -> Result<Vec<String>> {
    let (mut words, mut word, mut quoted, mut started) = (Vec::new(), String::new(), false, false);
    for c in text.chars() {
        match c {
            '"' => { quoted = !quoted; started = true; }
            c if c.is_whitespace() && !quoted => {
                if started { words.push(std::mem::take(&mut word)); started = false; }
            }
            c => { word.push(c); started = true; }
        }
    }
    if quoted {
        return Err(Error::new(1, "The start command has unbalanced quotes."));
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

/// Model names that the Ollama at `endpoint` lists (`GET /api/tags`).
pub(super) fn models(endpoint: &str) -> Result<Vec<String>> {
    let (host, port) = address(endpoint)
        .ok_or_else(|| Error::new(1, "The Ollama address must be http://HOST:PORT."))?;
    let addr = (host.as_str(), port).to_socket_addrs()?.next()
        .ok_or_else(|| Error::new(1, "Cannot resolve the Ollama address."))?;
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|_| Error::new(1, format!("Ollama does not answer at {endpoint}.")))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    write!(stream, "GET /api/tags HTTP/1.0\r\nHost: {host}:{port}\r\nAccept: application/json\r\n\r\n")?;
    let mut response = Vec::new();
    stream.take(4 * 1024 * 1024).read_to_end(&mut response)?;
    let text = String::from_utf8_lossy(&response);
    let (head, body) = text.split_once("\r\n\r\n")
        .ok_or_else(|| Error::new(1, "Invalid Ollama response."))?;
    if !head.starts_with("HTTP/1.") || head.split_whitespace().nth(1) != Some("200") {
        return Err(Error::new(1, "Ollama did not list its models."));
    }
    let data: Value = serde_json::from_str(body).map_err(|_| Error::new(1, "Invalid Ollama model list."))?;
    let mut names: Vec<String> = data["models"].as_array().into_iter().flatten()
        .filter_map(|m| m["name"].as_str().map(str::to_owned))
        .filter(|name| !name.is_empty() && name.len() <= 256)
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> LocalModel {
        LocalModel { endpoint: "http://127.0.0.1:11500/".into(), model: "qwen3.8-cc:latest".into(), context: 262144, prepare: vec![] }
    }

    #[test]
    fn every_model_slot_names_the_local_model() {
        let values: std::collections::BTreeMap<_, _> = environment(&model()).into_iter().collect();
        assert_eq!(values["ANTHROPIC_BASE_URL"], "http://127.0.0.1:11500");
        assert_eq!(values["ANTHROPIC_AUTH_TOKEN"], "ollama");
        for key in ["ANTHROPIC_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL", "ANTHROPIC_DEFAULT_HAIKU_MODEL", "CLAUDE_CODE_SUBAGENT_MODEL", "CLAUDE_CODE_AUTO_MODE_MODEL"] {
            assert_eq!(values[key], "qwen3.8-cc:latest", "{key}");
        }
        assert_eq!(values["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "262144");
        let mut default = model();
        default.context = 0;
        assert!(!environment(&default).iter().any(|(k, _)| k == "CLAUDE_CODE_MAX_CONTEXT_TOKENS"));
    }

    #[test]
    fn only_plain_http_addresses_and_names_are_accepted() {
        assert!(validate(&model()).is_ok());
        for endpoint in ["https://127.0.0.1:1", "http://127.0.0.1", "http://user@host:1", "http://h:1/path", "ftp://h:1"] {
            assert!(validate(&LocalModel { endpoint: endpoint.into(), ..model() }).is_err(), "{endpoint}");
        }
        assert!(validate(&LocalModel { model: "two words".into(), ..model() }).is_err());
    }
}
