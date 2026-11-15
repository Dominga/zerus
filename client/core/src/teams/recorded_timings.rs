//! Checked existing bookkeeping facts. No native work-duration inference.
use super::read::TeamError;
use super::view::NAME_CHARS;
use serde::{Deserialize, Deserializer, Serialize};

fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TimingSnapshot {
    pub schema: u8,
    pub revision: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectRecord {
    pub request_id: String,
    pub node: u32,
    pub generation: u32,
    pub member: String,
    pub kind: String,
    pub state: String,
    pub reserved_ms: i64,
    #[serde(deserialize_with = "nullable")]
    pub first_outcome_recorded_ms: Option<i64>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageRecord {
    pub node: u32,
    pub target: u32,
    pub generation: u32,
    pub state: String,
    pub created_ms: i64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unavailable {
    RecordMissing,
    InvalidReference,
    AmbiguousReference,
    ReferenceLimit,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFacts {
    #[serde(deserialize_with = "nullable")]
    effect: Option<EffectRecord>,
    #[serde(deserialize_with = "nullable")]
    stage: Option<StageRecord>,
    #[serde(deserialize_with = "nullable")]
    effect_unavailable: Option<Unavailable>,
    #[serde(deserialize_with = "nullable")]
    stage_unavailable: Option<Unavailable>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectView {
    pub record: EffectRecord,
    #[serde(deserialize_with = "nullable")]
    pub bookkeeping_elapsed_ms: Option<i64>,
    pub clock_or_range_unknown: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedFacts {
    #[serde(deserialize_with = "nullable")]
    pub effect: Option<EffectView>,
    #[serde(deserialize_with = "nullable")]
    pub stage: Option<StageRecord>,
    #[serde(deserialize_with = "nullable")]
    pub effect_unavailable: Option<Unavailable>,
    #[serde(deserialize_with = "nullable")]
    pub stage_unavailable: Option<Unavailable>,
}

fn invalid() -> TeamError {
    TeamError::Invalid {
        message: "Recorded timing facts are unsupported or malformed. Use Events only.".into(),
    }
}
fn name(s: &str) -> bool {
    !s.is_empty() && s.len() <= NAME_CHARS * 4 && s.chars().count() <= NAME_CHARS
}
pub(super) fn valid_effect(record: &EffectRecord) -> bool {
    record.node > 0
        && record.generation > 0
        && name(&record.member)
        && name(&record.kind)
        && name(&record.state)
        && uuid::Uuid::parse_str(&record.request_id)
            .is_ok_and(|id| id.to_string() == record.request_id)
}
pub(super) fn valid_stage(record: &StageRecord) -> bool {
    record.node > 0 && record.target > 0 && record.generation > 0 && name(&record.state)
}
pub(super) fn valid_view(facts: &RecordedFacts) -> bool {
    !(facts.effect.is_some() && facts.effect_unavailable.is_some()
        || facts.stage.is_some() && facts.stage_unavailable.is_some())
        && facts.stage.as_ref().is_none_or(valid_stage)
        && facts.effect.as_ref().is_none_or(|e| {
            valid_effect(&e.record)
                && e.bookkeeping_elapsed_ms
                    == e.record
                        .first_outcome_recorded_ms
                        .and_then(|end| elapsed(e.record.reserved_ms, end))
                && e.clock_or_range_unknown
                    == (e.record.first_outcome_recorded_ms.is_some()
                        && e.bookkeeping_elapsed_ms.is_none())
        })
}
pub fn elapsed(start: i64, end: i64) -> Option<i64> {
    end.checked_sub(start)
        .filter(|ms| *ms >= 0 && *ms <= 9_007_199_254_740_991)
}

pub(super) fn snapshot(value: &Value) -> Result<TimingSnapshot, TeamError> {
    let snapshot: TimingSnapshot = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if snapshot.schema != 1 || snapshot.revision == 0 || snapshot.revision > 9_007_199_254_740_991 {
        return Err(invalid());
    }
    Ok(snapshot)
}
pub(super) fn facts(value: &Value) -> Result<RecordedFacts, TeamError> {
    let facts: RawFacts = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
    if let Some(stage) = &facts.stage {
        if stage.node == 0 || stage.target == 0 || stage.generation == 0 || !name(&stage.state) {
            return Err(invalid());
        }
    }
    let effect = facts
        .effect
        .map(|record| {
            if record.node == 0
                || record.generation == 0
                || !name(&record.member)
                || !name(&record.kind)
                || !name(&record.state)
                || !uuid::Uuid::parse_str(&record.request_id)
                    .is_ok_and(|id| id.to_string() == record.request_id)
            {
                return Err(invalid());
            }
            let bookkeeping_elapsed_ms = record
                .first_outcome_recorded_ms
                .and_then(|end| elapsed(record.reserved_ms, end));
            let clock_or_range_unknown =
                record.first_outcome_recorded_ms.is_some() && bookkeeping_elapsed_ms.is_none();
            Ok(EffectView {
                record,
                bookkeeping_elapsed_ms,
                clock_or_range_unknown,
            })
        })
        .transpose()?;
    let projected = RecordedFacts {
        effect,
        stage: facts.stage,
        effect_unavailable: facts.effect_unavailable,
        stage_unavailable: facts.stage_unavailable,
    };
    if !valid_view(&projected) {
        return Err(invalid());
    }
    Ok(projected)
}

#[cfg(test)]
mod tests;
