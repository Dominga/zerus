//! Keep cache warm (`hgs [@host] keep-warm NAME --json`): the change call, its
//! strict classification and the help probe that admits it. The host decides
//! eligibility and timing; this only asks and reports what it confirmed.
use crate::actions::{help_invocation, prepend_alias, Exit, Invocation, Target};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// The CLI's help marker for the keep-warm command.
pub const MARKER: &str = "session-keep-warm-v1";
const UPDATE: &str = "Update Zerus on this computer to keep the prompt cache warm.";
const UNCONFIRMED: &str = "The computer did not confirm the keep-warm change. Refresh the session.";
const IDENTITY: &str = "The session has no confirmed conversation yet.";
const HELP_CAP: usize = 256 * 1024;
const REPLY_CAP: usize = 64 * 1024;

/// What the person chose: off, 1 to 7 hours, or always.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Choice {
    Off,
    Hours { hours: u8 },
    Always,
}

/// Whether `stdout` lists the marker as a whole line.
pub fn has_marker(stdout: &[u8]) -> bool {
    stdout
        .split(|&b| b == b'\n')
        .any(|line| line.trim_ascii() == MARKER.as_bytes())
}

/// `hgs [@host] keep-warm NAME --json`; 15 s here and 35 s on a peer.
pub fn invocation(
    target: &Target,
    choice: Choice,
    expected_revision: i64,
) -> Result<Invocation, String> {
    let identity = |value: &str, maximum: usize| {
        !value.is_empty()
            && value.len() <= maximum
            && !value.chars().any(|c| c < ' ' || c == '\u{7f}')
    };
    if target.name.is_empty()
        || !identity(&target.run_id, 128)
        || !identity(&target.conversation_id, 256)
    {
        return Err(IDENTITY.to_owned());
    }
    if expected_revision < 0 {
        return Err(UNCONFIRMED.to_owned());
    }
    let mut input = json!({
        "expected_run_id": target.run_id,
        "expected_conversation_id": target.conversation_id,
        "expected_revision": expected_revision,
        "enabled": choice != Choice::Off,
    });
    match choice {
        Choice::Off => {}
        Choice::Hours { hours } if (1..=7).contains(&hours) => input["hours"] = json!(hours),
        Choice::Hours { .. } => return Err("Choose 1 to 7 hours or Always.".to_owned()),
        Choice::Always => input["always"] = json!(true),
    }
    let mut args = vec![
        "keep-warm".to_owned(),
        target.name.clone(),
        "--json".to_owned(),
    ];
    let peer = prepend_alias(&mut args, &target.computer);
    Ok(Invocation {
        args,
        stdin: Some(serde_json::to_vec(&input).map_err(|error| error.to_string())?),
        timeout: Duration::from_secs(if peer { 35 } else { 15 }),
        request_id: None,
    })
}

/// The words for a refusal code the host gave without its own reason.
fn refusal(error: &str) -> &'static str {
    match error {
        "conflict" => {
            "Keep cache warm changed on another device. Refresh the session and try again."
        }
        "capacity" => "At most 64 sessions on one computer can keep their cache warm.",
        "stale_identity" | "not_found" => "The session changed. Refresh it and try again.",
        "unsupported" => "This agent cannot keep its cache warm.",
        _ => UNCONFIRMED,
    }
}

