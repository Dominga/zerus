//! Exact task documents are held before the Teams writer. A cached Todo
//! observation alone cannot exclude a person's unobserved send reservation.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use zerus_replica::{ProjectStore, Stage};
mod copy;

#[derive(Clone, Copy)]
pub(crate) enum Selection {
    Neighbors(u32),
    Interval(u32, u32),
    One(u32),
}

struct Proof {
    scope: PhaseControlScope,
    availability: Availability,
    tasks: Vec<(u32, TaskRef, String)>,
}

pub(crate) struct TaskCustody {
    team: String,
    home: String,
    project: String,
    revision: u64,
    phases: BTreeMap<u32, Proof>,
    // The cross-process project lock remains held through the caller's
    // final read or writer commit. This private handle has no other users.
    _store: Option<ProjectStore>,
    copy: Option<copy::Copy>,
}

fn blocked(reason: &str) -> Availability {
    Availability {
        allowed: false,
        reason: Some(reason.into()),
    }
}

fn selected(db: &Connection, team: &str, selection: Selection) -> Result<Vec<Phase>> {
    let mut ordered = phases(db, team)?;
    ordered.retain(|p| p.n != 0);
    ordered.sort_by_key(|p| (if p.order == 0 { p.n } else { p.order }, p.n));
    let position = |n| {
        ordered
            .iter()
            .position(|p| p.n == n)
            .ok_or_else(|| storage("The phase task custody scope is missing."))
    };
    let (lo, hi) = match selection {
        Selection::One(n) => {
            let i = position(n)?;
            (i, i)
        }
        Selection::Neighbors(n) => {
            let i = position(n)?;
            (i.saturating_sub(1), (i + 1).min(ordered.len() - 1))
        }
        Selection::Interval(a, b) => {
            let a = position(a)?;
            let b = position(b)?;
            (a.min(b), a.max(b))
        }
    };
    Ok(ordered[lo..=hi].to_vec())
}

