//! Exact input for the first service-owned creation effect. These closed values
//! carry no current writer, frontend, profile, exclusion or execution authority.
use crate::{creation, paths::Style, repository, ComputerId, Invocation};
use serde::{de::IgnoredAny, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub const ABI: &str = "desktop-creation-clone-v1";
pub const HELP_MARKER: &str = "Desktop creation clone ABI: desktop-creation-clone-v1";
/// Leaves room for the authenticated envelope inside the existing control line.
pub const SOURCE_BYTES: usize = 32 << 10;
pub const PROFILE: &str = "creation-clone-v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    CreationClone,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub configuration: String,
    pub service_epoch: String,
}
impl Scope {
    pub fn valid(&self) -> bool {
        super::digest(&self.configuration) && uuid(&self.service_epoch)
    }
}

/// Exact native reducer checkpoint. Imported recovery uses a separate route;
/// neither an import nor this value itself supplies fresh execution consent.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    pub creation_id: String,
    pub revision: u64,
    pub original: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub clone_request_id: String,
    pub expected_machine: String,
    pub url: String,
    pub parent: String,
    pub name: String,
}

/// Persisted per-kind binding, deliberately unrelated to task-start fields.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub kind: Kind,
    pub schema: u32,
    pub configuration: String,
    pub creation_id: String,
    pub clone_request_id: String,
    pub revision: u64,
    pub source_sha256: String,
    pub plan_sha256: String,
    pub consent_sha256: String,
}

pub struct Verified {
    pub intent: Intent,
    pub plan: Plan,
    pub binding: Binding,
}

fn uuid(value: &str) -> bool {
    creation::canonical_uuid(value) && value != "00000000-0000-0000-0000-000000000000"
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

// Check raw keys before Value can erase ambiguity. Opaque original fields are
// retained byte-for-byte; rejecting their duplicates loses no supported input.
fn unique(bytes: &[u8]) -> Result<(), &'static str> {
    struct Object;
    impl<'de> serde::de::Visitor<'de> for Object {
        type Value = ();
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("one unambiguous creation checkpoint")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
            let mut keys = HashSet::new();
            while let Some(key) = map.next_key::<String>()? {
                if !keys.insert(key) {
                    return Err(serde::de::Error::custom("duplicate creation field"));
                }
                map.next_value::<IgnoredAny>()?;
            }
            Ok(())
        }
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    serde::de::Deserializer::deserialize_map(&mut decoder, Object)
        .map_err(|_| "invalid_creation_checkpoint")?;
    decoder.end().map_err(|_| "invalid_creation_checkpoint")
}