/// The confirmed `keep_warm` object, or the reason the host gave.
pub fn classify(
    target: &Target,
    choice: Choice,
    expected_revision: i64,
    exit: Exit,
) -> Result<Value, String> {
    let (code, stdout, stderr) = match &exit {
        Exit::Code {
            code,
            stdout,
            stderr,
        } => (*code, stdout.as_slice(), stderr.as_str()),
        Exit::Abnormal { stderr } => (-1, &[][..], stderr.as_str()),
        _ => return Err(UNCONFIRMED.to_owned()),
    };
    let reply = serde_json::from_slice::<Value>(stdout)
        .ok()
        .filter(Value::is_object);
    if code != 0 {
        if let Some(reply) = reply.filter(|reply| reply["ok"] == false) {
            let reason = reply["reason"]
                .as_str()
                .map(str::trim)
                .filter(|reason| !reason.is_empty());
            return Err(reason.map_or_else(
                || refusal(reply["error"].as_str().unwrap_or("")).to_owned(),
                |reason| reason.chars().take(500).collect(),
            ));
        }
        let text = stderr.trim();
        return Err(if text.is_empty() {
            UNCONFIRMED.to_owned()
        } else {
            text.chars().take(500).collect()
        });
    }
    let Some(mut reply) = reply else {
        return Err(UNCONFIRMED.to_owned());
    };
    let warm = &reply["keep_warm"];
    let revision = warm["revision"].as_i64();
    let applied = reply["applied"].as_bool();
    let enabled = choice != Choice::Off;
    let until_matches = match choice {
        Choice::Always => warm["until"].is_null(),
        Choice::Hours { .. } => warm["until"]
            .as_f64()
            .is_some_and(|until| until.is_finite() && until > 0.0),
        Choice::Off => true,
    };
    let confirmed = reply["ok"] == true
        && reply["run_id"].as_str() == Some(target.run_id.as_str())
        && reply["conversation_id"].as_str() == Some(target.conversation_id.as_str())
        && warm["enabled"].as_bool() == Some(enabled)
        && until_matches
        && match (applied, revision) {
            (Some(true), Some(revision)) => revision == expected_revision + 1,
            (Some(false), Some(revision)) => revision == expected_revision,
            _ => false,
        };
    if !confirmed {
        return Err(UNCONFIRMED.to_owned());
    }
    Ok(reply["keep_warm"].take())
}

