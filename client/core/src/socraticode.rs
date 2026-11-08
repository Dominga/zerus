//! WhiteWh fork: the SocratiCode index of a session's folder
//! (`tray/src/SocratiCodePanel.h`, the fork's `src/extensions/socraticode.rs`).
//!
//! The panel reads `hgs [@host] socraticode status PATH --qdrant native|docker`
//! and drives the one next step the CLI offers: `install`, `install-qdrant`,
//! `start`, `stop` or `connect`, with the same arguments, timeouts and texts
//! Qt uses. The index itself, Qdrant, Ollama and the folder's agents belong to
//! `hgs` and SocratiCode; this module only reads their report and projects it.
//!
//! Reference: `SocratiCodePanel.h` (render 171-246, act 134-151), `HgsClient.cpp:727-742`.
use crate::actions::{Exit, Invocation};
use crate::ComputerId;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// `requestSocratiCode`: 30 s for status, stop and connect, 120 s for a start
/// that may bring Qdrant and Ollama up, 300 s for either install.
pub const STATUS_SECONDS: u64 = 30;
pub const START_SECONDS: u64 = 120;
pub const INSTALL_SECONDS: u64 = 300;
/// The largest accepted status: it lists a folder's index, not its files.
pub const STATUS_LIMIT: usize = 4 * 1024 * 1024;
/// Qt's `parseObject` failure text for a reply that is no object.
pub const INVALID: &str = "Invalid SocratiCode response";

/// The one action `hgs socraticode` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Status,
    Start,
    Stop,
    Connect,
    Install,
    InstallQdrant,
}

impl Act {
    pub fn word(self) -> &'static str {
        match self {
            Act::Status => "status",
            Act::Start => "start",
            Act::Stop => "stop",
            Act::Connect => "connect",
            Act::Install => "install",
            Act::InstallQdrant => "install-qdrant",
        }
    }

    /// `install` and `install-qdrant` name no folder.
    pub fn machine_wide(self) -> bool {
        matches!(self, Act::Install | Act::InstallQdrant)
    }

    pub fn seconds(self) -> u64 {
        match self {
            Act::Install | Act::InstallQdrant => INSTALL_SECONDS,
            Act::Start => START_SECONDS,
            _ => STATUS_SECONDS,
        }
    }

    pub fn parse(word: &str) -> Option<Act> {
        Some(match word {
            "status" => Act::Status,
            "start" => Act::Start,
            "stop" => Act::Stop,
            "connect" => Act::Connect,
            "install" => Act::Install,
            "install-qdrant" => Act::InstallQdrant,
            _ => return None,
        })
    }
}

fn hgs_args(computer: &ComputerId, words: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(words.iter().map(|word| (*word).to_owned()));
    args
}

/// `hgs [@host] socraticode ACTION [PATH --qdrant native|docker]`. The folder
/// is one argv word and cannot begin with a dash, where `hgs` reads options.
pub fn invocation(
    computer: &ComputerId,
    act: Act,
    folder: &str,
    native: bool,
) -> Result<Invocation, String> {
    let mut words = vec!["socraticode", act.word()];
    if !act.machine_wide() {
        if folder.is_empty() || folder.starts_with('-') {
            return Err(
                "Choose a folder path that is not empty and does not begin with a dash.".into(),
            );
        }
        words.push(folder);
        words.push("--qdrant");
        words.push(if native { "native" } else { "docker" });
    }
    Ok(Invocation {
        args: hgs_args(computer, &words),
        stdin: None,
        timeout: Duration::from_secs(act.seconds()),
        request_id: None,
    })
}

/// A `status` reply, field by field as Qt reads it. Every part is optional:
/// a missing or mistyped field is Qt's `toObject`/`toBool`/`toInteger` default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub path: String,
    pub plugin_installed: bool,
    pub docker: bool,
    pub mode: String,
    pub url: String,
    pub qdrant_reachable: bool,
    pub agents: bool,
    pub native_installed: bool,
    pub native_installable: bool,
    pub native_version: String,
    pub node: bool,
    pub ollama_reachable: bool,
    pub ollama_model: String,
    pub ollama_model_present: bool,
    pub index_state: String,
    pub chunks: u64,
    pub files_total: u64,
    pub files_indexed: u64,
    pub last_indexed_at: String,
    pub watching: bool,
    pub runner_running: bool,
    pub runner_phase: String,
    pub runner_error: String,
}