impl Intent {
    pub fn verify(self) -> Result<Verified, &'static str> {
        if !self.scope.valid()
            || !uuid(&self.creation_id)
            || !(1..=i64::MAX as u64).contains(&self.revision)
            || self.original.is_empty()
            || self.original.len() > SOURCE_BYTES
        {
            return Err("invalid_creation_intent");
        }
        unique(self.original.as_bytes())?;
        let value: Value =
            serde_json::from_str(&self.original).map_err(|_| "invalid_creation_checkpoint")?;
        let text = |key| value[key].as_str().unwrap_or("");
        if !creation::valid_operation(&self.creation_id, &value)
            || value["revision"].as_u64() != Some(self.revision)
            || text("state") != "cloning"
            || value["repository"] != true
            || !text("host").is_empty()
            || !uuid(text("clone_id"))
            || !uuid(text("expected_machine"))
            || [
                "clone_done",
                "worktree_done",
                "launch_dispatched",
                "stop_requested",
            ]
            .iter()
            .any(|key| value[*key] == true)
            || ["worktree_id", "bound_session", "acquisition_confirmed"]
                .iter()
                .any(|key| value.get(*key).is_some())
            || value
                .get("launch_id")
                .is_some_and(|id| !id.as_str().is_some_and(uuid))
        {
            return Err("creation_intent_changed");
        }
        let parent = text("parent");
        let prefix = format!("{}/", parent.trim_end_matches('/'));
        let name = text("target")
            .strip_prefix(&prefix)
            .filter(|name| repository::valid_name(name) && *name == text("repository_name"))
            .ok_or("invalid_clone_destination")?;
        if !crate::paths::absolute(parent, Style::Posix)
            || parent.len() > 8192
            || parent.chars().any(char::is_control)
            || text("url").len() > 4096
            || !creation::safe_repository_url(text("url"))
            || name.chars().count() > 80
        {
            return Err("invalid_clone_destination");
        }
        let plan = Plan {
            clone_request_id: text("clone_id").into(),
            expected_machine: text("expected_machine").into(),
            url: text("url").into(),
            parent: parent.into(),
            name: name.into(),
        };
        let encoded = serde_json::to_vec(&plan).map_err(|_| "invalid_clone_plan")?;
        let mut consent = b"desktop-creation-clone-consent-v1\0".to_vec();
        consent.extend_from_slice(self.original.as_bytes());
        let binding = Binding {
            kind: Kind::CreationClone,
            schema: 1,
            configuration: self.scope.configuration.clone(),
            creation_id: self.creation_id.clone(),
            clone_request_id: plan.clone_request_id.clone(),
            revision: self.revision,
            source_sha256: hash(self.original.as_bytes()),
            plan_sha256: hash(&encoded),
            consent_sha256: hash(&consent),
        };
        Ok(Verified {
            intent: self,
            plan,
            binding,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creation::{Answer, Call, Creations, Effect, Journal, Placement};
    use serde_json::json;
    use std::collections::VecDeque;

    const ID: &str = "11111111-1111-4111-8111-111111111111";
    const MACHINE: &str = "22222222-2222-4222-8222-222222222222";
    const EPOCH: &str = "33333333-3333-4333-8333-333333333333";
    struct NeverPlace;
    impl Placement for NeverPlace {
        fn resolve(&mut self, _: &mut Value, _: &Value) -> Result<(), String> {
            panic!("the clone leaf never performs project placement")
        }
    }
    fn checkpoint() -> Intent {
        let mut n = 10u64;
        let mut reducer = Creations::new(
            || 1,
            move || {
                n += 1;
                format!("00000000-0000-4000-8000-{n:012x}")
            },
        );
        reducer.load(Journal::Missing);
        let (admitted, effects) = reducer.submit_reserved(
            json!({
                "cmd":"codex", "tag":"example", "target":"/srv/repo", "parent":"/srv",
                "host":"", "repository":true, "url":"https://example.invalid/repo.git",
                "repository_name":"repo", "auto":false
            }),
            ID,
        );
        assert_eq!(admitted.unwrap(), ID);
        let mut effects: VecDeque<_> = effects.into();
        let mut saved = None;
        while let Some(effect) = effects.pop_front() {
            match effect {
                Effect::Write { write, bytes, .. } => {
                    saved = Some(bytes);
                    effects.extend(reducer.written(write, true));
                }
                Effect::Call {
                    flight,
                    call: Call::CreationDirectory { .. },
                } => {
                    effects.extend(reducer.answered(
                        flight,
                        Answer {
                            ok: true,
                            data: json!({"creation_identity_v1":true,"computer_id":MACHINE}),
                            error: String::new(),
                        },
                        &mut NeverPlace,
                    ));
                }
                Effect::Call {
                    flight,
                    call:
                        Call::Repository {
                            action: repository::Action::Check,
                            ..
                        },
                } => {
                    effects.extend(reducer.answered(flight, Answer {
                        ok:true, data:json!({"status":"ready","parent":"/srv","path":"/srv/repo","name":"repo"}),
                        error:String::new()
                    }, &mut NeverPlace));
                }
                Effect::Call {
                    call:
                        Call::Repository {
                            action: repository::Action::Clone,
                            request_id,
                            ..
                        },
                    ..
                } => {
                    let journal: Value = serde_json::from_slice(&saved.unwrap()).unwrap();
                    let value = &journal["operations"][ID];
                    assert_eq!(value["clone_id"], request_id);
                    return Intent {
                        scope: Scope {
                            configuration: "a".repeat(64),
                            service_epoch: EPOCH.into(),
                        },
                        creation_id: ID.into(),
                        revision: value["revision"].as_u64().unwrap(),
                        original: serde_json::to_string(value).unwrap(),
                    };
                }
                Effect::Changed { .. } => (),
                _ => panic!("unexpected effect before the original clone"),
            }
        }
        panic!("the real creation reducer did not produce its clone")
    }

    #[test]
    fn accepts_the_real_reducers_durable_clone_checkpoint_with_reserved_launch_identity() {
        let intent = checkpoint();
        let original = intent.original.clone();
        let verified = intent.verify().unwrap();
        assert_eq!(verified.intent.original, original);
        assert_eq!(verified.binding.source_sha256, hash(original.as_bytes()));
        assert_eq!(verified.plan.name, "repo");
        let invocation = verified.plan.invocation(repository::Action::Clone);
        assert!(invocation.args.contains(&verified.plan.clone_request_id));
        assert!(invocation.args.contains(&MACHINE.into()));
    }

    #[test]
    fn unknown_original_fields_are_retained_and_bound_to_consent() {
        let first = checkpoint().verify().unwrap();
        let mut changed = checkpoint();
        let mut value: Value = serde_json::from_str(&changed.original).unwrap();
        value["opaque_future"] = json!({"keep":"original bytes"});
        changed.original = serde_json::to_string(&value).unwrap();
        let second = changed.verify().unwrap();
        assert!(second.intent.original.contains("opaque_future"));
        assert_eq!(first.binding.plan_sha256, second.binding.plan_sha256);
        assert_ne!(first.binding.source_sha256, second.binding.source_sha256);
        assert_ne!(first.binding.consent_sha256, second.binding.consent_sha256);
    }

    #[test]
    fn ambiguous_historical_or_changed_checkpoints_never_become_fresh_clone_input() {
        for (key, value) in [
            ("state", json!("unknown")),
            ("host", json!("peer")),
            ("launch_dispatched", json!(true)),
            ("clone_done", json!(true)),
            ("stop_requested", json!(true)),
            ("repository_name", json!("different")),
            ("expected_machine", json!("")),
            ("revision", json!(0)),
        ] {
            let mut intent = checkpoint();
            let mut raw: Value = serde_json::from_str(&intent.original).unwrap();
            raw[key] = value;
            intent.original = serde_json::to_string(&raw).unwrap();
            assert!(intent.verify().is_err(), "accepted changed field {key}");
        }
        let mut intent = checkpoint();
        intent.original = intent.original.replacen('{', "{\"state\":\"cloning\",", 1);
        assert!(intent.verify().is_err());
        let mut intent = checkpoint();
        intent.original = " ".repeat(SOURCE_BYTES + 1);
        assert!(intent.verify().is_err());
    }
}

impl Plan {
    pub fn invocation(&self, action: repository::Action) -> Invocation {
        repository::invocation(
            &ComputerId::local(),
            action,
            &self.url,
            &self.parent,
            &self.name,
            &self.clone_request_id,
            &self.expected_machine,
        )
    }
}
