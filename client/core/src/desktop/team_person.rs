//! Closed Teams owner-card intent. These values are data, never permission.
use super::creation_clone::Scope;
use crate::teams::action::{TeamAction, TeamCall};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const HELP_MARKER: &str = "Desktop Teams Person ABI: desktop-team-person-v1";
pub const BYTES: usize = 64 << 10;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub request_id: String,
    pub team: String,
    pub expected_revision: u64,
    pub card: u64,
    pub decision: String,
}
impl Plan {
    pub fn from_call(call: &TeamCall) -> Option<Self> {
        let TeamAction::Decide { card, decision } = &call.action else {
            return None;
        };
        let plan = Self {
            request_id: call.request_id.clone(),
            team: call.team.clone(),
            expected_revision: call.expected_revision?,
            card: *card,
            decision: decision.clone(),
        };
        plan.valid().then_some(plan)
    }
    pub fn valid(&self) -> bool {
        zerus_desktop_migration::canonical_uuid(&self.request_id)
            && zerus_desktop_migration::canonical_uuid(&self.team)
            && self.expected_revision <= i64::MAX as u64
            && self.card > 0
            && self.card <= i64::MAX as u64
            && !self.decision.is_empty()
            && self.decision.len() <= 128
            && !self.decision.chars().any(char::is_control)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    pub machine: String,
    pub store: String,
    pub facts: String,
    pub plan: Plan,
}
impl Intent {
    pub fn valid(&self) -> bool {
        self.scope.valid()
            && zerus_desktop_migration::canonical_uuid(&self.machine)
            && super::digest(&self.store)
            && super::digest(&self.facts)
            && self.plan.valid()
    }
}

/// Native confirmation text comes from this canonical point read, never from
/// the renderer's team caption or confirmation Boolean.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub intent: Intent,
    pub team_name: String,
    pub card_kind: String,
    pub card_text: String,
    pub card_parameters: Vec<(String, String)>,
    pub card_notes: Vec<String>,
}
impl Review {
    pub fn valid(&self) -> bool {
        self.intent.valid()
            && self.team_name.len() <= 4096
            && self.card_kind.len() <= 128
            && self.card_text.len() <= 32 << 10
            && self.card_parameters.len() <= 8
            && self
                .card_parameters
                .iter()
                .all(|(key, value)| key.len() <= 32 && value.len() <= 1024)
            && self.card_notes.len() <= 16
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= BYTES)
    }
}

pub fn digest(value: &impl Serialize) -> Option<String> {
    let bytes = serde_json::to_vec(value).ok()?;
    (bytes.len() <= 1 << 20).then(|| format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{
        ownership::Mode,
        ui_requests::{Envelope, Request},
        Build, Responder,
    };
    use serde_json::json;

    fn intent() -> Intent {
        Intent {
            scope: Scope {
                configuration: "a".repeat(64),
                service_epoch: uuid::Uuid::new_v4().to_string(),
            },
            machine: uuid::Uuid::new_v4().to_string(),
            store: "b".repeat(64),
            facts: "c".repeat(64),
            plan: Plan {
                request_id: uuid::Uuid::new_v4().to_string(),
                team: uuid::Uuid::new_v4().to_string(),
                expected_revision: 7,
                card: 1,
                decision: "acknowledged".into(),
            },
        }
    }
    #[test]
    fn person_and_confirmation_fields_are_not_part_of_the_native_wire() {
        let intent = intent();
        assert!(intent.valid());
        for key in ["confirmed", "person", "origin", "permit"] {
            let mut value = json!({"op":"team_person_decide","intent":intent});
            value["intent"][key] = json!(true);
            assert!(serde_json::from_value::<Request>(value).is_err(), "{key}");
        }
        let request = Request::TeamPersonDecide {
            intent: Box::new(intent.clone()),
        };
        let envelope = Envelope {
            abi: crate::desktop::ui_requests::ABI.into(),
            attempt: uuid::Uuid::new_v4().to_string(),
            request: request.clone(),
        };
        assert!(envelope.valid());
        assert_eq!(request.id(), intent.plan.request_id);
        let mut another = intent;
        another.plan.card += 1;
        assert_ne!(
            request.fingerprint(),
            Request::TeamPersonDecide {
                intent: Box::new(another)
            }
            .fingerprint()
        );
    }
    #[test]
    fn responder_keeps_native_mode_and_configuration_binding() {
        const CASE: &str =
            "desktop::team_person::tests::responder_keeps_native_mode_and_configuration_binding";
        if std::env::var_os("ZERUS_TEAM_PERSON_RESPONDER_TEST_CHILD").is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", CASE, "--test-threads=1", "--nocapture"])
                .env_remove("HGS_SESSION")
                .env_remove("HGS_SESSION_ID")
                .env_remove("HGS_RUN_ID")
                .env("ZERUS_TEAM_PERSON_RESPONDER_TEST_CHILD", "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
            return;
        }
        let intent = intent();
        for (configuration, mode, accepted) in [
            ("a".repeat(64), None, false),
            ("a".repeat(64), Some(Mode::Native), false),
            ("f".repeat(64), Some(Mode::Service), false),
            ("a".repeat(64), Some(Mode::Service), true),
        ] {
            let responder = Responder::new(
                "com.hgdev.hgs-tray".into(),
                Build {
                    version: "1.2.3".into(),
                    source_commit: "a".repeat(40),
                    source_dirty: Some(false),
                },
                uuid::Uuid::new_v4().to_string(),
                true,
                false,
            );
            assert!(responder.configure_once(configuration));
            if let Some(mode) = mode {
                assert!(responder.configure_ownership_mode_once(mode));
            }
            let request = Request::TeamPersonDecide {
                intent: Box::new(intent.clone()),
            };
            assert_eq!(
                responder
                    .prepare_ui_request(uuid::Uuid::new_v4().to_string(), request)
                    .is_ok(),
                accepted
            );
        }
    }
}
