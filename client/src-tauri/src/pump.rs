//! From the core's change sink to `next_batch`: one thread drains the sink
//! into the coalescer, and `next_batch` takes everything pending, parking
//! up to [`PARK`] while nothing is. A mirror of the last-known rows, statuses,
//! and machine cards lets a reloaded webview start from the full catalog. The
//! organization never reaches the webview: [`Grouping`] turns it into each
//! row's project and the headers (zerus-5wmh.31).

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde_json::Value;
use zerus_client_core::swarm::edit::Intent;
use zerus_client_core::swarm::grouping::{Grouping, SyncView};
use zerus_client_core::swarm::layout::Layout;
use zerus_client_core::views::FleetEntry;
use zerus_client_core::{
    Batch, Change, Coalescer, ComputerId, ComputerStatus, MachineMeta, ViewRow,
};

/// The longest `next_batch` waits for a change before it returns an empty batch.
pub const PARK: Duration = Duration::from_millis(250);

#[derive(Default)]
struct Inner {
    coalescer: Coalescer,
    /// A change arrived since the last take.
    dirty: bool,
    mirror: Mirror,
    grouping: Grouping,
}

/// Shared by the drain thread and the commands.
#[derive(Clone, Default)]
pub struct Pump(
    Arc<(Mutex<Inner>, Condvar)>,
    /// Takes about to wait, so a test can order a change after the park.
    #[cfg(test)]
    Arc<std::sync::atomic::AtomicUsize>,
);

impl Pump {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0 .0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues one change and wakes a parked `take`.
    pub fn push(&self, change: Change) {
        let mut inner = self.lock();
        inner.mirror.observe(&change);
        inner.grouping.observe(&change);
        inner.coalescer.push(change);
        self.wake(inner);
    }

    /// Marks the pump dirty and wakes a parked `take`.
    fn wake(&self, mut inner: MutexGuard<'_, Inner>) {
        inner.dirty = true;
        drop(inner);
        self.0 .1.notify_all();
    }

    /// Everything pending, waiting up to `park` while nothing is.
    pub fn take(&self, park: Duration) -> Batch {
        let inner = self.lock();
        #[cfg(test)]
        self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (mut inner, _) = self
            .0
             .1
            .wait_timeout_while(inner, park, |inner| !inner.dirty)
            .unwrap_or_else(PoisonError::into_inner);
        inner.dirty = false;
        let mut batch = inner.coalescer.take();
        // The organization stays in Rust; the webview gets each row's project
        // and the headers (zerus-5wmh.31), only when they changed.
        batch.organizations.clear();
        let (groups, grouping) = inner.grouping.take();
        batch.groups = groups;
        batch.grouping = grouping;
        batch
    }

    /// Queues the full last-known catalog again (a reloaded webview starts empty).
    pub fn resend(&self) {
        let mut inner = self.lock();
        inner.grouping.resend();
        let changes = inner.mirror.changes();
        for change in changes {
            inner.coalescer.push(change);
        }
        self.wake(inner);
    }

    /// The organization the list shows; `revision` changes whenever it does.
    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn present(
        &self,
        organization: &Value,
        membership: Arc<BTreeMap<String, String>>,
        revision: u64,
        sync: SyncView,
    ) {
        let mut inner = self.lock();
        inner.grouping.present(organization, membership, revision);
        inner.grouping.set_sync(sync);
        self.wake(inner);
    }

    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn set_sync(&self, sync: SyncView) {
        let mut inner = self.lock();
        inner.grouping.set_sync(sync);
        self.wake(inner);
    }

    /// An edit the user made, applied to the list at once.
    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn edited(&self, intent: &Intent) {
        let mut inner = self.lock();
        inner.grouping.edited(intent);
        self.wake(inner);
    }

    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn set_collapsed(&self, project: &str, collapsed: bool) {
        let mut inner = self.lock();
        inner.grouping.set_collapsed(project, collapsed);
        self.wake(inner);
    }

    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn members_of(&self, project: &str) -> Vec<(String, String)> {
        self.lock().grouping.members_of(project)
    }

    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn default_project(&self) -> String {
        self.lock().grouping.default_project().to_owned()
    }

    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn take_renames(&self) -> Vec<(String, String)> {
        self.lock().grouping.take_renames()
    }

    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn take_layout(&self) -> Option<Vec<u8>> {
        self.lock().grouping.take_layout()
    }

    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn load_layout(&self, bytes: &[u8]) {
        let mut inner = self.lock();
        inner.grouping.replace_layout(Layout::from_json(bytes));
        self.wake(inner);
    }

