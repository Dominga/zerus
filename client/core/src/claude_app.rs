//! WhiteWh fork: the Claude desktop app's Code-tab sessions, continued in
//! Zerus (`tray/src/ClaudeAppSessionsDialog.h`, the fork's `src/claude_app.rs`).
//!
//! The read is exactly Qt's: `hgs [@host] claude-app ls`, whose reply lists the
//! newest sessions of the app on that computer. Continuing one is
//! `hgs [@host] claude-app resume ID -n NAME --launch-id UUID -d [--force]`,
//! which starts Claude Code with that conversation in its folder as a new
//! detached Zerus session. The client never writes the app's files.
//!
//! Reference: `tray/src/ClaudeAppSessionsDialog.h` (125-155 render, 168-185
//! confirm), `HgsClient.cpp:702-725` and
//! `SessionsWindow.cpp:3688-3701`.
use crate::actions::{Exit, Invocation};
use crate::ComputerId;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

/// `requestClaudeAppSessions` waits 30 s; `resumeClaudeAppSession` 90 s.
pub const LIST_SECONDS: u64 = 30;
pub const RESUME_SECONDS: u64 = 90;
/// The largest accepted list: hgs lists at most 200 small sessions.
pub const LIST_LIMIT: usize = 4 * 1024 * 1024;
/// Qt's `ClaudeAppSessionsDialog::request` says this when a reply is no object.
pub const INVALID: &str = "Invalid Claude app session list";

fn hgs_args(computer: &ComputerId, words: &[&str]) -> Vec<String> {
    let mut args: Vec<String> = computer
        .alias()
        .map(|alias| format!("@{alias}"))
        .into_iter()
        .collect();
    args.extend(words.iter().map(|word| (*word).to_owned()));
    args
}

fn call(args: Vec<String>, seconds: u64) -> Invocation {
    Invocation {
        args,
        stdin: None,
        timeout: Duration::from_secs(seconds),
        request_id: None,
    }
}

/// `hgs [@host] claude-app ls`, 30 s (`requestClaudeAppSessions`).
pub fn list_invocation(computer: &ComputerId) -> Invocation {
    call(hgs_args(computer, &["claude-app", "ls"]), LIST_SECONDS)
}

/// One app session as `ls` reports it. Missing or mistyped fields read as Qt's
/// `QJsonValue::toString`/`toBool` do: empty, false or zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Session {
    pub id: String,
    /// The native Claude Code conversation (`cliSessionId`).
    pub conversation: String,
    pub cwd: String,
    pub title: String,
    pub suggested_name: String,
    pub archived: bool,
    pub running: bool,
    pub running_process: bool,
    pub history: bool,
    pub folder_exists: bool,
    pub last_activity: f64,
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_owned()
}

fn flag(value: &Value, key: &str) -> bool {
    value[key].as_bool().unwrap_or(false)
}

fn session(value: &Value) -> Option<Session> {
    if !value.is_object() {
        return None;
    }
    Some(Session {
        id: text(value, "id"),
        conversation: text(value, "conversation"),
        cwd: text(value, "cwd"),
        title: text(value, "title"),
        suggested_name: text(value, "suggested_name"),
        archived: flag(value, "archived"),
        running: flag(value, "running"),
        running_process: flag(value, "running_process"),
        history: flag(value, "history"),
        folder_exists: flag(value, "folder_exists"),
        last_activity: value["last_activity"].as_f64().unwrap_or(0.0),
    })
}

/// The `claude-app ls` reply: an object (Qt's `parseObject`), whose `sessions`
/// array is read in its order and whose `available`/`truncated` flags keep
/// their meaning. Anything else, or a failed call, is the dialog's error.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct List {
    pub sessions: Vec<Session>,
    pub truncated: bool,
    /// False when the app keeps no sessions on this computer.
    pub available: bool,
}

