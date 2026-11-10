//! The Machines screen's data: `hgs machine ls`, `machine resolve ALIAS` and
//! `machine check ALIAS` (tray/src/MachinesPage.cpp). The frontend combines
//! each row with the stream's connection state and the machine card.
use super::schedule::Poll;
use super::{flag, items, text, whole};
use serde::Serialize;
use serde_json::Value;

/// The most saved machines the screen lists.
const MACHINES: usize = 64;

/// The explicit local WSL connection of a machine (MachinesPage.cpp:232-238).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct WslProfile {
    pub distribution: String,
    pub user: String,
    pub hgs_path: String,
}

/// One saved peer of `machine ls` with its Zerus overrides.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MachineRow {
    pub alias: String,
    pub enabled: bool,
    /// `local` (saved in Zerus) or `config` (from HGS configuration).
    pub source: String,
    pub inherited: bool,
    /// `ssh` or `wsl-local` (MachinesPage.cpp:229-231).
    pub transport: String,
    /// The explicit WSL connection, on a `wsl-local` machine.
    pub wsl: Option<WslProfile>,
    /// The platform class this machine serves as a test runner, "" for none
    /// (MachinesPage.cpp:247-252).
    pub test_runner: String,
    pub hostname: String,
    pub user: String,
    pub port: Option<u64>,
    pub identity_file: String,
    /// The editor's origin paragraph (MachinesPage.cpp:641-643).
    pub origin: String,
}

/// The selected peer's effective SSH connection (`machine resolve`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Effective {
    pub alias: String,
    pub pending: bool,
    pub error: bool,
    pub text: String,
    /// `ssh` or `wsl-local` (MachinesPage.cpp:684-688).
    pub transport: String,
    /// The inherited values for the field placeholders ("Inherited: ...").
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub key: String,
    /// The WSL distribution and its Linux hgs, on a `wsl-local` machine.
    pub distribution: String,
    pub hgs_path: String,
}

/// The last "Test connection" (`machine check`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CheckResult {
    pub alias: String,
    pub pending: bool,
    pub error: bool,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MachinesView {
    pub loaded: bool,
    pub refreshing: bool,
    pub error: Option<String>,
    /// This computer's name as `machine ls` reports it.
    pub local: String,
    pub environment_override: bool,
    /// Exact optimistic-lock token from the same machine-list snapshot.
    pub connection_revision: String,
    pub machines: Vec<MachineRow>,
    pub effective: Option<Effective>,
    pub check: Option<CheckResult>,
    /// A list read or a check runs (MachinesPage `busy`).
    pub busy: bool,
}

/// The Machines reads so far.
#[derive(Clone, Debug, Default)]
pub struct MachinesState {
    pub list: Option<Value>,
    pub list_error: Option<String>,
    pub poll: Poll,
    resolve_alias: Option<String>,
    resolve: Option<Result<Value, String>>,
    resolving: bool,
    check_alias: Option<String>,
    check: Option<Result<Value, String>>,
    checking: bool,
    writing: bool,
}

fn row(machine: &Value, environment: bool) -> MachineRow {
    let connection = machine.get("connection").unwrap_or(&Value::Null);
    let source = text(machine, "source");
    let transport = if text(connection, "transport") == "wsl-local" { "wsl-local" } else { "ssh" };
    let wsl = connection.get("wsl").filter(|value| value.is_object()).map(|wsl| WslProfile {
        distribution: text(wsl, "distribution"),
        user: text(wsl, "user"),
        hgs_path: text(wsl, "hgs_path"),
    });
    let mut origin = String::new();
    if transport == "wsl-local" {
        // MachinesPage.cpp:642-643.
        origin.push_str("Local WSL uses the explicit distribution, user and Linux HGS executable. OpenCode must already be running; Zerus never starts or replaces it in this mode.");
    } else {
        if source == "config" {
            origin.push_str("This alias comes from HGS configuration. ");
        }
        origin.push_str("SSH supplies inherited values from your OpenSSH configuration and defaults. Fields below are Zerus overrides; blank fields keep the inherited value.");
        if environment {
            origin.push_str(" HGS_PEERS in the environment overrides the list of monitored machines.");
        }
    }
    MachineRow {
        alias: text(machine, "alias"),
        enabled: flag(machine, "enabled"),
        inherited: flag(machine, "inherited"),
        transport: transport.to_owned(),
        wsl,
        test_runner: text(connection, "test_runner"),
        hostname: text(connection, "hostname"),
        user: text(connection, "user"),
        port: whole(connection, "port").filter(|port| *port > 0),
        identity_file: text(connection, "identity_file"),
        origin,
        source,
    }
}

