//! Bounded installer metadata, never a caller or ownership permission.
use super::*;
use serde::{Deserialize, Serialize};

pub(in crate::desktop) const RECORD_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::desktop) enum Mode {
    Passive,
    Background,
}
impl Mode {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "passive" => Ok(Self::Passive),
            "background" => Ok(Self::Background),
            _ => bail!("Mode must be background or passive."),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Passive => "passive",
            Self::Background => "background",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::desktop) enum Phase {
    Prepared,
    RevokeOffered,
    Held,
    StopOffered,
    Stopped,
    UpdateOffered,
    Updated,
    StartOffered,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct ObservedInstance {
    pub guid: String,
    pub pid: u32,
    pub birth: u64,
    pub epoch: String,
}
impl ObservedInstance {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            uuid::Uuid::parse_str(&self.guid).is_ok_and(|id| !id.is_nil())
                && self.pid > 0
                && self.birth > 0
                && canonical_id(&self.epoch),
            "Invalid observed task identity."
        );
        Ok(())
    }
}

fn canonical_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| !uuid.is_nil() && uuid.to_string() == id)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Pending {
    pub request_id: String,
    pub desired: Mode,
    pub phase: Phase,
    pub original: Option<ObservedInstance>,
    pub started: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Record {
    pub schema: u8,
    pub configuration: String,
    pub sid: String,
    pub command: Vec<String>,
    pub pending: Option<Pending>,
    pub cancelled: Option<String>,
    pub completed: Option<String>,
}
impl Record {
    pub fn empty(plan: &Plan) -> Result<Self> {
        ensure!(
            plan.argv.len() == 7 || (plan.argv.len() == 8 && plan.argv[7] == "--background"),
            "Invalid maintained desktop command."
        );
        Ok(Self {
            schema: 1,
            configuration: plan.configuration.clone(),
            sid: plan.sid.clone(),
            command: plan.argv[..7].to_vec(),
            pending: None,
            cancelled: None,
            completed: None,
        })
    }
    pub fn decode(bytes: &[u8], plan: &Plan) -> Result<Self> {
        ensure!(
            bytes.len() <= RECORD_BYTES,
            "Desktop mode record exceeds its bound."
        );
        let value: Self = serde_json::from_slice(bytes)
            .context("Invalid desktop mode record; inspect it before changing the task.")?;
        let expected = Self::empty(plan)?;
        ensure!(
            value.schema == 1
                && value.configuration == expected.configuration
                && value.sid == expected.sid
                && value.command == expected.command,
            "Desktop mode record selects another registration."
        );
        if let Some(pending) = &value.pending {
            ensure!(
                canonical_id(&pending.request_id),
                "Invalid pending transition UUID."
            );
            if let Some(original) = &pending.original {
                original.validate()?;
            }
            if let Some(started) = &pending.started {
                ensure!(
                    uuid::Uuid::parse_str(started).is_ok_and(|id| !id.is_nil()),
                    "Invalid original started task UUID."
                );
            }
        }
        ensure!(
            value.cancelled.as_deref().is_none_or(canonical_id),
            "Invalid cancelled transition UUID."
        );
        ensure!(
            value.completed.as_deref().is_none_or(canonical_id),
            "Invalid completed transition UUID."
        );
        Ok(value)
    }
    pub fn encode(&self, plan: &Plan) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)?;
        Self::decode(&bytes, plan)?;
        Ok(bytes)
    }
    pub fn begin(&mut self, desired: Mode, original: Option<ObservedInstance>) -> Result<&Pending> {
        ensure!(self.pending.is_none(), "An original transition is pending; inspect it or explicitly cancel its exact UUID before a new choice.");
        if let Some(original) = &original {
            original.validate()?;
        }
        self.pending = Some(Pending {
            request_id: uuid::Uuid::new_v4().to_string(),
            desired,
            phase: Phase::Prepared,
            original,
            started: None,
        });
        Ok(self.pending.as_ref().expect("inserted"))
    }
    pub fn advance(&mut self, expected: Phase, next: Phase) -> Result<()> {
        let pending = self
            .pending
            .as_mut()
            .context("Original transition is absent.")?;
        ensure!(
            pending.phase == expected,
            "Original transition phase changed."
        );
        let allowed = matches!(
            (expected, next),
            (Phase::Prepared, Phase::RevokeOffered | Phase::Held)
                | (Phase::RevokeOffered, Phase::Held)
                | (Phase::Held, Phase::StopOffered | Phase::Stopped)
                | (Phase::StopOffered, Phase::Stopped)
                | (Phase::Stopped, Phase::UpdateOffered)
                | (Phase::UpdateOffered, Phase::Updated)
                | (Phase::Updated, Phase::StartOffered)
                | (Phase::StartOffered, Phase::Ready)
        );
        ensure!(allowed, "A manager effect cannot be replayed or skipped.");
        pending.phase = next;
        Ok(())
    }
    /// Call only after fresh native caller/current-Service-grant checks under
    /// the installer lock. Metadata cancellation has no native side effect.
    pub fn cancel(&mut self, request_id: &str) -> Result<()> {
        let pending = self
            .pending
            .as_ref()
            .context("No original rollback is pending.")?;
        ensure!(
            canonical_id(request_id) && pending.request_id == request_id,
            "Pending rollback UUID changed."
        );
        ensure!(pending.desired == Mode::Passive && matches!(pending.phase,
            Phase::Prepared | Phase::RevokeOffered | Phase::Held),
            "A manager effect was already offered; inspect its unknown result rather than discarding the original identity.");
        self.cancelled = Some(request_id.into());
        self.pending = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plan() -> Plan {
        super::super::tests::plan("check")
    }
    #[test]
    fn explicit_mode_names_are_closed_and_round_trip_without_enabling_a_default() {
        for value in ["passive", "background"] {
            assert_eq!(Mode::parse(value).unwrap().name(), value);
        }
        for value in ["", "auto", "service", "Background"] {
            assert!(Mode::parse(value).is_err());
        }
    }
    #[test]
    fn pending_identity_never_rotates_or_replays_an_offered_manager_phase() {
        let mut record = Record::empty(&plan()).unwrap();
        let original = record
            .begin(Mode::Passive, None)
            .unwrap()
            .request_id
            .clone();
        record
            .advance(Phase::Prepared, Phase::RevokeOffered)
            .unwrap();
        for desired in [Mode::Passive, Mode::Background] {
            assert!(record.begin(desired, None).is_err());
        }
        assert_eq!(record.pending.as_ref().unwrap().request_id, original);
        record.advance(Phase::RevokeOffered, Phase::Held).unwrap();
        record.advance(Phase::Held, Phase::StopOffered).unwrap();
        let bytes = record.encode(&plan()).unwrap();
        let mut restored = Record::decode(&bytes, &plan()).unwrap();
        assert!(restored.advance(Phase::Held, Phase::StopOffered).is_err());
        assert!(restored
            .advance(Phase::StopOffered, Phase::StopOffered)
            .is_err());
        restored
            .advance(Phase::StopOffered, Phase::Stopped)
            .unwrap();
        assert_eq!(restored.pending.unwrap().request_id, original);
    }
    #[test]
    fn only_exact_record_cancellation_allows_a_separately_explicit_new_intent() {
        let mut record = Record::empty(&plan()).unwrap();
        let original = record
            .begin(Mode::Passive, None)
            .unwrap()
            .request_id
            .clone();
        let saved = record.clone();
        assert!(record.cancel(&uuid::Uuid::new_v4().to_string()).is_err());
        assert_eq!(record, saved);
        record.cancel(&original).unwrap();
        assert_eq!(record.cancelled.as_deref(), Some(original.as_str()));
        let next = record
            .begin(Mode::Passive, None)
            .unwrap()
            .request_id
            .clone();
        assert_ne!(next, original);
        assert_eq!(record.command, saved.command);
    }
    #[test]
    fn malformed_foreign_or_unbounded_metadata_never_resets_the_default() {
        let record = Record::empty(&plan()).unwrap();
        let raw = serde_json::to_value(&record).unwrap();
        for (field, value) in [
            ("schema", json!(2)),
            ("configuration", json!("b".repeat(64))),
            ("sid", json!("S-1-5-21-9999")),
            ("command", json!([])),
            ("permission", json!(true)),
        ] {
            let mut other = raw.clone();
            other[field] = value;
            assert!(
                Record::decode(&serde_json::to_vec(&other).unwrap(), &plan()).is_err(),
                "{field}"
            );
        }
        assert!(Record::decode(&vec![b' '; RECORD_BYTES + 1], &plan()).is_err());
        assert!(Record::decode(b"{", &plan()).is_err());
        assert_eq!(
            Record::decode(&record.encode(&plan()).unwrap(), &plan()).unwrap(),
            record
        );
    }
    #[test]
    fn an_unknown_stop_update_or_start_cannot_be_discarded_as_an_unsent_rollback() {
        for phase in [
            Phase::StopOffered,
            Phase::Stopped,
            Phase::UpdateOffered,
            Phase::Updated,
            Phase::StartOffered,
            Phase::Ready,
        ] {
            let mut record = Record::empty(&plan()).unwrap();
            let id = record
                .begin(Mode::Passive, None)
                .unwrap()
                .request_id
                .clone();
            record.pending.as_mut().unwrap().phase = phase;
            let saved = record.clone();
            assert!(record.cancel(&id).is_err());
            assert_eq!(record, saved);
        }
    }
}