pub fn listed(exit: Exit) -> Result<List, String> {
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
    Ok(List {
        sessions: value["sessions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(session)
            .collect(),
        truncated: flag(&value, "truncated"),
        available: flag(&value, "available"),
    })
}

/// Qt's `ClaudeAppSessionsDialog::normalized`: native separators, without a
/// trailing separator except at a root.
pub fn normalize(path: &str) -> String {
    let mut text = path.replace('\\', "/");
    while text.len() > 1 && text.ends_with('/') && !text.ends_with(":/") {
        text.pop();
    }
    text
}

/// Qt's `sameFolder`: paths compare without case when they carry a backslash or
/// a colon (a Windows path), else with it.
pub fn same_folder(path: &str, folder: &str) -> bool {
    let left = normalize(path);
    let right = normalize(folder);
    if path.contains('\\') || path.contains(':') {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

/// Qt's State column: a running session first, then archived, then stopped;
/// no history or a missing folder replaces all three.
pub fn state(session: &Session) -> &'static str {
    if !session.history {
        "no_history"
    } else if !session.folder_exists {
        "folder_missing"
    } else if session.running {
        "open_in_app"
    } else if session.archived {
        "archived"
    } else {
        "stopped"
    }
}

/// One row of the dialog: the session, its State code and whether its folder
/// is the dialog's "This folder only" folder.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    #[serde(flatten)]
    pub session: Session,
    pub state: &'static str,
    pub in_folder: bool,
}

/// The dialog's read: the app's sessions with their State and folder match,
/// filtered by nothing (the checkbox filters the rows the webview holds).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct View {
    pub sessions: Vec<Row>,
    pub truncated: bool,
    pub available: bool,
    /// The folder the dialog opened with, as Qt's checkbox tooltip shows it.
    pub folder: String,
    pub folder_enabled: bool,
}

pub fn view(list: &List, folder: &str) -> View {
    View {
        sessions: list
            .sessions
            .iter()
            .map(|session| Row {
                state: state(session),
                in_folder: !folder.is_empty() && same_folder(&session.cwd, folder),
                session: session.clone(),
            })
            .collect(),
        truncated: list.truncated,
        available: list.available,
        folder: folder.to_owned(),
        folder_enabled: !folder.is_empty(),
    }
}

/// `hgs [@host] claude-app resume ID -n NAME --launch-id UUID -d [--force]`,
/// 90 s. One argv word per value: the positional ID cannot be read as an
/// option, the name passes Qt's SessionTag rule and the launch ID is a
/// canonical UUID (the request identity the caller waits for).
pub fn resume_invocation(
    computer: &ComputerId,
    session: &str,
    name: &str,
    launch_id: &str,
    force: bool,
) -> Result<Invocation, String> {
    if !valid_session_id(session) {
        return Err("A Claude app session ID looks like local_UUID".into());
    }
    if let Some(problem) = crate::actions::tag_problem(name) {
        return Err(problem.into());
    }
    if !uuid::Uuid::parse_str(launch_id).is_ok_and(|id| id.to_string() == launch_id) {
        return Err("The launch ID is not a canonical UUID.".into());
    }
    let mut words = vec!["claude-app", "resume", session, "-n", name, "--launch-id"];
    words.push(launch_id);
    words.push("-d");
    if force {
        words.push("--force");
    }
    Ok(call(hgs_args(computer, &words), RESUME_SECONDS))
}