fn effective(alias: &str, pending: bool, wsl: bool, reply: Option<&Result<Value, String>>) -> Effective {
    let mut out = Effective {
        alias: alias.to_owned(),
        pending,
        error: false,
        text: String::new(),
        transport: if wsl { "wsl-local".to_owned() } else { "ssh".to_owned() },
        hostname: String::new(),
        user: String::new(),
        port: String::new(),
        key: String::new(),
        distribution: String::new(),
        hgs_path: String::new(),
    };
    match reply {
        None => out.text = if wsl { "Reading WSL connection…" } else { "Reading effective SSH connection…" }.into(),
        Some(Err(error)) => {
            out.error = true;
            out.text = format!("Could not read effective SSH settings: {error}\nYour connection overrides are shown below.");
        }
        Some(Ok(value)) => {
            let found = value.get("effective").unwrap_or(&Value::Null);
            if text(found, "transport") == "wsl-local" {
                // MachinesPage.cpp:684-688.
                out.transport = "wsl-local".into();
                out.distribution = text(found, "distribution");
                out.hgs_path = text(found, "hgs_path");
                out.user = text(found, "user");
                out.text = format!("WSL: {}, user {}\nHGS: {}\nOpenCode: existing-only", out.distribution, out.user, out.hgs_path);
                return out;
            }
            let keys: Vec<String> = items(found, "identity_files")
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            out.hostname = text(found, "hostname");
            out.user = text(found, "user");
            out.port = text(found, "port");
            out.key = keys.first().cloned().unwrap_or_default();
            let mut lines = vec![format!(
                "Effective connection: {}@{}, port {}",
                out.user, out.hostname, out.port
            )];
            if !keys.is_empty() {
                lines.push(format!("Key paths: {}", keys.join(", ")));
            }
            let jump = text(found, "proxy_jump");
            if !jump.is_empty() {
                lines.push(format!("Jump host: {jump}"));
            } else if flag(found, "proxy_command") {
                lines.push("Proxy command configured in OpenSSH".into());
            }
            out.text = lines.join("\n");
        }
    }
    out
}

fn check(alias: &str, pending: bool, reply: Option<&Result<Value, String>>) -> CheckResult {
    let (error, message) = match reply {
        None => (false, "Testing SSH and checking HGS…".to_owned()),
        Some(Err(error)) => (true, error.clone()),
        Some(Ok(data)) if text(data, "transport") == "wsl-local" => {
            // MachinesPage.cpp:389-393.
            let ok = flag(data, "ok");
            let text = if ok {
                format!("Connected to WSL {} as {}: {}. OpenCode policy: existing-only.", text(data, "distribution"), text(data, "user"), text(data, "hgs"))
            } else {
                text(data, "error")
            };
            (!ok, text)
        }
        Some(Ok(data)) if flag(data, "ok") => (
            false,
            format!("Connected: {} {}, {}, {}", text(data, "os"), text(data, "arch"), text(data, "hgs"), text(data, "tmux")),
        ),
        Some(Ok(data)) if flag(data, "ssh_ok") => {
            let mut missing = Vec::new();
            for (key, name) in [("hgs", "HGS"), ("tmux", "tmux"), ("cargo", "Rust")] {
                if text(data, key).is_empty() {
                    missing.push(name);
                }
            }
            let missing = if missing.is_empty() { "a working HGS installation".to_owned() } else { missing.join(", ") };
            let message = if text(data, "os").is_empty() {
                text(data, "error")
            } else {
                format!(
                    "SSH connected: {} {}. Missing: {missing}. Install the newest stable Rust and tmux in the SSH terminal if needed, then use Install / update HGS.",
                    text(data, "os"),
                    text(data, "arch")
                )
            };
            (true, message)
        }
        Some(Ok(data)) => (
            true,
            format!("SSH connection failed: {}\nOpen SSH terminal to check credentials or approve the host key, then test again.", text(data, "error")),
        ),
    };
    CheckResult {
        alias: alias.to_owned(),
        pending,
        error,
        text: message,
    }
}