/// A missing part reads as Qt's null/empty default.
static NULL: Value = Value::Null;

fn object<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&NULL)
}

fn text(value: &Value, key: &str) -> String {
    object(value, key).as_str().unwrap_or("").to_owned()
}

fn flag(value: &Value, key: &str) -> bool {
    object(value, key).as_bool().unwrap_or(false)
}

fn count(value: &Value, key: &str) -> u64 {
    object(value, key).as_u64().unwrap_or(0)
}

fn status_of(value: &Value) -> Status {
    let qdrant = object(value, "qdrant");
    let native = object(qdrant, "native");
    let ollama = object(value, "ollama");
    let index = object(value, "index");
    let runner = object(value, "runner");
    Status {
        path: text(value, "path"),
        plugin_installed: flag(object(value, "plugin"), "installed"),
        docker: flag(value, "docker"),
        mode: text(qdrant, "mode"),
        url: text(qdrant, "url"),
        qdrant_reachable: flag(qdrant, "reachable"),
        agents: flag(qdrant, "agents"),
        native_installed: flag(native, "installed"),
        native_installable: flag(native, "installable"),
        native_version: text(native, "version"),
        node: flag(value, "node"),
        ollama_reachable: flag(ollama, "reachable"),
        ollama_model: text(ollama, "model"),
        ollama_model_present: flag(ollama, "model_present"),
        index_state: text(index, "state"),
        chunks: count(index, "chunks"),
        files_total: count(index, "files_total"),
        files_indexed: count(index, "files_indexed"),
        last_indexed_at: text(index, "last_indexed_at"),
        watching: flag(value, "watching"),
        runner_running: flag(runner, "running"),
        runner_phase: text(runner, "phase"),
        runner_error: text(runner, "error"),
    }
}

/// A `status` reply, or Qt's error for a failed call or a reply that is no
/// object. The CLI's own refusal (a folder that is gone) keeps its sentence.
pub fn status(exit: Exit) -> Result<Status, String> {
    let stdout = match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => stdout,
        exit => return Err(crate::actions::failure_detail(exit)),
    };
    let value: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
    if !value.is_object() {
        return Err(INVALID.into());
    }
    Ok(status_of(&value))
}

/// What an action answered. `installed` is Qt's check after `install` and
/// `install-qdrant`; other actions ignore it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Acted {
    pub installed: bool,
}

/// An action's reply: exit 0 and an object is done, else `failureDetail`
/// (install steps that failed keep their own standard error).
pub fn acted(exit: Exit) -> Result<Acted, String> {
    let stdout = match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => stdout,
        exit => return Err(crate::actions::failure_detail(exit)),
    };
    let value: Value = serde_json::from_slice(&stdout).unwrap_or(Value::Null);
    if !value.is_object() {
        return Err(INVALID.into());
    }
    Ok(Acted {
        installed: flag(&value, "installed"),
    })
}

/// The state line's parts; the webview words them with `tr()`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum State {
    /// A read is in flight and no answer is held yet.
    Checking,
    /// No read has run yet (the panel just opened).
    NotChecked,
    /// The read failed and no answer is held.
    Unavailable,
    Indexed {
        chunks: u64,
        files: u64,
        /// The clock value; the frontend words the age.
        last_indexed_at: String,
        watching: bool,
        qdrant: bool,
        startable: bool,
    },
    Indexing {
        indexed: u64,
        total: u64,
        qdrant: bool,
        startable: bool,
    },
    NotIndexed {
        qdrant: bool,
        startable: bool,
    },
}

/// One line of the panel's detail block.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum Detail {
    /// The Claude Code plugin is not installed.
    PluginMissing,
    /// SocratiCode's own Docker Qdrant is missing.
    DockerMissing,
    /// A native Qdrant is wanted and not there; it may be installable.
    NativeMissing {
        installable: bool,
    },
    /// A native Qdrant is used, the folder's agents still name Docker's.
    AgentsDocker {
        url: String,
    },
    NodeMissing,
    OllamaModelMissing {
        model: String,
    },
    /// The retained runner failed; Qt shows its own first lines.
    RunnerFailed {
        error: String,
    },
}

