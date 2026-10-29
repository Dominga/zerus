//! Archive placement after a verified Terminate (SessionsWindow.cpp:3106-3125).
//! The first matching bounded archive page started after success supplies the
//! identities. Neither an old page nor an uncertain action can supply them.
use super::*;
use std::collections::{HashSet, VecDeque};
use uuid::Uuid;
use zerus_client_core::Target;

pub(crate) const MAX_TERMINATIONS: usize = 64;

#[derive(Clone)]
pub(crate) struct Termination {
    id: Uuid,
    computer: String,
    name: String,
    run: String,
    created: i64,
    machine: String,
}

struct Pending {
    capture: Termination,
    members: Vec<String>,
    submitted: bool,
}

#[derive(Default)]
pub(super) struct Terminations(VecDeque<Pending>);

impl Pump {
    /// Pin birth identity before the guarded CLI call, not after teardown.
    pub(crate) fn capture_termination(&self, target: &Target) -> Option<Termination> {
        if target.run_id.is_empty() {
            return None;
        }
        let inner = self.lock();
        let row = inner
            .mirror
            .computers
            .get(&target.computer)?
            .rows
            .values()
            .find(|row| {
                row.name == target.name
                    && row.run_id.as_deref() == Some(&target.run_id)
                    && row.state != "archived"
                    && row.archive_id.is_none()
                    && !row.machine.is_empty()
            })?;
        Some(Termination {
            id: Uuid::new_v4(),
            computer: target.computer.clone(),
            name: target.name.clone(),
            run: target.run_id.clone(),
            created: row.created,
            machine: row.machine.clone(),
        })
    }

    /// Called only for a successful guarded Terminate or Kill.
    pub(crate) fn termination_succeeded(&self, capture: Termination) {
        let mut inner = self.lock();
        inner.terminations.0.retain(|pending| {
            let old = &pending.capture;
            old.computer != capture.computer
                || old.name != capture.name
                || old.run != capture.run
                || old.created != capture.created
        });
        if inner.terminations.0.len() >= MAX_TERMINATIONS {
            inner.terminations.0.pop_front();
        }
        inner.terminations.0.push_back(Pending {
            capture,
            members: Vec::new(),
            submitted: false,
        });
        self.wake(inner);
    }

    /// Snapshot before starting one read; a later termination is ineligible.
    pub(crate) fn archive_termination_ids(&self, computer: &ComputerId) -> Vec<Uuid> {
        self.lock()
            .terminations
            .0
            .iter()
            .filter(|p| p.capture.computer == computer.0 && !p.submitted)
            .map(|p| p.capture.id)
            .collect()
    }

    pub(crate) fn observe_archive_terminations(
        &self,
        computer: &ComputerId,
        eligible: &[Uuid],
        rows: &[ViewRow],
    ) {
        let mut inner = self.lock();
        for pending in &mut inner.terminations.0 {
            let capture = &pending.capture;
            if capture.computer != computer.0
                || pending.submitted
                || !pending.members.is_empty()
                || !eligible.contains(&capture.id)
            {
                continue;
            }
            pending.members = rows
                .iter()
                .take(zerus_client_core::reads::ARCHIVE_PAGE_ROWS)
                .filter(|row| {
                    row.state == "archived"
                        && row.name == capture.name
                        && row.run_id.as_deref() == Some(capture.run.as_str())
                })
                .filter_map(|row| row.archive_id.as_deref().filter(|id| !id.is_empty()))
                .map(|id| zerus_client_core::swarm::archive_member(&capture.machine, id))
                .collect();
        }
    }

    /// One scan per relevant computer, never one catalog scan per pending
    /// termination. Temporary lookup storage is bounded by the 64 captures.
    pub(crate) fn archive_moves(&self) -> Vec<(Uuid, Intent)> {
        let inner = self.lock();
        let mut wanted: HashMap<&str, HashMap<(&str, &str, i64), Uuid>> = HashMap::new();
        for p in &inner.terminations.0 {
            if !p.submitted && !p.members.is_empty() {
                let c = &p.capture;
                wanted
                    .entry(&c.computer)
                    .or_default()
                    .insert((&c.name, &c.run, c.created), c.id);
            }
        }
        let mut online = HashSet::new();
        let mut remains = HashSet::new();
        for (computer, wanted) in wanted {
            let Some(rows) = inner.mirror.computers.get(computer) else {
                continue;
            };
            if !matches!(
                rows.status,
                Some(ComputerStatus::Live | ComputerStatus::Polled)
            ) || rows.machine.as_ref().and_then(|m| m.ok) == Some(false)
            {
                continue;
            }
            online.insert(computer);
            for row in rows.rows.values().filter(|row| row.state == "running") {
                if let Some(id) = wanted.get(&(
                    row.name.as_str(),
                    row.run_id.as_deref().unwrap_or(""),
                    row.created,
                )) {
                    remains.insert(*id);
                }
            }
        }
        inner
            .terminations
            .0
            .iter()
            .filter(|p| {
                !p.submitted
                    && !p.members.is_empty()
                    && online.contains(p.capture.computer.as_str())
                    && !remains.contains(&p.capture.id)
            })
            .map(|p| {
                (
                    p.capture.id,
                    Intent::Move {
                        members: p.members.clone(),
                        project: inner.grouping.group_for(&zerus_client_core::swarm::member(
                            &p.capture.machine,
                            &p.capture.name,
                        )),
                        before: None,
                    },
                )
            })
            .collect()
    }

