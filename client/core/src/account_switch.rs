//! The owner moves one running session to another account of the same agent
//! (`hgs switch-account`): the agent is stopped, the conversation is copied to
//! the target's store and resumed, and a failed launch rolls back. The webview
//! never runs hgs; this module builds the guarded call and bounds its receipt.

use crate::actions::{Exit, Invocation};
use crate::run::run_limited;
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

/// A switch copies a conversation and restarts the agent; Qt waits 180 s.
const TIMEOUT: Duration = Duration::from_secs(180);
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
    fn a_refused_switch_is_its_words() {
        // The runner is exercised through the fake-hgs tests of the action
        // layers; here the validation stays independent of hgs.
        let error = switch(Path::new("hgs"), "a b", "account", "x").unwrap_err();
        assert!(error.contains("canonical request id"), "{error}");
    }
}
