//! Dormant original marker reservation. Data lookup never reconstructs its
//! live resource, and no installed caller or production freshness mint exists.
use super::{
    gate::{RunGate, RunWriterGate},
    Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

mod codec;
mod publication;
mod source;
use source::Source;

const MARKER_BYTES: usize = 32 << 10;
const RECORD_BYTES: usize = 1 << 20;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Correlation {
    terminal_session: String,
    root_birth: String,
    supervisor_birth: String,
    source_sha256: String,
    configuration_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Data {
    version: u8,
    request_id: String,
    intent_sha256: String,
    binding: Value,
    correlation: Correlation,
    owner_pid: u32,
    owner_birth: String,
    phase: String,
}

/// Deliberately no production constructor, Clone or deserializer. The future
/// original request producer owes freshness/consent and physical IO admission.
pub(super) struct Original {
    source: Source,
    request_id: String,
    intent_sha256: String,
    correlation: Correlation,
    until: Instant,
    began: Instant,
    consumed: Arc<AtomicBool>,
}

/// An original fence resource, never an input/signal/finalization permission.
/// Drop closes only native resources and cannot edit the persisted marker.
pub(super) struct Reservation {
    data: Data,
    _run: RunGate,
}

pub(super) enum Observation {
    Absent,
    Stored(Box<Data>),
    Unrecognized,
}

/// Stored data only, including an exact repeat. There is deliberately no
/// conversion from Observation or Data to Original/Reservation.
pub(super) fn observe(record: &Value) -> Observation {
    let Some(value) = record.get("terminating") else {
        return Observation::Absent;
    };
    if codec::encoded(record, RECORD_BYTES).is_err() {
        return Observation::Unrecognized;
    }
    let mut projected = record.clone();
    if let Some(object) = projected.as_object_mut() {
        object.remove("terminating");
    }
    match codec::decode(value) {
        Ok(value) if codec::binding(&projected).ok().as_ref() == Some(&value.binding) => {
            Observation::Stored(Box::new(value))
        }
        _ => Observation::Unrecognized,
    }
}

pub(super) fn reserve(original: Original) -> Result<Reservation> {
    if original
        .consumed
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(super::REFUSED.into());
    }
    original.check()?;
    // Earlier writers may need the normal state writer to finish. Never
    // acquire their exclusive run gate while already retaining that writer.
    let gate = RunWriterGate::acquire(original.source.scope(), original.until)?;
    original.check()?;
    let data = Data {
        version: 1,
        request_id: original.request_id.clone(),
        intent_sha256: original.intent_sha256.clone(),
        binding: original.source.binding().clone(),
        correlation: original.correlation.clone(),
        owner_pid: original.source.owner().pid,
        owner_birth: original.source.owner().birth.clone(),
        phase: "reserved".into(),
    };
    codec::validate(&data)?;
    original.source.publish(&gate, &data, original.until)?;
    original.check()?;
    gate.recheck()?;
    // No global writer is retained across the future owner's wait. The SAME
    // original run lease remains held, and Drop never edits the marker.
    Ok(Reservation {
        data,
        _run: gate.release_writer(),
    })
}

impl Original {
    fn check(&self) -> Result<()> {
        let now = Instant::now();
        if now < self.began
            || now >= self.until
            || self
                .until
                .checked_duration_since(self.began)
                .is_none_or(|d| d > std::time::Duration::from_secs(30))
            || !super::uuid(&self.request_id)
            || !super::digest(&self.intent_sha256)
        {
            return Err(super::REFUSED.into());
        }
        self.source.check()
    }

    // No production freshness mint. The later original durable-request/IO
    // producer must be separately admitted; status/absence cannot call this.
    #[cfg(test)]
    fn capture(
        writer: &crate::state::storage::Guard,
        record: &Value,
        request: &str,
        intent: &str,
        correlation: Correlation,
        until: Instant,
    ) -> Result<Self> {
        let began = Instant::now();
        let value = Self {
            source: Source::capture(writer, record, until)?,
            request_id: request.into(),
            intent_sha256: intent.into(),
            correlation,
            until,
            began,
            consumed: Arc::new(AtomicBool::new(false)),
        };
        value.check()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
