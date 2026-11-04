//! One fixed recovery kind. Wire data, hashes and profile names supply no
//! current native frontend, executor, lease, consent or execution authority.
use crate::{creation::canonical_uuid, operation_recovery::task_start as policy};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROFILE: &str = "task-start-reconcile-v1";
pub const HELP_MARKER: &str = "Desktop task start reconcile ABI: desktop-task-start-reconcile-v1";
/// Embedded originals must leave room inside the unchanged 64 KiB control line.
/// The larger pure-policy/storage limits do not enlarge this wire boundary.
pub const SOURCE_BYTES: usize = 32 << 10;

mod result;
pub use result::{
    Attempt, Lookup, Outcome, Phase, Query, Receipt, Review, Ticket, Unknown, RESULT_ABI,
    RESULT_BYTES,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub scope: Scope,
    /// A fresh explicit reconciliation, never the task's original delivery ID.
    pub reconcile_id: String,
    pub binding: policy::Binding,
    /// The complete exact producer checkpoint, including retained_input.
    pub original: String,
}
impl std::fmt::Debug for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskStartReconcileIntent")
            .field("reconcile_id", &self.reconcile_id)
            .field("original_request_id", &self.binding.key.request_id)
            .finish_non_exhaustive()
    }
}

pub struct Verified {
    pub intent: Intent,
    pub original: policy::Original,
}

fn uuid(value: &str) -> bool {
    canonical_uuid(value) && value != "00000000-0000-0000-0000-000000000000"
}

// An executing-kind request cannot rely on Value's last-key-wins projection.
// This stricter decoder is local to this bounded wire route. Generic outboxes
// and imported opaque originals keep their existing decoders and bytes.
struct Unique(Value);
pub(crate) fn original_value(original: &str) -> Result<Value, &'static str> {
    serde_json::from_str::<Unique>(original)
        .map(|v| v.0)
        .map_err(|_| "invalid_task_start_reconcile_original")
}
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Unique;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unambiguous original JSON")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Unique, E> {
                serde_json::Number::from_f64(value)
                    .map(|value| Unique(Value::Number(value)))
                    .ok_or_else(|| E::custom("invalid original number"))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(
                self,
                mut seq: S,
            ) -> Result<Unique, S::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<Unique>()? {
                    values.push(value.0);
                }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Unique, M::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("duplicate original field"));
                    }
                    values.insert(key, map.next_value::<Unique>()?.0);
                }
                Ok(Unique(Value::Object(values)))
            }
        }
        decoder.deserialize_any(Visitor)
    }
}

impl Intent {
    /// Pure verification only. The actor separately needs the original native
    /// two-second proof after BEGIN, current source/executor and live Consent.
    pub fn verify(self) -> Result<Verified, &'static str> {
        if !self.scope.valid()
            || !uuid(&self.reconcile_id)
            || !uuid(&self.binding.key.request_id)
            || self.reconcile_id == self.binding.key.request_id
            || self.binding.key.configuration != self.scope.configuration
            || self.original.is_empty()
            || self.original.len() > SOURCE_BYTES
        {
            return Err("invalid_task_start_reconcile_intent");
        }
        let record = original_value(&self.original)?;
        let input = record["intent"]["retained_input"]
            .as_str()
            .filter(|input| !input.is_empty())
            .ok_or("task_start_original_input_required")?;
        let _: Unique =
            serde_json::from_str(input).map_err(|_| "invalid_task_start_reconcile_input")?;
        let original = policy::Original::verify(
            self.binding.clone(),
            self.original.as_bytes(),
            input.as_bytes(),
        )
        .map_err(|_| "task_start_reconcile_original_changed")?;
        if !matches!(
            original.step(policy::Input::Review, false),
            policy::Decision::NeedsReview(policy::Review::NoLookupResult)
        ) {
            return Err("task_start_reconcile_original_not_pending");
        }
        Ok(Verified {
            intent: self,
            original,
        })
    }
}

#[cfg(test)]
pub(crate) fn test_intent() -> Intent {
    use zerus_desktop_migration::sha256;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/desktop-operation-port-v1.json"
    ))
    .unwrap();
    let mut record: Value = serde_json::from_str(fixture["raw_record"].as_str().unwrap()).unwrap();
    record["intent"]["retained_input"] = fixture["retained_input"].clone();
    let mut binding: policy::Binding = serde_json::from_value(fixture["binding"].clone()).unwrap();
    let original = serde_json::to_string(&record).unwrap();
    binding.source_record_sha256 = sha256(original.as_bytes());
    binding.ui_snapshot_sha256 = crate::tasks_write::outbox::snapshot(&record);
    Intent {
        scope: Scope {
            configuration: binding.key.configuration.clone(),
            service_epoch: "00000006-1111-4111-8111-111111111111".into(),
        },
        reconcile_id: "00000008-1111-4111-8111-111111111111".into(),
        binding,
        original,
    }
}

#[cfg(test)]
mod tests;