/// The same rule as hgs's `claude_app::valid_id`.
pub fn valid_session_id(id: &str) -> bool {
    id.strip_prefix("local_").is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() <= 64
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// Whether continuing this session asks first: it may still run in the app.
pub fn asks_first(session: &Session) -> bool {
    session.running
}

/// Qt's warning text choice: a live process is surer than a recent change.
pub fn running_process(session: &Session) -> bool {
    session.running_process
}

/// A `claude-app resume` outcome: exit 0 starts the session, anything else is
/// `failureDetail`'s detail.
pub fn resumed(exit: Exit) -> Result<(), String> {
    match exit {
        Exit::Code { code: 0, .. } => Ok(()),
        exit => Err(crate::actions::failure_detail(exit)),
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

    fn listed_sessions() -> Vec<Value> {
        vec![
            json!({"id": "local_aaa", "conversation": "048ba331-3768-4b1c-9e2f-0123456789ab",
                   "cwd": "D:\\Work\\GIT\\repo", "title": "Fix the DX11 build",
                   "suggested_name": "fix-the-dx11-build", "archived": false,
                   "running": true, "running_process": true, "history": true,
                   "folder_exists": true, "last_activity": 1000.0}),
            json!({"id": "local_bbb", "conversation": "11111111-2222-3333-4444-555555555555",
                   "cwd": "d:/work/git/repo/", "title": "", "suggested_name": "app-11111111",
                   "archived": true, "running": false, "running_process": false,
                   "history": false, "folder_exists": false, "last_activity": 2000.0}),
        ]
    }

    #[test]
    fn the_read_is_the_cli_list_on_exactly_that_computer() {
        assert_eq!(
            list_invocation(&ComputerId::local()).args,
            ["claude-app", "ls"]
        );
        let peer = list_invocation(&ComputerId::from("studio"));
        assert_eq!(peer.args, ["@studio", "claude-app", "ls"]);
        assert_eq!(peer.timeout, Duration::from_secs(LIST_SECONDS));
    }

    #[test]
    fn a_list_keeps_qt_fields_and_flags() {
        let list = listed(reply(json!({"sessions": listed_sessions(),
            "truncated": true, "available": true})))
        .unwrap();
        assert_eq!(list.sessions.len(), 2);
        assert_eq!(list.sessions[0].title, "Fix the DX11 build");
        assert_eq!(list.sessions[0].suggested_name, "fix-the-dx11-build");
        assert_eq!(list.sessions[1].suggested_name, "app-11111111");
        assert!(list.truncated && list.available);
        assert_eq!(state(&list.sessions[0]), "open_in_app");
    }

    #[test]
    fn a_malformed_or_failed_list_is_the_dialog_error() {
        assert_eq!(listed(reply(json!([]))), Err(INVALID.into()));
        assert_eq!(listed(reply(Value::Null)), Err(INVALID.into()));
        let nothing = listed(reply(json!({}))).unwrap();
        assert!(nothing.sessions.is_empty() && !nothing.available && !nothing.truncated);
        // A non-object entry is not a session; the others still read.
        let mixed = listed(reply(json!({"sessions": [7, listed_sessions()[0].clone()],
            "available": false})))
        .unwrap();
        assert_eq!(mixed.sessions.len(), 1);
        assert_eq!(
            listed(Exit::Code {
                code: 1,
                stdout: json!({"sessions": []}).to_string().into_bytes(),
                stderr: "The Claude app keeps no sessions on this computer.\n".into()
            }),
            Err("The Claude app keeps no sessions on this computer.".into())
        );
        assert_eq!(listed(Exit::TimedOut), Err("timed out".into()));
    }

    #[test]
    fn the_state_column_follows_qt_order() {
        let mut session = Session {
            history: true,
            folder_exists: true,
            running: true,
            archived: true,
            ..Session::default()
        };
        assert_eq!(state(&session), "open_in_app");
        session.running = false;
        assert_eq!(state(&session), "archived");
        session.archived = false;
        assert_eq!(state(&session), "stopped");
        session.folder_exists = false;
        assert_eq!(state(&session), "folder_missing");
        session.history = false;
        assert_eq!(state(&session), "no_history");
    }

    #[test]
    fn folder_matching_is_qts() {
        assert_eq!(normalize("D:\\Work\\GIT\\repo\\"), "D:/Work/GIT/repo");
        assert_eq!(normalize("D:\\"), "D:/");
        assert_eq!(normalize("/home/user/repo///"), "/home/user/repo");
        assert!(same_folder("D:\\Work\\GIT\\repo", "d:/work/git/repo"));
        assert!(same_folder("d:/work/git/repo/", "D:\\Work\\GIT\\repo"));
        assert!(!same_folder("/home/user/repo", "/home/user/Repo"));
        assert!(!same_folder("/home/user/repo", "/home/user/repo2"));
    }

    #[test]
    fn ages_stay_in_the_frontend_and_this_read_keeps_its_clock_value() {
        let list = listed(reply(
            json!({"sessions": listed_sessions(), "available": true}),
        ))
        .unwrap();
        assert_eq!(list.sessions[0].last_activity, 1000.0);
        assert_eq!(list.sessions[1].last_activity, 2000.0);
    }

    #[test]
    fn the_view_marks_the_dialogs_folder() {
        let list = listed(reply(
            json!({"sessions": listed_sessions(), "available": true}),
        ))
        .unwrap();
        let folder = view(&list, "D:\\Work\\GIT\\repo");
        assert_eq!(folder.sessions.len(), 2);
        assert!(folder.sessions[0].in_folder);
        assert!(
            folder.sessions[1].in_folder,
            "a trailing separator and case do not matter"
        );
        assert_eq!(folder.sessions[1].state, "no_history");
        assert!(folder.folder_enabled);
        let all = view(&list, "");
        assert!(all.sessions.iter().all(|row| !row.in_folder));
        assert!(!all.folder_enabled);
    }

    #[test]
    fn resuming_is_the_checked_cli_call() {
        let launch = "0b8b1c4e-2a0d-4d7e-9f5e-6a1b2c3d4e5f";
        let inv =
            resume_invocation(&ComputerId::local(), "local_aaa", "video", launch, false).unwrap();
        assert_eq!(
            inv.args,
            [
                "claude-app",
                "resume",
                "local_aaa",
                "-n",
                "video",
                "--launch-id",
                launch,
                "-d"
            ]
        );
        assert_eq!(inv.timeout, Duration::from_secs(RESUME_SECONDS));
        assert_eq!(
            resume_invocation(
                &ComputerId::from("studio"),
                "local_aaa",
                "video",
                launch,
                true
            )
            .unwrap()
            .args,
            [
                "@studio",
                "claude-app",
                "resume",
                "local_aaa",
                "-n",
                "video",
                "--launch-id",
                launch,
                "-d",
                "--force"
            ]
        );
        // The positional ID and the name are checked, the launch ID canonical.
        for bad in [
            "",
            "aaaa",
            "local_",
            "local_../x",
            "local_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(
                resume_invocation(&ComputerId::local(), bad, "video", launch, false).is_err(),
                "{bad}"
            );
        }
        for name in ["", " video", "a.b"] {
            assert!(
                resume_invocation(&ComputerId::local(), "local_aaa", name, launch, false).is_err(),
                "{name}"
            );
        }
        assert!(resume_invocation(
            &ComputerId::local(),
            "local_aaa",
            "video",
            "not-a-uuid",
            false
        )
        .is_err());
        assert!(resume_invocation(
            &ComputerId::local(),
            "local_aaa",
            "video",
            "0B8B1C4E-2A0D-4D7E-9F5E-6A1B2C3D4E5F",
            false
        )
        .is_err());
        assert!(valid_session_id("local_a-b"));
    }

    #[test]
    fn a_resume_succeeds_only_on_exit_zero() {
        assert_eq!(
            resumed(Exit::Code {
                code: 0,
                stdout: Vec::new(),
                stderr: String::new()
            }),
            Ok(())
        );
        assert_eq!(
            resumed(Exit::Code {
                code: 1,
                stdout: Vec::new(),
                stderr: "A Claude process still continues this conversation.\n".into()
            }),
            Err("A Claude process still continues this conversation.".into())
        );
        assert_eq!(resumed(Exit::TimedOut), Err("timed out".into()));
        let running = Session {
            running: true,
            running_process: true,
            ..Session::default()
        };
        assert!(asks_first(&running));
        assert!(running_process(&running));
        assert!(!asks_first(&Session::default()));
    }
}
