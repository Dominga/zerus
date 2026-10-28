//! When the screen reads run: pure decisions over a millisecond clock the
//! hub passes in (client/src-tauri/src/screens.rs runs the reads). Sources:
//! AccountsPage::ensureCatalogs and requestQueuedCatalog
//! (AccountsPage.cpp:450-468), AccountUsageStore::ensure, dispatch and
//! finish (AccountUsageStore.cpp:24-64), SwarmController's 5 s poll.

use super::accounts::{AccountsState, UsageRef};
use super::{items, text};
use crate::ComputerId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One `account ls` per host per 30 s unless forced (AccountsPage.cpp:456).
pub const CATALOG_INTERVAL: u64 = 30_000;
/// One inspect per profile per 5 minutes (AccountUsageStore::RefreshInterval).
pub const USAGE_INTERVAL: u64 = 300_000;
/// Inspects in flight at once (AccountUsageStore::dispatch).
pub const USAGE_CONCURRENCY: usize = 2;

/// The shared project catalog's poll (SwarmController pollInterval).
pub const SWARM_INTERVAL: u64 = 5_000;

/// A screen whose reads the hub runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Screen {
    Overview,
    Projects,
    Machines,
    Accounts,
}

impl Screen {
    /// The name the `screens-changed` event carries.
    pub fn name(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Projects => "projects",
            Self::Machines => "machines",
            Self::Accounts => "accounts",
        }
    }
}

/// A read that runs at most once at a time, again `interval` after the
/// previous start.
#[derive(Clone, Debug, Default)]
pub struct Poll {
    interval: u64,
    started: Option<u64>,
    pending: bool,
}

impl Poll {
    pub fn new(interval: u64) -> Self {
        Self {
            interval,
            started: None,
            pending: false,
        }
    }

    /// Whether a read starts now; when it does, it is marked running.
    pub fn due(&mut self, now: u64, force: bool) -> bool {
        if self.pending {
            return false;
        }
        if !force
            && self
                .started
                .is_some_and(|at| now.saturating_sub(at) < self.interval)
        {
            return false;
        }
        self.started = Some(now);
        self.pending = true;
        true
    }

    pub fn finish(&mut self) {
        self.pending = false;
    }

    pub fn pending(&self) -> bool {
        self.pending
    }
}

impl AccountsState {
    /// Drops what belongs to hosts no longer listed.
    fn prune(&mut self, hosts: &[ComputerId]) {
        self.catalogs.retain(|host, _| hosts.contains(host));
        self.usage.retain(|key, _| hosts.contains(&key.0));
        self.warmed.retain(|host| hosts.contains(host));
        self.queue.retain(|r| hosts.contains(&r.host));
    }

    fn start_catalogs(&mut self, hosts: &[ComputerId], now: u64, force: bool) -> Vec<ComputerId> {
        let mut start = Vec::new();
        for host in hosts {
            let slot = self.catalogs.entry(host.clone()).or_default();
            if slot.pending {
                // Only an explicit refresh can queue one follow-up.
                slot.queued |= force;
                continue;
            }
            if force
                || slot
                    .requested
                    .is_none_or(|at| now.saturating_sub(at) >= CATALOG_INTERVAL)
            {
                slot.requested = Some(now);
                slot.pending = true;
                start.push(host.clone());
            }
        }
        start
    }

    /// The hosts whose `account ls` starts now (marked running). `hosts` is
    /// the current host list: others are forgotten.
    pub fn ensure_catalogs(
        &mut self,
        hosts: &[ComputerId],
        now: u64,
        force: bool,
    ) -> Vec<ComputerId> {
        self.prune(hosts);
        self.start_catalogs(hosts, now, force)
    }

    /// One host's `account ls` for New session, which reads on every open,
    /// computer change and Refresh (`NewSessionDialog::loadAccounts`): it
    /// starts now (marked running) unless one runs, which is shared without a
    /// follow-up. No other host changes. True when it starts. A host read
    /// this way needs no warm-up read of its own.
    pub fn ensure_catalog(&mut self, host: &ComputerId, now: u64) -> bool {
        self.warmed.insert(host.clone());
        let slot = self.catalogs.entry(host.clone()).or_default();
        if slot.pending {
            return false;
        }
        slot.requested = Some(now);
        slot.pending = true;
        true
    }

