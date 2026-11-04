//! A FIRST explicit background choice from a plain passive registration
//! (zerus-yfbd.1). History and routing only: never a Caller, Grant or manager
//! permit, and never a reviewed choice. It exists only while no original was
//! interrupted, reviewed or archived; such history always takes its review
//! route instead, whatever a caller asks for.
use super::*;
use review::{ChoicePhase, ChoiceScope};
use std::collections::BTreeSet;

const FIRST: usize = 48; // Bytes, including reserved review growth, can refuse sooner.

/// Routing only: the caller's NEW original UUID. Never consent or authority.
pub(in crate::desktop) struct First {
    pub request: String,
}

/// The durable original-ID result of one first choice. Distinct from a
/// reviewed choice receipt: it names no reviewed original.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Receipt {
    pub request: String,
    pub desired: Mode,
    pub phase: ChoicePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<ChoiceScope>,
}

impl Record {
    /// Under the owned registration lock, before any manager effect.
    pub fn first_choice(&self, first: &First) -> Result<String> {
        ensure!(
            self.reviewed.is_empty() && self.choices.is_empty() && self.pending.is_none(),
            "A first mode choice is unavailable: an original change is pending, interrupted or reviewed. Review it by its original UUID instead."
        );
        ensure!(
            canonical_id(&first.request)
                && self.first_choices.len() < FIRST
                && !self.first_choices.iter().any(|r| r.request == first.request)
                && self.completed.as_deref() != Some(first.request.as_str())
                && self.cancelled.as_deref() != Some(first.request.as_str()),
            "This first choice was already offered or its history is full; read its original mode-result, never replay it."
        );
        Ok(first.request.clone())
    }

    /// Durably reserve the caller's UUID with byte headroom for its later
    /// completion scope, review barrier and archive, before admission.
    pub fn reserve_first(&mut self, first: &First) -> Result<()> {
        self.first_choice(first)?;
        self.ensure_review_slot()?;
        let mut next = self.clone();
        next.first_choices.push(Receipt {
            request: first.request.clone(),
            desired: Mode::Background,
            phase: ChoicePhase::Unknown,
            scope: None,
        });
        next.ensure_review_capacity(&Pending::prepared(
            first.request.clone(),
            Mode::Background,
            None,
        ))?;
        self.first_choices = next.first_choices;
        Ok(())
    }

    /// The record against its independent first-choice anchor (the latest
    /// reserved first choice, kept outside the desktop state folder). Returns
    /// the lost original ID when either side has a first choice the other
    /// does not show: a missing, emptied or replaced journal, a wiped state
    /// folder, a missing anchor, or a different latest choice. Lost is never
    /// treated as never written.
    pub fn first_lost(&self, anchor: Option<&str>) -> Option<String> {
        let latest = self.first_choices.last().map(|r| r.request.as_str());
        match (anchor, latest) {
            (None, None) => None,
            (Some(anchor), Some(latest)) if anchor == latest => None,
            (Some(anchor), _) => Some(anchor.into()),
            (None, Some(latest)) => Some(latest.into()),
        }
    }

