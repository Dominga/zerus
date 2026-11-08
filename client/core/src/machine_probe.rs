//! "Test session host" (MachinesPage.cpp:215-219, 413-423): one bounded
//! `hgs session-host status --json` on this computer, which answers with the
//! host that runs now, its staged CLI and, while it runs, its sessions. The
//! probe never starts, stops or installs anything.

use crate::actions::{failure_detail, Exit, Invocation};
use crate::machine::LocalHost;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// The most of one status reply kept.
pub const REPLY_LIMIT: usize = 64 * 1024;

/// What the host answered: its facts and, while running, its sessions.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HostTest {
    pub host: LocalHost,
    pub sessions: Option<u64>,
    pub clients: Option<u64>,
}

/// The probe hgs receives.
pub fn invocation() -> Invocation {
    Invocation {
        args: vec!["session-host".into(), "status".into(), "--json".into()],
        stdin: None,
        timeout: Duration::from_secs(30),
        request_id: None,
    }
}

/// Reads one status reply: its `host` object and, while running, its counts.
pub fn parse_status(reply: &Value) -> Result<HostTest, String> {
    let host = reply
        .get("host")
        .cloned()
        .ok_or_else(|| "The session host status named no host.".to_owned())?;
    let host: LocalHost =
        serde_json::from_value(host).map_err(|error| format!("Invalid session host status: {error}"))?;
    let count = |key: &str| reply["host"].get(key).and_then(Value::as_u64);
    Ok(HostTest {
        sessions: count("sessions"),
        clients: count("clients"),
        host,
    })
}

/// Runs the probe and reads its `host` object; anything else is the reason it
/// could not answer.
pub fn host_status(hgs: &Path) -> Result<HostTest, String> {
    let exit = crate::run::run_limited(hgs, &invocation(), &[], REPLY_LIMIT)
        .map_err(|_| "The session host status reply is too large.".to_owned())?;
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => {
            let reply: Value = serde_json::from_slice(&stdout)
                .map_err(|_| "Invalid session host status response.".to_owned())?;
            parse_status(&reply)
        }
        exit => Err(failure_detail(exit)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_is_one_bounded_status_call() {
        let call = invocation();
        assert_eq!(call.args, ["session-host", "status", "--json"]);
        assert!(call.stdin.is_none() && call.request_id.is_none());
    }

    #[test]
    fn a_status_reply_keeps_the_host_and_its_counts() {
        let test = parse_status(&serde_json::json!({
            "enabled": true,
            "host": {"state": "running", "pid": 4242, "protocol": 1, "conpty": "bundled",
                     "integrity": "medium", "windows_session": 1, "sessions": 2, "clients": 1},
        }))
        .unwrap();
        assert_eq!((test.host.state.as_deref(), test.host.pid, test.host.windows_session), (Some("running"), Some(4242), Some(1)));
        assert_eq!((test.sessions, test.clients), (Some(2), Some(1)));
        // A host that is not running carries its reason instead.
        let stopped = parse_status(&serde_json::json!({"host": {"state": "stopped"}})).unwrap();
        assert_eq!((stopped.host.state.as_deref(), stopped.sessions), (Some("stopped"), None));
        assert_eq!(parse_status(&serde_json::json!({"enabled": true})).unwrap_err(), "The session host status named no host.");
    }
}
