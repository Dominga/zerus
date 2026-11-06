//! Original-instance refresh metadata, never native Caller/Grant/drain authority.
//! It shares the mode journal's pending slot and review/byte rules. A mode frame
//! cannot continue this operation, even before its first manager effect.
use super::*;
use review::{ChoicePhase, Consumer};
use std::collections::BTreeSet;

pub const ABI: &str = "desktop-registration-refresh-v1";
pub const TARGET_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Target {
    pub configuration: String,
    pub selection: String,
    pub mode: Mode,
    pub original: ObservedInstance,
    pub original_generation: u64,
    pub journal: String,
    pub owner_epoch: String,
    pub grant_binding: String,
    pub original_image: String,
    pub replacement_image: String,
}
impl Target {
    pub fn validate(&self) -> Result<()> {
        self.original.validate()?;
        ensure!(
            hex(&self.configuration, 64)
                && hex(&self.selection, 64)
                && canonical_id(&self.journal)
                && canonical_id(&self.owner_epoch)
                && hex(&self.grant_binding, 64)
                && self.original_generation <= 9_007_199_254_740_991
                && hex(&self.original_image, 48)
                && hex(&self.replacement_image, 48)
                && self.original_image != self.replacement_image,
            "Original refresh target is unavailable."
        );
        Ok(())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= TARGET_BYTES,
            "Refresh target exceeds its bound."
        );
        let target: Self = serde_json::from_slice(bytes)?;
        target.validate()?;
        Ok(target)
    }
}

pub(in crate::desktop) mod budget;
pub(in crate::desktop) mod engine;
pub(in crate::desktop) mod observation;

/// A fresh local reservation, not a native permit. Decoding a record cannot
/// recreate it; the actual session must still own all native proofs/resources.
pub(in crate::desktop) struct Reserved {
    record: Record,
}

const HISTORY: usize = 48; // The shared 8 KiB bound can refuse sooner.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::desktop) enum Kind {
    OriginalInstanceRefresh,
}

