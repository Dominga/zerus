//! The session Terminal's rules, ported from the Qt client: when a session
//! has a terminal and why not, when a new target reconnects, how dropped file
//! paths are pasted, and the status texts. The attach process itself (`hgs
//! a NAME --existing` in a PTY) follows in this module.
//!
//! Reference: `tray/src/NativeSessionCapabilities.h` (`terminal`,
//! `terminalReason`), `tray/src/TerminalView.cpp` (`setSession`, statuses),
//! `tray/src/SessionsWindow.cpp` (availability, pasting dropped paths).
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a session reports about its terminal: its row's fields, which the
/// selected session's inspected details override.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalFacts {
    /// The agent command (`cmd`, else `agent`), such as `claude` or `dsh`.
    pub cmd: String,
    /// The row's `terminal_supported`, true when it reports none.
    pub terminal_supported: bool,
    /// The row has a boolean `terminal_supported`.
    pub terminal_reported: bool,
    pub terminal_attach_supported: bool,
    /// The row has a boolean `terminal_attach_supported` (OpenCode).
    pub terminal_attach_reported: bool,
    /// The row's `terminal_reason`.
    pub terminal_reason: String,
    /// The row's `terminal_attach_reason` (OpenCode).
    pub terminal_attach_reason: String,
}

fn flag(details: &Value, key: &str) -> Option<bool> {
    details.get(key).and_then(Value::as_bool)
}

/// Whether the session has a terminal (`NativeSessionCapabilities::terminal`,
/// as the frontend's `terminal()` in `actions-ui.ts` reads it: DeepSeek under
/// either command name has Native UI instead).
pub fn supported(facts: &TerminalFacts, details: &Value) -> bool {
    if facts.cmd == "dsh" || facts.cmd == "deepseek" {
        return false;
    }
    if facts.cmd == "opencode" {
        if let Some(attach) = flag(details, "terminal_attach_supported") {
            return attach;
        }
        if facts.terminal_attach_reported {
            return facts.terminal_attach_supported;
        }
    }
    if let Some(supported) = flag(details, "terminal_supported") {
        return supported;
    }
    if facts.cmd == "opencode" && !facts.terminal_reported {
        return false;
    }
    facts.terminal_supported
}

/// Why the session has no terminal (`NativeSessionCapabilities::terminalReason`).
pub fn unsupported_reason(facts: &TerminalFacts, details: &Value) -> String {
    let text = |key: &str, fallback: &str| -> String {
        details
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    if facts.cmd == "opencode"
        && (flag(details, "terminal_attach_supported").is_some() || facts.terminal_attach_reported)
    {
        let reason = text("terminal_attach_reason", &facts.terminal_attach_reason);
        return if reason.is_empty() {
            "Refresh this session after its OpenCode service reconnects.".into()
        } else {
            reason
        };
    }
    let reason = text("terminal_reason", &facts.terminal_reason);
    if reason.is_empty() {
        "This native adapter does not provide Terminal.".into()
    } else {
        reason
    }
}

/// Whether the Terminal can connect now, else the reason the view shows
/// (`SessionsWindow.cpp`, `m_terminal->setAvailable`).
pub fn available(
    facts: &TerminalFacts,
    details: &Value,
    state: &str,
    online: bool,
    restored_pending: bool,
) -> Result<(), String> {
    let supported = supported(facts, details);
    if online && state == "running" && supported && !restored_pending {
        return Ok(());
    }
    Err(if !online {
        "Machine unavailable. Reconnect when it is online.".into()
    } else if restored_pending {
        "Refresh the restored session before opening Terminal.".into()
    } else if !supported {
        unsupported_reason(facts, details)
    } else {
        "Resume this session before opening its terminal.".into()
    })
}

/// Where a Terminal attaches: `hgs`, the computer's alias (empty for this
/// computer), the session name and its run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub hgs: String,
    pub host: String,
    pub name: String,
    pub run_id: String,
}

/// Whether moving from `old` to `new` disconnects and clears the screen
/// (`TerminalView::setSession`): another endpoint, another name without the
/// same run, or another known run.
pub fn target_changed(old: &Target, new: &Target) -> bool {
    let same_endpoint = old.hgs == new.hgs && old.host == new.host;
    let same_run = !old.run_id.is_empty() && old.run_id == new.run_id;
    !same_endpoint
        || (old.name != new.name && !same_run)
        || (!old.run_id.is_empty() && !new.run_id.is_empty() && old.run_id != new.run_id)
}

/// The run a Terminal keeps after a new target: an inspection that omits the
/// run never erases an established one unless the target changed.
pub fn kept_run(old: &Target, new: &Target) -> String {
    if target_changed(old, new) || !new.run_id.is_empty() {
        new.run_id.clone()
    } else {
        old.run_id.clone()
    }
}

/// Dropped file paths as Terminal input: each quoted for the session's shell
/// (POSIX single quotes, or PowerShell on Windows with native separators),
/// joined by spaces with a trailing space.
pub fn paste_paths(paths: &[String], windows: bool) -> Result<String, String> {
    let mut quoted = Vec::with_capacity(paths.len());
    for path in paths {
        if path.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
            return Err(
                "File names containing control characters cannot be pasted into Terminal.".into(),
            );
        }
        quoted.push(if windows {
            format!("'{}'", path.replace('/', "\\").replace('\'', "''"))
        } else {
            format!("'{}'", path.replace('\'', "'\\''"))
        });
    }
    Ok(format!("{} ", quoted.join(" ")))
}

/// The Terminal's status texts.
pub mod status {
    pub const IDLE: &str = "Connect to use the session's live terminal";

    pub fn connecting(name: &str) -> String {
        format!("Connecting to {name}…")
    }

