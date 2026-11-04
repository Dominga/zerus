//! Separate, bounded legacy CLI project-list completion family. No requests,
//! names or paths are retained here; only original-scope IDs and digest proof.
use crate::swarm::legacy_projects::{Binding, Code, Evidence, Plan, bytes};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub(crate) const KEY: &str = "legacy_project_lists_v1";
pub const MAX_MACHINES: usize = 256;
pub const MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    evidence: Evidence,
    complete: bool,
}
impl Record {
    pub fn binding(&self) -> &Binding {
        &self.evidence.binding
    }
    pub fn complete(&self) -> bool {
        self.complete
    }
    pub fn code(&self) -> Option<Code> {
        if self.complete {
            None
        } else {
            Some(self.evidence.problem().unwrap_or(Code::Pending))
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Family {
    version: u8,
    records: BTreeMap<String, Record>,
}
fn load(extra: &Map<String, Value>) -> Result<Family, Code> {
    let Some(value) = extra.get(KEY) else {
        return Ok(Family {
            version: 1,
            records: BTreeMap::new(),
        });
    };
    let encoded = bytes(value, MAX_BYTES)?;
    let family: Family = serde_json::from_slice(&encoded).map_err(|_| Code::Invalid)?;
    if family.version != 1
        || family.records.len() > MAX_MACHINES
        || family.records.iter().any(|(machine, r)| {
            machine != &r.evidence.binding.machine
                || !r.evidence.valid()
                || (r.complete && r.evidence.problem().is_some())
        })
    {
        return Err(Code::Invalid);
    }
    Ok(family)
}
fn save(extra: &mut Map<String, Value>, family: &Family) -> Result<(), Code> {
    let encoded = bytes(family, MAX_BYTES)?;
    let value = serde_json::from_slice(&encoded).map_err(|_| Code::Invalid)?;
    extra.insert(KEY.into(), value);
    Ok(())
}
pub(super) fn read(extra: &Map<String, Value>, machine: &str) -> Result<Option<Record>, Code> {
    if !crate::creation::canonical_uuid(machine) {
        return Err(Code::Invalid);
    }
    Ok(load(extra)?.records.remove(machine))
}
pub(super) fn prepare(extra: &mut Map<String, Value>, plan: &Plan) -> Result<bool, Code> {
    let proof = plan.evidence();
    if !proof.valid() {
        return Err(Code::Invalid);
    }
    let mut family = load(extra)?;
    if let Some(record) = family.records.get(&proof.binding.machine) {
        // A completed physical computer is not imported again through another
        // alias/catalog. An unresolved original cannot transfer to a new scope.
        return if record.complete || record.binding() == &proof.binding {
            Ok(false)
        } else {
            Err(Code::Changed)
        };
    }
    if family.records.len() == MAX_MACHINES {
        return Err(Code::Capacity);
    }
    family.records.insert(
        proof.binding.machine.clone(),
        Record {
            evidence: proof.clone(),
            complete: false,
        },
    );
    save(extra, &family)?;
    Ok(true)
}
pub(super) fn observe(
    extra: &mut Map<String, Value>,
    binding: &Binding,
    snapshot: &Value,
) -> Result<bool, Code> {
    let mut family = load(extra)?;
    let record = family
        .records
        .get_mut(&binding.machine)
        .ok_or(Code::Invalid)?;
    if record.complete {
        return Ok(false);
    }
    if !record.evidence.observed(binding, snapshot)? {
        return Ok(false);
    }
    record.complete = true;
    save(extra, &family)?;
    Ok(true)
}
pub(super) fn unavailable(code: Code) -> Value {
    json!({"family":"legacy-projects","state":"waiting","keys":["projects","folders"],"reason":code.as_str()})
}
pub(super) fn report(extra: &Map<String, Value>) -> Value {
    let family = match load(extra) {
        Ok(f) => f,
        Err(code) => return unavailable(code),
    };
    let codes: std::collections::BTreeSet<_> = family
        .records
        .values()
        .filter_map(Record::code)
        .map(Code::as_str)
        .collect();
    json!({"family":"legacy-projects","state":if codes.is_empty() {"imported"} else {"waiting"},
        "keys":if family.records.is_empty() {Vec::<&str>::new()} else {vec!["projects","folders"]},
        "reasons":codes})
}

#[cfg(test)]
mod tests;