    /// Each newly listed host once, even before Accounts is opened. `hosts`
    /// is the current host list: others are forgotten.
    pub fn warm(&mut self, hosts: &[ComputerId], now: u64) -> Vec<ComputerId> {
        self.prune(hosts);
        let fresh: Vec<ComputerId> = hosts
            .iter()
            .filter(|host| self.warmed.insert((*host).clone()))
            .cloned()
            .collect();
        self.start_catalogs(&fresh, now, false)
    }

    /// Files a reply; true when a queued follow-up starts now (marked running).
    /// A fresh catalog drops the inspects of profiles it no longer lists.
    pub fn finish_catalog(
        &mut self,
        host: &ComputerId,
        reply: Result<Value, String>,
        now: u64,
        hosts: &[ComputerId],
    ) -> bool {
        let slot = self.catalogs.entry(host.clone()).or_default();
        slot.pending = false;
        match reply {
            Ok(value) => {
                let listed: Vec<(String, String)> = items(&value, "profiles")
                    .iter()
                    .map(|raw| (text(raw, "id"), text(raw, "home")))
                    .collect();
                slot.data = Some(value);
                slot.error = None;
                self.usage.retain(|key, _| {
                    key.0 != *host || listed.contains(&(key.1.clone(), key.2.clone()))
                });
                self.queue.retain(|r| {
                    r.host != *host || listed.contains(&(r.id.clone(), r.home.clone()))
                });
            }
            Err(error) => slot.error = Some(error),
        }
        let slot = self.catalogs.entry(host.clone()).or_default();
        if std::mem::take(&mut slot.queued) && hosts.contains(host) {
            slot.requested = Some(now);
            slot.pending = true;
            return true;
        }
        false
    }

    /// Queues a profile's inspect when it is due (AccountUsageStore::ensure).
    pub fn ensure_usage(&mut self, r: &UsageRef, reachable: bool, now: u64, force: bool) {
        if r.id.is_empty() {
            return;
        }
        let slot = self.usage.entry(r.key()).or_default();
        if slot.pending {
            return;
        }
        let changed = !r.auth_revision.is_empty() && r.auth_revision != slot.auth_revision;
        if changed && !slot.auth_revision.is_empty() {
            slot.data = None;
            slot.failed = false;
        }
        if !r.auth_revision.is_empty() {
            slot.auth_revision = r.auth_revision.clone();
        }
        if !reachable
            || (!force
                && !changed
                && slot
                    .attempted
                    .is_some_and(|at| now.saturating_sub(at) < USAGE_INTERVAL))
        {
            return;
        }
        slot.attempted = Some(now);
        slot.pending = true;
        self.queue.push_back(r.clone());
    }

    /// The queued inspects that start now, at most two in flight.
    pub fn dispatch(&mut self, reachable: &dyn Fn(&ComputerId) -> bool) -> Vec<UsageRef> {
        let mut start = Vec::new();
        while self.running < USAGE_CONCURRENCY {
            let Some(r) = self.queue.pop_front() else {
                break;
            };
            if !reachable(&r.host) {
                if let Some(slot) = self.usage.get_mut(&r.key()) {
                    slot.pending = false;
                }
                continue;
            }
            self.running += 1;
            start.push(r);
        }
        start
    }

    /// Files an inspect's reply; None is a failed read (AccountUsageStore::finish).
    /// A failure keeps the previous data and stores nothing when there was none.
    pub fn finish_usage(&mut self, r: &UsageRef, reply: Option<Value>) {
        self.running = self.running.saturating_sub(1);
        // The profile left the catalog meanwhile: nothing to file.
        let Some(slot) = self.usage.get_mut(&r.key()) else {
            return;
        };
        slot.pending = false;
        let matches = reply.as_ref().is_some_and(|data| {
            data.get("status").is_some()
                && text(data, "id") == r.id
                && (r.home.is_empty() || text(data, "home") == r.home)
                && (text(data, "provider").is_empty() || text(data, "provider") == r.provider)
        });
        let failed = !matches
            || reply
                .as_ref()
                .is_some_and(|data| text(data, "status") == "unavailable");
        slot.failed = failed;
        if !failed {
            slot.data = reply;
        }
    }