    pub fn connected(name: &str) -> String {
        format!("Connected: {name}")
    }

    /// tmux keeps Unix sessions, the session host Windows ones.
    pub fn disconnected(machine: &str) -> String {
        if machine.is_empty() {
            "Disconnected — the session keeps running".into()
        } else {
            format!("Disconnected — the session continues on {machine}")
        }
    }

    /// The attach client ended: a clean exit is a disconnect.
    pub fn finished(code: i32, machine: &str) -> String {
        if code == 0 {
            disconnected(machine)
        } else {
            format!("Attach client exited ({code}) — session output is preserved above")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn facts(cmd: &str) -> TerminalFacts {
        TerminalFacts {
            cmd: cmd.into(),
            terminal_supported: cmd != "dsh" && cmd != "opencode",
            ..TerminalFacts::default()
        }
    }

    #[test]
    fn support_follows_the_native_capabilities() {
        let none = json!({});
        assert!(supported(&facts("claude"), &none));
        assert!(!supported(&facts("deepseek"), &none));
        assert!(!supported(
            &facts("dsh"),
            &json!({"terminal_supported": true})
        ));
        assert!(!supported(
            &facts("claude"),
            &json!({"terminal_supported": false})
        ));
        // OpenCode: the attach report first, then a reported terminal only.
        assert!(!supported(&facts("opencode"), &none));
        assert!(supported(
            &facts("opencode"),
            &json!({"terminal_attach_supported": true})
        ));
        let mut reported = facts("opencode");
        reported.terminal_attach_reported = true;
        reported.terminal_attach_supported = true;
        assert!(supported(&reported, &none));
        let mut terminal = facts("opencode");
        terminal.terminal_reported = true;
        terminal.terminal_supported = true;
        assert!(supported(&terminal, &none));
    }

    #[test]
    fn availability_gives_qt_reasons_in_order() {
        let none = json!({});
        let claude = facts("claude");
        assert_eq!(available(&claude, &none, "running", true, false), Ok(()));
        assert_eq!(
            available(&claude, &none, "running", false, true).unwrap_err(),
            "Machine unavailable. Reconnect when it is online."
        );
        assert_eq!(
            available(&claude, &none, "running", true, true).unwrap_err(),
            "Refresh the restored session before opening Terminal."
        );
        assert_eq!(
            available(&claude, &none, "paused", true, false).unwrap_err(),
            "Resume this session before opening its terminal."
        );
        assert_eq!(
            available(&facts("dsh"), &none, "running", true, false).unwrap_err(),
            "This native adapter does not provide Terminal."
        );
        assert_eq!(
            available(
                &facts("dsh"),
                &json!({"terminal_reason": "Use Native UI."}),
                "running",
                true,
                false
            )
            .unwrap_err(),
            "Use Native UI."
        );
        let mut opencode = facts("opencode");
        opencode.terminal_attach_reported = true;
        assert_eq!(
            available(&opencode, &none, "running", true, false).unwrap_err(),
            "Refresh this session after its OpenCode service reconnects."
        );
        opencode.terminal_attach_reason = "The service is offline.".into();
        assert_eq!(
            available(&opencode, &none, "running", true, false).unwrap_err(),
            "The service is offline."
        );
    }

    #[test]
    fn targets_reconnect_like_set_session() {
        let target = |host: &str, name: &str, run: &str| Target {
            hgs: "hgs".into(),
            host: host.into(),
            name: name.into(),
            run_id: run.into(),
        };
        let old = target("", "claude/p/a", "r1");
        assert!(!target_changed(&old, &target("", "claude/p/a", "r1")));
        // A rename of the same run keeps the connection.
        assert!(!target_changed(&old, &target("", "claude/p/renamed", "r1")));
        assert!(target_changed(&old, &target("studio", "claude/p/a", "r1")));
        assert!(target_changed(&old, &target("", "claude/p/a", "r2")));
        assert!(target_changed(
            &target("", "claude/p/a", ""),
            &target("", "claude/p/b", "")
        ));
        // An inspection that omits the run keeps the established one.
        assert!(!target_changed(&old, &target("", "claude/p/a", "")));
        assert_eq!(kept_run(&old, &target("", "claude/p/a", "")), "r1");
        assert_eq!(
            kept_run(
                &target("", "claude/p/a", ""),
                &target("", "claude/p/a", "r9")
            ),
            "r9"
        );
    }

    #[test]
    fn dropped_paths_are_quoted_for_the_shell() {
        let paths = vec![
            "/home/example/it's here.txt".to_owned(),
            "/home/example/b".to_owned(),
        ];
        assert_eq!(
            paste_paths(&paths, false).unwrap(),
            "'/home/example/it'\\''s here.txt' '/home/example/b' "
        );
        assert_eq!(
            paste_paths(&["C:/Users/example/it's.txt".to_owned()], true).unwrap(),
            "'C:\\Users\\example\\it''s.txt' "
        );
        assert_eq!(
            paste_paths(&["/home/example/a\nb".to_owned()], false).unwrap_err(),
            "File names containing control characters cannot be pasted into Terminal."
        );
    }

    #[test]
    fn statuses_are_qt_texts() {
        assert_eq!(
            status::connecting("claude/p/a"),
            "Connecting to claude/p/a…"
        );
        assert_eq!(status::connected("claude/p/a"), "Connected: claude/p/a");
        assert_eq!(
            status::finished(0, ""),
            "Disconnected — the session keeps running"
        );
        assert_eq!(
            status::finished(0, "studio"),
            "Disconnected — the session continues on studio"
        );
        assert_eq!(
            status::finished(2, "studio"),
            "Attach client exited (2) — session output is preserved above"
        );
    }
}
