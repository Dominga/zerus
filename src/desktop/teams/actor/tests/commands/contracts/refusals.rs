//! The reviewer's F1/F3 sequences, through the real isolated source CLI.
use super::super::super::super::{
    cancel::{Answer, Fact, Pending},
    native::Native,
    ports::{DeliveryResult, Ports},
    Runtime,
};
use super::*;
use crate::teams::ops::{HandoutOutcome, KeyClearState};
use std::sync::{atomic::AtomicBool, Arc};
use zerus_replica::execution::{NativeRequest, Origin, Receipt, ReportKind};

struct Real<'a> {
    f: &'a Fixture,
    native: Native,
    children: usize,
    records: Vec<Fact>,
    states: Vec<String>,
    clears: usize,
}
impl Ports for Real<'_> {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        unreachable!()
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
        unreachable!("terminal task")
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        Ok(false)
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
        unreachable!()
    }
    fn cancels(&mut self, s: &Store, t: &str, after: u32) -> Result<Vec<Pending>> {
        Native::pending_cancels(s, t, after)
    }
    fn cancel(&mut self, t: &Team, pending: &Pending, original: &str) -> Result<Answer> {
        let (args, input) = self.f.cancel_vector();
        let reply = self.f.call(&args, &input, false);
        self.children += 1;
        self.states.push(reply["state"].as_str().unwrap().into());
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|s| *s.borrow_mut() = Some(reply.to_string().into_bytes()));
        self.native.run_cancel(t, pending, original)
    }
    fn record_cancel(&mut self, s: &mut Store, t: &Team, f: &Fact, now: i64) -> Result<()> {
        self.records.push(f.clone());
        Native::save_cancel(s, t, f, now)
    }
    fn terminal_timers(
        &mut self,
        _: &mut super::super::super::super::auxiliary::State,
        s: &mut Store,
        now: i64,
    ) -> Result<()> {
        for k in s.keyed_tasks(TEAM, 0, 16)? {
            let (args, input) = self.f.clear_vector(k.node, &k.task);
            assert_eq!(self.f.call(&args, &input, false)["state"], "cleared");
            super::super::super::super::write(
                s,
                TEAM,
                Operation::RecordKeyClear {
                    node: k.node,
                    state: KeyClearState::Cleared,
                    reason: String::new(),
                },
                now,
            )?;
            self.clears += 1;
        }
        Ok(())
    }
}
fn drive(f: &Fixture, count: i64, answered: &str) {
    let team = f.h.store.team(TEAM).unwrap().unwrap();
    let native = Native {
        config: f.config.clone(),
        machine: team.home.clone(),
        epoch: "refusal-contract".into(),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let mut p = Real {
        f,
        native,
        children: 0,
        records: vec![],
        states: vec![],
        clears: 0,
    };
    let mut s = Store::open(&f.config.state).unwrap();
    let mut runtime = Runtime::open(&f.config.state, &team.home).unwrap();
    for pass in 0..count {
        runtime
            .page(&mut s, &mut p, &[], 100 + pass * 900_000, false)
            .unwrap();
    }
    assert_eq!(p.children, 1);
    assert_eq!(p.states, vec![answered.to_owned()]);
    assert_eq!(p.records.len(), 1);
    assert!(!p.records[0].refused);
    assert_eq!(p.records[0].released, answered == "released");
    assert!(!s.request_refused(TEAM, &p.records[0].request_id).unwrap());
    assert!(s.pending_cancels(TEAM, 16, 0).unwrap().is_empty());
    assert_eq!(p.clears, 1);
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    assert!(tasks.load(&f.task.uuid).unwrap().team().unwrap().is_none());
}
fn delivered(f: &mut Fixture) {
    let id = f.id.clone();
    assert_eq!(f.delivery(&id, "m1", false)["state"], "submitted");
    f.h.outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "own delivery before observation".into(),
        },
    );
}
fn report_unobserved(f: &Fixture, id: &str) {
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    let mut doc = tasks.load(&f.task.uuid).unwrap();
    let holder = doc
        .view()
        .unwrap()
        .execution
        .unwrap()
        .current
        .unwrap()
        .executor
        .unwrap();
    let actor = TaskActor::Agent(holder);
    let at = chrono::Utc::now().timestamp_millis();
    doc.begin_attempt(id, &actor, at).unwrap();
    assert!(
        doc.report_attempt(id, "Work is ready", ReportKind::Ready, None, &actor, 101)
            .unwrap()
            .moves
    );
    tasks.save(&mut doc).unwrap();
}
fn stopped(f: &mut Fixture, archived: bool) {
    f.h.run(&person(), Operation::Stop).unwrap();
    if archived {
        f.h.run(&person(), Operation::Archive).unwrap();
    }
}
fn first_refusal_variants() {
    for archived in [false, true] {
        let mut f = Fixture::new();
        delivered(&mut f);
        stopped(&mut f, archived);
        drive(&f, 20, "cancelled");
    }
    let mut f = Fixture::new();
    f.submitted();
    f.reported();
    f.h.run(
        &person(),
        Operation::Return {
            node: f.node,
            reason: "Revise".into(),
        },
    )
    .unwrap();
    let id = f.h.reopen(f.node, "m1");
    assert_eq!(f.delivery(&id, "m1", false)["state"], "submitted");
    f.h.reopen_outcome(
        &id,
        HandoutOutcome::Submitted {
            receipt: "reopened".into(),
        },
    );
    stopped(&mut f, false);
    drive(&f, 20, "cancelled");
    let mut f = Fixture::new();
    delivered(&mut f);
    report_unobserved(&f, &f.id);
    stopped(&mut f, false);
    drive(&f, 20, "released");
}
fn copy_attempt(base: &[u8], to: &SessionKey, at: i64) -> zerus_replica::TaskDoc {
    let mut doc = zerus_replica::TaskDoc::from_bytes(base).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let native = NativeRequest {
        machine: to.machine.clone(),
        name: to.session.clone(),
        run_id: "run-m1".into(),
        conversation_id: "conversation-m1".into(),
        digest: "a".repeat(64),
    };
    doc.reserve_attempt(
        &id,
        Origin::Send,
        to,
        native.clone(),
        &TaskActor::Person,
        at,
    )
    .unwrap();
    doc.settle_attempt(
        &id,
        zerus_replica::Settlement::Receipt(&Receipt {
            request_id: id.clone(),
            name: native.name,
            run_id: native.run_id,
            conversation_id: native.conversation_id,
            digest: native.digest,
            first_message: false,
            submitted: true,
        }),
        &TaskActor::Person,
        at + 1,
    )
    .unwrap();
    doc
}
fn conflict_observed(f: &mut Fixture) {
    let id = f.id.clone();
    f.h.outcome(
        &id,
        HandoutOutcome::Refused {
            reason: "Not delivered".into(),
        },
    );
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    let mut doc = tasks.load(&f.task.uuid).unwrap();
    let base = doc.snapshot().unwrap();
    let member =
        f.h.store
            .member(TEAM, "m1")
            .unwrap()
            .unwrap()
            .session
            .unwrap();
    let to = SessionKey {
        machine: member.machine,
        session_id: member.session_id,
        session: "codex/command-m1".into(),
    };
    for at in [20, 30] {
        doc.import(&copy_attempt(&base, &to, at).snapshot().unwrap())
            .unwrap();
    }
    tasks.save(&mut doc).unwrap();
    drop(tasks);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let o = f.h.store.task_observation(TEAM, f.node).unwrap().unwrap();
    assert_eq!(
        (o.attempt, o.attempt_id.as_str(), o.attempt_conflict),
        (1, "", true)
    );
}
fn person_cancels(f: &Fixture) {
    let tasks = ProjectStore::open(&f.config.state.join("replica"), &f.task.project).unwrap();
    let mut doc = tasks.load(&f.task.uuid).unwrap();
    doc.record_cancel("The person cancelled it", &TaskActor::Person, 40)
        .unwrap();
    tasks.save(&mut doc).unwrap();
}
pub(super) fn run() {
    first_refusal_variants();
    for before_stop in [false, true] {
        let mut f = Fixture::new();
        conflict_observed(&mut f);
        if before_stop {
            person_cancels(&f);
        }
        stopped(&mut f, false);
        if !before_stop {
            person_cancels(&f);
        }
        drive(&f, 100, "cancelled");
    }
}
