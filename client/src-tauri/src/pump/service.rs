//! Service presentation retained across webview reloads. Receipt instants stay
//! native; only an aged remaining_ms crosses into the new view's clock.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use zerus_client_core::desktop::service::Snapshot;

pub(super) struct Cached {
    snapshot: Snapshot,
    /// One receipt per waiting computer, bounded by the checked service roster.
    waiting: BTreeMap<String, Receipt>,
}

#[derive(Clone, Copy)]
struct Receipt {
    incarnation: Option<u64>,
    serial: u64,
    received_at: Instant,
    remaining: Duration,
}

impl Receipt {
    fn remaining(&self, now: Instant) -> Duration {
        self.remaining
            .saturating_sub(now.saturating_duration_since(self.received_at))
    }
}

impl Cached {
    pub(super) fn matches(&self, snapshot: &Snapshot) -> bool {
        self.snapshot == *snapshot
    }

    pub(super) fn receive(snapshot: Snapshot, previous: Option<&Self>, now: Instant) -> Self {
        let previous = previous.filter(|old| old.snapshot.epoch == snapshot.epoch);
        let waiting = snapshot
            .fleet_computers
            .iter()
            .flatten()
            .filter_map(|row| {
                let retry = row
                    .retry
                    .as_ref()
                    .filter(|retry| retry.phase == "waiting")?;
                let receipt = Receipt {
                    incarnation: row.incarnation,
                    serial: retry.serial,
                    received_at: now,
                    remaining: Duration::from_millis(retry.remaining_ms?),
                };
                // Status publications can carry a previously received report.
                // Even a fresh later frame of the same wait only shortens it.
                let kept = previous
                    .and_then(|old| old.waiting.get(&row.id))
                    .filter(|old| {
                        old.incarnation == receipt.incarnation
                            && old.serial == receipt.serial
                            && old.remaining(now) <= receipt.remaining
                    })
                    .copied()
                    .unwrap_or(receipt);
                Some((row.id.clone(), kept))
            })
            .collect();
        Self { snapshot, waiting }
    }

    pub(super) fn replay(&self, now: Instant) -> Snapshot {
        let mut snapshot = self.snapshot.clone();
        for row in snapshot.fleet_computers.iter_mut().flatten() {
            if let (Some(retry), Some(receipt)) = (&mut row.retry, self.waiting.get(&row.id)) {
                retry.remaining_ms =
                    Some(u64::try_from(receipt.remaining(now).as_millis()).unwrap_or(u64::MAX));
            }
        }
        snapshot
    }
}
