//! The owner settings hgs keeps in its own state, which Settings > Sessions
//! reads and writes with the person's own call (SettingsPage.h:242-250):
//! "Ask before an agent starts subagents" is `hgs subagents-confirm` and
//! "Block DeepSeek session-log upload" is `hgs session-upload-block`. Qt runs
//! `<command> get|set --json` with `"by":"human"`; the desktop does the same
//! here, so the webview never parses hgs's state files. A refused or failed
//! call changes nothing and the settings page reads the value again.

use crate::actions::{failure_detail, Exit, Invocation};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

/// The most of one reply kept: these replies are a single bool.
pub const REPLY_LIMIT: usize = 64 * 1024;

/// The two settings Settings > Sessions offers (SettingsPage.h:242-250).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HgsSetting {
    /// `hgs subagents-confirm`: ask before a session starts subagents.
    SubagentsConfirm,
    /// `hgs session-upload-block`: start DeepSeek sessions with its log upload off.
    SessionUploadBlock,
}

impl HgsSetting {
    /// The hgs command, as SettingsPage.h names it.
    pub fn command(self) -> &'static str {
        match self {
            Self::SubagentsConfirm => "subagents-confirm",
            Self::SessionUploadBlock => "session-upload-block",
        }
    }

    /// The setting's own key in its reply (`{"confirm":true}` / `{"block":true}`).
    pub fn key(self) -> &'static str {
        match self {
            Self::SubagentsConfirm => "confirm",
            Self::SessionUploadBlock => "block",
        }
    }

    /// The default hgs applies when the setting was never stored.
    pub fn default_value(self) -> bool {
        true
    }
}

/// One read or write: `value` none reads, some sets it. `request_id` repeats
/// an earlier write whose outcome was unknown, so hgs answers the same
/// receipt instead of applying it twice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HgsSettingCall {
    pub setting: HgsSetting,
    #[serde(default)]
    pub value: Option<bool>,
    #[serde(default)]
    pub request_id: Option<String>,
}

/// The value hgs read or stored.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HgsSettingView {
    pub setting: HgsSetting,
    pub value: bool,
}

/// A read or write and how it ended. A `failed` outcome changes nothing.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum HgsSettingOutcome {
    Done {
        view: HgsSettingView,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    Failed {
        detail: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
}

impl HgsSettingCall {
    /// The call hgs receives and, for a write, the request id it is bound to.
    pub fn prepare(&self) -> (Invocation, Option<String>) {
        let request_id = self
            .value
            .map(|_| self.request_id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()));
        let operation = if self.value.is_some() { "set" } else { "get" };
        let stdin = self.value.map(|value| {
            json!({
                "request_id": request_id,
                self.setting.key(): value,
                "by": "human",
            })
            .to_string()
            .into_bytes()
        });
        (
            Invocation {
                args: vec![self.setting.command().to_owned(), operation.to_owned(), "--json".to_owned()],
                stdin,
                timeout: Duration::from_secs(15),
                request_id: request_id.clone(),
            },
            request_id,
        )
    }
}

/// The value in a reply, or none for anything that is not this setting's bool.
pub fn parse(value: &Value, setting: HgsSetting) -> Option<bool> {
    value.as_object()?.get(setting.key())?.as_bool()
}

/// Runs one read or write and classifies the reply.
pub fn run(hgs: &Path, call: &HgsSettingCall) -> HgsSettingOutcome {
    let (invocation, request_id) = call.prepare();
    match crate::run::run_limited(hgs, &invocation, &[], REPLY_LIMIT) {
        Ok(Exit::Code { code: 0, stdout, .. }) => {
            match serde_json::from_slice::<Value>(&stdout).ok().as_ref().and_then(|value| parse(value, call.setting)) {
                Some(value) => HgsSettingOutcome::Done {
                    view: HgsSettingView { setting: call.setting, value },
                    request_id,
                },
                None => HgsSettingOutcome::Failed {
                    detail: "Invalid hgs setting response.".to_owned(),
                    request_id,
                },
            }
        }
        Ok(exit) => HgsSettingOutcome::Failed { detail: failure_detail(exit), request_id },
        Err(_) => HgsSettingOutcome::Failed {
            detail: "The hgs setting reply is too large.".to_owned(),
            request_id,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(setting: HgsSetting, value: Option<bool>) -> HgsSettingCall {
        HgsSettingCall { setting, value, request_id: None }
    }

    #[test]
    fn a_read_names_the_command_and_sends_nothing() {
        let (invocation, request_id) = call(HgsSetting::SubagentsConfirm, None).prepare();
        assert_eq!(invocation.args, ["subagents-confirm", "get", "--json"]);
        assert!(invocation.stdin.is_none() && request_id.is_none() && invocation.request_id.is_none());
        let (upload, _) = call(HgsSetting::SessionUploadBlock, None).prepare();
        assert_eq!(upload.args, ["session-upload-block", "get", "--json"]);
    }

    #[test]
    fn a_write_sends_its_value_as_the_person_with_a_request_id() {
        let (invocation, request_id) = call(HgsSetting::SubagentsConfirm, Some(false)).prepare();
        assert_eq!(invocation.args, ["subagents-confirm", "set", "--json"]);
        let input: Value = serde_json::from_slice(invocation.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(input["confirm"], false);
        assert_eq!(input["by"], "human");
        let id = request_id.clone().unwrap();
        assert_eq!(input["request_id"], id.as_str());
        assert_eq!(invocation.request_id.as_deref(), Some(id.as_str()));
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        let (upload, _) = call(HgsSetting::SessionUploadBlock, Some(true)).prepare();
        let input: Value = serde_json::from_slice(upload.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(input["block"], true);
    }

    #[test]
    fn a_retry_keeps_its_request_id() {
        let id = "11111111-1111-4111-8111-111111111111";
        let retry = HgsSettingCall { setting: HgsSetting::SubagentsConfirm, value: Some(true), request_id: Some(id.into()) };
        let (invocation, request_id) = retry.prepare();
        assert_eq!(request_id.as_deref(), Some(id));
        assert_eq!(invocation.request_id.as_deref(), Some(id));
    }

    #[test]
    fn only_this_settings_own_bool_is_a_value() {
        assert_eq!(parse(&json!({"confirm":false}), HgsSetting::SubagentsConfirm), Some(false));
        assert_eq!(parse(&json!({"block":true}), HgsSetting::SessionUploadBlock), Some(true));
        assert_eq!(parse(&json!({"confirm":true}), HgsSetting::SessionUploadBlock), None);
        assert_eq!(parse(&json!({"confirm":"yes"}), HgsSetting::SubagentsConfirm), None);
        assert_eq!(parse(&json!(["confirm"]), HgsSetting::SubagentsConfirm), None);
        assert!(HgsSetting::SubagentsConfirm.default_value());
        assert!(HgsSetting::SessionUploadBlock.default_value());
    }
}
