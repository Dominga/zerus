//! Session folder actions and external terminal connections.
//! Folder availability does only a local stat; a folder click rechecks its
//! exact inspection. External connections bind the indexed run and use the
//! attach-only CLI guard. Menus and renders never launch a process.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::State;
use zerus_client_core::details::{
    details_invocation, parse_details, parse_subagent_identity, subagent_identity_invocation,
};
use zerus_client_core::{paths, terminal_launch, Target, ViewRow};

use crate::composer::{fetch::Opener, lock};

const UNAVAILABLE: &str = "Session folder is unavailable.";
const CHANGED: &str = "The session state changed. Open its actions again.";

pub struct SessionFolders {
    dir: PathBuf,
    opener: Opener,
    terminal: Arc<Mutex<terminal_launch::Launcher>>,
}

impl SessionFolders {
    pub(crate) fn opener(&self) -> Opener {
        self.opener.clone()
    }

    pub fn new(dir: PathBuf, opener: Opener) -> Self {
        Self {
            dir,
            opener,
            terminal: Arc::new(Mutex::new(terminal_launch::Launcher::new(
                Default::default(),
            ))),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Folder {
    target: Target,
    archive_id: Option<String>,
    agent_id: Option<String>,
    directory: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Folder,
    Shell,
}

#[derive(Debug, Serialize)]
pub struct Availability {
    folder: bool,
    shell: bool,
    folder_hint: String,
}

fn absolute(computer: &str, directory: &str) -> bool {
    !directory.is_empty()
        && directory.len() <= 32 * 1024
        && !directory.chars().any(char::is_control)
        && paths::absolute(directory, paths::of(None, computer.is_empty()))
}

fn availability(
    computer: &str,
    directory: &str,
    is_dir: impl FnOnce(&Path) -> bool,
) -> Availability {
    let shell = absolute(computer, directory);
    // Never stat a peer's path on this computer, even if the names match.
    let folder = computer.is_empty() && shell && is_dir(Path::new(directory));
    Availability {
        folder,
        shell,
        folder_hint: if !computer.is_empty() {
            "Available only for sessions on this computer".into()
        } else if folder {
            format!("Open session folder in file manager\n{directory}")
        } else {
            "Session folder is unavailable".into()
        },
    }
}

#[tauri::command]
pub async fn session_folder_status(
    computer: String,
    directory: String,
) -> Result<Availability, String> {
    tauri::async_runtime::spawn_blocking(move || availability(&computer, &directory, Path::is_dir))
        .await
        .map_err(|error| error.to_string())
}

fn current(row: Option<&ViewRow>, folder: &Folder) -> bool {
    current_target(row, &folder.target)
}

fn current_target(row: Option<&ViewRow>, target: &Target) -> bool {
    row.is_some_and(|row| {
        !target.run_id.is_empty()
            && !target.conversation_id.is_empty()
            && row.name == target.name
            && row.run_id.as_deref() == Some(&target.run_id)
            && row.conversation_id.as_deref() == Some(&target.conversation_id)
    })
}

fn terminal_arguments(
    row: Option<&ViewRow>,
    target: &Target,
    online: bool,
) -> Result<Vec<String>, String> {
    // Startup may precede the first native conversation binding. External
    // attach needs the original live run, unlike inspected folder actions.
    let Some(row) = row.filter(|row| {
        !target.run_id.is_empty()
            && row.name == target.name
            && row.run_id.as_deref() == Some(&target.run_id)
            && (target.conversation_id.is_empty()
                || row.conversation_id.as_deref() == Some(&target.conversation_id))
    }) else {
        return Err(CHANGED.into());
    };
    let facts = zerus_client_core::terminal::TerminalFacts {
        cmd: row.agent.clone(),
        terminal_supported: row.terminal_supported,
        terminal_reported: row.terminal_reported,
        terminal_attach_supported: row.terminal_attach_supported,
        terminal_attach_reported: row.terminal_attach_reported,
        ..Default::default()
    };
    zerus_client_core::terminal::available(
        &facts,
        &serde_json::json!({}),
        &row.state,
        online,
        false,
    )?;
    let mut args = zerus_client_core::shell::open_arguments(&target.computer, &target.name, None);
    // A delayed external terminal can never resume a stopped session or attach
    // to an unrelated replacement that reuses its name.
    args.extend([
        "--existing".into(),
        "--run-id".into(),
        target.run_id.clone(),
    ]);
    Ok(args)
}

fn terminal_arguments_from(
    pump: &crate::pump::Pump,
    target: &Target,
) -> Result<Vec<String>, String> {
    let key = zerus_watch_protocol::key_for(Some(&target.run_id), "");
    terminal_arguments(
        pump.row(&target.computer, &key).as_ref(),
        target,
        pump.online(&target.computer),
    )
}

#[tauri::command]
pub async fn session_terminal_open(
    app: State<'_, crate::commands::AppState>,
    folders: State<'_, SessionFolders>,
    target: Target,
) -> Result<Option<String>, String> {
    if crate::actions::blocked() {
        return Err(crate::actions::REPLAY_NEEDS_HGS.into());
    }
    let pump = app.pump();
    let terminal = Arc::clone(&folders.terminal);
    let dir = folders.dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut terminal = lock(&terminal);
        terminal.set_settings(crate::desk::terminal_settings(&dir));
        let args = terminal_arguments_from(&pump, &target)?;
        terminal.open_program(&zerus_client_core::default_hgs().to_string_lossy(), &args)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn inspected_directory(
    folder: &Folder,
    exit: zerus_client_core::Exit,
    fallback: &str,
) -> Option<String> {
    if let Some(agent) = &folder.agent_id {
        parse_subagent_identity(&folder.target, agent, exit)?.cwd
    } else {
        Some(
            parse_details(&folder.target, exit)?
                .cwd
                .unwrap_or_else(|| fallback.into()),
        )
    }
}

#[tauri::command]
pub async fn session_folder_open(
    app: State<'_, crate::commands::AppState>,
    folders: State<'_, SessionFolders>,
    input: Folder,
    kind: Kind,
) -> Result<Option<String>, String> {
    if crate::actions::blocked() {
        return Err(crate::actions::REPLAY_NEEDS_HGS.into());
    }
    if !absolute(&input.target.computer, &input.directory)
        || input.target.run_id.is_empty()
        || input.target.conversation_id.is_empty()
    {
        return Err(UNAVAILABLE.into());
    }
    let pump = app.pump();
    let row = pump.row_named(&input.target.computer, &input.target.name);
    let archived = input.archive_id.as_ref().is_some_and(|id| !id.is_empty());
    if !archived && !current(row.as_ref(), &input) {
        return Err(CHANGED.into());
    }
    if matches!(kind, Kind::Shell) && !pump.online(&input.target.computer) {
        return Err("Connect to this session's host to open a shell.".into());
    }
    let call = if let Some(agent) = &input.agent_id {
        subagent_identity_invocation(&input.target, input.archive_id.as_deref(), agent)
            .ok_or(UNAVAILABLE)?
    } else {
        details_invocation(&input.target, input.archive_id.as_deref())
    };
    let hgs = zerus_client_core::default_hgs();
    let checked = input.clone();
    let fallback = row.map(|row| row.cwd).unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let exit = zerus_client_core::run_limited(&hgs, &call, &[], 4 * 1024 * 1024)
            .map_err(|_| UNAVAILABLE)?;
        let directory = inspected_directory(&checked, exit, &fallback).ok_or(UNAVAILABLE)?;
        if directory != checked.directory {
            return Err(CHANGED.to_owned());
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())??;
    // Recheck after the read, including a run replaced while hgs was answering.
    if !archived
        && !current(
            pump.row_named(&input.target.computer, &input.target.name)
                .as_ref(),
            &input,
        )
    {
        return Err(CHANGED.into());
    }
    if matches!(kind, Kind::Shell) && !pump.online(&input.target.computer) {
        return Err("Connect to this session's host to open a shell.".into());
    }
    let opener = folders.opener.clone();
    let dir = folders.dir.clone();
    // Keep the maintained launcher in app state; no second shell/process stack.
    match kind {
        Kind::Folder => tauri::async_runtime::spawn_blocking(move || {
            if !availability(&input.target.computer, &input.directory, Path::is_dir).folder {
                return Err(UNAVAILABLE.into());
            }
            if !(opener.open)(Path::new(&input.directory)) {
                return Err("Could not open the session folder in the file manager.".into());
            }
            Ok(None)
        })
        .await
        .map_err(|error| error.to_string())?,
        Kind::Shell => {
            let terminal = Arc::clone(&folders.terminal);
            tauri::async_runtime::spawn_blocking(move || {
                let mut terminal = lock(&terminal);
                terminal.set_settings(crate::desk::terminal_settings(&dir));
                terminal.open_folder_shell(
                    &zerus_client_core::default_hgs().to_string_lossy(),
                    &input.target.computer,
                    &input.directory,
                )
            })
            .await
            .map_err(|error| error.to_string())?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    fn folder() -> Folder {
        Folder {
            target: Target {
                computer: "peer".into(),
                name: "codex/sample-project/review".into(),
                run_id: "run-1".into(),
                conversation_id: "conversation-1".into(),
            },
            directory: "/srv/sample-project".into(),
            archive_id: None,
            agent_id: None,
        }
    }

    fn exit(value: serde_json::Value) -> zerus_client_core::Exit {
        zerus_client_core::Exit::Code {
            code: 0,
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: String::new(),
        }
    }

    fn terminal_row() -> ViewRow {
        ViewRow {
            name: folder().target.name,
            run_id: Some("run-1".into()),
            conversation_id: Some("conversation-1".into()),
            agent: "codex".into(),
            state: "running".into(),
            terminal_supported: true,
            ..Default::default()
        }
    }

    #[test]
    fn external_connection_is_pinned_to_an_existing_local_or_remote_run() {
        let row = terminal_row();
        let mut target = folder().target;
        assert_eq!(
            terminal_arguments(Some(&row), &target, true).unwrap(),
            ["@peer", "a", &row.name, "--existing", "--run-id", "run-1"]
        );
        target.computer.clear();
        assert_eq!(
            terminal_arguments(Some(&row), &target, true).unwrap(),
            ["a", &row.name, "--existing", "--run-id", "run-1"]
        );
    }

    #[test]
    fn external_startup_connection_needs_only_its_original_run() {
        let mut target = folder().target;
        target.conversation_id.clear();
        for conversation_id in [None, Some(String::new()), Some("first-binding".into())] {
            let row = ViewRow {
                conversation_id,
                ..terminal_row()
            };
            assert_eq!(
                terminal_arguments(Some(&row), &target, true).unwrap(),
                ["@peer", "a", &row.name, "--existing", "--run-id", "run-1"]
            );
            // Folder inspection still requires both IDs, even for this run.
            assert!(!current_target(Some(&row), &target));
        }
    }

    #[test]
    fn external_startup_connection_keeps_source_and_no_resume_guards() {
        let mut target = folder().target;
        target.conversation_id.clear();
        let row = ViewRow {
            conversation_id: None,
            ..terminal_row()
        };
        assert!(terminal_arguments(None, &target, true).is_err());
        assert!(terminal_arguments(Some(&row), &target, false).is_err());
        for changed in [
            ViewRow {
                run_id: None,
                ..row.clone()
            },
            ViewRow {
                run_id: Some("replacement".into()),
                ..row.clone()
            },
            ViewRow {
                name: "codex/sample-project/renamed".into(),
                ..row.clone()
            },
            ViewRow {
                state: "paused".into(),
                ..row.clone()
            },
            ViewRow {
                state: "stopped".into(),
                ..row.clone()
            },
            ViewRow {
                terminal_supported: false,
                terminal_reported: true,
                ..row.clone()
            },
        ] {
            assert!(terminal_arguments(Some(&changed), &target, true).is_err());
        }
        target.run_id.clear();
        assert!(terminal_arguments(Some(&row), &target, true).is_err());
    }

    #[test]
    fn external_connection_selects_the_exact_indexed_run_when_labels_are_shared() {
        use zerus_client_core::{Change, ComputerId, ComputerStatus};
        let pump = crate::pump::Pump::default();
        let target = folder().target;
        let row = terminal_row();
        pump.push(Change::Replace {
            computer: ComputerId("peer".into()),
            rows: vec![
                ("run:run-1".into(), row.clone()),
                (
                    "run:other".into(),
                    ViewRow {
                        run_id: Some("other".into()),
                        conversation_id: Some("other".into()),
                        ..row
                    },
                ),
            ],
        });
        assert!(terminal_arguments_from(&pump, &target).is_ok());
        pump.push(Change::Remove {
            computer: ComputerId("peer".into()),
            key: "run:run-1".into(),
            reason: None,
        });
        assert_eq!(terminal_arguments_from(&pump, &target), Err(CHANGED.into()));
        pump.push(Change::Upsert {
            computer: ComputerId("peer".into()),
            key: "run:run-1".into(),
            row: terminal_row(),
        });
        pump.push(Change::Computer {
            computer: ComputerId("peer".into()),
            status: ComputerStatus::Ended,
        });
        assert!(terminal_arguments_from(&pump, &target).is_err());
    }

    #[test]
    fn external_connection_refuses_missing_changed_offline_stopped_or_unsupported_targets() {
        let row = terminal_row();
        let target = folder().target;
        assert!(terminal_arguments(None, &target, true).is_err());
        for key in ["name", "run", "conversation"] {
            let mut changed = target.clone();
            match key {
                "name" => changed.name = "codex/sample-project/replacement".into(),
                "run" => changed.run_id = "replacement".into(),
                _ => changed.conversation_id = "replacement".into(),
            }
            assert_eq!(
                terminal_arguments(Some(&row), &changed, true),
                Err(CHANGED.into())
            );
        }
        assert!(terminal_arguments(Some(&row), &target, false).is_err());
        for state in ["paused", "stopped", "archived"] {
            assert!(terminal_arguments(
                Some(&ViewRow {
                    state: state.into(),
                    ..row.clone()
                }),
                &target,
                true
            )
            .is_err());
        }
        for cmd in ["deepseek", "dsh", "opencode"] {
            assert!(terminal_arguments(
                Some(&ViewRow {
                    agent: cmd.into(),
                    terminal_supported: false,
                    ..row.clone()
                }),
                &target,
                true
            )
            .is_err());
        }
        let opencode = ViewRow {
            agent: "opencode".into(),
            terminal_supported: false,
            terminal_attach_supported: true,
            terminal_attach_reported: true,
            ..row
        };
        assert!(terminal_arguments(Some(&opencode), &target, true).is_ok());
    }

    #[test]
    fn peer_shell_never_stats_or_opens_a_local_namesake() {
        let called = Cell::new(false);
        let state = availability("peer", "/srv/sample-project", |_| {
            called.set(true);
            true
        });
        assert!(state.shell);
        assert!(!state.folder);
        assert!(!called.get());
        assert_eq!(
            state.folder_hint,
            "Available only for sessions on this computer"
        );
        assert!(!availability("peer", "relative", |_| panic!("peer stat")).shell);
        assert!(!availability("peer", "C:/project", |_| panic!("peer stat")).shell);
    }

    #[test]
    fn only_existing_absolute_local_directories_enable_the_file_manager() {
        let directory = std::env::temp_dir().join(format!("zerus-folder-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.to_str().unwrap();
        assert!(availability("", path, Path::is_dir).folder);
        std::fs::remove_dir(&directory).unwrap();
        let state = availability("", path, Path::is_dir);
        assert!(!state.folder);
        assert!(state.shell); // Qt permits a shell to try a missing absolute folder.
        assert_eq!(state.folder_hint, "Session folder is unavailable");
        assert!(!availability("", "relative", |_| panic!("relative stat")).folder);
        assert!(!availability("", "", |_| panic!("empty stat")).shell);
    }

    #[test]
    fn fresh_inspection_uses_present_empty_cwd_and_binds_both_ids() {
        let input = folder();
        let value =
            json!({"run_id":"run-1", "conversation_id":"conversation-1", "cwd":"/srv/current"});
        assert_eq!(
            inspected_directory(&input, exit(value.clone()), "/fallback").as_deref(),
            Some("/srv/current")
        );
        for key in ["run_id", "conversation_id"] {
            let mut changed = value.clone();
            changed[key] = json!("replacement");
            assert!(inspected_directory(&input, exit(changed), "/fallback").is_none());
        }
        let mut empty = value.clone();
        empty["cwd"] = json!("");
        assert_eq!(
            inspected_directory(&input, exit(empty), "/fallback").as_deref(),
            Some("")
        );
        let mut absent = value;
        absent.as_object_mut().unwrap().remove("cwd");
        assert_eq!(
            inspected_directory(&input, exit(absent), "/fallback").as_deref(),
            Some("/fallback")
        );
    }

    #[test]
    fn child_folder_is_its_own_exact_inspection_and_never_the_parent_fallback() {
        let mut input = folder();
        input.agent_id = Some("child-1".into());
        let value = json!({"name":input.target.name,"run_id":"run-1", "parent_conversation_id":"conversation-1", "agent_id":"child-1", "conversation_id":"child-conversation", "cwd":"/srv/child"});
        assert_eq!(
            inspected_directory(&input, exit(value.clone()), "/parent").as_deref(),
            Some("/srv/child")
        );
        for key in ["name", "run_id", "parent_conversation_id", "agent_id"] {
            let mut changed = value.clone();
            changed[key] = json!("replacement");
            assert!(inspected_directory(&input, exit(changed), "/parent").is_none());
        }
        let mut absent = value;
        absent.as_object_mut().unwrap().remove("cwd");
        assert!(inspected_directory(&input, exit(absent), "/parent").is_none());
    }
}