impl MachinesState {
    /// `machine ls` on show and on Refresh; false while one or a check runs
    /// (MachinesPage::reload ignores it while any request runs).
    pub fn start_list(&mut self, now: u64) -> bool {
        !self.checking && !self.writing && self.poll.due(now, true)
    }

    /// A failure keeps the rows and shows the error.
    pub fn finish_list(&mut self, reply: Result<Value, String>) {
        self.poll.finish();
        match reply {
            Ok(value) => {
                self.list = Some(value);
                self.list_error = None;
            }
            Err(error) => self.list_error = Some(error),
        }
    }

    /// The selected peer's effective connection. A new selection replaces an
    /// older one; false when this alias is already being read.
    pub fn start_resolve(&mut self, alias: &str) -> bool {
        if self.resolving && self.resolve_alias.as_deref() == Some(alias) {
            return false;
        }
        self.resolve_alias = Some(alias.to_owned());
        self.resolve = None;
        self.resolving = true;
        true
    }

    /// Files a reply; a reply for an older selection is dropped.
    pub fn finish_resolve(&mut self, alias: &str, reply: Result<Value, String>) {
        if self.resolve_alias.as_deref() == Some(alias) {
            self.resolving = false;
            self.resolve = Some(reply);
        }
    }

    /// "Test connection": one at a time, never while the list reads.
    pub fn start_check(&mut self, alias: &str) -> bool {
        if self.busy() {
            return false;
        }
        self.check_alias = Some(alias.to_owned());
        self.check = None;
        self.checking = true;
        true
    }

    pub fn finish_check(&mut self, alias: &str, reply: Result<Value, String>) {
        if self.check_alias.as_deref() == Some(alias) {
            self.checking = false;
            self.check = Some(reply);
        }
    }

    pub fn start_write(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        self.writing = true;
        true
    }

    pub fn finish_write(&mut self) {
        self.writing = false;
    }

    pub fn busy(&self) -> bool {
        self.poll.pending() || self.checking || self.writing
    }

