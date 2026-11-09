//! The owner moves one running session to another account of the same agent
//! (`hgs switch-account`): the agent is stopped, the conversation is copied to
//! the target's store and resumed, and a failed launch rolls back. Task 352: an
//! account of another agent moves the session to that agent with a handoff;
//! `preview` tells the confirmation what the choice would do. The webview
//! never runs hgs; this module builds the guarded call and bounds its receipt.

use crate::actions::{Exit, Invocation};
use crate::run::run_limited;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// A switch copies a conversation and restarts the agent; a move to another
/// agent reads its history and starts the new session. Qt waits as long.
const TIMEOUT: Duration = Duration::from_secs(300);
/// A preview only reads the session.
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(90);
/// The receipt is small; a larger reply is not a switch receipt.
const RECEIPT_BYTES: usize = 4 * 1024 * 1024;

/// Switches `session` to `account` on this computer. `request` is the caller's
/// canonical UUID: Zerus keeps the receipt by it and never runs the same
/// request twice. A refused switch has no receipt and comes back as its words.
pub fn switch(hgs: &Path, session: &str, account: &str, request: &str) -> Result<Value, String> {
    if !valid_name(session) || !valid_name(account) || !valid_request(request) {
        return Err("A switch needs a session, an account and a canonical request id.".into());
    }
    let call = Invocation {
        args: vec![
            "switch-account".into(),
            session.into(),
            "--account".into(),
            account.into(),
            "--request-id".into(),
            request.into(),
        ],
        stdin: None,
        timeout: TIMEOUT,
        request_id: None,
    };
    let exit = run_limited(hgs, &call, &[], RECEIPT_BYTES)
        .map_err(|error| format!("The switch reply is too large: {}", error.cap))?;
    match exit {
        Exit::Code {
            code,
            stdout,
            stderr,
        } => {
            let receipt: Value = serde_json::from_slice(&stdout)
                .map_err(|_| "The switch gave no receipt.".to_owned())?;
            if code == 0 || receipt["status"].as_str().is_some() {
                Ok(receipt)
            } else {
                Err(if stderr.trim().is_empty() {
                    "The switch refused.".into()
                } else {
                    stderr.trim().to_owned()
                })
            }
        }
        Exit::TimedOut => Err("The switch did not finish; check the session before trying again.".into()),
        Exit::StartFailed(error) => Err(format!("Could not start hgs: {error}")),
        Exit::WriteFailed | Exit::Abnormal { .. } => Err("The switch did not run.".into()),
    }
}

/// Task 352: what choosing `account` for `session` would do, changing
/// nothing: `kind` is `account` (the same agent) or `move` (another agent,
/// with `new_session`, `carries`, `drops`), and `refusal` says why it cannot
/// run, empty when it can.
pub fn preview(hgs: &Path, session: &str, account: &str) -> Result<Value, String> {
    if !valid_name(session) || !valid_name(account) {
        return Err("A preview needs a session and an account.".into());
    }
    let call = Invocation {
        args: vec![
            "switch-account".into(),
            session.into(),
            "--account".into(),
            account.into(),
            "--preview".into(),
        ],
        stdin: None,
        timeout: PREVIEW_TIMEOUT,
        request_id: None,
    };
    let exit = run_limited(hgs, &call, &[], RECEIPT_BYTES)
        .map_err(|error| format!("The preview reply is too large: {}", error.cap))?;
    match exit {
        Exit::Code { code: 0, stdout, .. } => {
            let preview: Value = serde_json::from_slice(&stdout).map_err(|_| "The preview gave no answer.".to_owned())?;
            if preview["kind"].as_str().is_some() {
                Ok(preview)
            } else {
                Err("The preview gave no answer.".into())
            }
        }
        Exit::Code { stderr, .. } => Err(if stderr.trim().is_empty() {
            "This hgs cannot preview a move; update it.".into()
        } else {
            stderr.trim().to_owned()
        }),
        Exit::TimedOut => Err("The preview did not answer in time.".into()),
        Exit::StartFailed(error) => Err(format!("Could not start hgs: {error}")),
        Exit::WriteFailed | Exit::Abnormal { .. } => Err("The preview did not run.".into()),
    }
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

/// A canonical UUID: the switch keeps its receipt by this exact text.
fn valid_request(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_a_canonical_uuid_and_names_are_single_tokens() {
        assert!(valid_request("11111111-1111-4111-8111-111111111111"));
        assert!(!valid_request(""));
        assert!(!valid_request("11111111111141118111111111111111"));
        assert!(!valid_request("not-a-uuid"));
        assert!(valid_name("claude/project/session"));
        assert!(!valid_name("two words"));
        assert!(!valid_name(""));
    }

    #[test]
    fn a_preview_needs_a_session_and_an_account() {
        let error = preview(Path::new("hgs"), "", "account").unwrap_err();
        assert!(error.contains("session and an account"), "{error}");
        let error = preview(Path::new("hgs"), "claude/p/s", "two words").unwrap_err();
        assert!(error.contains("session and an account"), "{error}");
    }

    #[test]
    fn a_refused_switch_is_its_words() {
        // The runner is exercised through the fake-hgs tests of the action
        // layers; here the validation stays independent of hgs.
        let error = switch(Path::new("hgs"), "a b", "account", "x").unwrap_err();
        assert!(error.contains("canonical request id"), "{error}");
    }
}