    /// Neither Overview nor Accounts shows: the queued inspects and any
    /// queued follow-up `account ls` are dropped. A dropped inspect never
    /// ran, so it is due again when a screen shows; reads already running
    /// finish.
    pub fn cancel_queued(&mut self) {
        for r in std::mem::take(&mut self.queue) {
            if let Some(slot) = self.usage.get_mut(&r.key()) {
                slot.pending = false;
                slot.attempted = None;
            }
        }
        for slot in self.catalogs.values_mut() {
            slot.queued = false;
        }
    }

    /// When the next `account ls` of `hosts` or inspect of `refs` falls due,
    /// on the hub's clock. Running and queued reads are left out (a reply
    /// wakes the hub), and so are profiles on unreachable hosts (a fleet
    /// change does).
    pub fn next_due(&self, hosts: &[ComputerId], refs: &[(UsageRef, bool)]) -> Option<u64> {
        let catalogs = hosts.iter().filter_map(|host| {
            let slot = self.catalogs.get(host).filter(|slot| !slot.pending)?;
            Some(slot.requested?.saturating_add(CATALOG_INTERVAL))
        });
        let usage = refs
            .iter()
            .filter(|(_, reachable)| *reachable)
            .filter_map(|(r, _)| {
                let slot = self.usage.get(&r.key()).filter(|slot| !slot.pending)?;
                Some(slot.attempted?.saturating_add(USAGE_INTERVAL))
            });
        catalogs.chain(usage).min()
    }