/// The whole change: the local help probe, the call and its classification. Blocks.
pub fn change(
    hgs: &std::path::Path,
    target: &Target,
    choice: Choice,
    expected_revision: i64,
) -> Result<Value, String> {
    let call = invocation(target, choice, expected_revision)?;
    let probe = crate::run::run_limited(hgs, &help_invocation(), &[], HELP_CAP);
    if !probe
        .as_ref()
        .is_ok_and(|exit| matches!(exit, Exit::Code { code: 0, stdout, .. } if has_marker(stdout)))
    {
        return Err(UPDATE.to_owned());
    }
    let exit = crate::run::run_limited(hgs, &call, &[], REPLY_CAP).unwrap_or_else(|too_large| {
        Exit::Abnormal {
            stderr: too_large.stderr,
        }
    });
    classify(target, choice, expected_revision, exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(computer: &str) -> Target {
        Target {
            computer: computer.into(),
            name: "codex/project/warm".into(),
            run_id: "run-1".into(),
            conversation_id: "conversation-1".into(),
        }
    }

    fn reply(value: Value, code: i32) -> Exit {
        Exit::Code {
            code,
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: String::new(),
        }
    }

    fn confirmed(enabled: bool, until: Value, revision: i64) -> Value {
        json!({"ok":true,"applied":true,"run_id":"run-1","conversation_id":"conversation-1",
            "keep_warm":{"enabled":enabled,"until":until,"revision":revision,"state":"waiting","supported":true}})
    }

    #[test]
    fn the_call_names_the_session_identity_and_one_duration() {
        let call = invocation(&target(""), Choice::Hours { hours: 3 }, 4).unwrap();
        assert_eq!(call.args, ["keep-warm", "codex/project/warm", "--json"]);
        assert_eq!(call.timeout, Duration::from_secs(15));
        let body: Value = serde_json::from_slice(call.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"expected_run_id":"run-1","expected_conversation_id":"conversation-1",
            "expected_revision":4,"enabled":true,"hours":3})
        );
        let peer = invocation(&target("mac"), Choice::Always, 0).unwrap();
        assert_eq!(peer.args[0], "@mac");
        assert_eq!(peer.timeout, Duration::from_secs(35));
        let body: Value = serde_json::from_slice(peer.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(
            (body["always"].clone(), body.get("hours")),
            (json!(true), None)
        );
        let off: Value = serde_json::from_slice(
            invocation(&target(""), Choice::Off, 2)
                .unwrap()
                .stdin
                .as_ref()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            (off["enabled"].clone(), off.get("hours"), off.get("always")),
            (json!(false), None, None)
        );
        for hours in [0, 8] {
            assert!(invocation(&target(""), Choice::Hours { hours }, 0).is_err());
        }
        assert!(invocation(&target(""), Choice::Off, -1).is_err());
        let mut blank = target("");
        blank.conversation_id.clear();
        assert!(invocation(&blank, Choice::Off, 0).is_err());
    }

    #[test]
    fn only_the_exact_confirmed_change_is_accepted() {
        let t = target("");
        let hours = Choice::Hours { hours: 2 };
        let ok = classify(&t, hours, 3, reply(confirmed(true, json!(1.0e9), 4), 0)).unwrap();
        assert_eq!(
            (ok["enabled"].clone(), ok["revision"].clone()),
            (json!(true), json!(4))
        );
        let always = classify(
            &t,
            Choice::Always,
            3,
            reply(confirmed(true, Value::Null, 4), 0),
        )
        .unwrap();
        assert_eq!(always["until"], Value::Null);
        assert!(classify(
            &t,
            Choice::Off,
            3,
            reply(confirmed(false, Value::Null, 4), 0)
        )
        .is_ok());
        // A repeated off is confirmed without a new revision.
        let mut unchanged = confirmed(false, Value::Null, 3);
        unchanged["applied"] = json!(false);
        assert!(classify(&t, Choice::Off, 3, reply(unchanged, 0)).is_ok());
        // Another session, another revision or another state is not this change.
        for bad in [
            {
                let mut v = confirmed(true, json!(1.0e9), 4);
                v["run_id"] = json!("run-2");
                v
            },
            confirmed(true, json!(1.0e9), 5),
            confirmed(false, Value::Null, 4),
            confirmed(true, Value::Null, 4),
        ] {
            assert_eq!(
                classify(&t, hours, 3, reply(bad, 0)).unwrap_err(),
                UNCONFIRMED
            );
        }
    }

    #[test]
    fn refusals_keep_the_hosts_reason() {
        let t = target("");
        let refused = json!({"ok":false,"error":"unsupported","reason":"OpenAI keeps a 30-minute cache only for GPT-5.6 and later."});
        assert_eq!(
            classify(&t, Choice::Always, 0, reply(refused, 1)).unwrap_err(),
            "OpenAI keeps a 30-minute cache only for GPT-5.6 and later."
        );
        for (error, text) in [
            ("conflict", "changed on another device"),
            ("capacity", "64 sessions"),
            ("stale_identity", "Refresh"),
        ] {
            let message = classify(
                &t,
                Choice::Always,
                0,
                reply(json!({"ok":false,"error":error}), 1),
            )
            .unwrap_err();
            assert!(message.contains(text), "{error}: {message}");
        }
        let stderr = Exit::Code {
            code: 1,
            stdout: Vec::new(),
            stderr: "hgs: update the selected computer to keep the prompt cache warm\n".into(),
        };
        assert_eq!(
            classify(&t, Choice::Always, 0, stderr).unwrap_err(),
            "hgs: update the selected computer to keep the prompt cache warm"
        );
        assert_eq!(
            classify(&t, Choice::Always, 0, Exit::TimedOut).unwrap_err(),
            UNCONFIRMED
        );
    }

    #[test]
    fn the_marker_is_a_whole_help_line() {
        assert!(has_marker(b"usage\n       session-keep-warm-v1\r\nmore"));
        assert!(!has_marker(b"usage session-keep-warm-v1 inline"));
        assert!(!has_marker(b"session-keep-warm-v2"));
    }
}
