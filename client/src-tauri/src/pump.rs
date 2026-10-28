//! From the core's change sink to `next_batch`: one thread drains the sink
//! into the coalescer, and `next_batch` takes everything pending, parking
//! up to [`PARK`] while nothing is. A mirror of the last-known rows, statuses
//! and machine cards lets a reloaded webview start from the full catalog.

use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

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
}

/// Shared by the drain thread and the commands.
#[derive(Clone, Default)]
pub struct Pump(Arc<(Mutex<Inner>, Condvar)>);

impl Pump {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0 .0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues one change and wakes a parked `take`.
    pub fn push(&self, change: Change) {
        let mut inner = self.lock();
        inner.mirror.observe(&change);
        inner.coalescer.push(change);
        inner.dirty = true;
        drop(inner);
        self.0 .1.notify_all();
    }

    /// Everything pending, waiting up to `park` while nothing is.
    pub fn take(&self, park: Duration) -> Batch {
        let inner = self.lock();
        let (mut inner, _) = self
            .0
             .1
            .wait_timeout_while(inner, park, |inner| !inner.dirty)
            .unwrap_or_else(PoisonError::into_inner);
        inner.dirty = false;
        inner.coalescer.take()
    }

    /// Queues the full last-known catalog again (a reloaded webview starts empty).
    pub fn resend(&self) {
        let mut inner = self.lock();
        let changes = inner.mirror.changes();
        for change in changes {
            inner.coalescer.push(change);
        }
        inner.dirty = true;
        drop(inner);
        self.0 .1.notify_all();
    }

    /// Drains a change sink on its own thread until every sender is gone.
    pub fn drain(&self, changes: Receiver<Change>) {
        let pump = self.clone();
        std::thread::Builder::new()
            .name("zerus-client-pump".into())
            .spawn(move || {
                for change in changes {
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
            Change::Activity { .. } | Change::Processes { .. } | Change::Tasks { .. } => {}
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
}
