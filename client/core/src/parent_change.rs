//! Desktop changes of a session's parent link (zerus-sa9qn.9): `hgs [@host]
//! parent NAME --json` with `set` (a session the person chose on the same
//! computer) or `clear`, behind the `session-parent-v1` help marker. The host
//! verifies the session's exact run and conversation, the chosen parent, cycles
//! and replacement; the desktop asks for confirmation before it calls.
use crate::actions::{help_invocation, prepend_alias, Exit, Invocation, Target};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

pub const MARKER: &str = "session-parent-v1";
pub const UPDATE: &str = "Update hgs on this session's computer to change parent sessions.";
const UNCONFIRMED: &str =
    "The parent session change was not confirmed. Refresh and check the session.";
const IDENTITY: &str = "This session has no complete identity yet; wait until it has started.";
const HELP_CAP: usize = 256 * 1024;
const REPLY_CAP: usize = 64 * 1024;

/// What the person asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParentChange {
    /// Remove the session's parent link.
    Clear,
    /// Make the chosen session on the same computer its parent; `replace`
    /// confirms that another parent is replaced.
    Set {
        session_id: String,
        name: String,
        conversation_id: String,
        run_id: String,
        replace: bool,
    },
}

fn text_ok(value: &str, maximum: usize) -> bool {
    value.len() <= maximum && !value.chars().any(char::is_control)
}

fn canonical(id: &str) -> bool {
    crate::creation::canonical_uuid(id)
}

/// The exact call, or why it cannot be made.
pub fn invocation(target: &Target, change: &ParentChange) -> Result<Invocation, String> {
    if target.name.is_empty()
        || !text_ok(&target.name, 512)
        || target.run_id.is_empty()
        || !text_ok(&target.run_id, 512)
        || target.conversation_id.is_empty()
        || !text_ok(&target.conversation_id, 512)
    {
        return Err(IDENTITY.to_owned());
    }
    let input = match change {
        ParentChange::Clear => json!({"expected_run_id": target.run_id,
            "expected_conversation_id": target.conversation_id, "action": "clear"}),
        ParentChange::Set {
            session_id,
            name,
            conversation_id,
            run_id,
            replace,
        } => {
            if !canonical(session_id)
                || [name, conversation_id, run_id]
                    .iter()
                    .any(|value| !text_ok(value, 512))
            {
                return Err("Choose a parent session that has started.".to_owned());
            }
            json!({"expected_run_id": target.run_id, "expected_conversation_id": target.conversation_id,
                "action": "set", "replace": replace, "parent": {"session_id": session_id, "name": name,
                "conversation_id": conversation_id, "run_id": run_id}})
        }
    };
    let mut args = vec![
        "parent".to_owned(),
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

/// The local `hgs --help` lists the marker on a line of its own.
pub fn help_confirms(exit: &Exit) -> bool {
    matches!(exit, Exit::Code { code: 0, stdout, .. }
        if stdout.split(|&b| b == b'\n').any(|line| line.trim_ascii() == MARKER.as_bytes()))
}

/// The host's confirmed link: `{ok, name, parent, previous}` for this session.
pub fn classify(target: &Target, exit: Exit) -> Result<Value, String> {
    let stderr = match &exit {
        Exit::Code { stderr, .. } | Exit::Abnormal { stderr } => stderr.trim(),
        _ => "",
    };
    let fail = || {
        if stderr.is_empty() {
            UNCONFIRMED.to_owned()
        } else {
            stderr.chars().take(400).collect()
        }
    };
    let Exit::Code {
        code: 0, stdout, ..
    } = &exit
    else {
        return Err(fail());
    };
    match serde_json::from_slice::<Value>(stdout) {
        Ok(reply) if reply["ok"] == true && reply["name"] == target.name.as_str() => Ok(reply),
        _ => Err(fail()),
    }
}

/// Probe the selected `hgs`, then change the link and return the host's reply.
pub fn change(
    hgs: &std::path::Path,
    target: &Target,
    change: &ParentChange,
) -> Result<Value, String> {
    let call = invocation(target, change)?;
    let probe = crate::run::run_limited(hgs, &help_invocation(), &[], HELP_CAP);
    if !probe.as_ref().is_ok_and(help_confirms) {
        return Err(UPDATE.to_owned());
    }
    let exit = crate::run::run_limited(hgs, &call, &[], REPLY_CAP).unwrap_or_else(|too_large| {
        Exit::Abnormal {
            stderr: too_large.stderr,
        }
    });
    classify(target, exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(computer: &str) -> Target {
        Target {
            computer: computer.into(),
            name: "codex/p/kid".into(),
            run_id: "run".into(),
            conversation_id: "conv".into(),
        }
    }
    fn set() -> ParentChange {
        ParentChange::Set {
            session_id: "22222222-2222-4222-8222-222222222222".into(),
            name: "claude/p/lead".into(),
            conversation_id: "c".into(),
            run_id: "r".into(),
            replace: true,
        }
    }
    fn stdin(call: &Invocation) -> Value {
        serde_json::from_slice(call.stdin.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn clear_and_set_name_the_exact_session_and_parent() {
        let clear = invocation(&target(""), &ParentChange::Clear).unwrap();
        assert_eq!(clear.args, ["parent", "codex/p/kid", "--json"]);
        assert_eq!(
            stdin(&clear),
            json!({"expected_run_id":"run","expected_conversation_id":"conv","action":"clear"})
        );
        let peer = invocation(&target("mac"), &set()).unwrap();
        assert_eq!(peer.args, ["@mac", "parent", "codex/p/kid", "--json"]);
        assert_eq!(
            stdin(&peer)["parent"],
            json!({"session_id":"22222222-2222-4222-8222-222222222222",
            "name":"claude/p/lead","conversation_id":"c","run_id":"r"})
        );
        assert_eq!(stdin(&peer)["replace"], true);
    }

    #[test]
    fn an_incomplete_session_or_parent_is_never_sent() {
        let mut incomplete = target("");
        incomplete.run_id.clear();
        assert!(invocation(&incomplete, &ParentChange::Clear).is_err());
        let ParentChange::Set {
            replace,
            conversation_id,
            run_id,
            name,
            ..
        } = set()
        else {
            unreachable!()
        };
        let bad = ParentChange::Set {
            session_id: "not-an-id".into(),
            name,
            conversation_id,
            run_id,
            replace,
        };
        assert!(invocation(&target(""), &bad).is_err());
    }

    #[test]
    fn only_the_hosts_confirmation_for_this_session_counts() {
        let reply = |stdout: &str| Exit::Code {
            code: 0,
            stdout: stdout.as_bytes().to_vec(),
            stderr: String::new(),
        };
        let ok = classify(
            &target(""),
            reply(r#"{"ok":true,"name":"codex/p/kid","parent":null,"previous":null}"#),
        );
        assert!(ok.is_ok());
        assert!(classify(&target(""), reply(r#"{"ok":true,"name":"codex/p/other"}"#)).is_err());
        assert!(classify(&target(""), reply("not json")).is_err());
        let refused = Exit::Code {
            code: 1,
            stdout: Vec::new(),
            stderr: "hgs: The chosen parent changed; refresh and retry\n".into(),
        };
        assert_eq!(
            classify(&target(""), refused).unwrap_err(),
            "hgs: The chosen parent changed; refresh and retry"
        );
        assert!(help_confirms(&reply("usage\n       session-parent-v1\n")));
        assert!(!help_confirms(&reply("usage: session-parent-v10\n")));
    }
}