    /// The pending original is a first choice: only its own UUID reads it.
    pub(super) fn pending_first(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|p| self.first_choices.iter().any(|r| r.request == p.request_id))
    }

    pub(super) fn check_first(&self) -> Result<()> {
        let mut ids = BTreeSet::new();
        ensure!(
            self.first_choices.len() <= FIRST
                && self.first_choices.iter().all(|r| canonical_id(&r.request)
                    && ids.insert(&r.request)
                    && r.desired == Mode::Background
                    && !self.choices.iter().any(|c| c.request == r.request)
                    && r.scope
                        .as_ref()
                        .is_none_or(|s| s.valid() && r.phase == ChoicePhase::Settled)
                    && if r.phase == ChoicePhase::Unknown {
                        // Its original is pending, or was interrupted and is
                        // now held by the ordinary review history.
                        self.pending
                            .iter()
                            .chain(self.reviewed.iter().map(|a| &a.original))
                            .any(|p| p.request_id == r.request && p.desired == Mode::Background)
                    } else {
                        self.pending
                            .as_ref()
                            .is_none_or(|p| p.request_id != r.request)
                            && !self
                                .reviewed
                                .iter()
                                .any(|a| a.original.request_id == r.request)
                    }),
            "Original first-choice history is unavailable."
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use review::{Boot, Consumer, Outcome, State};

    fn plan() -> Plan {
        super::super::super::tests::plan("check")
    }
    fn boot() -> Boot {
        Boot {
            machine: "11111111-1111-4111-8111-111111111111".into(),
            boot: "22222222-2222-4222-8222-222222222222".into(),
        }
    }
    fn instance() -> ObservedInstance {
        ObservedInstance {
            guid: uuid::Uuid::new_v4().to_string(),
            pid: 23,
            birth: 42,
            epoch: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn first() -> First {
        First {
            request: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn scope() -> ChoiceScope {
        ChoiceScope {
            journal: uuid::Uuid::new_v4().to_string(),
            owner_epoch: uuid::Uuid::new_v4().to_string(),
            service_epoch: uuid::Uuid::new_v4().to_string(),
            generation: 7,
        }
    }
    fn reopen(record: &Record) -> Record {
        Record::decode(&record.encode(&plan()).unwrap(), &plan()).unwrap()
    }

    #[test]
    fn a_first_background_choice_reserves_its_own_uuid_and_settles_in_scope() {
        let mut record = Record::empty(&plan()).unwrap();
        let choice = first();
        record.reserve_first(&choice).unwrap();
        record
            .begin_id(Mode::Background, Some(instance()), choice.request.clone())
            .unwrap();
        assert_eq!(record.pending.as_ref().unwrap().request_id, choice.request);
        let reserved = reopen(&record);
        assert_eq!(reserved.first_choices[0].phase, ChoicePhase::Unknown);
        assert!(reserved.choices.is_empty(), "never a reviewed choice");
        let scope = scope();
        record
            .complete_in_scope(&choice.request, scope.clone())
            .unwrap();
        let settled = reopen(&record);
        let receipt = &settled.first_choices[0];
        assert_eq!(
            (receipt.phase, receipt.scope.clone()),
            (ChoicePhase::Settled, Some(scope))
        );
        assert!(settled.pending.is_none());
        let mut passive = record.clone();
        passive.first_choices[0].desired = Mode::Passive;
        assert!(passive.encode(&plan()).is_err());
        let mut early = reserved.clone();
        early.first_choices[0].scope = Some(self::scope());
        assert!(
            early.encode(&plan()).is_err(),
            "a scope only on a settled receipt"
        );
    }

    #[test]
    fn a_pending_interrupted_or_reviewed_history_closes_the_first_choice() {
        let mut pending = Record::empty(&plan()).unwrap();
        pending.begin(Mode::Background, Some(instance())).unwrap();
        let mut reviewed = pending.clone();
        let id = reviewed.pending.as_ref().unwrap().request_id.clone();
        reviewed.pending.as_mut().unwrap().phase = Phase::StopOffered;
        reviewed.review_original(&id, &boot(), true).unwrap();
        let mut archived = reviewed.clone();
        archived
            .review_original(
                &id,
                &Boot {
                    boot: "33333333-3333-4333-8333-333333333333".into(),
                    ..boot()
                },
                true,
            )
            .unwrap();
        assert!(archived.pending.is_none() && !archived.reviewed.is_empty());
        for (case, record) in [
            ("pending", pending),
            ("reboot required", reviewed),
            ("reviewed archive", archived),
        ] {
            let before = record.clone();
            let mut attempt = record.clone();
            let error = attempt.reserve_first(&first()).unwrap_err();
            assert!(error.to_string().contains("Review it"), "{case}: {error:#}");
            assert_eq!(attempt, before, "{case}");
        }
    }

    #[test]
    fn a_first_choice_uuid_is_never_reused_after_settling_or_refusal() {
        let mut record = Record::empty(&plan()).unwrap();
        let settled = first();
        record.reserve_first(&settled).unwrap();
        record
            .begin_id(Mode::Background, Some(instance()), settled.request.clone())
            .unwrap();
        record.complete_in_scope(&settled.request, scope()).unwrap();
        let mut record = reopen(&record);
        let refused = first();
        record.reserve_first(&refused).unwrap();
        record
            .begin_id(Mode::Background, Some(instance()), refused.request.clone())
            .unwrap();
        // A closed pre-manager boundary is the only route to a refusal.
        record.cancel(&refused.request).unwrap();
        let record = reopen(&record);
        assert_eq!(
            record
                .first_choices
                .iter()
                .map(|r| r.phase)
                .collect::<Vec<_>>(),
            [ChoicePhase::Settled, ChoicePhase::Refused]
        );
        for old in [&settled, &refused] {
            let before = record.clone();
            let mut attempt = record.clone();
            assert!(attempt.reserve_first(old).is_err());
            assert_eq!(attempt, before);
        }
        let mut fresh = record.clone();
        fresh.reserve_first(&first()).unwrap();
    }

    #[test]
    fn an_interrupted_first_choice_takes_the_ordinary_review_and_stays_unknown() {
        let mut record = Record::empty(&plan()).unwrap();
        let choice = first();
        record.reserve_first(&choice).unwrap();
        record
            .begin_id(Mode::Background, Some(instance()), choice.request.clone())
            .unwrap();
        let original = record.pending.as_mut().unwrap();
        original.phase = Phase::StopOffered;
        original.consumer = Some(Consumer {
            pid: 7,
            birth: 9,
            boot: boot(),
        });
        assert!(record.ready_cancellation(&choice.request).is_err());
        assert_eq!(
            record
                .review_original(&choice.request, &boot(), false)
                .unwrap(),
            State::RebootRequired
        );
        let mut record = reopen(&record);
        assert!(record.reserve_first(&first()).is_err());
        assert_eq!(
            record
                .review_original(
                    &choice.request,
                    &Boot {
                        boot: "33333333-3333-4333-8333-333333333333".into(),
                        ..boot()
                    },
                    false
                )
                .unwrap(),
            State::ReviewedUnknown
        );
        let record = reopen(&record);
        assert_eq!(record.reviewed[0].outcome, Outcome::Unknown);
        assert_eq!(record.first_choices[0].phase, ChoicePhase::Unknown);
        // The later explicit choice is the ordinary reviewed one.
        let mut later = record.clone();
        later
            .reserve_choice(
                &review::Choice {
                    reviewed: choice.request.clone(),
                    request: uuid::Uuid::new_v4().to_string(),
                },
                Mode::Background,
            )
            .unwrap();
        let mut collision = record.clone();
        assert!(collision
            .reserve_choice(
                &review::Choice {
                    reviewed: choice.request.clone(),
                    request: choice.request.clone(),
                },
                Mode::Background
            )
            .is_err());
    }

    #[test]
    fn the_record_and_its_independent_anchor_must_agree_on_the_latest_first_choice() {
        let empty = Record::empty(&plan()).unwrap();
        assert_eq!(empty.first_lost(None), None, "never offered: nothing lost");
        let reserve = |record: &mut Record, choice: &First| {
            record.reserve_first(choice).unwrap();
            record
                .begin_id(Mode::Background, None, choice.request.clone())
                .unwrap();
        };
        let mut record = Record::empty(&plan()).unwrap();
        let choice = first();
        reserve(&mut record, &choice);
        let reserved = reopen(&record);
        assert_eq!(reserved.first_lost(Some(&choice.request)), None);
        // A missing, emptied or replaced journal, or a wiped state folder,
        // while the anchor survives: lost, never "never written".
        assert_eq!(
            empty.first_lost(Some(&choice.request)),
            Some(choice.request.clone())
        );
        // History without its anchor, or a different latest choice: lost.
        assert_eq!(reserved.first_lost(None), Some(choice.request.clone()));
        let other = first();
        assert_eq!(
            reserved.first_lost(Some(&other.request)),
            Some(other.request.clone())
        );
        // The anchor follows the latest reserved choice.
        let mut later = reserved.clone();
        later.cancel(&choice.request).unwrap();
        reserve(&mut later, &other);
        let later = reopen(&later);
        assert_eq!(later.first_lost(Some(&other.request)), None);
        assert_eq!(
            later.first_lost(Some(&choice.request)),
            Some(choice.request.clone())
        );
    }

    #[test]
    fn an_ordinary_frame_never_continues_a_pending_first_choice() {
        for phase in [Phase::Prepared, Phase::Held] {
            let mut record = Record::empty(&plan()).unwrap();
            let choice = first();
            record.reserve_first(&choice).unwrap();
            record
                .begin_id(Mode::Background, Some(instance()), choice.request.clone())
                .unwrap();
            record.pending.as_mut().unwrap().phase = phase;
            let reopened = reopen(&record);
            let before = reopened.encode(&plan()).unwrap();
            let error = reopened.choice(None).unwrap_err();
            assert!(
                error.to_string().contains("first mode choice is pending"),
                "{phase:?}: {error:#}"
            );
            assert_eq!(reopened.encode(&plan()).unwrap(), before);
            assert_eq!(reopened.first_choices[0].phase, ChoicePhase::Unknown);
            // The closed pre-manager cancellation stays available.
            let mut cancelled = reopened.clone();
            cancelled.cancel(&choice.request).unwrap();
            assert_eq!(cancelled.first_choices[0].phase, ChoicePhase::Refused);
        }
        // Without a typed first-choice pending, the ordinary form is unchanged.
        let mut ordinary = Record::empty(&plan()).unwrap();
        ordinary.begin(Mode::Background, Some(instance())).unwrap();
        assert_eq!(ordinary.choice(None).unwrap(), None);
    }

    #[test]
    fn first_receipts_are_bounded_unique_and_never_a_reviewed_choice_id() {
        let mut record = Record::empty(&plan()).unwrap();
        for _ in 0..FIRST {
            let choice = first();
            record.reserve_first(&choice).unwrap();
            record
                .begin_id(Mode::Background, None, choice.request.clone())
                .unwrap();
            record.cancel(&choice.request).unwrap();
        }
        let full = record.clone();
        let mut attempt = record.clone();
        let error = attempt.reserve_first(&first()).unwrap_err();
        assert!(
            error.to_string().contains("history is full")
                || error.to_string().contains("byte headroom"),
            "{error:#}"
        );
        assert_eq!(attempt, full);
        let mut duplicate = Record::empty(&plan()).unwrap();
        let choice = first();
        duplicate.first_choices.push(Receipt {
            request: choice.request.clone(),
            desired: Mode::Background,
            phase: ChoicePhase::Refused,
            scope: None,
        });
        duplicate
            .first_choices
            .push(duplicate.first_choices[0].clone());
        assert!(duplicate.encode(&plan()).is_err());
    }

    #[test]
    fn the_last_admitted_byte_leaves_room_for_scope_barrier_and_archive() {
        let mut record = Record::empty(&plan()).unwrap();
        let choice = first();
        let mut probe = record.clone();
        probe.first_choices.push(Receipt {
            request: choice.request.clone(),
            desired: Mode::Background,
            phase: ChoicePhase::Unknown,
            scope: None,
        });
        let pending = Pending::prepared(choice.request.clone(), Mode::Background, None);
        let mut selected = plan();
        let padding = RECORD_BYTES - probe.review_capacity(&pending).unwrap();
        selected.argv[0].insert_str(3, &"p".repeat(padding.min(199)));
        let mut remaining = padding.saturating_sub(199);
        while remaining > 0 {
            let part = remaining.min(199);
            selected.argv[0].insert_str(3, &format!("{}\\", "p".repeat(part.saturating_sub(2))));
            remaining -= part;
        }
        record.command = selected.argv[..7].to_vec();
        probe.command = record.command.clone();
        let capacity = probe.review_capacity(&pending).unwrap();
        assert!(
            capacity <= RECORD_BYTES && capacity + 4 > RECORD_BYTES,
            "{capacity}"
        );
        let mut over = record.clone();
        let mut over_plan = selected.clone();
        over_plan.argv[0].insert_str(3, "pppp");
        over.command = over_plan.argv[..7].to_vec();
        let before = over.clone();
        let error = over.reserve_first(&choice).unwrap_err();
        assert!(error.to_string().contains("byte headroom"), "{error:#}");
        assert_eq!(over, before);
        record.reserve_first(&choice).unwrap();
        record
            .begin_id(Mode::Background, None, choice.request.clone())
            .unwrap();
        let uuid = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        let widest = Boot {
            machine: uuid.into(),
            boot: uuid.into(),
        };
        let mut attempt = record.clone();
        let original = attempt.pending.as_mut().unwrap();
        original.phase = Phase::StartOffered;
        original.original = Some(ObservedInstance {
            guid: format!("urn:uuid:{uuid}"),
            pid: u32::MAX,
            birth: u64::MAX,
            epoch: uuid.into(),
        });
        original.started = Some(format!("urn:uuid:{uuid}"));
        original.consumer = Some(Consumer {
            pid: u32::MAX,
            birth: u64::MAX,
            boot: widest.clone(),
        });
        let mut settled = attempt.clone();
        settled
            .complete_in_scope(
                &choice.request,
                ChoiceScope {
                    journal: uuid.into(),
                    owner_epoch: uuid.into(),
                    service_epoch: uuid.into(),
                    generation: 9_007_199_254_740_991,
                },
            )
            .unwrap();
        assert!(settled.encode(&selected).unwrap().len() <= RECORD_BYTES);
        attempt.encode(&selected).unwrap();
        assert_eq!(
            attempt
                .review_original(&choice.request, &widest, false)
                .unwrap(),
            State::RebootRequired
        );
        let barrier = attempt.encode(&selected).unwrap();
        let mut reopened = Record::decode(&barrier, &selected).unwrap();
        reopened
            .review_original(
                &choice.request,
                &Boot {
                    boot: boot().boot,
                    ..widest
                },
                false,
            )
            .unwrap();
        let archive = reopened.encode(&selected).unwrap();
        assert!(barrier.len() <= RECORD_BYTES && archive.len() <= RECORD_BYTES);
    }
}