/// Selection/history only. Native code retains and rechecks the actual Grant,
/// checked database, images, original task/process/pipe and handoff exclusion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Scope {
    pub kind: Kind,
    pub mode: Mode,
    pub journal: String,
    pub owner_epoch: String,
    pub grant_binding: String,
    pub selection: String,
    pub original_generation: u64,
    pub original_image: String,
    pub replacement_image: String,
}
fn hex(value: &str, n: usize) -> bool {
    value.len() == n
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Scope {
    fn validate(&self) -> Result<()> {
        ensure!(
            hex(&self.selection, 64)
                && canonical_id(&self.journal)
                && canonical_id(&self.owner_epoch)
                && hex(&self.grant_binding, 64)
                && hex(&self.original_image, 48)
                && hex(&self.replacement_image, 48)
                && self.original_image != self.replacement_image
                && self.original_generation <= 9_007_199_254_740_991,
            "Original refresh scope is unavailable."
        );
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Ready {
    pub instance: ObservedInstance,
    pub generation: u64,
}

/// The result stays bound to its original request and original images. It is
/// neither a reviewed mode choice nor a business-eligibility/Grant receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop) struct Receipt {
    pub request: String,
    pub original: ObservedInstance,
    pub scope: Scope,
    pub phase: ChoicePhase,
    pub ready: Option<Ready>,
}
/// Call only on a status value read from the SAME authenticated retained
/// endpoint. This predicate creates no Stop/drain permit or native custody.
pub fn drained(value: &serde_json::Value) -> Result<bool> {
    let upgrade = &value["executable_upgrade"];
    let drain = &upgrade["refresh"];
    ensure!(
        upgrade["code"] == "hgs_replaced"
            && upgrade["state"] == "manual_refresh_required"
            && upgrade["new_admission"] == false
            && drain["abi"] == "desktop-retirement-drain-v1",
        "Original actor cannot prove refresh retirement/drain."
    );
    let mut all = true;
    for key in [
        "runtime_drained",
        "owner_loans_released",
        "connections_drained",
        "operations_drained",
    ] {
        all &= drain[key]
            .as_bool()
            .context("Original resource drain is unavailable.")?;
    }
    ensure!(
        drain["drained"].as_bool() == Some(all),
        "Original resource drain is inconsistent."
    );
    Ok(all)
}

impl Record {
    pub fn pending_refresh(&self) -> bool {
        self.pending.as_ref().is_some_and(|p| p.refresh.is_some())
    }
    pub fn reserve_refresh(
        &mut self,
        request: &str,
        original: ObservedInstance,
        scope: Scope,
        consumer: Consumer,
    ) -> Result<Reserved> {
        self.ensure_review_slot()?;
        scope.validate()?;
        original.validate()?;
        ensure!(self.pending.is_none() && canonical_id(request)
            && self.completed.as_deref() != Some(request)
            && self.cancelled.as_deref() != Some(request)
            && !self.reviewed.iter().any(|a|a.original.request_id == request)
            && !self.choices.iter().any(|r|r.request == request)
            && !self.first_choices.iter().any(|r|r.request == request)
            && !self.refreshes.iter().any(|r|r.request == request)
            && self.refreshes.len() < HISTORY,
            "Original refresh was already reserved or another operation/history bound refuses; read the original UUID without replay.");
        let mut pending = Pending::prepared(request.into(), scope.mode, Some(original.clone()));
        pending.consumer = Some(consumer);
        pending.refresh = Some(scope.clone());
        let mut next = self.clone();
        next.refreshes.push(Receipt {
            request: request.into(),
            original,
            scope,
            phase: ChoicePhase::Unknown,
            ready: None,
        });
        next.ensure_review_capacity(&pending)?;
        next.pending = Some(pending);
        next.check_reviews()?;
        next.check_first()?;
        next.check_refreshes()?;
        *self = next;
        Ok(Reserved {
            record: self.clone(),
        })
    }
    pub fn finish_refresh(&mut self, request: &str, ready: Ready) -> Result<()> {
        let pending = self
            .pending
            .as_ref()
            .context("Original refresh is absent.")?;
        ensure!(
            pending.request_id == request
                && pending.refresh.is_some()
                && pending.review.is_none()
                && pending.phase == Phase::Ready
                && pending.started.as_deref() == Some(ready.instance.guid.as_str()),
            "Original refresh completion changed."
        );
        let mut next = self.clone();
        let receipt = next
            .refreshes
            .iter_mut()
            .find(|r| r.request == request)
            .context("Original refresh receipt is absent.")?;
        ensure!(
            receipt.phase == ChoicePhase::Unknown,
            "Original refresh was already settled."
        );
        receipt.phase = ChoicePhase::Settled;
        receipt.ready = Some(ready);
        next.pending = None;
        // Keep the historical MODE completion marker and choice receipts;
        // refresh has its own result and does not relabel a mode choice.
        next.check_reviews()?;
        next.check_first()?;
        next.check_refreshes()?;
        *self = next;
        Ok(())
    }
    pub fn refresh_lost(&self, anchor: Option<&str>) -> Option<String> {
        match (anchor, self.refreshes.last().map(|r| r.request.as_str())) {
            (None, None) => None,
            (Some(a), Some(r)) if a == r => None,
            (Some(a), _) => Some(a.into()),
            (None, Some(r)) => Some(r.into()),
        }
    }
    pub(super) fn check_refreshes(&self) -> Result<()> {
        ensure!(
            self.refreshes.len() <= HISTORY,
            "Original refresh history is full."
        );
        let mut ids = BTreeSet::new();
        for receipt in &self.refreshes {
            receipt.original.validate()?;
            receipt.scope.validate()?;
            ensure!(
                canonical_id(&receipt.request)
                    && ids.insert(&receipt.request)
                    && !self.choices.iter().any(|c| c.request == receipt.request)
                    && !self
                        .first_choices
                        .iter()
                        .any(|c| c.request == receipt.request),
                "Original refresh receipt identity changed."
            );
            let original = self
                .pending
                .iter()
                .chain(self.reviewed.iter().map(|a| &a.original))
                .find(|p| p.request_id == receipt.request);
            if receipt.phase == ChoicePhase::Unknown {
                let original = original.context("Original unknown refresh lost its intent.")?;
                ensure!(
                    original.original.as_ref() == Some(&receipt.original)
                        && original.refresh.as_ref() == Some(&receipt.scope)
                        && original.desired == receipt.scope.mode
                        && receipt.ready.is_none(),
                    "Original refresh binding changed."
                );
            } else {
                ensure!(
                    original.is_none(),
                    "A terminal refresh cannot replace an unknown original."
                );
            }
            if receipt.phase == ChoicePhase::Settled {
                let ready = receipt
                    .ready
                    .as_ref()
                    .context("Refresh lacks its original readiness.")?;
                ready.instance.validate()?;
                ensure!(
                    ready.generation <= 9_007_199_254_740_991
                        && uuid::Uuid::parse_str(&ready.instance.guid)?
                            != uuid::Uuid::parse_str(&receipt.original.guid)?
                        && ready.instance.epoch != receipt.original.epoch
                        && (ready.instance.pid, ready.instance.birth)
                            != (receipt.original.pid, receipt.original.birth),
                    "Refresh readiness selected another original."
                );
            } else {
                ensure!(
                    receipt.ready.is_none(),
                    "An unknown/refused refresh has no settled readiness."
                );
            }
        }
        for pending in self
            .pending
            .iter()
            .chain(self.reviewed.iter().map(|a| &a.original))
        {
            if let Some(scope) = &pending.refresh {
                scope.validate()?;
                ensure!(
                    (matches!(pending.phase, Phase::StartOffered | Phase::Ready)
                        || pending.started.is_none())
                        && (pending.phase != Phase::Ready || pending.started.is_some()),
                    "Refresh start identity precedes its offered start or is missing."
                );
                ensure!(
                    pending.consumer.is_some()
                        && pending.original.is_some()
                        && pending.desired == scope.mode
                        && matches!(
                            pending.phase,
                            Phase::Prepared
                                | Phase::Held
                                | Phase::StopOffered
                                | Phase::Stopped
                                | Phase::StartOffered
                                | Phase::Ready
                        )
                        && self
                            .refreshes
                            .iter()
                            .any(|r| r.request == pending.request_id),
                    "Original refresh intent is unavailable."
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