    /// An account read runs or waits for its turn.
    pub fn reading(&self) -> bool {
        self.running > 0
            || !self.queue.is_empty()
            || self.catalogs.values().any(|slot| slot.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn r(id: &str) -> UsageRef {
        UsageRef {
            host: ComputerId::local(),
            id: id.into(),
            home: format!("/home/example/{id}"),
            provider: "claude".into(),
            auth_revision: "a1".into(),
        }
    }

    fn reply(id: &str, status: &str) -> Value {
        json!({"id": id, "home": format!("/home/example/{id}"), "provider": "claude", "status": status})
    }

    fn ids(refs: &[UsageRef]) -> Vec<&str> {
        refs.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn account_catalogs_run_once_per_host_every_thirty_seconds_with_one_forced_follow_up() {
        let hosts = [ComputerId::local(), ComputerId::from("studio")];
        let mac = ComputerId::from("mac");
        let all = [hosts[0].clone(), hosts[1].clone(), mac.clone()];
        let mut s = AccountsState::default();
        assert_eq!(s.ensure_catalogs(&hosts, 0, false), hosts);
        assert!(
            s.ensure_catalogs(&hosts, 40_000, false).is_empty(),
            "one in flight per host"
        );
        assert!(
            s.ensure_catalogs(&hosts, 40_000, true).is_empty(),
            "a forced refresh waits for the one running"
        );
        let catalog = json!({"profiles": [], "revision": "r1"});
        assert!(
            s.finish_catalog(&hosts[0], Ok(catalog.clone()), 41_000, &hosts),
            "the queued follow-up starts"
        );
        assert!(!s.finish_catalog(
            &hosts[0],
            Err("hgs did not answer in time".into()),
            42_000,
            &hosts
        ));
        assert_eq!(
            s.catalogs[&hosts[0]].data,
            Some(catalog),
            "a failure keeps the last catalog"
        );
        assert_eq!(
            s.catalogs[&hosts[0]].error.as_deref(),
            Some("hgs did not answer in time")
        );
        assert!(s.finish_catalog(
            &hosts[1],
            Ok(json!({"profiles": [], "revision": "r2"})),
            42_000,
            &hosts
        ));
        assert!(!s.finish_catalog(&hosts[1], Ok(json!({"profiles": []})), 43_000, &hosts));
        assert_eq!(s.warm(&all, 60_000), std::slice::from_ref(&mac));
        s.finish_catalog(&mac, Ok(json!({"profiles": []})), 61_000, &all);
        assert!(s.warm(&all, 200_000).is_empty(), "a host is warmed once");
        assert_eq!(
            s.ensure_catalogs(&all, 100_000, false),
            all,
            "all are past thirty seconds"
        );
        assert!(s.reading());
    }

    // G3: New session reads one computer's catalog on every open, computer
    // change and Refresh (NewSessionDialog::loadAccounts 865-874), on the hub:
    // a running one is shared without a follow-up, and no other host changes.
    #[test]
    fn one_computers_catalog_starts_unless_one_runs_and_leaves_the_others() {
        let (local, studio) = (ComputerId::local(), ComputerId::from("studio"));
        let mut s = AccountsState::default();
        s.warm(&[local.clone(), studio.clone()], 0);
        s.finish_catalog(
            &local,
            Ok(json!({"profiles": []})),
            1,
            std::slice::from_ref(&local),
        );
        assert!(!s.ensure_catalog(&studio, 5), "studio's warm-up still runs");
        assert!(!s.catalogs[&studio].queued, "shared, not queued again");
        assert!(
            s.ensure_catalog(&local, 10),
            "read a moment ago, read again"
        );
        assert!(s.catalogs[&local].pending && !s.catalogs[&local].queued);
        assert_eq!(s.catalogs[&local].requested, Some(10));
        assert!(!s.ensure_catalog(&local, 50), "a running read is shared");
        assert!(!s.catalogs[&local].queued);
        s.finish_catalog(
            &local,
            Ok(json!({"profiles": []})),
            51,
            std::slice::from_ref(&local),
        );
        assert!(s.ensure_catalog(&local, 52));
        let mac = ComputerId::from("mac");
        assert!(s.ensure_catalog(&mac, 0), "never read");
        assert!(s.catalogs.contains_key(&studio), "the others are kept");
        s.finish_catalog(
            &mac,
            Ok(json!({"profiles": []})),
            1,
            std::slice::from_ref(&mac),
        );
        assert!(
            s.warm(std::slice::from_ref(&mac), 2).is_empty(),
            "read for New session: no warm-up read again"
        );
    }

    #[test]
    fn hosts_that_leave_the_list_are_forgotten_and_warmed_again_when_they_return() {
        let (local, studio) = (ComputerId::local(), ComputerId::from("studio"));
        let both = [local.clone(), studio.clone()];
        let mut s = AccountsState::default();
        s.warm(&both, 0);
        s.finish_catalog(&studio, Ok(json!({"profiles": []})), 1, &both);
        s.ensure_usage(
            &UsageRef {
                host: studio.clone(),
                ..r("a")
            },
            true,
            0,
            false,
        );
        assert!(s.warmed.contains(&studio) && s.catalogs.contains_key(&studio));
        s.ensure_catalogs(&both[..1], 5, false);
        assert!(!s.catalogs.contains_key(&studio) && !s.warmed.contains(&studio));
        assert!(s.usage.is_empty() && s.queue.is_empty());
        s.warm(&both, 10);
        s.finish_catalog(&studio, Ok(json!({"profiles": []})), 11, &both);
        s.warm(&both[..1], 20);
        assert!(!s.catalogs.contains_key(&studio) && !s.warmed.contains(&studio));
        assert_eq!(
            s.warm(&both, 30),
            [studio],
            "a returning host is warmed again"
        );
    }

    #[test]
    fn a_fresh_catalog_drops_the_inspects_of_profiles_it_no_longer_lists() {
        let host = ComputerId::local();
        let hosts = [host.clone()];
        let mut s = AccountsState::default();
        for id in ["a", "b"] {
            s.ensure_usage(&r(id), true, 0, false);
        }
        s.dispatch(&|_: &ComputerId| true);
        s.finish_usage(&r("a"), Some(reply("a", "ok")));
        let listed = json!({"profiles": [{"id": "a", "home": "/home/example/a"}]});
        s.ensure_catalogs(&hosts, 0, false);
        s.finish_catalog(&host, Ok(listed), 1, &hosts);
        assert!(s.usage.contains_key(&r("a").key()));
        assert!(!s.usage.contains_key(&r("b").key()));
        s.finish_usage(&r("b"), Some(reply("b", "ok")));
        assert!(
            !s.usage.contains_key(&r("b").key()),
            "a late reply does not revive it"
        );
        assert!(!s.reading());
    }

    // Review Focus 4: slow and failing inspects.
    #[test]
    fn inspects_run_two_at_a_time_and_a_failure_keeps_its_data_and_waits_five_minutes() {
        let mut s = AccountsState::default();
        let all = |_: &ComputerId| true;
        for id in ["a", "b", "c"] {
            s.ensure_usage(&r(id), true, 0, false);
        }
        assert_eq!(ids(&s.dispatch(&all)), ["a", "b"]);
        assert!(s.dispatch(&all).is_empty(), "at most two at once");
        // a times out (the read fails); c takes its place while b is still slow.
        s.finish_usage(&r("a"), None);
        assert_eq!(ids(&s.dispatch(&all)), ["c"]);
        s.finish_usage(&r("b"), Some(reply("b", "ok")));
        s.finish_usage(&r("c"), Some(reply("c", "ok")));
        let a = &s.usage[&r("a").key()];
        assert!(a.failed && !a.pending);
        assert_eq!(a.data, None, "a failed first read stores no stub");
        // Nothing retries inside five minutes unless forced.
        s.ensure_usage(&r("a"), true, 299_999, false);
        assert!(s.dispatch(&all).is_empty());
        // b's next reply names another home: a failure that keeps b's data.
        s.ensure_usage(&r("b"), true, 300_000, false);
        assert_eq!(ids(&s.dispatch(&all)), ["b"]);
        s.finish_usage(
            &r("b"),
            Some(json!({"id": "b", "home": "/home/example/elsewhere", "provider": "claude", "status": "ok"})),
        );
        assert!(s.usage[&r("b").key()].failed);
        assert_eq!(s.usage[&r("b").key()].data, Some(reply("b", "ok")));
        // An unavailable reply is a failure too, and keeps the earlier data.
        s.ensure_usage(&r("c"), true, 1, true);
        assert_eq!(ids(&s.dispatch(&all)), ["c"]);
        s.finish_usage(&r("c"), Some(reply("c", "unavailable")));
        assert!(s.usage[&r("c").key()].failed);
        assert_eq!(s.usage[&r("c").key()].data, Some(reply("c", "ok")));
        // A forced refresh runs at once.
        s.ensure_usage(&r("a"), true, 299_999, true);
        assert_eq!(ids(&s.dispatch(&all)), ["a"]);
        assert!(s.reading());
    }

    #[test]
    fn a_new_credential_revision_reads_at_once_and_an_unreachable_host_is_skipped() {
        let mut s = AccountsState::default();
        s.ensure_usage(&r("a"), true, 0, false);
        s.dispatch(&|_: &ComputerId| true);
        s.finish_usage(&r("a"), Some(reply("a", "ok")));
        let mut next = r("a");
        next.auth_revision = "a2".into();
        s.ensure_usage(&next, true, 1_000, false);
        assert_eq!(
            s.usage[&next.key()].data,
            None,
            "the old revision's data is dropped"
        );
        assert_eq!(ids(&s.dispatch(&|_: &ComputerId| true)), ["a"]);
        s.finish_usage(&next, Some(reply("a", "ok")));
        s.ensure_usage(&r("b"), false, 0, false);
        assert!(
            s.dispatch(&|_: &ComputerId| true).is_empty(),
            "an offline host is never asked"
        );
        s.ensure_usage(&r("c"), true, 0, false);
        assert!(
            s.dispatch(&|_: &ComputerId| false).is_empty(),
            "a host that went offline before its turn is skipped"
        );
        assert!(!s.usage[&r("c").key()].pending);
        assert!(!s.reading());
    }

    #[test]
    fn hiding_drops_the_queued_inspects_and_follow_up_and_lets_running_reads_finish() {
        let host = ComputerId::local();
        let hosts = [host.clone()];
        let all = |_: &ComputerId| true;
        let mut s = AccountsState::default();
        s.ensure_catalogs(&hosts, 0, false);
        s.ensure_catalogs(&hosts, 0, true);
        for id in ["a", "b", "c"] {
            s.ensure_usage(&r(id), true, 0, false);
        }
        assert_eq!(ids(&s.dispatch(&all)), ["a", "b"]);
        s.cancel_queued();
        assert!(
            s.usage[&r("a").key()].pending && s.usage[&r("b").key()].pending,
            "running inspects finish"
        );
        let c = &s.usage[&r("c").key()];
        assert!(
            !c.pending && c.attempted.is_none(),
            "a dropped inspect never ran"
        );
        let profiles: Vec<Value> = ["a", "b", "c"]
            .iter()
            .map(|id| json!({"id": id, "home": format!("/home/example/{id}")}))
            .collect();
        let listed = json!({ "profiles": profiles });
        assert!(
            !s.finish_catalog(&host, Ok(listed), 1, &hosts),
            "the follow-up is dropped"
        );
        s.finish_usage(&r("a"), Some(reply("a", "ok")));
        s.finish_usage(&r("b"), Some(reply("b", "ok")));
        assert!(!s.reading());
        s.ensure_usage(&r("c"), true, 2, false);
        s.ensure_usage(&r("a"), true, 2, false);
        assert_eq!(
            ids(&s.dispatch(&all)),
            ["c"],
            "the dropped one is due again when a screen shows"
        );
    }

    #[test]
    fn the_next_due_time_is_the_earliest_catalog_or_inspect_that_is_not_running() {
        let host = ComputerId::local();
        let hosts = [host.clone()];
        let mut s = AccountsState::default();
        assert_eq!(s.next_due(&hosts, &[]), None, "nothing read yet");
        s.ensure_catalogs(&hosts, 1_000, false);
        assert_eq!(
            s.next_due(&hosts, &[]),
            None,
            "a running catalog wakes the hub with its reply"
        );
        s.finish_catalog(&host, Ok(json!({"profiles": []})), 2_000, &hosts);
        assert_eq!(s.next_due(&hosts, &[]), Some(1_000 + CATALOG_INTERVAL));
        let refs = [(r("a"), true), (r("b"), true), (r("c"), false)];
        for (r, reachable) in &refs {
            s.ensure_usage(r, *reachable, 10_000, false);
        }
        assert_eq!(
            s.next_due(&hosts, &refs),
            Some(1_000 + CATALOG_INTERVAL),
            "queued inspects and offline hosts are left out"
        );
        s.dispatch(&|_: &ComputerId| true);
        s.finish_usage(&r("a"), Some(reply("a", "ok")));
        s.ensure_catalogs(&hosts, 40_000, false);
        assert_eq!(s.next_due(&hosts, &refs), Some(10_000 + USAGE_INTERVAL));
    }

    // Review Focus 5 (the timing half; the hub test in Task 3 covers visibility).
    #[test]
    fn the_swarm_poll_runs_every_five_seconds_and_never_twice_at_once() {
        let mut poll = Poll::new(SWARM_INTERVAL);
        assert!(poll.due(0, false));
        assert!(!poll.due(6_000, false), "one runs");
        assert!(
            !poll.due(6_000, true),
            "a forced read waits for the one running"
        );
        poll.finish();
        assert!(poll.due(6_000, false));
        poll.finish();
        assert!(!poll.due(10_999, false));
        assert!(poll.due(11_000, false));
        poll.finish();
        assert!(poll.due(11_001, true), "forced: at once");
    }

    #[test]
    fn the_screen_names_match_the_screens_changed_event() {
        assert_eq!(
            serde_json::to_value(Screen::Overview).unwrap(),
            json!("overview")
        );
        assert_eq!(Screen::Projects.name(), "projects");
    }
}
