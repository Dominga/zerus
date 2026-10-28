//! Settings > Worktrees (tray/src/WorktreeSettings.h): each owning
//! computer's cleanup rules, read and written with `hgs [@route] worktrees
//! policy get|set --json` after a `--help` probe for its marker. Opening the
//! page never removes a checkout or starts a maintenance pass. A policy that
//! is incomplete or malformed fails closed: nothing can be applied until a
//! valid one has been read, and a change is never retried.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::actions::{Exit, Invocation};

/// The help line that shows the command exists.
pub const MARKER: &str = "worktree-maintenance-v1";
/// How long a probe or a call may run.
const TIMEOUT: Duration = Duration::from_secs(15);
/// The most either may print.
const MAX_BYTES: usize = 128 * 1024;

/// A rule set as the page edits it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    pub enabled: bool,
    pub after_session: bool,
    pub unused_integrated: bool,
    pub missing: bool,
    /// 1 to 8760.
    pub grace_hours: i64,
}

/// A computer's policy as it was read: its rules, and the whole object a
/// change is made on (its version and revision included).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub rules: Rules,
    pub raw: Map<String, Value>,
}

fn args(route: &str, rest: &[&str]) -> Vec<String> {
    let mut args = Vec::new();
    if !route.is_empty() {
        args.push(format!("@{route}"));
    }
    args.extend(rest.iter().map(|arg| (*arg).to_owned()));
    args
}

/// `hgs [@route] --help`.
pub fn probe(route: &str) -> Invocation {
    Invocation {
        args: args(route, &["--help"]),
        stdin: None,
        timeout: TIMEOUT,
        request_id: None,
    }
}

/// `hgs [@route] worktrees policy get|set --json`, a change's policy on stdin.
pub fn invocation(route: &str, change: Option<&Map<String, Value>>) -> Invocation {
    let action = if change.is_some() { "set" } else { "get" };
    Invocation {
        args: args(route, &["worktrees", "policy", action, "--json"]),
        stdin: change.map(|policy| Value::Object(policy.clone()).to_string().into_bytes()),
        timeout: TIMEOUT,
        request_id: None,
    }
}

/// A clean exit within the output limit, as Qt's `success` checks it.
fn finished(exit: Option<Exit>) -> Option<Vec<u8>> {
    match exit {
        Some(Exit::Code {
            code: 0, stdout, ..
        }) => Some(stdout),
        _ => None,
    }
}

pub fn probed(exit: Option<Exit>) -> Result<(), String> {
    finished(exit)
        .filter(|help| String::from_utf8_lossy(help).contains(MARKER))
        .map(drop)
        .ok_or_else(|| {
            "This computer is unavailable or needs an updated hgs CLI for cleanup settings.".into()
        })
}

/// QJsonValue::toInt: a whole number that fits an int, otherwise 0.
fn to_int(value: Option<&Value>) -> i64 {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.fract() == 0.0 && *n >= f64::from(i32::MIN) && *n <= f64::from(i32::MAX))
        .map_or(0, |n| n as i64)
}

/// The reply to a read (`changed` false) or a change, checked as Qt checks
/// it: schema 1, version 1, a numeric revision, a grace of 1 to 8760 hours
/// and the four switches.
pub fn answer(changed: bool, exit: Option<Exit>) -> Result<Policy, String> {
    let Some(stdout) = finished(exit) else {
        return Err(if changed {
            "Save was not confirmed. Refresh before changing settings again."
        } else {
            "Could not read cleanup settings."
        }
        .into());
    };
    let invalid = || "Computer returned invalid cleanup settings.".to_owned();
    let reply = match serde_json::from_slice::<Value>(&stdout) {
        Ok(Value::Object(reply)) => reply,
        _ => return Err(invalid()),
    };
    let raw = reply
        .get("policy")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let switch = |key: &str| raw.get(key).and_then(Value::as_bool);
    let grace = to_int(raw.get("grace_hours"));
    let valid = to_int(reply.get("schema")) == 1
        && to_int(raw.get("version")) == 1
        && raw.get("revision").is_some_and(Value::is_number)
        && (1..=8760).contains(&grace);
    match (
        valid,
        switch("enabled"),
        switch("after_session"),
        switch("unused_integrated"),
        switch("missing"),
    ) {
        (true, Some(enabled), Some(after_session), Some(unused_integrated), Some(missing)) => {
            Ok(Policy {
                rules: Rules {
                    enabled,
                    after_session,
                    unused_integrated,
                    missing,
                    grace_hours: grace,
                },
                raw,
            })
        }
        _ => Err(invalid()),
    }
}

