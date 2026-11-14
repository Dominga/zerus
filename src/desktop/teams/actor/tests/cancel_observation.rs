//! Only the native command's already obtained answer is replaced. The ordinary
//! production observer reads real private documents and records the sole end.
use super::super::{native::Native, Runtime};
use super::*;
use crate::teams::ops::Operation;
use std::path::Path;
use zerus_replica::{Actor as TaskActor, Position, ProjectStore, Stage, Target};
fn edit(state: &Path, task: &TaskRef, change: impl FnOnce(&mut zerus_replica::TaskDoc)) {
    let store = ProjectStore::open(&state.join("replica"), &task.project).unwrap();
    let mut doc = store.load(&task.uuid).unwrap();
    change(&mut doc);
    store.save(&mut doc).unwrap();
}
fn executor(machine: &str) -> zerus_replica::SessionKey {
    zerus_replica::SessionKey {
        machine: machine.into(),
        session: "codex/example/member".into(),
        session_id: "run_id:member".into(),
    }
}
fn fixture() -> (Harness, Native, TaskRef, super::super::cancel::Pending) {
    let mut h = Harness::new();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let tasks = ProjectStore::open(&h.temp.path().join("replica"), &team.project).unwrap();
    let mut doc = tasks
        .new_task(
            "Work",
            Stage::Todo,
            Target::Any,
            TaskActor::Person,
            &Position::End,
            "example",
            1,
        )
        .unwrap();
    let task = TaskRef {
        project: team.project.clone(),
        uuid: doc.uuid().into(),
    };
    tasks.save(&mut doc).unwrap();
    drop(tasks);
    let node = crate::teams::tasks::send(
        &crate::teams::tasks::Caller {
            state: h.temp.path(),
            machine: &team.home,
            actor: TaskActor::Person,
        },
        &task,
        TEAM,
        None,
        None,
    )
    .unwrap()
    .node
    .unwrap();
    send(h.temp.path(), &task, &team.home, 2);
    crate::teams::tasks::observe(h.temp.path(), &task).unwrap();
    h.run(
        &person(),
        Operation::Cancel {
            node,
            reason: "Cancel this work".into(),
        },
    )
    .unwrap();
    let pending = Native::pending_cancels(&h.store, TEAM, 0)
        .unwrap()
        .remove(0);
    edit(h.temp.path(), &task, |doc| {
        doc.record_cancel("Cancelled by its command", &TaskActor::Person, 3)
            .unwrap()
    });
    let native = super::production::native(&h);
    (h, native, task, pending)
}
fn send(state: &Path, task: &TaskRef, machine: &str, now: i64) {
    use zerus_replica::execution::{NativeRequest, Origin, Receipt};
    edit(state, task, |doc| {
        doc.floor().unwrap();
        if doc.view().unwrap().stage != Stage::Todo {
            doc.set_stage(Stage::Todo, &TaskActor::Person, now).unwrap();
        }
        let to = executor(machine);
        let id = uuid::Uuid::new_v4().to_string();
        let native = NativeRequest {
            machine: machine.into(),
            name: to.session.clone(),
            run_id: "fixture-run".into(),
            conversation_id: "fixture-conversation".into(),
            digest: "a".repeat(64),
        };
        doc.reserve_attempt(
            &id,
            Origin::Send,
            &to,
            native.clone(),
            &TaskActor::Person,
            now,
        )
        .unwrap();
        doc.settle_attempt(
            &id,
            zerus_replica::Settlement::Receipt(&Receipt {
                request_id: id.clone(),
                name: native.name,
                run_id: native.run_id,
                conversation_id: native.conversation_id,
                first_message: false,
                digest: native.digest,
                submitted: true,
            }),
            &TaskActor::Person,
            now + 1,
        )
        .unwrap();
    });
}
fn revive(state: &Path, task: &TaskRef, machine: &str) {
    send(state, task, machine, 4);
}

