//! The timers through their two entries, with fake sessions and effects and
//! a real store and service ledger in a private temporary state directory.
use super::*;
pub(super) use crate::teams::ops::{tests::*, HandoutOutcome, TaskStage};
use std::collections::{BTreeMap, VecDeque};
use zerus_team_plan::model::TaskRef;
mod answers;
mod cleanup;
mod doorbell;
mod late;
mod record;
mod timers;
mod work;

pub(super) const T0: i64 = 1234;
pub(super) const MINUTE: i64 = 60_000;

#[derive(Default)]
pub(super) struct FakeSessions(pub BTreeMap<String, Presence>);
impl FakeSessions {
    pub fn idle(&mut self, member: &str, run: &str) {
        self.0.insert(member.into(), presence(member, run));
    }
    pub fn working(&mut self, member: &str) {
        if let Some(p) = self.0.get_mut(member) {
            p.working = true;
        }
    }
}
pub(super) fn presence(member: &str, run: &str) -> Presence {
    Presence {
        name: format!("claude/example/{member}"),
        run: run.into(),
        conversation: format!("conversation-{member}"),
        live: true,
        working: false,
        background: false,
        pending: false,
    }
}
impl Sessions for FakeSessions {
    fn presence(&mut self, _: &Team, member: &Member) -> Option<Presence> {
        self.0.get(&member.id).cloned()
    }
}

type Hook = Box<dyn FnOnce()>;
#[derive(Default)]
pub(super) struct FakeEffects {
    /// Doorbells that passed their admission, as the child would run them.
    pub rang: Vec<Ring>,
    pub refusals: usize,
    pub rungs: VecDeque<Rung>,
    /// The lead's current run as `hgs send` would check it.
    pub cli_run: Option<String>,
    pub before_spawn: Option<Hook>,
    pub answers: BTreeMap<u32, Answer>,
    pub asked: Vec<(TaskRef, ClearRequest)>,
    pub launches: BTreeMap<String, LaunchRead>,
    pub reads: usize,
}
impl Effects for FakeEffects {
    fn ring(&mut self, store: &Store, ring: &Ring) -> Rung {
        if let Some(hook) = self.before_spawn.take() {
            hook();
        }
        let db = store.db.as_ref().unwrap();
        let tx = rusqlite::Transaction::new_unchecked(db, rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        if crate::teams::store::timers::admit(&tx, &ring.gate).is_err() {
            self.refusals += 1;
            return Rung::NotStarted("The doorbell admission refused; nothing started".into());
        }
        drop(tx);
        self.rang.push(ring.clone());
        if self.cli_run.as_ref().is_some_and(|run| run != &ring.run) {
            return Rung::Refused("The expected run changed".into());
        }
        self.rungs.pop_front().unwrap_or(Rung::Submitted)
    }
    fn clear(&mut self, task: &TaskRef, request: &ClearRequest) -> Answer {
        self.asked.push((task.clone(), request.clone()));
        self.answers
            .get(&request.node)
            .cloned()
            .unwrap_or(Answer::Cleared)
    }
    fn launch(&mut self, plan: &crate::desktop::teams::ports::Plan) -> Result<LaunchRead> {
        self.reads += 1;
        self.launches.get(&plan.launch_id).cloned().ok_or_else(|| {
            zerus_team_plan::Problem::new(code::INVALID_STATE, "Native read is unavailable")
        })
    }
}
use crate::desktop::teams::ports::LaunchRead;

pub(super) struct World {
    pub h: Harness,
    pub ledger: Ledger,
    pub timers: Timers,
    pub sessions: FakeSessions,
    pub effects: FakeEffects,
}
impl World {
    pub fn new() -> Self {
        let h = Harness::new();
        let ledger = Ledger::open(h.temp.path()).unwrap();
        Self {
            h,
            ledger,
            timers: Timers::new("epoch-1"),
            sessions: Default::default(),
            effects: Default::default(),
        }
    }
    pub fn pass(&mut self, now: i64) {
        let Self {
            h,
            ledger,
            timers,
            sessions,
            effects,
        } = self;
        timers
            .team(&mut h.store, ledger, sessions, effects, TEAM, now)
            .unwrap();
    }
    pub fn answers(&mut self, now: i64) {
        let Self {
            h,
            ledger,
            timers,
            sessions,
            effects,
        } = self;
        timers
            .answers(&mut h.store, ledger, sessions, effects, TEAM, now)
            .unwrap();
    }
    pub fn terminal(&mut self, now: i64) {
        let Self {
            h,
            ledger,
            timers,
            effects,
            ..
        } = self;
        timers.terminal(&mut h.store, ledger, effects, now).unwrap();
    }
    /// The service restarts: memory is gone, the ledger and store remain.
    pub fn restart(&mut self, epoch: &str) {
        self.timers = Timers::new(epoch);
        self.ledger = Ledger::open(self.h.temp.path()).unwrap();
        self.h.store = Store::open(self.h.temp.path()).unwrap();
    }
    pub fn items(&self, kind: &str) -> Vec<crate::teams::store::InboxItem> {
        let rows = self.h.store.inbox(TEAM, 200, None).unwrap().rows;
        rows.into_iter().filter(|i| i.kind == kind).collect()
    }
    pub fn cards(&self, kind: &str) -> Vec<crate::teams::store::Card> {
        let rows = self.h.store.cards(TEAM, 200, None).unwrap().rows;
        rows.into_iter().filter(|c| c.kind == kind).collect()
    }
    /// A running team with a bound lead and m1, and one report waiting.
    pub fn reported(&mut self) -> (u32, Actor) {
        let (node, lead) = self.h.reported();
        self.sessions.idle("lead", "run-1");
        (node, lead)
    }
}