    /// The fleet as the screens see it: every computer whose status has not
    /// ended, this computer first (the mirror is keyed by the id's string,
    /// and "" sorts first), with its machine card's host, platform and the
    /// control-console fields. Without a card a computer keeps local sessions.
    pub fn fleet(&self) -> Vec<FleetEntry> {
        let inner = self.lock();
        inner
            .mirror
            .computers
            .iter()
            .filter_map(|(computer, rows)| {
                let status = rows.status?;
                if status == ComputerStatus::Ended {
                    return None;
                }
                let machine = rows.machine.as_ref();
                Some(FleetEntry {
                    computer: ComputerId(computer.clone()),
                    status: status.name().to_owned(),
                    attempt: status.attempt(),
                    local_sessions: machine.and_then(|m| m.local_sessions).unwrap_or(true),
                    local_host: machine.is_some_and(|m| m.local_host.is_some()),
                    host: machine.and_then(|m| m.host.clone()),
                    platform: machine.and_then(|m| m.platform.clone()),
                })
            })
            .collect()
    }

    /// Drains a change sink on its own thread until every sender is gone.
    pub fn drain(&self, changes: Receiver<Change>) {
        self.drain_with(changes, |_| {});
    }

    /// Like [`Pump::drain`], running `observe` on each change just before it is queued.
    #[allow(dead_code, reason = "wired by the organization hub in the next task")]
    pub fn drain_with(
        &self,
        changes: Receiver<Change>,
        observe: impl Fn(&Change) + Send + 'static,
    ) {
        let pump = self.clone();
        std::thread::Builder::new()
            .name("zerus-client-pump".into())
            .spawn(move || {
                for change in changes {
                    observe(&change);
                    pump.push(change);
                }
            })
            .expect("the pump thread starts");
    }
}

/// The last-known rows (in arrival order), status and machine card of every
/// computer.
#[derive(Default)]
struct Mirror {
    /// Keyed by the computer id's string, so the core's id type needs no traits.
    computers: BTreeMap<String, Rows>,
}

#[derive(Default)]
struct Rows {
    status: Option<ComputerStatus>,
    /// The latest card data; an `ended` status drops it.
    machine: Option<MachineMeta>,
    order: Vec<String>,
    rows: HashMap<String, ViewRow>,
}

impl Rows {
    fn upsert(&mut self, key: &str, row: &ViewRow) {
        if self.rows.insert(key.to_owned(), row.clone()).is_none() {
            self.order.push(key.to_owned());
        }
    }
}

impl Mirror {
    fn observe(&mut self, change: &Change) {
        match change {
            Change::Upsert { computer, key, row } => self
                .computers
                .entry(computer.0.clone())
                .or_default()
                .upsert(key, row),
            Change::Remove { computer, key, .. } => {
                if let Some(rows) = self.computers.get_mut(&computer.0) {
                    if rows.rows.remove(key).is_some() {
                        rows.order.retain(|k| k != key);
                    }
                }
            }
            Change::Replace { computer, rows } => {
                let entry = self.computers.entry(computer.0.clone()).or_default();
                entry.order.clear();
                entry.rows.clear();
                for (key, row) in rows {
                    entry.upsert(key, row);
                }
            }
            Change::Computer { computer, status } => {
                let entry = self.computers.entry(computer.0.clone()).or_default();
                entry.status = Some(*status);
                if *status == ComputerStatus::Ended {
                    entry.machine = None;
                }
            }
            Change::Machine { computer, machine } => {
                self.computers
                    .entry(computer.0.clone())
                    .or_default()
                    .machine = Some(machine.clone())
            }
            Change::Organization { .. }
            | Change::Activity { .. }
            | Change::Processes { .. }
            | Change::Tasks { .. } => {}
        }
    }