/// The policy a change sends: the one read, its switches and grace replaced.
pub fn change(read: &Map<String, Value>, rules: &Rules) -> Result<Map<String, Value>, String> {
    if !(1..=8760).contains(&rules.grace_hours) {
        return Err("Computer returned invalid cleanup settings.".into());
    }
    let mut policy = read.clone();
    policy.insert("enabled".into(), Value::Bool(rules.enabled));
    policy.insert("after_session".into(), Value::Bool(rules.after_session));
    policy.insert(
        "unused_integrated".into(),
        Value::Bool(rules.unused_integrated),
    );
    policy.insert("missing".into(), Value::Bool(rules.missing));
    policy.insert("grace_hours".into(), Value::from(rules.grace_hours));
    Ok(policy)
}

/// Probes the route's `hgs`, then reads (`change` None) or changes its
/// policy. Blocks; never on the main thread.
pub fn call(
    hgs: &Path,
    route: &str,
    change: Option<&Map<String, Value>>,
) -> Result<Policy, String> {
    probed(crate::run::run_limited(hgs, &probe(route), &[], MAX_BYTES).ok())?;
    answer(
        change.is_some(),
        crate::run::run_limited(hgs, &invocation(route, change), &[], MAX_BYTES).ok(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn code(stdout: &str) -> Option<Exit> {
        Some(Exit::Code {
            code: 0,
            stdout: stdout.as_bytes().to_vec(),
            stderr: String::new(),
        })
    }

    fn reply() -> Value {
        json!({"schema": 1, "policy": {"version": 1, "revision": 4, "enabled": true, "after_session": true,
            "unused_integrated": false, "missing": true, "grace_hours": 24}, "last_result": null})
    }

    #[test]
    fn the_route_s_cli_is_probed_then_asked() {
        assert_eq!(probe("mac").args, ["@mac", "--help"]);
        assert_eq!(probe("").args, ["--help"]);
        assert!(probed(code(
            "hgs [@host] worktrees policy get|set --json   worktree-maintenance-v1"
        ))
        .is_ok());
        let unavailable =
            "This computer is unavailable or needs an updated hgs CLI for cleanup settings.";
        assert_eq!(probed(code("hgs [@host] ls")).unwrap_err(), unavailable);
        assert_eq!(probed(Some(Exit::TimedOut)).unwrap_err(), unavailable);
        assert_eq!(probed(None).unwrap_err(), unavailable);
        let read = invocation("mac", None);
        assert_eq!(read.args, ["@mac", "worktrees", "policy", "get", "--json"]);
        assert_eq!((read.stdin, read.timeout), (None, Duration::from_secs(15)));
        let policy = answer(false, code(&reply().to_string())).unwrap();
        let sent = change(
            &policy.raw,
            &Rules {
                enabled: false,
                grace_hours: 72,
                ..policy.rules.clone()
            },
        )
        .unwrap();
        let write = invocation("", Some(&sent));
        assert_eq!(write.args, ["worktrees", "policy", "set", "--json"]);
        let payload: Value = serde_json::from_slice(&write.stdin.unwrap()).unwrap();
        // The revision read goes back, so a change made meanwhile is refused.
        assert_eq!(
            payload,
            json!({"version": 1, "revision": 4, "enabled": false, "after_session": true,
            "unused_integrated": false, "missing": true, "grace_hours": 72})
        );
    }

    #[test]
    fn a_malformed_policy_fails_closed() {
        let policy = answer(false, code(&reply().to_string())).unwrap();
        assert_eq!(
            policy.rules,
            Rules {
                enabled: true,
                after_session: true,
                unused_integrated: false,
                missing: true,
                grace_hours: 24
            }
        );
        let invalid = "Computer returned invalid cleanup settings.";
        for (path, value) in [
            ("schema", json!(2)),
            ("policy/version", json!(2)),
            ("policy/revision", json!("4")),
            ("policy/grace_hours", json!(0)),
            ("policy/grace_hours", json!(8761)),
            ("policy/grace_hours", json!(1.5)),
            ("policy/enabled", json!(1)),
            ("policy/missing", Value::Null),
        ] {
            let mut bad = reply();
            let pointer = format!("/{path}");
            *bad.pointer_mut(&pointer).unwrap() = value;
            assert_eq!(
                answer(false, code(&bad.to_string())).unwrap_err(),
                invalid,
                "{path}"
            );
        }
        let mut missing = reply();
        missing["policy"]
            .as_object_mut()
            .unwrap()
            .remove("after_session");
        assert_eq!(
            answer(false, code(&missing.to_string())).unwrap_err(),
            invalid
        );
        assert_eq!(answer(false, code("not json")).unwrap_err(), invalid);
        assert_eq!(
            answer(false, None).unwrap_err(),
            "Could not read cleanup settings."
        );
        assert_eq!(
            answer(true, Some(Exit::TimedOut)).unwrap_err(),
            "Save was not confirmed. Refresh before changing settings again."
        );
        assert!(change(
            &policy.raw,
            &Rules {
                grace_hours: 0,
                ..policy.rules
            }
        )
        .is_err());
    }
}