    pub(crate) fn archive_move_submitted(&self, id: Uuid) {
        if let Some(p) = self
            .lock()
            .terminations
            .0
            .iter_mut()
            .find(|p| p.capture.id == id)
        {
            p.submitted = true;
        }
    }

    pub(crate) fn archive_move_saved(&self, id: Uuid) {
        self.lock().terminations.0.retain(|p| p.capture.id != id);
    }

    /// Taken under the organization hub lock with the draft snapshot: all
    /// submitted moves belong to that draft, never to a later checkpoint.
    pub(crate) fn submitted_termination_ids(&self) -> Vec<Uuid> {
        self.lock()
            .terminations
            .0
            .iter()
            .filter(|p| p.submitted)
            .map(|p| p.capture.id)
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn termination_count(&self) -> usize {
        self.lock().terminations.0.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> (Pump, Target, ViewRow) {
        let pump = Pump::default();
        let target = Target {
            computer: "studio".into(),
            name: "codex/sample-project/review".into(),
            run_id: "run-1".into(),
            conversation_id: "conversation-1".into(),
        };
        let row = ViewRow {
            machine: "studio".into(),
            name: target.name.clone(),
            run_id: Some(target.run_id.clone()),
            conversation_id: Some(target.conversation_id.clone()),
            created: 7,
            state: "running".into(),
            ..Default::default()
        };
        pump.push(Change::Computer {
            computer: ComputerId::from("studio"),
            status: ComputerStatus::Live,
        });
        pump.push(Change::Upsert {
            computer: ComputerId::from("studio"),
            key: "run:run-1".into(),
            row: row.clone(),
        });
        let org = json!({"default_project":"ungrouped","projects":[{"id":"alpha","sessions":["studio\ncodex/sample-project/review"]},{"id":"ungrouped","sessions":[]}]});
        pump.present(
            &org,
            Arc::new(zerus_client_core::swarm::members(&org)),
            1,
            Default::default(),
        );
        (pump, target, row)
    }

    fn archived(row: &ViewRow) -> ViewRow {
        ViewRow {
            state: "archived".into(),
            archive_id: Some("00000000-0000-4000-8000-000000000001".into()),
            ..row.clone()
        }
    }

    fn removed(pump: &Pump) {
        pump.push(Change::Remove {
            computer: ComputerId::from("studio"),
            key: "run:run-1".into(),
            reason: Some("archived".into()),
        });
    }

    #[test]
    fn a_verified_termination_moves_the_archive_identity_once_from_a_fresh_page() {
        let (pump, target, row) = setup();
        let computer = ComputerId::from("studio");
        let old_page = pump.archive_termination_ids(&computer);
        pump.termination_succeeded(pump.capture_termination(&target).unwrap());
        pump.observe_archive_terminations(&computer, &old_page, &[archived(&row)]);
        removed(&pump);
        assert!(
            pump.archive_moves().is_empty(),
            "a page begun before success proves nothing"
        );
        let fresh = pump.archive_termination_ids(&computer);
        pump.observe_archive_terminations(&computer, &fresh, &[archived(&row)]);
        let moves = pump.archive_moves();
        assert_eq!(moves.len(), 1);
        assert_eq!(
            moves[0].1,
            Intent::Move {
                members: vec!["studio\narchive\n00000000-0000-4000-8000-000000000001".into()],
                project: "alpha".into(),
                before: None
            }
        );
        pump.archive_move_submitted(moves[0].0);
        assert!(
            pump.archive_moves().is_empty(),
            "an accepted edit is never submitted twice"
        );
        pump.archive_move_saved(moves[0].0);
        assert!(pump.archive_termination_ids(&computer).is_empty());
        pump.observe_archive_terminations(&computer, &fresh, &[archived(&row)]);
        assert!(pump.archive_moves().is_empty());
    }

    #[test]
    fn termination_placement_waits_for_a_matching_archive_and_verified_online_exit() {
        let (pump, target, row) = setup();
        let computer = ComputerId::from("studio");
        pump.termination_succeeded(pump.capture_termination(&target).unwrap());
        let ids = pump.archive_termination_ids(&computer);
        pump.observe_archive_terminations(
            &computer,
            &ids,
            &[ViewRow {
                state: "running".into(),
                ..archived(&row)
            }],
        );
        let mut wrong = archived(&row);
        wrong.name.push_str("-other");
        pump.observe_archive_terminations(&computer, &ids, &[wrong]);
        let mut wrong = archived(&row);
        wrong.run_id = Some("other-run".into());
        pump.observe_archive_terminations(&computer, &ids, &[wrong]);
        let mut wrong = archived(&row);
        wrong.archive_id = Some(String::new());
        pump.observe_archive_terminations(&computer, &ids, &[wrong]);
        removed(&pump);
        assert!(pump.archive_moves().is_empty());
        pump.observe_archive_terminations(&computer, &ids, &[archived(&row)]);
        pump.push(Change::Upsert {
            computer: computer.clone(),
            key: "run:run-1".into(),
            row: row.clone(),
        });
        assert!(pump.archive_moves().is_empty(), "the same birth still runs");
        removed(&pump);
        for status in [
            ComputerStatus::Probing,
            ComputerStatus::Unsupported,
            ComputerStatus::Reconnecting { attempt: 1 },
            ComputerStatus::Ended,
        ] {
            pump.push(Change::Computer {
                computer: computer.clone(),
                status,
            });
            assert!(
                pump.archive_moves().is_empty(),
                "an unknown/offline snapshot cannot verify exit"
            );
        }
        pump.push(Change::Computer {
            computer: computer.clone(),
            status: ComputerStatus::Live,
        });
        pump.push(Change::Machine {
            computer: computer.clone(),
            machine: MachineMeta {
                ok: Some(false),
                ..Default::default()
            },
        });
        assert!(
            pump.archive_moves().is_empty(),
            "a failed listing is not online proof"
        );
        pump.push(Change::Machine {
            computer: computer.clone(),
            machine: MachineMeta {
                ok: Some(true),
                ..Default::default()
            },
        });
        assert_eq!(pump.archive_moves().len(), 1);
        pump.push(Change::Computer {
            computer,
            status: ComputerStatus::Polled,
        });
        assert_eq!(
            pump.archive_moves().len(),
            1,
            "older polling peers can verify the same exit"
        );
    }

    #[test]
    fn termination_birth_verification_uses_name_run_created_and_running_state() {
        let (pump, target, row) = setup();
        let computer = ComputerId::from("studio");
        pump.termination_succeeded(pump.capture_termination(&target).unwrap());
        let ids = pump.archive_termination_ids(&computer);
        pump.observe_archive_terminations(&computer, &ids, &[archived(&row)]);
        assert!(pump.archive_moves().is_empty());
        for replacement in [
            ViewRow {
                created: 8,
                ..row.clone()
            },
            ViewRow {
                name: "codex/sample-project/other".into(),
                ..row.clone()
            },
            ViewRow {
                run_id: Some("new-run".into()),
                ..row.clone()
            },
            ViewRow {
                state: "paused".into(),
                ..row.clone()
            },
        ] {
            pump.push(Change::Upsert {
                computer: computer.clone(),
                key: "run:run-1".into(),
                row: replacement,
            });
            assert_eq!(
                pump.archive_moves().len(),
                1,
                "the original birth no longer runs"
            );
        }
    }

    #[test]
    fn termination_memory_and_retained_page_identities_are_bounded() {
        let (pump, target, row) = setup();
        let computer = ComputerId::from("studio");
        for n in 0..MAX_TERMINATIONS + 1 {
            let target = Target {
                name: format!("codex/sample-project/review-{n}"),
                run_id: format!("run-{n}"),
                ..target.clone()
            };
            let row = ViewRow {
                name: target.name.clone(),
                run_id: Some(target.run_id.clone()),
                ..row.clone()
            };
            pump.push(Change::Upsert {
                computer: computer.clone(),
                key: target.run_id.clone(),
                row,
            });
            pump.termination_succeeded(pump.capture_termination(&target).unwrap());
        }
        assert_eq!(
            pump.archive_termination_ids(&computer).len(),
            MAX_TERMINATIONS
        );
        assert_eq!(pump.termination_count(), MAX_TERMINATIONS);
        assert_eq!(
            pump.lock().terminations.0.front().unwrap().capture.name,
            "codex/sample-project/review-1"
        );
        pump.termination_succeeded(pump.capture_termination(&target).unwrap());
        let ids = pump.archive_termination_ids(&computer);
        let rows: Vec<_> = (0..60)
            .map(|n| ViewRow {
                archive_id: Some(format!("00000000-0000-4000-8000-{n:012}")),
                ..archived(&row)
            })
            .collect();
        pump.observe_archive_terminations(&computer, &ids, &rows);
        removed(&pump);
        let moves = pump.archive_moves();
        let Some((_, Intent::Move { members, .. })) = moves.iter().find(
            |(_, intent)| matches!(intent, Intent::Move { members, .. } if !members.is_empty()),
        ) else {
            panic!("no archive move")
        };
        assert_eq!(members.len(), zerus_client_core::reads::ARCHIVE_PAGE_ROWS);
    }
}
