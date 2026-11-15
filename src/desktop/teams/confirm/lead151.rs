//! Product-written team/launch/binding, standing ledger and audit records.
//! Only session observations and the original child's outcome are supplied.
use super::{engine, ledger::Ledger, native::launch_binding, ports::*, types::*};
use crate::teams::ops::{
    tests::{service, Harness, TEAM},
    Operation,
};
use serde_json::json;
mod planning;

struct World {
    h: Harness,
    ledger: Ledger,
    b: Binding,
    s: Snapshot,
    sends: Vec<Plan>,
    unknown: bool,
}
impl World {
    fn new() -> Self {
        Self::from_harness(crate::desktop::teams::tests::bound151_fixture())
    }
    fn from_harness(h: Harness) -> Self {
        let config = crate::config::Config {
            home: h.temp.path().to_string_lossy().into(),
            dir: h.temp.path().join("config"),
            state: h.temp.path().into(),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        let (b, name) = launch_binding(&config, TEAM, "lead").unwrap();
        let s = Snapshot {
            machine: b.machine.clone(),
            launch: b.launch.clone(),
            session: b.session.clone(),
            name,
            run: b.run.clone(),
            conversation: b.conversation.clone(),
            agent: b.agent.clone(),
            account: b.account.clone(),
            folder: b.folder.clone(),
            live: true,
            ready: true,
            paused: false,
            ended: false,
            self_exit: false,
            exit_identity: String::new(),
            archive: None,
            receipt_at: None,
            restore_refusal: None,
            resume_wait: None,
            questions: vec![],
        };
        let ledger = Ledger::open(h.temp.path()).unwrap();
        Self {
            h,
            ledger,
            b,
            s,
            sends: vec![],
            unknown: false,
        }
    }
    fn pass(&mut self) {
        engine::step(self, &self.b.clone(), "epoch-1", 1234).unwrap();
    }
    fn restart(&mut self) {
        self.ledger = Ledger::open(self.h.temp.path()).unwrap();
    }
    fn records(&self) -> u32 {
        let db = rusqlite::Connection::open_with_flags(
            self.h.temp.path().join("teams/standing-v1/journal.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        db.query_row(
            "SELECT count(*) FROM standing_effects WHERE team=?1",
            [TEAM],
            |r| r.get(0),
        )
        .unwrap()
    }
}
impl Ports for World {
    fn facts(&mut self, _: &Binding) -> Result<Facts> {
        Ok(Facts {
            team: self.h.store.team(TEAM).unwrap().unwrap(),
            member: self.h.store.member(TEAM, "lead").unwrap().unwrap(),
            binding: self.b.clone(),
            preview: true,
            planning_paused: self
                .h
                .store
                .planning(TEAM)
                .unwrap()
                .is_some_and(|p| p.paused),
        })
    }
    fn observe(&mut self, _: &Binding) -> Result<Snapshot> {
        Ok(self.s.clone())
    }
    fn ownership(&mut self, _: &Binding) -> Result<Ownership> {
        Ok(Ownership {
            folder: self.b.folder.clone(),
            common: self.b.common.clone(),
            nonce: self.b.nonce.clone(),
            launch: self.b.launch.clone(),
            exists: true,
            unchanged: true,
        })
    }
    fn unsettled(&mut self, b: &Binding) -> Result<Option<Record>> {
        self.ledger.unsettled(b)
    }
    fn pending_brief(&mut self, b: &Binding) -> Result<Option<Plan>> {
        if !self.ledger.lead_brief_needed(b)? {
            return Ok(None);
        }
        self.ledger.pending_lead_brief(
            b,
            &self.s,
            &self.h.store.team(TEAM).unwrap().unwrap(),
            self.h.store.planning(TEAM).unwrap().as_ref(),
        )
    }
    fn claim(&mut self, p: &Plan, epoch: &str, now: i64) -> Result<Claim> {
        self.ledger.claim(p, epoch, now)
    }
    fn save(&mut self, row: &Record) -> Result<()> {
        self.ledger.save(row)?;
        if let Some(b) = self.ledger.binding(&self.b)? {
            self.b = b;
            if let Some(s) = &row.result {
                self.s = s.clone();
            }
        }
        Ok(())
    }
    fn audit_begin(&mut self, p: &Plan) -> Result<Audit> {
        let mut db = crate::approval_log::open(&self.h.temp.path().join("approval-audit")).unwrap();
        let r = crate::approval_log::begin_team(&mut db, &json!({"host":p.binding.machine,"session":p.snapshot.name,"provider":p.binding.agent,"question":p.question}), if p.kind == Kind::Brief { crate::approval_log::TeamKind::Brief } else { crate::approval_log::TeamKind::Resume }).unwrap();
        assert_eq!(r["eligible"], true);
        Ok(Audit {
            id: r["id"].as_i64().unwrap(),
            request: r["request_id"].as_str().unwrap().into(),
            answers: r["answers"].clone(),
            fresh: r["fresh"] == true,
        })
    }
    fn audit_finish(&mut self, row: &Record) -> Result<()> {
        let a = row.audit.as_ref().unwrap();
        let db = crate::approval_log::open(&self.h.temp.path().join("approval-audit")).unwrap();
        crate::approval_log::finish(
            &db,
            &json!({"id":a.id,"request_id":a.request,"status":row.state,"detail":row.detail}),
        )
        .unwrap();
        Ok(())
    }
    fn card(&mut self, row: &Record) -> Result<()> {
        self.h
            .run(
                &service(),
                Operation::OpenCard {
                    kind: "standing_confirmation".into(),
                    text: row.detail.clone(),
                    node: None,
                    decisions: None,
                    parameters: None,
                },
            )
            .unwrap();
        Ok(())
    }
    fn original(
        &mut self,
        row: &Record,
        check: &mut dyn FnMut(&mut Self) -> Result<()>,
    ) -> Outcome {
        check(self).unwrap();
        self.sends.push(row.plan.clone());
        if self.unknown {
            return Outcome::Unknown("Receipt unavailable".into());
        }
        if row.plan.kind == Kind::Resume {
            let mut s = self.s.clone();
            s.run = "resumed-run".into();
            s.live = true;
            s.ready = true;
            s.ended = false;
            s.self_exit = false;
            s.exit_identity.clear();
            s.receipt_at = Some(1234);
            Outcome::Approved(Some(Box::new(s)))
        } else {
            Outcome::Approved(None)
        }
    }
}
#[test]
fn lead_launch_brief_is_once_across_passes_restart_and_first_conversation() {
    let mut w = World::new();
    w.h.settings(|s| s.delegations.resume_exited = false);
    assert!(
        w.b.conversation.is_empty(),
        "the launch writer bound a pre-first-message lead"
    );
    let node = w.h.add(&crate::teams::ops::tests::person(), 17);
    w.h.submit(node, "m1");
    let observation = w.h.store.task_observation(TEAM, node).unwrap();
    let before = w.h.store.queue(TEAM, 200, None).unwrap().rows;
    w.pass();
    assert_eq!(w.sends.len(), 1);
    assert_eq!(w.sends[0].kind, Kind::Brief);
    assert_eq!(w.sends[0].text, "You are the lead of team Fixes. Read `hgs team current` for your team and `hgs team inbox` for items waiting for you.");
    assert!(w.sends[0].parent.is_none());
    w.pass();
    w.restart();
    w.pass();
    w.s.conversation = "first-conversation".into();
    w.pass();
    assert_eq!(w.sends.len(), 1);
    assert!(!w.ledger.lead_brief_needed(&w.b).unwrap());
    assert_eq!(w.records(), 1, "no extra brief reservation across passes");
    assert_eq!(w.h.store.queue(TEAM, 200, None).unwrap().rows, before);
    assert_eq!(w.h.store.task_observation(TEAM, node).unwrap(), observation);
}
#[test]
fn resumed_lead_gets_its_own_brief_once_without_an_extra_launch_brief() {
    let mut w = World::new();
    if !w
        .h
        .store
        .team(TEAM)
        .unwrap()
        .unwrap()
        .settings
        .delegations
        .resume_exited
    {
        w.h.settings(|s| s.delegations.resume_exited = true);
    }
    w.pass();
    w.s.live = false;
    w.s.ready = false;
    w.s.ended = true;
    w.s.self_exit = true;
    w.s.conversation = "saved-conversation".into();
    w.s.exit_identity = "exact-exit".into();
    w.pass();
    assert_eq!(w.sends.len(), 2);
    assert_eq!(w.sends[1].kind, Kind::Resume);
    w.pass();
    assert_eq!(w.sends.len(), 3);
    assert_eq!(w.sends[2].kind, Kind::Brief);
    assert!(w.sends[2].parent.is_some());
    assert!(w.sends[2]
        .text
        .starts_with("You are the lead of team Fixes."));
    w.pass();
    w.restart();
    w.pass();
    assert_eq!(w.sends.len(), 3);
    assert!(!w.ledger.lead_brief_needed(&w.b).unwrap());
    assert_eq!(
        w.records(),
        3,
        "one launch brief, one resume and one resumed brief"
    );
}
#[test]
fn unknown_lead_brief_never_replays_after_restart() {
    let mut w = World::new();
    w.unknown = true;
    w.pass();
    w.restart();
    for _ in 0..4 {
        w.pass();
    }
    assert_eq!(w.sends.len(), 1);
    let key = &w.sends[0].key;
    assert_eq!(w.ledger.get(key).unwrap().unwrap().state, "unknown");
    assert!(!w.ledger.lead_brief_needed(&w.b).unwrap());
    assert_eq!(
        w.records(),
        1,
        "unknown delivery never creates another claim"
    );
}
#[test]
fn lead_brief_waits_for_a_ready_unpaused_question_free_session() {
    let mut w = World::new();
    w.s.ready = false;
    w.pass();
    assert!(w.sends.is_empty());
    w.s.ready = true;
    w.s.paused = true;
    w.pass();
    assert!(w.sends.is_empty());
    // A temporary inability to send must not consume the one brief.
    w.s.paused = false;
    w.s.questions.push(json!({"source":"agent_question"}));
    w.pass();
    assert!(w.sends.is_empty());
    assert_eq!(w.records(), 0);
    w.s.questions.clear();
    w.pass();
    assert_eq!(w.sends.len(), 1);
}

#[test]
fn the_startup_audit_is_brief_only_and_keeps_the_exact_request() {
    let mut w = World::new();
    let team = w.h.store.team(TEAM).unwrap().unwrap();
    let plan = engine::lead_brief(&w.b, &w.s, &team);
    let first = w.audit_begin(&plan).unwrap();
    let duplicate = w.audit_begin(&plan).unwrap();
    assert!(first.fresh);
    assert!(!duplicate.fresh);
    assert_eq!(first.request, duplicate.request);
    let mut db = crate::approval_log::open(&w.h.temp.path().join("approval-audit")).unwrap();
    let payload = json!({"host":plan.binding.machine,"session":plan.snapshot.name,"provider":plan.binding.agent,"question":plan.question});
    assert!(
        crate::approval_log::begin_team(&mut db, &payload, crate::approval_log::TeamKind::Resume)
            .is_err(),
        "a resume still requires a saved conversation"
    );
    let mut changed = payload;
    changed["question"]["question_id"] = json!("resume:other");
    assert_eq!(
        crate::approval_log::begin_team(&mut db, &changed, crate::approval_log::TeamKind::Brief)
            .unwrap()["eligible"],
        false
    );
}

#[test]
fn pausing_the_team_does_not_consume_its_initial_lead_brief() {
    let mut w = World::new();
    w.h.run(&crate::teams::ops::tests::person(), Operation::Pause)
        .unwrap();
    w.pass();
    assert!(w.sends.is_empty());
    assert_eq!(w.records(), 0);
    w.h.run(&crate::teams::ops::tests::person(), Operation::Resume)
        .unwrap();
    w.pass();
    assert_eq!(w.sends.len(), 1);
    assert_eq!(w.records(), 1);
}
