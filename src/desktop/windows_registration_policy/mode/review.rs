//! Bounded original-bound metadata. No type in this module permits a manager
//! effect, process adoption, Grant, revoke, or resource-charge release.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const HISTORY: usize = 8; // The existing 8 KiB record bound also applies.
const CHOICES: usize = 48; // Bytes, including reserved review growth, can refuse sooner.

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Boot {
    pub machine: String,
    pub boot: String,
}
impl Boot {
    pub fn validate(&self) -> Result<()> {
        let counter = self.boot.strip_prefix("windows-loader-v1:");
        let native_counter =
            counter.is_some_and(|s| s.parse::<u32>().is_ok_and(|n| n > 0 && n.to_string() == s));
        ensure!(
            canonical_id(&self.machine) && (canonical_id(&self.boot) || native_counter),
            "Native boot identity unavailable."
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Consumer {
    pub pid: u32,
    pub birth: u64,
    pub boot: Boot,
}
impl Consumer {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.pid > 0 && self.birth > 0,
            "Native consumer identity unavailable."
        );
        self.boot.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::desktop) enum State {
    RebootRequired,
    ReviewedUnknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Disposition {
    pub state: State,
    pub barrier: Boot,
    pub binding: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::desktop) enum Outcome {
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Archive {
    pub original: Pending,
    pub outcome: Outcome,
}

/// Routing only, never a confirmation, native Caller or Grant capability.
pub(in crate::desktop) struct Choice {
    pub reviewed: String,
    pub request: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::desktop) enum ChoicePhase {
    Unknown,
    Settled,
    Refused,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Receipt {
    pub reviewed: String,
    pub request: String,
    pub desired: Mode,
    pub phase: ChoicePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ChoiceScope>,
}

/// Read-only history linkage, never a Grant or original-cleanup witness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct ChoiceScope {
    pub journal: String,
    pub owner_epoch: String,
    pub service_epoch: String,
    pub generation: u64,
}
impl ChoiceScope {
    pub(super) fn valid(&self) -> bool {
        [&self.journal, &self.owner_epoch, &self.service_epoch]
            .iter()
            .all(|s| canonical_id(s))
            && self.generation <= 9_007_199_254_740_991
    }
}

impl Record {
    pub fn choice(&self, choice: Option<&Choice>) -> Result<Option<String>> {
        ensure!(!self.pending_refresh(), "An original refresh is pending; only its original-ID result, pre-manager cancellation or explicit Review is available.");
        if self.reviewed.is_empty() && choice.is_none() {
            // An ordinary frame never adopts or continues a pending FIRST
            // choice, not even before its manager effects: that original is
            // read by its own UUID, cancelled at a closed pre-manager boundary
            // or reviewed.
            ensure!(
                !self.pending_first(),
                "A first mode choice is pending; read its original mode-result, cancel it before any manager effect, or review it by its original UUID. This request cannot continue it."
            );
            return Ok(None);
        }
        let last = self
            .reviewed
            .last()
            .context("No original review selects this choice.")?;
        let choice = choice.context("After review, confirm a NEW mode choice with --after-review UUID --request-id NEW_UUID.")?;
        ensure!(self.choices.len() < CHOICES && !self.choices.iter().any(|c| c.request == choice.request)
            && !self.first_choices.iter().any(|c| c.request == choice.request)
            && !self.refreshes.iter().any(|c| c.request == choice.request),
            "Original choice was already offered or history is full; read its original mode-result, never replay it.");
        ensure!(canonical_id(&choice.request) && choice.reviewed == last.original.request_id
            && self.completed.as_deref() != Some(choice.request.as_str())
            && self.cancelled.as_deref() != Some(choice.request.as_str())
            && !self.reviewed.iter().any(|a| a.original.request_id == choice.request)
            && self.pending.is_none(),
            "The original review or NEW request identity changed; inspect the original without replay.");
        Ok(Some(choice.request.clone()))
    }

    pub fn reserve_choice(&mut self, choice: &Choice, desired: Mode) -> Result<()> {
        self.choice(Some(choice))?;
        self.ensure_review_slot()?;
        let mut next = self.clone();
        next.choices.push(Receipt {
            reviewed: choice.reviewed.clone(),
            request: choice.request.clone(),
            desired,
            phase: ChoicePhase::Unknown,
            scope: None,
        });
        next.ensure_review_capacity(&Pending::prepared(choice.request.clone(), desired, None))?;
        self.choices = next.choices;
        Ok(())
    }

    pub(super) fn choice_phase(&mut self, request: &str, phase: ChoicePhase) -> Result<()> {
        if let Some(receipt) = self.choices.iter_mut().find(|c| c.request == request) {
            ensure!(
                receipt.phase == ChoicePhase::Unknown || receipt.phase == phase,
                "Original choice already has a terminal receipt."
            );
            receipt.phase = phase;
        }
        if let Some(receipt) = self.first_choices.iter_mut().find(|c| c.request == request) {
            ensure!(
                receipt.phase == ChoicePhase::Unknown || receipt.phase == phase,
                "Original first choice already has a terminal receipt."
            );
            receipt.phase = phase;
        }
        if let Some(receipt) = self.refreshes.iter_mut().find(|c| c.request == request) {
            ensure!(phase == ChoicePhase::Refused && receipt.phase == ChoicePhase::Unknown,
                "Refresh settlement requires its own original-bound readiness.");
            receipt.phase = phase;
        }
        Ok(())
    }

    pub fn complete(&mut self, request: &str) -> Result<()> {
        ensure!(!self.pending_refresh(), "A mode completion cannot settle an original refresh.");
        ensure!(
            self.pending
                .as_ref()
                .is_none_or(|p| p.request_id == request),
            "Original completion UUID changed."
        );
        self.choice_phase(request, ChoicePhase::Settled)?;
        self.completed = Some(request.into());
        self.pending = None;
        Ok(())
    }
    pub fn complete_in_scope(&mut self, request: &str, scope: ChoiceScope) -> Result<()> {
        ensure!(
            scope.valid(),
            "Current background choice scope unavailable."
        );
        if let Some(receipt) = self.choices.iter_mut().find(|c| c.request == request) {
            ensure!(
                receipt.desired == Mode::Background && receipt.phase == ChoicePhase::Unknown,
                "Current background choice identity changed."
            );
            receipt.scope = Some(scope.clone());
        }
        if let Some(receipt) = self.first_choices.iter_mut().find(|c| c.request == request) {
            ensure!(
                receipt.desired == Mode::Background && receipt.phase == ChoicePhase::Unknown,
                "Current background choice identity changed."
            );
            receipt.scope = Some(scope);
        }
        self.complete(request)
    }
    pub(super) fn ensure_review_slot(&self) -> Result<()> {
        ensure!(
            self.reviewed.len() < HISTORY,
            "Original review history is full; retained identities cannot be discarded."
        );
        Ok(())
    }

    /// Reserve bytes BEFORE admission, not after an uncertain effect. The
    /// native transition may still capture consumer/start identities and reach
    /// any offered phase before interruption. Size-only models below use the
    /// largest accepted UUID, integer and phase encodings, never evidence or a
    /// native permission. Both the barrier and immutable archive must fit.
    pub(super) fn review_capacity(&self, pending: &Pending) -> Result<usize> {
        let uuid = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        let identity = Boot {
            machine: uuid.into(),
            boot: uuid.into(),
        };
        let mut largest = pending.clone();
        largest.desired = Mode::Background;
        largest.phase = Phase::RevokeOffered;
        largest.original = Some(ObservedInstance {
            guid: format!("urn:uuid:{uuid}"),
            pid: u32::MAX,
            birth: u64::MAX,
            epoch: uuid.into(),
        });
        largest.started = Some(format!("urn:uuid:{uuid}"));
        largest.consumer = Some(Consumer {
            pid: u32::MAX,
            birth: u64::MAX,
            boot: identity.clone(),
        });
        largest.review = Some(Disposition {
            state: State::RebootRequired,
            barrier: identity,
            binding: "f".repeat(64),
        });
        let mut model = self.clone();
        model.pending = Some(largest.clone());
        let barrier = serde_json::to_vec(&model)?.len();
        largest.review.as_mut().expect("size model").state = State::ReviewedUnknown;
        model.pending = None;
        model.reviewed.push(Archive {
            original: largest,
            outcome: Outcome::Unknown,
        });
        let reviewed = serde_json::to_vec(&model)?.len();
        // Completion removes pending intent but may append a fixed background
        // scope. Reserve that reachable layout too, before any manager effect.
        model.reviewed = self.reviewed.clone();
        model.completed = Some(pending.request_id.clone());
        let largest_scope = ChoiceScope {
            journal: uuid.into(),
            owner_epoch: uuid.into(),
            service_epoch: uuid.into(),
            generation: u64::MAX,
        };
        if let Some(receipt) = model
            .choices
            .iter_mut()
            .find(|c| c.request == pending.request_id && c.desired == Mode::Background)
        {
            receipt.phase = ChoicePhase::Settled;
            receipt.scope = Some(largest_scope.clone());
        }
        if let Some(receipt) = model
            .first_choices
            .iter_mut()
            .find(|c| c.request == pending.request_id && c.desired == Mode::Background)
        {
            receipt.phase = ChoicePhase::Settled;
            receipt.scope = Some(largest_scope);
        }
        // A refresh keeps its own original scope and grows only its exact new
        // readiness receipt at completion. This is byte reservation, not native
        // evidence; model every field that can grow before a terminal save.
        if let Some(receipt) = model.refreshes.iter_mut().find(|r| r.request == pending.request_id) {
            receipt.phase = ChoicePhase::Settled;
            receipt.ready = Some(refresh::Ready {
                instance: ObservedInstance { guid: format!("urn:uuid:{uuid}"), pid: u32::MAX,
                    birth: u64::MAX, epoch: uuid.into() }, generation: u64::MAX });
            model.cancelled = Some(uuid.into());
        }
        Ok(barrier.max(reviewed).max(serde_json::to_vec(&model)?.len()))
    }
    pub(super) fn ensure_review_capacity(&self, pending: &Pending) -> Result<()> {
        ensure!(self.review_capacity(pending)? <= RECORD_BYTES,
            "Desktop mode history has no byte headroom for original review; no new intent was admitted. Keep the record and current mode; inspect status or original mode-result. No maintained history rollover is available.");
        Ok(())
    }
    fn binding(&self, pending: &Pending) -> Result<String> {
        let mut original = pending.clone();
        original.review = None;
        // Versioned tuple encoding avoids map-order/feature-dependent hashes.
        let bytes =
            serde_json::to_vec(&(1u8, &self.configuration, &self.sid, &self.command, original))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    pub(super) fn check_reviews(&self) -> Result<()> {
        let mut choices = BTreeSet::new();
        ensure!(
            self.choices.len() <= CHOICES
                && self.choices.iter().all(|c| canonical_id(&c.request)
                    && c.scope.as_ref().is_none_or(|s| s.valid()
                        && c.desired == Mode::Background
                        && c.phase == ChoicePhase::Settled)
                    && canonical_id(&c.reviewed)
                    && choices.insert(&c.request)
                    && self
                        .reviewed
                        .iter()
                        .any(|a| a.original.request_id == c.reviewed)
                    && c.request != c.reviewed
                    && if c.phase == ChoicePhase::Unknown {
                        self.pending
                            .iter()
                            .chain(self.reviewed.iter().map(|a| &a.original))
                            .any(|p| p.request_id == c.request && p.desired == c.desired)
                    } else {
                        self.pending
                            .as_ref()
                            .is_none_or(|p| p.request_id != c.request)
                            && !self
                                .reviewed
                                .iter()
                                .any(|a| a.original.request_id == c.request)
                    }),
            "Original choice history is unavailable."
        );
        ensure!(
            self.reviewed.len() <= HISTORY,
            "Original review history is full; retained identities cannot be discarded."
        );
        let mut ids = BTreeSet::new();
        for pending in self
            .pending
            .iter()
            .chain(self.reviewed.iter().map(|a| &a.original))
        {
            ensure!(
                ids.insert(&pending.request_id) && canonical_id(&pending.request_id),
                "Original review identity changed."
            );
            if let Some(original) = &pending.original {
                original.validate()?;
            }
            if let Some(started) = &pending.started {
                ensure!(
                    uuid::Uuid::parse_str(started).is_ok_and(|id| !id.is_nil()),
                    "Original started identity changed."
                );
            }
            if let Some(consumer) = &pending.consumer {
                consumer.validate()?;
            }
            if let Some(review) = &pending.review {
                review.barrier.validate()?;
                ensure!(
                    review.binding == self.binding(pending)?
                        && pending
                            .consumer
                            .as_ref()
                            .is_none_or(|c| c.boot.machine == review.barrier.machine),
                    "Original review binding changed."
                );
            }
        }
        ensure!(
            self.pending.as_ref().is_none_or(|p| p
                .review
                .as_ref()
                .is_none_or(|r| r.state == State::RebootRequired))
                && self.reviewed.iter().all(|a| a
                    .original
                    .review
                    .as_ref()
                    .is_some_and(|r| r.state == State::ReviewedUnknown)),
            "Original review disposition is inconsistent."
        );
        Ok(())
    }

    /// The adapter supplies only fresh native observation, never a JSON
    /// permission. `legacy_stop_gone` additionally requires the exact old
    /// PID/birth physically gone AND GUID absent under its unchanged owned
    /// definition and registration lock. It is not a service-drain proof.
    pub fn review_original(
        &mut self,
        request: &str,
        current: &Boot,
        legacy_stop_gone: bool,
    ) -> Result<State> {
        current.validate()?;
        self.check_reviews()?;
        if let Some(archived) = self
            .reviewed
            .iter()
            .find(|a| a.original.request_id == request)
        {
            ensure!(
                archived
                    .original
                    .review
                    .as_ref()
                    .is_some_and(|r| r.barrier.machine == current.machine),
                "Original machine changed."
            );
            ensure!(
                self.pending.is_none(),
                "Another original transition is pending; review its exact UUID."
            );
            return Ok(State::ReviewedUnknown); // Original-ID metadata lookup only.
        }
        let original = self
            .pending
            .as_ref()
            .context("Original transition is absent.")?;
        ensure!(
            canonical_id(request) && original.request_id == request,
            "Original transition UUID changed."
        );
        ensure!(
            original
                .consumer
                .as_ref()
                .is_none_or(|c| c.boot.machine == current.machine)
                && original
                    .review
                    .as_ref()
                    .is_none_or(|r| r.barrier.machine == current.machine),
            "Original machine changed."
        );
        let boot_changed = original
            .consumer
            .as_ref()
            .is_some_and(|c| c.boot.boot != current.boot)
            || original
                .review
                .as_ref()
                .is_some_and(|r| r.barrier.boot != current.boot);
        let legacy_gone = original.consumer.is_none()
            && original.phase == Phase::StopOffered
            && original.original.is_some()
            && original.started.is_none()
            && legacy_stop_gone;
        let state = if boot_changed || legacy_gone {
            State::ReviewedUnknown
        } else {
            State::RebootRequired
        };
        if state == State::RebootRequired {
            ensure!(
                original.review.is_none(),
                "Restart the computer, then explicitly Review interrupted mode change again."
            );
        } else {
            ensure!(
                self.reviewed.len() < HISTORY,
                "Original review history is full; retained identities cannot be discarded."
            );
        }
        let binding = self.binding(original)?;
        let barrier = original
            .review
            .as_ref()
            .map(|r| r.barrier.clone())
            .or_else(|| original.consumer.as_ref().map(|c| c.boot.clone()))
            .unwrap_or_else(|| current.clone());
        self.pending.as_mut().expect("original").review = Some(Disposition {
            state,
            barrier,
            binding,
        });
        if state == State::ReviewedUnknown {
            self.reviewed.push(Archive {
                original: self.pending.take().expect("original"),
                outcome: Outcome::Unknown,
            });
        }
        self.check_reviews()?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn boot() -> Boot {
        Boot {
            machine: "11111111-1111-4111-8111-111111111111".into(),
            boot: "22222222-2222-4222-8222-222222222222".into(),
        }
    }

    #[test]
    fn a_background_choice_keeps_its_original_review_and_current_incarnation() {
        let (mut record, reviewed) = original(Phase::StopOffered);
        record.review_original(&reviewed, &boot(), true).unwrap();
        let request = uuid::Uuid::new_v4().to_string();
        record
            .reserve_choice(
                &Choice {
                    reviewed: reviewed.clone(),
                    request: request.clone(),
                },
                Mode::Background,
            )
            .unwrap();
        let scope = ChoiceScope {
            journal: uuid::Uuid::new_v4().to_string(),
            owner_epoch: uuid::Uuid::new_v4().to_string(),
            service_epoch: uuid::Uuid::new_v4().to_string(),
            generation: 9_007_199_254_740_991,
        };
        record.complete_in_scope(&request, scope.clone()).unwrap();
        let reopened = Record::decode(&record.encode(&plan()).unwrap(), &plan()).unwrap();
        let receipt = reopened.choices.last().unwrap();
        assert_eq!(receipt.reviewed, reviewed);
        assert_eq!(receipt.request, request);
        assert_eq!(receipt.phase, ChoicePhase::Settled);
        assert_eq!(receipt.scope, Some(scope));
        let mut wrong = record.clone();
        wrong.choices.last_mut().unwrap().desired = Mode::Passive;
        assert!(wrong.encode(&plan()).is_err());
    }

    #[test]
    fn native_loader_sequence_is_closed_and_never_a_date_or_registry_age() {
        for value in ["windows-loader-v1:1", "windows-loader-v1:4294967295"] {
            Boot {
                boot: value.into(),
                ..boot()
            }
            .validate()
            .unwrap();
        }
        for value in [
            "windows-loader-v1:0",
            "windows-loader-v1:01",
            "windows-loader-v1:-1",
            "windows-loader-v1:4294967296",
            "2026-11-04",
            "",
            "windows-loader-v1:1\0",
        ] {
            assert!(Boot {
                boot: value.into(),
                ..boot()
            }
            .validate()
            .is_err());
        }
    }
    fn next_boot() -> Boot {
        Boot {
            boot: "33333333-3333-4333-8333-333333333333".into(),
            ..boot()
        }
    }
    fn plan() -> Plan {
        super::super::super::tests::plan("check")
    }
    fn instance() -> ObservedInstance {
        ObservedInstance {
            guid: uuid::Uuid::new_v4().to_string(),
            pid: 23,
            birth: 42,
            epoch: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn original(phase: Phase) -> (Record, String) {
        let mut record = Record::empty(&plan()).unwrap();
        let id = record
            .begin(Mode::Background, Some(instance()))
            .unwrap()
            .request_id
            .clone();
        record.pending.as_mut().unwrap().phase = phase;
        (record, id)
    }
    fn immutable(pending: &Pending) -> Vec<u8> {
        let mut original = pending.clone();
        original.review = None;
        serde_json::to_vec(&original).unwrap()
    }

    #[test]
    fn every_ambiguous_legacy_phase_needs_the_original_barrier_and_a_different_boot() {
        for phase in [
            Phase::Prepared,
            Phase::RevokeOffered,
            Phase::Held,
            Phase::StopOffered,
            Phase::Stopped,
            Phase::UpdateOffered,
            Phase::Updated,
            Phase::StartOffered,
            Phase::Ready,
        ] {
            let (mut record, id) = original(phase);
            let before = immutable(record.pending.as_ref().unwrap());
            assert_eq!(
                record.review_original(&id, &boot(), false).unwrap(),
                State::RebootRequired
            );
            assert_eq!(immutable(record.pending.as_ref().unwrap()), before);
            let barrier = record.encode(&plan()).unwrap();
            assert!(record.review_original(&id, &boot(), false).is_err());
            assert_eq!(record.encode(&plan()).unwrap(), barrier);
            let mut reopened = Record::decode(&barrier, &plan()).unwrap();
            assert_eq!(
                reopened.review_original(&id, &next_boot(), false).unwrap(),
                State::ReviewedUnknown
            );
            assert!(reopened.pending.is_none());
            assert_eq!(immutable(&reopened.reviewed[0].original), before);
            assert_eq!(reopened.reviewed[0].outcome, Outcome::Unknown);
            assert_eq!(
                Record::decode(&reopened.encode(&plan()).unwrap(), &plan()).unwrap(),
                reopened
            );
        }
    }

    #[test]
    fn a_captured_consumer_never_uses_same_boot_death_as_cleanup_proof() {
        let (mut record, id) = original(Phase::StopOffered);
        record.pending.as_mut().unwrap().consumer = Some(Consumer {
            pid: 1,
            birth: 2,
            boot: boot(),
        });
        assert_eq!(
            record.review_original(&id, &boot(), true).unwrap(),
            State::RebootRequired
        );
        assert!(record.cancel(&id).is_err());
        assert!(record.advance(Phase::StopOffered, Phase::Stopped).is_err());
        assert_eq!(
            record.review_original(&id, &next_boot(), false).unwrap(),
            State::ReviewedUnknown
        );
        assert!(record.pending.is_none());
    }

    #[test]
    fn only_the_narrow_legacy_stop_accepts_native_process_and_guid_absence() {
        let (mut record, id) = original(Phase::StopOffered);
        assert_eq!(
            record.review_original(&id, &boot(), true).unwrap(),
            State::ReviewedUnknown
        );
        assert_eq!(record.reviewed[0].original.phase, Phase::StopOffered);
        assert_eq!(record.reviewed[0].outcome, Outcome::Unknown);
        for phase in [
            Phase::Prepared,
            Phase::Stopped,
            Phase::UpdateOffered,
            Phase::StartOffered,
            Phase::Ready,
        ] {
            let (mut record, id) = original(phase);
            assert_eq!(
                record.review_original(&id, &boot(), true).unwrap(),
                State::RebootRequired
            );
        }
        let (mut record, id) = original(Phase::StopOffered);
        record.pending.as_mut().unwrap().started = Some(uuid::Uuid::new_v4().to_string());
        assert_eq!(
            record.review_original(&id, &boot(), true).unwrap(),
            State::RebootRequired
        );
    }

    #[test]
    fn missing_boot_changed_machine_uuid_and_replaced_binding_refuse_without_writes() {
        let (mut record, id) = original(Phase::StopOffered);
        record.review_original(&id, &boot(), false).unwrap();
        let before = record.clone();
        for native in [
            Boot {
                boot: String::new(),
                ..boot()
            },
            Boot {
                machine: uuid::Uuid::new_v4().to_string(),
                ..next_boot()
            },
        ] {
            assert!(record.review_original(&id, &native, false).is_err());
            assert_eq!(record, before);
        }
        assert!(record
            .review_original(&uuid::Uuid::new_v4().to_string(), &next_boot(), false)
            .is_err());
        assert_eq!(record, before);
        record
            .pending
            .as_mut()
            .unwrap()
            .original
            .as_mut()
            .unwrap()
            .birth += 1;
        let replaced = record.clone();
        assert!(record.review_original(&id, &next_boot(), false).is_err());
        assert_eq!(record, replaced);
        assert!(Record::decode(&serde_json::to_vec(&record).unwrap(), &plan()).is_err());
    }

    #[test]
    fn a_missing_barrier_cannot_invent_an_unrecorded_boot_order() {
        let (mut record, id) = original(Phase::UpdateOffered);
        record.review_original(&id, &boot(), false).unwrap();
        record.pending.as_mut().unwrap().review = None;
        assert_eq!(
            record.review_original(&id, &next_boot(), false).unwrap(),
            State::RebootRequired
        );
        assert_eq!(
            record
                .pending
                .as_ref()
                .unwrap()
                .review
                .as_ref()
                .unwrap()
                .barrier,
            next_boot()
        );
    }

    #[test]
    fn the_barrier_binds_every_part_of_the_original_scope_not_a_display_subset() {
        let (mut record, id) = original(Phase::StopOffered);
        record.review_original(&id, &boot(), false).unwrap();
        let mut changed = Vec::new();
        let mut copy = record.clone();
        copy.configuration.push('a');
        changed.push(copy);
        let mut copy = record.clone();
        copy.sid.push_str("-1");
        changed.push(copy);
        let mut copy = record.clone();
        copy.command[0].push('x');
        changed.push(copy);
        let mut copy = record.clone();
        copy.pending.as_mut().unwrap().desired = Mode::Passive;
        changed.push(copy);
        let mut copy = record.clone();
        copy.pending.as_mut().unwrap().phase = Phase::Stopped;
        changed.push(copy);
        let mut copy = record.clone();
        copy.pending
            .as_mut()
            .unwrap()
            .original
            .as_mut()
            .unwrap()
            .pid += 1;
        changed.push(copy);
        let mut copy = record.clone();
        copy.pending
            .as_mut()
            .unwrap()
            .original
            .as_mut()
            .unwrap()
            .guid = uuid::Uuid::new_v4().to_string();
        changed.push(copy);
        let mut copy = record.clone();
        copy.pending
            .as_mut()
            .unwrap()
            .original
            .as_mut()
            .unwrap()
            .epoch = uuid::Uuid::new_v4().to_string();
        changed.push(copy);
        let mut copy = record.clone();
        copy.pending.as_mut().unwrap().started = Some(uuid::Uuid::new_v4().to_string());
        changed.push(copy);
        for (index, mut copy) in changed.into_iter().enumerate() {
            let before = copy.clone();
            assert!(
                copy.review_original(&id, &next_boot(), false).is_err(),
                "changed original field {index}"
            );
            assert_eq!(copy, before);
        }
    }

    #[test]
    fn immutable_unknown_history_is_bounded_and_never_adopts_a_new_request() {
        let (mut record, id) = original(Phase::StopOffered);
        record.review_original(&id, &boot(), true).unwrap();
        let archive = record.reviewed[0].clone();
        let next = record
            .begin(Mode::Passive, None)
            .unwrap()
            .request_id
            .clone();
        assert_ne!(id, next);
        let before = record.clone();
        assert!(record.review_original(&id, &boot(), false).is_err());
        assert_eq!(record, before);
        assert_eq!(record.reviewed[0], archive);
        record.cancel(&next).unwrap();
        for _ in 1..HISTORY {
            let id = record
                .begin(Mode::Background, Some(instance()))
                .unwrap()
                .request_id
                .clone();
            record.pending.as_mut().unwrap().phase = Phase::StopOffered;
            record.review_original(&id, &boot(), true).unwrap();
        }
        let before = record.encode(&plan()).unwrap();
        assert!(record.begin(Mode::Passive, None).is_err());
        assert_eq!(record.encode(&plan()).unwrap(), before);
    }

    #[test]
    fn review_never_reopens_the_old_automatic_switch_lane_or_rotates_an_unknown_id() {
        let (mut record, id) = original(Phase::StopOffered);
        record.review_original(&id, &boot(), true).unwrap();
        let before = record.clone();
        assert!(record.choice(None).is_err());
        for choice in [
            Choice {
                reviewed: id.clone(),
                request: id.clone(),
            },
            Choice {
                reviewed: uuid::Uuid::new_v4().to_string(),
                request: uuid::Uuid::new_v4().to_string(),
            },
        ] {
            assert!(record.choice(Some(&choice)).is_err());
            assert_eq!(record, before);
        }
        let choice = Choice {
            reviewed: id,
            request: uuid::Uuid::new_v4().to_string(),
        };
        assert_eq!(
            record.choice(Some(&choice)).unwrap(),
            Some(choice.request.clone())
        );
        record.reserve_choice(&choice, Mode::Passive).unwrap();
        record
            .begin_id(Mode::Passive, None, choice.request.clone())
            .unwrap();
        assert!(record.choice(Some(&choice)).is_err());
        let rotated = Choice {
            request: uuid::Uuid::new_v4().to_string(),
            ..choice
        };
        assert!(record.choice(Some(&rotated)).is_err());
    }

    fn reviewed() -> (Record, String) {
        let (mut record, id) = original(Phase::StopOffered);
        record.review_original(&id, &boot(), true).unwrap();
        (record, id)
    }

    #[test]
    fn settled_noop_and_cancelled_choice_ids_survive_later_choices_and_restart() {
        let (mut record, id) = reviewed();
        let mut offered = Vec::new();
        for (desired, noop, cancelled) in [
            (Mode::Passive, true, false),
            (Mode::Background, false, false),
            (Mode::Passive, false, true),
        ] {
            let choice = Choice {
                reviewed: id.clone(),
                request: uuid::Uuid::new_v4().to_string(),
            };
            record.reserve_choice(&choice, desired).unwrap();
            if !noop {
                record
                    .begin_id(desired, None, choice.request.clone())
                    .unwrap();
            }
            if cancelled {
                record.cancel(&choice.request).unwrap();
            } else {
                record.complete(&choice.request).unwrap();
            }
            offered.push(choice);
            record = Record::decode(&record.encode(&plan()).unwrap(), &plan()).unwrap();
            for original in &offered {
                let before = record.clone();
                assert!(
                    record.choice(Some(original)).is_err(),
                    "an old request must never execute again"
                );
                assert_eq!(record, before);
            }
        }
        assert_eq!(
            record.choices.iter().map(|c| c.phase).collect::<Vec<_>>(),
            [
                ChoicePhase::Settled,
                ChoicePhase::Settled,
                ChoicePhase::Refused
            ]
        );
        assert!(record.pending.is_none());
    }

    #[test]
    fn an_unknown_choice_stays_unknown_after_review_and_cannot_cancel_away_custody() {
        let (mut record, id) = reviewed();
        let choice = Choice {
            reviewed: id,
            request: uuid::Uuid::new_v4().to_string(),
        };
        record.reserve_choice(&choice, Mode::Background).unwrap();
        record
            .begin_id(Mode::Background, Some(instance()), choice.request.clone())
            .unwrap();
        let pending = record.pending.as_mut().unwrap();
        pending.consumer = Some(Consumer {
            pid: 1,
            birth: 2,
            boot: boot(),
        });
        pending.phase = Phase::Ready;
        pending.started = Some(uuid::Uuid::new_v4().to_string());
        let before = record.clone();
        assert!(record.ready_cancellation(&choice.request).is_err());
        assert_eq!(record, before);
        record
            .review_original(&choice.request, &boot(), false)
            .unwrap();
        record
            .review_original(&choice.request, &next_boot(), false)
            .unwrap();
        let reopened = Record::decode(&record.encode(&plan()).unwrap(), &plan()).unwrap();
        assert_eq!(reopened.choices[0].phase, ChoicePhase::Unknown);
        assert_eq!(reopened.reviewed[1].original.request_id, choice.request);
        assert!(reopened.choice(Some(&choice)).is_err());
        let mut corrupt = reopened.clone();
        corrupt.choices[0].desired = Mode::Passive;
        assert!(Record::decode(&serde_json::to_vec(&corrupt).unwrap(), &plan()).is_err());
    }

    #[test]
    fn choice_receipts_are_bounded_and_never_evicted_to_make_an_old_id_new() {
        let (mut record, id) = reviewed();
        for _ in 0..CHOICES {
            let choice = Choice {
                reviewed: id.clone(),
                request: uuid::Uuid::new_v4().to_string(),
            };
            if record.reserve_choice(&choice, Mode::Passive).is_err() {
                break;
            }
            record.complete(&choice.request).unwrap();
        }
        let before = record.clone();
        assert!(record
            .reserve_choice(
                &Choice {
                    reviewed: id,
                    request: uuid::Uuid::new_v4().to_string()
                },
                Mode::Passive
            )
            .is_err());
        assert_eq!(record, before);
        assert!(!record.choices.is_empty() && record.choices.len() <= CHOICES);
        record.encode(&plan()).unwrap();
    }

    fn grow_command(record: &mut Record, selected: &mut Plan, bytes: usize) {
        // Keep a normal hgs.exe leaf and sub-255-character components. Each
        // separator costs two JSON bytes; `bytes` is encoded record growth.
        let mut remaining = bytes;
        let mut padding = String::new();
        while remaining > 200 {
            padding.push_str(&"p".repeat(199));
            padding.push('\\');
            remaining -= 201;
        }
        padding.push_str(&"p".repeat(remaining));
        selected.argv[0].insert_str(3, &padding);
        record.command = selected.argv[..7].to_vec();
        // This models a registration originally selected at the longer path,
        // not migration or a runtime rewrite of a retained original binding.
        for index in 0..record.reviewed.len() {
            let binding = record.binding(&record.reviewed[index].original).unwrap();
            record.reviewed[index]
                .original
                .review
                .as_mut()
                .unwrap()
                .binding = binding;
        }
    }

    #[test]
    fn a_pending_record_that_fits_without_review_is_refused_before_admission() {
        let (mut record, reviewed) = reviewed();
        let mut selected = plan();
        let id = uuid::Uuid::new_v4().to_string();
        let mut pending = Pending::prepared(id.clone(), Mode::Background, Some(instance()));
        pending.phase = Phase::StopOffered;
        pending.consumer = Some(Consumer {
            pid: 1,
            birth: 2,
            boot: boot(),
        });
        let mut legacy_probe = record.clone();
        legacy_probe.pending = Some(pending.clone());
        let padding = 8045 - legacy_probe.encode(&selected).unwrap().len();
        grow_command(&mut record, &mut selected, padding);
        legacy_probe = record.clone();
        legacy_probe.pending = Some(pending);
        assert_eq!(legacy_probe.encode(&selected).unwrap().len(), 8045);
        for native in [boot(), next_boot()] {
            let mut unreserved = legacy_probe.clone();
            unreserved.review_original(&id, &native, false).unwrap();
            assert!(
                unreserved.encode(&selected).is_err(),
                "old admission would strand this review"
            );
        }
        let before = record.clone();
        let error = record
            .begin_id(Mode::Background, Some(instance()), id.clone())
            .unwrap_err();
        assert!(error.to_string().contains("byte headroom"), "{error:#}");
        assert_eq!(record, before);
        let error = record
            .reserve_choice(
                &Choice {
                    reviewed,
                    request: id,
                },
                Mode::Background,
            )
            .unwrap_err();
        assert!(error.to_string().contains("byte headroom"), "{error:#}");
        assert_eq!(record, before);
        record.encode(&selected).unwrap();
    }

    #[test]
    fn the_last_admitted_byte_leaves_room_for_both_barrier_and_archive_after_every_phase() {
        let (mut record, reviewed) = reviewed();
        let choice = Choice {
            reviewed,
            request: uuid::Uuid::new_v4().to_string(),
        };
        record.reserve_choice(&choice, Mode::Background).unwrap();
        let pending = Pending::prepared(choice.request.clone(), Mode::Background, None);
        let mut selected = plan();
        let padding = RECORD_BYTES - record.review_capacity(&pending).unwrap();
        grow_command(&mut record, &mut selected, padding);
        assert_eq!(record.review_capacity(&pending).unwrap(), RECORD_BYTES);
        let mut unoffered = record.clone();
        unoffered.choices.pop();
        unoffered.reserve_choice(&choice, Mode::Background).unwrap();
        assert_eq!(
            unoffered, record,
            "receipt admission reserves the same final byte"
        );
        let mut over = record.clone();
        let mut over_plan = selected.clone();
        grow_command(&mut over, &mut over_plan, 1);
        let before = over.clone();
        assert!(over
            .begin_id(Mode::Background, None, choice.request.clone())
            .is_err());
        assert_eq!(over, before);
        record
            .begin_id(Mode::Background, None, choice.request.clone())
            .unwrap();
        let uuid = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        for phase in [
            Phase::Prepared,
            Phase::RevokeOffered,
            Phase::Held,
            Phase::StopOffered,
            Phase::Stopped,
            Phase::UpdateOffered,
            Phase::Updated,
            Phase::StartOffered,
            Phase::Ready,
        ] {
            let mut attempt = record.clone();
            let original = attempt.pending.as_mut().unwrap();
            original.phase = phase;
            original.original = Some(ObservedInstance {
                guid: format!("urn:uuid:{uuid}"),
                pid: u32::MAX,
                birth: u64::MAX,
                epoch: uuid.into(),
            });
            original.consumer = Some(Consumer {
                pid: u32::MAX,
                birth: u64::MAX,
                boot: Boot {
                    machine: uuid.into(),
                    boot: uuid.into(),
                },
            });
            if matches!(phase, Phase::StartOffered | Phase::Ready) {
                original.started = Some(format!("urn:uuid:{uuid}"));
            }
            let same_boot = original.consumer.as_ref().unwrap().boot.clone();
            attempt.encode(&selected).unwrap();
            assert_eq!(
                attempt
                    .review_original(&choice.request, &same_boot, false)
                    .unwrap(),
                State::RebootRequired
            );
            let barrier = attempt.encode(&selected).unwrap();
            let mut reopened = Record::decode(&barrier, &selected).unwrap();
            assert_eq!(
                reopened
                    .review_original(
                        &choice.request,
                        &Boot {
                            boot: boot().boot,
                            ..same_boot
                        },
                        false
                    )
                    .unwrap(),
                State::ReviewedUnknown
            );
            let archive = reopened.encode(&selected).unwrap();
            assert!(barrier.len() <= RECORD_BYTES && archive.len() <= RECORD_BYTES);
            assert_eq!(reopened.reviewed.last().unwrap().outcome, Outcome::Unknown);
        }
    }
}