/// The one next step, as Qt chooses it. `Start`'s text depends on a fresh
/// folder and on a reachable Qdrant.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Install,
    /// The pinned native Qdrant the download would fetch.
    InstallQdrant {
        version: String,
    },
    Start {
        fresh: bool,
        qdrant: bool,
    },
}

/// The panel as the webview renders it: the state line's parts, the details,
/// the one action, and which of Connect and Stop show. `poll` asks for the
/// 10-second refresh Qt runs while work is under way.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Panel {
    pub state: State,
    pub details: Vec<Detail>,
    pub action: Option<Action>,
    pub connect: bool,
    pub stop: bool,
    pub poll: bool,
}

/// The panel before any status is held (`render` with an empty `m_data`).
pub fn blank(busy: bool, failed: bool) -> Panel {
    Panel {
        state: if busy {
            State::Checking
        } else if failed {
            State::Unavailable
        } else {
            State::NotChecked
        },
        details: Vec::new(),
        action: None,
        connect: false,
        stop: false,
        poll: false,
    }
}

/// Qt's `render()` over a status: Zerus can start the Qdrant here, so a native
/// one (or one the folder's settings name on localhost) is startable.
pub fn panel(status: &Status) -> Panel {
    let qdrant = status.qdrant_reachable;
    let native = status.native_installed;
    let startable = native
        && (status.mode == "native"
            || (status.mode == "external" && status.url.starts_with("http://127.0.0.1")));
    let indexed = status.index_state == "indexed";
    let indexing = status.index_state == "indexing" || status.runner_phase == "indexing";
    let state = if indexed {
        State::Indexed {
            chunks: status.chunks,
            files: status.files_indexed,
            last_indexed_at: status.last_indexed_at.clone(),
            watching: status.watching,
            qdrant,
            startable,
        }
    } else if indexing {
        State::Indexing {
            indexed: status.files_indexed,
            total: status.files_total,
            qdrant,
            startable,
        }
    } else {
        State::NotIndexed { qdrant, startable }
    };
    let mut details = Vec::new();
    if !status.plugin_installed {
        details.push(Detail::PluginMissing);
    }
    if status.mode == "managed" && !status.docker {
        details.push(Detail::DockerMissing);
    }
    if status.mode == "native" && !native && !qdrant {
        details.push(Detail::NativeMissing {
            installable: status.native_installable,
        });
    }
    if !status.agents && status.mode == "native" {
        details.push(Detail::AgentsDocker {
            url: status.url.clone(),
        });
    }
    if !status.node {
        details.push(Detail::NodeMissing);
    }
    if status.ollama_reachable && !status.ollama_model_present {
        details.push(Detail::OllamaModelMissing {
            model: status.ollama_model.clone(),
        });
    }
    if status.runner_phase == "failed" && !status.runner_error.is_empty() {
        let error: String = status
            .runner_error
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_owned();
        details.push(Detail::RunnerFailed { error });
    }
    let needs_qdrant = !qdrant && !startable && !(status.mode == "managed" && status.docker);
    let action = if !status.plugin_installed {
        Some(Action::Install)
    } else if needs_qdrant && status.mode == "native" && status.native_installable {
        Some(Action::InstallQdrant {
            version: status.native_version.clone(),
        })
    } else if needs_qdrant {
        None
    } else if !status.runner_running && !status.watching {
        Some(Action::Start {
            fresh: status.index_state == "none",
            qdrant,
        })
    } else {
        None
    };
    Panel {
        state,
        details,
        action,
        connect: status.mode == "native" && !status.agents,
        stop: status.runner_running,
        poll: status.runner_running || indexing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(value: Value) -> Exit {
        Exit::Code {
            code: 0,
            stdout: value.to_string().into_bytes(),
            stderr: String::new(),
        }
    }

    fn status_json() -> Value {
        json!({
            "path": "D:\\Work\\repo", "project_id": "abc", "plugin": {"installed": true, "version": "1.16.1"},
            "docker": true,
            "qdrant": {"mode": "native", "url": "http://127.0.0.1:6333", "reachable": true, "agents": false,
                       "native": {"installed": true, "version": "1.19.1", "installable": true}},
            "ollama": {"provider": "ollama", "url": "http://127.0.0.1:11434", "reachable": true,
                       "model": "nomic-embed-text", "model_present": false, "installed": true},
            "node": true,
            "index": {"state": "indexed", "chunks": 1234, "files_total": 56, "files_indexed": 56,
                      "last_indexed_at": "2026-11-06T09:00:00.000Z"},
            "watching": true, "runner": {"running": true, "phase": "watching"}
        })
    }

    #[test]
    fn calls_are_the_cli_verbs_on_exactly_that_computer() {
        let status = invocation(&ComputerId::local(), Act::Status, "D:\\Work\\repo", true).unwrap();
        assert_eq!(
            status.args,
            [
                "socraticode",
                "status",
                "D:\\Work\\repo",
                "--qdrant",
                "native"
            ]
        );
        assert_eq!(status.timeout, Duration::from_secs(STATUS_SECONDS));
        let peer = invocation(&ComputerId::from("studio"), Act::Start, "/srv/repo", false).unwrap();
        assert_eq!(
            peer.args,
            [
                "@studio",
                "socraticode",
                "start",
                "/srv/repo",
                "--qdrant",
                "docker"
            ]
        );
        assert_eq!(peer.timeout, Duration::from_secs(START_SECONDS));
        // A machine-wide install names no folder and may take five minutes.
        for (act, word) in [
            (Act::Install, "install"),
            (Act::InstallQdrant, "install-qdrant"),
        ] {
            let inv = invocation(&ComputerId::local(), act, "", true).unwrap();
            assert_eq!(inv.args, ["socraticode", word]);
            assert_eq!(inv.timeout, Duration::from_secs(INSTALL_SECONDS));
        }
        // A folder is one argv word and cannot be read as an option.
        assert!(invocation(&ComputerId::local(), Act::Stop, "", true).is_err());
        assert!(invocation(&ComputerId::local(), Act::Status, "--qdrant", true).is_err());
        assert!(Act::parse("start").is_some());
        assert!(Act::parse("index").is_none());
    }

    #[test]
    fn a_status_reads_the_cli_report() {
        let parsed = status(reply(status_json())).unwrap();
        assert_eq!(parsed.mode, "native");
        assert_eq!(parsed.chunks, 1234);
        assert_eq!(parsed.last_indexed_at, "2026-11-06T09:00:00.000Z");
        assert!(parsed.plugin_installed && parsed.qdrant_reachable && parsed.runner_running);
        assert!(!parsed.ollama_model_present);
        assert_eq!(status(reply(json!([]))), Err(INVALID.into()));
        assert_eq!(status(Exit::TimedOut), Err("timed out".into()));
        let refusal = status(Exit::Code {
            code: 1,
            stdout: Vec::new(),
            stderr: "D:\\Work\\repo is not a folder.\n".into(),
        });
        assert_eq!(refusal, Err("D:\\Work\\repo is not a folder.".into()));
    }

    #[test]
    fn the_one_next_step_follows_qt() {
        let running = panel(&status(reply(status_json())).unwrap());
        assert!(matches!(
            running.state,
            State::Indexed {
                chunks: 1234,
                files: 56,
                watching: true,
                qdrant: true,
                startable: true,
                ..
            }
        ));
        assert_eq!(running.action, None, "a watched index needs nothing");
        // The folder's agents still name Docker's Qdrant, so Connect shows.
        assert!(running.stop && running.poll && running.connect);
        assert_eq!(
            running.details,
            [
                Detail::AgentsDocker {
                    url: "http://127.0.0.1:6333".into()
                },
                Detail::OllamaModelMissing {
                    model: "nomic-embed-text".into()
                }
            ]
        );
        // The plugin first; a Qdrant is installed only where Qt offers it.
        let mut plugin = status_json();
        plugin["plugin"]["installed"] = json!(false);
        assert_eq!(
            panel(&status(reply(plugin)).unwrap()).action,
            Some(Action::Install)
        );
        let mut missing = status_json();
        missing["qdrant"]["reachable"] = json!(false);
        missing["qdrant"]["native"]["installed"] = json!(false);
        missing["watching"] = json!(false);
        missing["runner"] = json!({"running": false});
        missing["index"]["state"] = json!("none");
        let missing = status(reply(missing)).unwrap();
        assert_eq!(
            panel(&missing).action,
            Some(Action::InstallQdrant {
                version: "1.19.1".into()
            })
        );
        assert!(panel(&missing)
            .details
            .contains(&Detail::NativeMissing { installable: true }));
        assert!(matches!(
            panel(&missing).state,
            State::NotIndexed { qdrant: false, .. }
        ));
        // Without an installable native Qdrant there is no step to offer.
        let mut blocked = missing.clone();
        blocked.native_installable = false;
        blocked.native_installed = false;
        assert_eq!(panel(&blocked).action, None);
        assert!(panel(&blocked)
            .details
            .contains(&Detail::NativeMissing { installable: false }));
        // An existing index without a runner offers Watch (not "Index and watch").
        let mut stopped = status(reply(status_json())).unwrap();
        stopped.runner_running = false;
        stopped.watching = false;
        let stopped = panel(&stopped);
        assert_eq!(
            stopped.action,
            Some(Action::Start {
                fresh: false,
                qdrant: true
            })
        );
        assert!(!stopped.stop && !stopped.poll);
        // A fresh folder asks to index; indexing polls.
        let mut fresh = status(reply(status_json())).unwrap();
        fresh.index_state = "none".into();
        fresh.runner_running = false;
        fresh.watching = false;
        assert_eq!(
            panel(&fresh).action,
            Some(Action::Start {
                fresh: true,
                qdrant: true
            })
        );
        let mut indexing = status(reply(status_json())).unwrap();
        indexing.index_state = "indexing".into();
        indexing.runner_running = false;
        indexing.watching = false;
        let indexing = panel(&indexing);
        assert!(matches!(
            indexing.state,
            State::Indexing {
                indexed: 56,
                total: 56,
                ..
            }
        ));
        assert!(indexing.poll);
    }

    #[test]
    fn docker_and_the_plugin_explain_themselves() {
        let mut managed = status_json();
        managed["qdrant"]["mode"] = json!("managed");
        managed["qdrant"]["reachable"] = json!(false);
        managed["docker"] = json!(false);
        managed["index"] = json!({"state": "none"});
        managed["watching"] = json!(false);
        managed["runner"] = json!({"running": false});
        let managed = panel(&status(reply(managed)).unwrap());
        assert!(managed.details.contains(&Detail::DockerMissing));
        // Docker's Qdrant is not Zerus's to start.
        assert_eq!(managed.action, None);
        assert!(!managed.connect);
        let mut failed = status_json();
        failed["runner"] =
            json!({"running": false, "phase": "failed", "error": "one\ntwo\nthree\nfour"});
        failed["node"] = json!(false);
        let failed = panel(&status(reply(failed)).unwrap());
        assert!(failed.details.contains(&Detail::NodeMissing));
        assert!(failed.details.contains(&Detail::RunnerFailed {
            error: "one\ntwo\nthree".into()
        }));
        // A connect offer shows for a native Qdrant the folder's agents lack.
        assert!(panel(&status(reply(status_json())).unwrap()).connect);
    }

    #[test]
    fn an_action_is_done_only_with_a_reply() {
        assert_eq!(
            acted(reply(json!({"installed": true}))),
            Ok(Acted { installed: true })
        );
        assert_eq!(
            acted(reply(json!({"installed": false, "steps": ["x"]}))),
            Ok(Acted { installed: false })
        );
        assert_eq!(acted(reply(json!("done"))), Err(INVALID.into()));
        assert_eq!(
            acted(Exit::Code {
                code: 1,
                stdout: Vec::new(),
                stderr: "SocratiCode needs Node.js with npx on this computer.\n".into()
            }),
            Err("SocratiCode needs Node.js with npx on this computer.".into())
        );
    }

    #[test]
    fn an_empty_panel_says_what_it_knows() {
        assert!(matches!(blank(false, false).state, State::NotChecked));
        assert!(matches!(blank(true, false).state, State::Checking));
        assert!(matches!(blank(false, true).state, State::Unavailable));
        let blank = blank(true, false);
        assert_eq!(blank.details, []);
        assert!(blank.action.is_none() && !blank.connect && !blank.stop && !blank.poll);
    }
}