    fn changes(&self) -> Vec<Change> {
        let mut out = Vec::new();
        for (computer, rows) in &self.computers {
            if let Some(status) = &rows.status {
                out.push(Change::Computer {
                    computer: ComputerId(computer.clone()),
                    status: *status,
                });
            }
            if let Some(machine) = &rows.machine {
                out.push(Change::Machine {
                    computer: ComputerId(computer.clone()),
                    machine: machine.clone(),
                });
            }
            let list = rows
                .order
                .iter()
                .filter_map(|key| rows.rows.get(key).map(|row| (key.clone(), row.clone())))
                .collect();
            out.push(Change::Replace {
                computer: ComputerId(computer.clone()),
                rows: list,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use zerus_client_core::OrganizationUpdate;

    fn row(name: &str) -> ViewRow {
        ViewRow {
            name: name.into(),
            ..ViewRow::default()
        }
    }

    fn upsert(key: &str, name: &str) -> Change {
        Change::Upsert {
            computer: ComputerId(String::new()),
            key: key.into(),
            row: row(name),
        }
    }

    #[test]
    fn take_parks_while_nothing_is_pending() {
        let pump = Pump::default();
        let started = Instant::now();
        let batch = pump.take(Duration::from_millis(60));
        assert!(started.elapsed() >= Duration::from_millis(55));
        assert!(batch.upserts.is_empty() && batch.replaces.is_empty() && batch.activity.is_empty());
    }

    #[test]
    fn a_change_wakes_a_parked_take() {
        let pump = Pump::default();
        let (tx, rx) = std::sync::mpsc::channel();
        pump.drain(rx);
        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            tx.send(upsert("run:a", "claude/p/a")).unwrap();
        });
        let started = Instant::now();
        let batch = pump.take(Duration::from_secs(5));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(batch.upserts.len(), 1);
        sender.join().unwrap();
    }

    #[test]
    fn a_burst_becomes_one_batch_with_the_latest_row_per_key() {
        let pump = Pump::default();
        for n in 0..5000 {
            pump.push(upsert(&format!("run:{}", n % 50), &format!("claude/p/{n}")));
        }
        let batch = pump.take(PARK);
        assert_eq!(batch.upserts.len(), 50);
        assert!(batch.upserts.iter().all(|r| r
            .row
            .name
            .ends_with(&format!("{}", 4950 + r.key[4..].parse::<usize>().unwrap()))));
    }

    #[test]
    fn resend_queues_the_last_known_catalog() {
        let pump = Pump::default();
        pump.push(Change::Replace {
            computer: ComputerId(String::new()),
            rows: vec![("run:a".into(), row("a")), ("run:b".into(), row("b"))],
        });
        pump.push(Change::Remove {
            computer: ComputerId(String::new()),
            key: "run:a".into(),
            reason: Some("archived".into()),
        });
        pump.push(upsert("run:c", "c"));
        pump.push(Change::Computer {
            computer: ComputerId(String::new()),
            status: ComputerStatus::Live,
        });
        let _ = pump.take(PARK);

        pump.resend();
        let batch = pump.take(PARK);
        assert_eq!(batch.replaces.len(), 1);
        let keys: Vec<_> = batch.replaces[0]
            .rows
            .iter()
            .map(|r| r.key.as_str())
            .collect();
        assert_eq!(keys, ["run:b", "run:c"]);
        assert_eq!(batch.computers[0].status, "live");
    }

    #[test]
    fn the_fleet_lists_unended_computers_with_their_cards_this_computer_first() {
        let pump = Pump::default();
        for (computer, status) in [
            ("studio", ComputerStatus::Reconnecting { attempt: 2 }),
            ("", ComputerStatus::Live),
            ("old", ComputerStatus::Ended),
        ] {
            pump.push(Change::Computer {
                computer: ComputerId(computer.into()),
                status,
            });
        }
        let card = MachineMeta {
            host: Some("workstation".into()),
            platform: Some("windows".into()),
            local_sessions: Some(false),
            local_host: Some(zerus_client_core::machine::LocalHost::default()),
            ..MachineMeta::default()
        };
        pump.push(Change::Machine {
            computer: ComputerId(String::new()),
            machine: card,
        });
        let fleet = pump.fleet();
        let rows: Vec<(&str, &str, u32)> = fleet
            .iter()
            .map(|e| (e.computer.0.as_str(), e.status.as_str(), e.attempt))
            .collect();
        assert_eq!(rows, [("", "live", 0), ("studio", "reconnecting", 2)]);
        assert!(!fleet[0].local_sessions && fleet[0].local_host);
        assert_eq!(
            (fleet[0].host.as_deref(), fleet[0].platform.as_deref()),
            (Some("workstation"), Some("windows"))
        );
        assert!(
            fleet[1].local_sessions && fleet[1].host.is_none(),
            "no card yet: local sessions assumed"
        );
    }

    fn machine(computer: &str, cpu: f64) -> Change {
        let mut meta = MachineMeta {
            host: Some(format!("{computer}-host")),
            ..MachineMeta::default()
        };
        meta.metrics.cpu_percent = Some(cpu);
        Change::Machine {
            computer: ComputerId(computer.into()),
            machine: meta,
        }
    }

    #[test]
    fn resend_replays_the_latest_machine_card_and_none_for_an_ended_computer() {
        let pump = Pump::default();
        for (computer, status) in [("", ComputerStatus::Live), ("studio", ComputerStatus::Live)] {
            pump.push(Change::Computer {
                computer: ComputerId(computer.into()),
                status,
            });
        }
        pump.push(machine("", 10.0));
        pump.push(machine("", 20.0));
        pump.push(machine("studio", 5.0));
        pump.push(Change::Replace {
            computer: ComputerId("studio".into()),
            rows: vec![],
        });
        pump.push(Change::Computer {
            computer: ComputerId("studio".into()),
            status: ComputerStatus::Ended,
        });
        let _ = pump.take(PARK);

        pump.resend();
        let batch = pump.take(PARK);
        assert_eq!(batch.machines.len(), 1, "an ended computer replays no card");
        assert_eq!(batch.machines[0].computer, ComputerId(String::new()));
        assert_eq!(batch.machines[0].machine.metrics.cpu_percent, Some(20.0));
        let statuses: Vec<_> = batch
            .computers
            .iter()
            .map(|c| (c.computer.0.as_str(), c.status.as_str()))
            .collect();
        assert_eq!(statuses, [("", "live"), ("studio", "ended")]);
        // The replayed changes put each card right after its status.
        let replayed = Mirror::changes(&pump.lock().mirror);
        let kinds: Vec<_> = replayed
            .iter()
            .map(|change| match change {
                Change::Computer { .. } => "computer",
                Change::Machine { .. } => "machine",
                Change::Replace { .. } => "replace",
                _ => "other",
            })
            .collect();
        assert_eq!(
            kinds,
            ["computer", "machine", "replace", "computer", "replace"]
        );
    }

    fn organization() -> Value {
        serde_json::json!({"default_project": "ungrouped", "projects": [
            {"id": "alpha", "name": "Alpha", "sessions": ["workstation\nclaude/p/a"]},
            {"id": "ungrouped", "name": "Inbox", "sessions": []}]})
    }

    fn local() -> ComputerId {
        ComputerId(String::new())
    }

    fn present(pump: &Pump, organization: &Value) {
        pump.present(
            organization,
            Arc::new(zerus_client_core::swarm::members(organization)),
            1,
            SyncView::default(),
        );
    }

    fn a_row() -> Change {
        Change::Replace {
            computer: local(),
            rows: vec![(
                "run:a".into(),
                ViewRow {
                    machine: "workstation".into(),
                    name: "claude/p/a".into(),
                    ..ViewRow::default()
                },
            )],
        }
    }

    #[test]
    fn the_batch_carries_each_rows_project_and_the_headers_never_the_organization() {
        let pump = Pump::default();
        pump.push(a_row());
        let organization = organization();
        pump.push(Change::Organization {
            computer: local(),
            update: OrganizationUpdate::State {
                value: Arc::new(serde_json::json!({"organization": organization.clone()})),
                digest: "d1".into(),
                membership: Arc::default(),
            },
        });
        present(&pump, &organization);
        let batch = pump.take(PARK);
        assert!(
            batch.organizations.is_empty(),
            "the organization stays in Rust"
        );
        assert_eq!(batch.groups.len(), 1);
        assert_eq!(batch.groups[0].group, "alpha");
        assert_eq!(batch.grouping.as_ref().unwrap().headers.len(), 2);
        let json = serde_json::to_string(&batch).unwrap();
        assert!(!json.contains("organizations") && !json.contains("\"sessions\""));
        pump.resend();
        let again = pump.take(PARK);
        assert_eq!(
            again.groups.len(),
            1,
            "a reloaded webview gets every row's project again"
        );
        assert!(again.grouping.is_some());
        assert!(again.organizations.is_empty());
    }

    /// Parks a `take` on another thread, runs `change` once it is waiting
    /// (the lock is held until then), and returns the batch it woke with.
    fn wake_with(pump: &Pump, change: impl FnOnce(&Pump)) -> Batch {
        let before = pump.1.load(std::sync::atomic::Ordering::SeqCst);
        let (tx, rx) = std::sync::mpsc::channel();
        let parked = {
            let pump = pump.clone();
            std::thread::spawn(move || tx.send(pump.take(Duration::from_secs(30))).unwrap())
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while pump.1.load(std::sync::atomic::Ordering::SeqCst) == before {
            assert!(Instant::now() < deadline, "the take never parked");
            std::thread::yield_now();
        }
        change(pump);
        let batch = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the change woke the parked take");
        parked.join().unwrap();
        batch
    }

    #[test]
    fn a_collapse_wakes_a_parked_take() {
        let pump = Pump::default();
        pump.push(a_row());
        present(&pump, &organization());
        let _ = pump.take(PARK);
        let batch = wake_with(&pump, |pump| pump.set_collapsed("alpha", true));
        assert!(batch.grouping.unwrap().headers[0].collapsed);
    }

    #[test]
    fn presenting_an_organization_wakes_a_parked_take() {
        let pump = Pump::default();
        pump.push(a_row());
        let _ = pump.take(PARK);
        let batch = wake_with(&pump, |pump| present(pump, &organization()));
        assert_eq!(batch.groups[0].group, "alpha");
        assert!(batch.grouping.is_some());
    }

    #[test]
    fn a_loaded_layout_keeps_the_observed_rows() {
        let pump = Pump::default();
        pump.push(a_row());
        present(&pump, &organization());
        let _ = pump.take(PARK);
        pump.load_layout(br#"{"collapsed":["alpha"]}"#);
        let batch = pump.take(PARK);
        assert!(batch.grouping.unwrap().headers[0].collapsed);
        pump.resend();
        let again = pump.take(PARK);
        assert_eq!(
            again.groups.len(),
            1,
            "the observed row survives the layout"
        );
        assert_eq!(again.groups[0].group, "alpha");
    }

    #[test]
    fn resend_replays_the_grouping_of_the_latest_organization_until_its_rows_are_replaced_empty() {
        let pump = Pump::default();
        pump.push(Change::Computer {
            computer: local(),
            status: ComputerStatus::Live,
        });
        pump.push(a_row());
        pump.push(Change::Organization {
            computer: local(),
            update: OrganizationUpdate::Failed {
                code: "organization_failed".into(),
                message: "damaged".into(),
            },
        });
        present(&pump, &organization());
        let _ = pump.take(PARK);
        pump.resend();
        let batch = pump.take(PARK);
        assert!(batch.organizations.is_empty());
        assert_eq!(batch.groups.len(), 1);
        assert!(batch.grouping.is_some());
        pump.push(Change::Organization {
            computer: local(),
            update: OrganizationUpdate::Ended,
        });
        pump.push(Change::Replace {
            computer: local(),
            rows: vec![],
        });
        pump.push(Change::Computer {
            computer: local(),
            status: ComputerStatus::Ended,
        });
        let _ = pump.take(PARK);
        pump.resend();
        let batch = pump.take(PARK);
        assert!(batch.organizations.is_empty());
        assert!(
            batch.groups.is_empty(),
            "no stale project for an ended computer's rows"
        );
    }
}