    pub fn view(&self) -> MachinesView {
        let list = self.list.as_ref();
        let environment = list.is_some_and(|list| flag(list, "environment_override"));
        let wsl_local = |alias: &str| {
            items(list.unwrap_or(&Value::Null), "machines")
                .iter()
                .find(|machine| text(machine, "alias") == alias)
                .is_some_and(|machine| text(machine.get("connection").unwrap_or(&Value::Null), "transport") == "wsl-local")
        };
        MachinesView {
            loaded: list.is_some(),
            refreshing: self.poll.pending(),
            error: self.list_error.clone(),
            local: list.map(|list| text(list, "local")).unwrap_or_default(),
            environment_override: environment,
            connection_revision: list.map(|list| text(list, "revision")).unwrap_or_default(),
            machines: list
                .map(|list| {
                    items(list, "machines")
                        .iter()
                        .take(MACHINES)
                        .map(|m| row(m, environment))
                        .collect()
                })
                .unwrap_or_default(),
            effective: self
                .resolve_alias
                .as_deref()
                .map(|alias| effective(alias, self.resolving, wsl_local(alias), self.resolve.as_ref())),
            check: self
                .check_alias
                .as_deref()
                .map(|alias| check(alias, self.checking, self.check.as_ref())),
            busy: self.busy(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::fixture;

    #[test]
    fn rows_carry_the_saved_overrides_and_qt_origin_texts() {
        let mut state = MachinesState::default();
        assert!(state.start_list(0));
        assert!(!state.start_list(10), "one list read at a time");
        assert!(state.view().refreshing && state.view().busy);
        state.finish_list(Ok(fixture("machines.json")));
        let view = state.view();
        assert!(view.loaded && !view.refreshing && view.error.is_none());
        assert_eq!(view.local, "workstation");
        let aliases: Vec<&str> = view.machines.iter().map(|m| m.alias.as_str()).collect();
        assert_eq!(aliases, ["studio", "build-01", "old-mac"]);
        let studio = &view.machines[0];
        assert_eq!(
            (
                studio.hostname.as_str(),
                studio.user.as_str(),
                studio.port,
                studio.enabled
            ),
            ("studio.example", "person", Some(2222), true)
        );
        assert_eq!(studio.origin, "SSH supplies inherited values from your OpenSSH configuration and defaults. Fields below are Zerus overrides; blank fields keep the inherited value.");
        assert!(view.machines[1]
            .origin
            .starts_with("This alias comes from HGS configuration. SSH supplies"));
        assert!(!view.machines[2].enabled);
        let mut overridden = fixture("machines.json");
        overridden["environment_override"] = serde_json::json!(true);
        state.start_list(20);
        state.finish_list(Ok(overridden));
        assert!(state.view().machines[0]
            .origin
            .ends_with(" HGS_PEERS in the environment overrides the list of monitored machines."));
        state.start_list(30);
        state.finish_list(Err("machine settings are locked".into()));
        let view = state.view();
        assert_eq!(
            (view.machines.len(), view.error.as_deref()),
            (3, Some("machine settings are locked")),
            "a failure keeps the rows"
        );
    }

    #[test]
    fn the_effective_connection_follows_the_latest_selection() {
        let mut state = MachinesState::default();
        assert!(state.start_resolve("build-01"));
        assert!(
            state.start_resolve("studio"),
            "a new selection replaces the pending one"
        );
        assert!(!state.start_resolve("studio"));
        assert_eq!(
            state.view().effective.unwrap().text,
            "Reading effective SSH connection…"
        );
        state.finish_resolve("build-01", Ok(fixture("resolve-studio.json")));
        assert!(
            state.view().effective.unwrap().pending,
            "an older selection's reply is dropped"
        );
        state.finish_resolve("studio", Ok(fixture("resolve-studio.json")));
        let effective = state.view().effective.unwrap();
        assert_eq!(effective.text,
            "Effective connection: person@studio.example, port 2222\nKey paths: /home/example/.ssh/id_ed25519, /home/example/.ssh/id_rsa\nJump host: bastion.example.net");
        assert_eq!(
            (
                effective.hostname.as_str(),
                effective.user.as_str(),
                effective.port.as_str(),
                effective.key.as_str()
            ),
            (
                "studio.example",
                "person",
                "2222",
                "/home/example/.ssh/id_ed25519"
            )
        );
        assert!(state.start_resolve("build-01"));
        state.finish_resolve("build-01", Err("ssh -G timed out".into()));
        let failed = state.view().effective.unwrap();
        assert!(failed.error);
        assert_eq!(failed.text, "Could not read effective SSH settings: ssh -G timed out\nYour connection overrides are shown below.");
    }

    #[test]
    fn a_wsl_machine_carries_its_profile_test_runner_and_effective_fields() {
        let mut state = MachinesState::default();
        state.start_list(0);
        state.finish_list(Ok(serde_json::json!({
            "local": "workstation",
            "revision": "a".repeat(64),
            "machines": [{
                "alias": "ubuntu", "enabled": true, "source": "local", "inherited": false,
                "connection": {"transport": "wsl-local", "enabled": true, "test_runner": "linux",
                    "wsl": {"distribution": "Ubuntu", "user": "dev", "hgs_path": "/home/test/.local/bin/hgs"}}
            }]
        })));
        let row = &state.view().machines[0];
        assert_eq!((row.transport.as_str(), row.test_runner.as_str()), ("wsl-local", "linux"));
        assert_eq!(row.wsl.as_ref().unwrap(), &WslProfile { distribution: "Ubuntu".into(), user: "dev".into(), hgs_path: "/home/test/.local/bin/hgs".into() });
        assert!(row.origin.starts_with("Local WSL uses the explicit distribution"));
        state.start_resolve("ubuntu");
        state.finish_resolve("ubuntu", Ok(serde_json::json!({
            "alias": "ubuntu",
            "effective": {"transport": "wsl-local", "distribution": "Ubuntu", "user": "dev",
                "hgs_path": "/home/test/.local/bin/hgs", "service_policy": "existing-only"}
        })));
        let effective = state.view().effective.unwrap();
        assert_eq!(
            (effective.transport.as_str(), effective.distribution.as_str(), effective.user.as_str(), effective.hgs_path.as_str()),
            ("wsl-local", "Ubuntu", "dev", "/home/test/.local/bin/hgs")
        );
        assert_eq!(effective.text, "WSL: Ubuntu, user dev\nHGS: /home/test/.local/bin/hgs\nOpenCode: existing-only");
        state.start_check("ubuntu");
        state.finish_check("ubuntu", Ok(serde_json::json!({"transport": "wsl-local", "ok": true, "distribution": "Ubuntu", "user": "dev", "hgs": "hgs 1.56.0"})));
        let check = state.view().check.unwrap();
        assert!(!check.error);
        assert_eq!(check.text, "Connected to WSL Ubuntu as dev: hgs 1.56.0. OpenCode policy: existing-only.");
    }

    #[test]
    fn a_check_runs_alone_and_reports_qt_texts() {
        let mut state = MachinesState::default();
        state.start_list(0);
        assert!(!state.start_check("studio"), "not while the list reads");
        state.finish_list(Ok(fixture("machines.json")));
        assert!(state.start_check("studio"));
        assert!(!state.start_check("studio") && state.busy());
        assert!(
            !state.start_list(5),
            "no list read while a check runs (Qt's reload)"
        );
        assert_eq!(
            state.view().check.unwrap().text,
            "Testing SSH and checking HGS…"
        );
        state.finish_check("studio", Ok(fixture("check-studio.json")));
        let check = state.view().check.unwrap();
        assert!(check.error && !check.pending);
        assert_eq!(check.text, "SSH connected: Linux x86_64. Missing: tmux, Rust. Install the newest stable Rust and tmux in the SSH terminal if needed, then use Install / update HGS.");
        let texts = |reply: Value| {
            let mut s = MachinesState::default();
            s.start_check("studio");
            s.finish_check("studio", Ok(reply));
            s.view().check.unwrap()
        };
        let ok = texts(
            serde_json::json!({"ok": true, "ssh_ok": true, "os": "Linux", "arch": "x86_64", "hgs": "hgs 1.56.0", "tmux": "tmux 3.5a"}),
        );
        assert_eq!(
            (ok.error, ok.text.as_str()),
            (false, "Connected: Linux x86_64, hgs 1.56.0, tmux 3.5a")
        );
        let down = texts(
            serde_json::json!({"ok": false, "ssh_ok": false, "error": "Permission denied (publickey)."}),
        );
        assert_eq!(down.text, "SSH connection failed: Permission denied (publickey).\nOpen SSH terminal to check credentials or approve the host key, then test again.");
    }
}