impl TaskCustody {
    /// The first snapshot is released before acquiring project custody.
    /// Only indexed phase membership is read; no task/catalog enumeration.
    pub(crate) fn acquire(
        db: &Connection,
        state: &Path,
        team: &str,
        selection: Selection,
    ) -> Result<Self> {
        #[cfg(test)]
        fixture::note_read();
        let tx = db.unchecked_transaction().map_err(storage)?;
        let header = rows::team(&tx, team)?
            .ok_or_else(|| storage("The phase task custody team is missing."))?;
        let selected = selected(&tx, team, selection)?;
        let mut answer = Self {
            team: team.into(),
            home: header.home,
            project: header.project,
            revision: header.revision,
            phases: BTreeMap::new(),
            _store: None,
            copy: None,
        };
        let mut total = 0usize;
        for p in &selected {
            let availability = unstarted(&tx, team, p)?;
            let mut tasks = Vec::new();
            if availability.allowed && p.state == PhaseState::Approved {
                let snapshot = phase_snapshot::load(&tx, team, p.n, header.revision)?;
                if !snapshot.complete || snapshot.nodes.is_empty() {
                    return Err(storage("The phase task custody membership is incomplete."));
                }
                total = total.saturating_add(snapshot.nodes.len());
                if total > phase_snapshot::MAX_NODES {
                    return Err(Problem::new(
                        code::OVER_CAP,
                        "The phase task custody exceeds its node bound.",
                    ));
                }
                for n in snapshot.nodes {
                    let task = n
                        .task
                        .ok_or_else(|| storage("The phase task identity is missing."))?;
                    if task.project != answer.project
                        || !uuid::Uuid::parse_str(&task.uuid)
                            .is_ok_and(|id| id.to_string() == task.uuid)
                    {
                        return Err(storage(
                            "The phase task identity does not match its project.",
                        ));
                    }
                    tasks.push((n.id, task, String::new()));
                }
            }
            answer.phases.insert(
                p.n,
                Proof {
                    scope: scope(&tx, team, p.n)?,
                    availability,
                    tasks,
                },
            );
        }
        tx.commit().map_err(storage)?;
        if answer.phases.values().all(|p| p.tasks.is_empty()) {
            return Ok(answer);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let copy = copy::Copy::pin(state, &answer.project)?;
        // This existing-only opening also serves copy review. No review,
        // repair, cleanup, directory or task creation is performed here.
        let project = ProjectStore::open_copy_review(
            &state.join("replica"),
            &answer.project,
            Duration::from_millis(300),
        )
        .map_err(storage)?
        .ok_or_else(|| {
            Problem::new(
                code::STORE_BUSY,
                "The phase task store is busy; its current facts are unavailable.",
            )
        })?;
        if !copy.current() {
            return Err(storage(
                "The phase task copy changed while custody was acquired.",
            ));
        }
        {
            let mut remaining = copy::MAX_BYTES;
            let mut admitted = project.admit_until(deadline).ok_or_else(|| {
                Problem::new(
                    code::STORE_BUSY,
                    "The phase task store is busy; its current facts are unavailable.",
                )
            })?;
            for proof in answer.phases.values_mut() {
                for (_, task, revision) in &mut proof.tasks {
                    if Instant::now() >= deadline {
                        return Err(storage(
                            "The phase task custody read exceeded its time bound.",
                        ));
                    }
                    copy::task_bytes(project.dir(), &task.uuid, &mut remaining)?;
                    let doc = admitted.load_complete(&task.uuid).map_err(storage)?;
                    let view = doc.view().map_err(storage)?;
                    let key = doc.team().map_err(storage)?;
                    let ours = key.is_some_and(|k| k.team == team && k.home == answer.home);
                    if view.project != answer.project
                        || view.uuid != task.uuid
                        || !view.writable
                        || !ours
                        || doc.team_invalid()
                        || view.cancelled_at.is_some()
                        || !matches!(view.stage, Stage::Todo | Stage::Backlog)
                        || view.execution.is_some()
                        || !doc.attempt_phases().map_err(storage)?.is_empty()
                        || !doc.unexplained_entries().map_err(storage)?.is_empty()
                    {
                        proof.availability = blocked("A current task document has execution history, a reservation or an unavailable unstarted identity.");
                    }
                    *revision = doc.revision();
                    if Instant::now() >= deadline {
                        return Err(storage(
                            "The phase task custody read exceeded its time bound.",
                        ));
                    }
                }
            }
        }
        answer._store = Some(project);
        answer.copy = Some(copy);
        #[cfg(test)]
        fixture::acquired();
        Ok(answer)
    }

    /// Rechecked under the final read/writer snapshot while project custody
    /// remains held. Freshness complements, never replaces, retained history.
    pub(crate) fn check(&self, db: &Connection, team: &Team, p: &Phase) -> Result<Availability> {
        let Some(proof) = self.phases.get(&p.n) else {
            return Ok(blocked("The exact phase task custody is unavailable."));
        };
        if self.team != team.id
            || self.home != team.home
            || self.project != team.project
            || self.revision != team.revision
            || proof.scope != scope(db, &team.id, p.n)?
        {
            return Ok(blocked(
                "The phase task scope changed before custody was acquired.",
            ));
        }
        if !proof.availability.allowed {
            return Ok(proof.availability.clone());
        }
        if self.copy.as_ref().is_some_and(|copy| !copy.current()) {
            return Ok(blocked("The exact phase task copy changed under custody."));
        }
        if p.state == PhaseState::Approved {
            let snapshot = phase_snapshot::load(db, &team.id, p.n, team.revision)?;
            if !snapshot.complete || snapshot.nodes.len() != proof.tasks.len() {
                return Ok(blocked("The exact phase task custody is incomplete."));
            }
            for (node, (id, task, revision)) in snapshot.nodes.iter().zip(&proof.tasks) {
                let matches: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM nodes WHERE team=?1 AND id=?2 AND task_revision=?3 UNION ALL SELECT 1 FROM node_archive WHERE team=?1 AND id=?2 AND task_revision=?3)",
                    params![team.id,id,revision], |r|r.get(0),
                ).map_err(storage)?;
                if node.id != *id || node.task.as_ref() != Some(task) || !matches {
                    return Ok(blocked("The current task document no longer matches its retained unstarted observation."));
                }
            }
        }
        unstarted(db, &team.id, p)
    }

    /// Canonical references to the same retained project/documents, never a
    /// second read under Teams custody. This digest grants no admission.
    pub(crate) fn native_facts(
        &self,
        db: &Connection,
        team: &Team,
        selection: Selection,
    ) -> Result<String> {
        let required = selected(db, &team.id, selection)?;
        if required.len() != self.phases.len()
            || required.iter().any(|p| !self.phases.contains_key(&p.n))
        {
            return Err(Problem::new(
                code::INVALID_STATE,
                "The exact phase task custody is incomplete.",
            ));
        }
        let mut scopes = Vec::new();
        for (id, proof) in &self.phases {
            let p = phase(db, &team.id, *id)?;
            let current = self.check(db, team, &p)?;
            if !current.allowed {
                return Err(Problem::new(
                    code::INVALID_STATE,
                    current
                        .reason
                        .unwrap_or_else(|| "The exact phase task custody is unavailable.".into()),
                ));
            }
            scopes.push((&proof.scope, &proof.tasks));
        }
        let bytes = serde_json::to_vec(&(
            "phase-control-task-custody-v1",
            &self.team,
            &self.home,
            &self.project,
            self.revision,
            &self.copy,
            scopes,
        ))
        .map_err(storage)?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(Problem::new(
                code::OVER_CAP,
                "The phase task scope exceeds the control proof bounds.",
            ));
        }
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

pub(crate) fn availability(
    custody: Option<&Result<TaskCustody>>,
    db: &Connection,
    team: &Team,
    phase: &Phase,
) -> Result<Availability> {
    match custody {
        Some(Ok(held)) => held.check(db, team, phase),
        Some(Err(problem)) => Ok(blocked(match problem.code.as_str() {
            code::STORE_BUSY => "The phase task store is busy; its current facts are unavailable.",
            code::OVER_CAP => "The phase task scope exceeds the control proof bounds.",
            _ => "The current phase task documents are unavailable. Read again.",
        })),
        None => Ok(blocked("The exact phase task custody is unavailable.")),
    }
}

/// A thread-local test rendezvous, never an environment-controlled product
/// pause. It holds only the private fixture's already-acquired resources.
#[cfg(test)]
pub(crate) mod fixture {
    thread_local! {
        static ACQUIRED: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = Default::default();
        static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    pub(crate) fn reads() -> usize {
        READS.with(|reads| reads.get())
    }
    pub(super) fn note_read() {
        READS.with(|reads| reads.set(reads.get() + 1));
    }
    pub(crate) fn once(hook: impl FnOnce() + 'static) {
        ACQUIRED.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    }
    pub(super) fn acquired() {
        let hook = ACQUIRED.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook();
        }
    }
}