fn answer(pending: &super::super::cancel::Pending) {
    crate::desktop::teams::child::FIXTURE_EXIT.with(|slot| {
        *slot.borrow_mut() = Some(
            json!({"schema":1,"team":TEAM,"node":pending.node,"state":"cancelled","error":"",
            "attempt":pending.attempt,"attempt_id":pending.attempt_id})
            .to_string()
            .into_bytes(),
        )
    });
}
fn outcomes(h: &Harness, node: u32) -> u32 {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM cancel_outcomes WHERE team=?1 AND node=?2",
            rusqlite::params![TEAM, node],
            |r| r.get(0),
        )
        .unwrap()
}
fn stop(state: &Path) {
    let mut store = Store::open(state).unwrap();
    let revision = store.team(TEAM).unwrap().unwrap().revision;
    crate::teams::ops::apply(
        &mut store,
        &person(),
        &crate::teams::ops::tests::request(revision, Operation::Stop),
        100,
    )
    .unwrap();
}
#[test]
fn live_cancelled_is_observed_in_the_same_visit_and_never_recorded_as_a_second_end() {
    for revived in [false, true] {
        let (mut h, mut native, task, pending) = fixture();
        let team = h.store.team(TEAM).unwrap().unwrap();
        let generation = h.store.generation(TEAM, pending.node).unwrap().unwrap();
        let mut runtime = Runtime::open(h.temp.path(), &team.home).unwrap();
        let state = h.temp.path().to_owned();
        let machine = team.home.clone();
        if revived {
            crate::desktop::original_child::AFTER_SPAWN.with(|s| {
                *s.borrow_mut() = Some(Box::new(move || revive(&state, &task, &machine)))
            });
        }
        answer(&pending);
        runtime
            .cancels(&mut h.store, &mut native, &team, 100)
            .unwrap();
        assert!(
            h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty(),
            "revived={revived}"
        );
        assert_eq!(
            outcomes(&h, pending.node),
            0,
            "only the observation records this live end"
        );
        let expected = if revived {
            NodeState::InProgress
        } else {
            NodeState::Cancelled
        };
        assert_eq!(h.node(pending.node).state, expected);
        assert_eq!(
            h.store.generation(TEAM, pending.node).unwrap(),
            Some(generation + u32::from(revived))
        );
        assert!(!runtime
            .ledger
            .page(TEAM, "")
            .unwrap()
            .iter()
            .any(|r| r.key.starts_with("cancel:")));
    }
}
#[test]
fn a_stop_after_the_cancel_answer_uses_the_terminal_fact_instead_of_observation() {
    let (mut h, native, _, pending) = fixture();
    let mut native = CountObservation { native, reads: 0 };
    let team = h.store.team(TEAM).unwrap().unwrap();
    let state = h.temp.path().to_owned();
    crate::desktop::original_child::AFTER_SPAWN
        .with(|slot| *slot.borrow_mut() = Some(Box::new(move || stop(&state))));
    answer(&pending);
    let mut runtime = Runtime::open(h.temp.path(), &team.home).unwrap();
    runtime
        .cancels(&mut h.store, &mut native, &team, 100)
        .unwrap();
    // Stop can supersede the old decision. That visit ends; its new pending
    // decision is handled on the next pass with a fresh original command.
    if let Some(next) = Native::pending_cancels(&h.store, TEAM, 0).unwrap().first() {
        answer(next);
        let fresh = h.store.team(TEAM).unwrap().unwrap();
        runtime
            .cancels(&mut h.store, &mut native, &fresh, 101)
            .unwrap();
    }
    assert_eq!(
        native.reads, 0,
        "Stop must prevent even a no-op task observation"
    );
    assert!(h.store.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
    assert!(
        !h.store
            .task_observation(TEAM, pending.node)
            .unwrap()
            .unwrap()
            .cancelled,
        "a terminal team must not be observed after the answer"
    );
    assert_eq!(outcomes(&h, pending.node), 1);
}

/// Count the real observer, preserving the production command, reader and
/// saved-fact adapters. A terminal no-op still counts as an unwanted call.
struct CountObservation {
    native: Native,
    reads: usize,
}
impl super::super::ports::Ports for CountObservation {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        unreachable!()
    }
    fn observe(&mut self, store: &mut Store, task: &TaskRef) -> Result<()> {
        self.reads += 1;
        super::super::ports::Ports::observe(&mut self.native, store, task)
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        unreachable!()
    }
    fn deliver(
        &mut self,
        _: &Team,
        _: &Node,
        _: &Member,
        _: &str,
    ) -> super::super::ports::DeliveryResult {
        unreachable!()
    }
    fn cancels(
        &mut self,
        store: &Store,
        team: &str,
        after: u32,
    ) -> Result<Vec<super::super::cancel::Pending>> {
        Native::pending_cancels(store, team, after)
    }
    fn cancel(
        &mut self,
        team: &Team,
        pending: &super::super::cancel::Pending,
        original: &str,
    ) -> Result<super::super::cancel::Answer> {
        self.native.run_cancel(team, pending, original)
    }
    fn record_cancel(
        &mut self,
        store: &mut Store,
        team: &Team,
        fact: &super::super::cancel::Fact,
        now: i64,
    ) -> Result<()> {
        Native::save_cancel(store, team, fact, now)
    }
}
