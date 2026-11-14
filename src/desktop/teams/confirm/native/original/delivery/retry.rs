//! Reviewer's witness: both passes use the engine, native storage and delivery.
//! Only observation is frozen; the child remains the isolated receipt stub.
use super::*;
use crate::desktop::teams::confirm::{engine, ledger::Ledger};

struct Seeded<'a> {
    native: Native<'a>,
    snapshot: Snapshot,
    busy_once: bool,
}
impl Ports for Seeded<'_> {
    fn facts(&mut self, b: &Binding) -> Result<Facts> {
        self.native.facts(b)
    }
    fn observe(&mut self, _: &Binding) -> Result<Snapshot> {
        Ok(self.snapshot.clone())
    }
    fn ownership(&mut self, b: &Binding) -> Result<Ownership> {
        self.native.ownership(b)
    }
    fn unsettled(&mut self, b: &Binding) -> Result<Option<Record>> {
        self.native.unsettled(b)
    }
    fn pending_brief(&mut self, b: &Binding) -> Result<Option<Plan>> {
        self.native.pending_brief(b)
    }
    fn claim(&mut self, p: &Plan, epoch: &str, now: i64) -> Result<Claim> {
        self.native.claim(p, epoch, now)
    }
    fn save(&mut self, r: &Record) -> Result<()> {
        self.native.save(r)
    }
    fn audit_begin(&mut self, p: &Plan) -> Result<Audit> {
        self.native.audit_begin(p)
    }
    fn audit_finish(&mut self, r: &Record) -> Result<()> {
        self.native.audit_finish(r)
    }
    fn card(&mut self, r: &Record) -> Result<()> {
        self.native.card(r)
    }
    fn original(&mut self, r: &Record, check: &mut dyn FnMut(&mut Self) -> Result<()>) -> Outcome {
        if let Err(why) = check(self) {
            return Outcome::NotStarted(why);
        }
        if !std::mem::take(&mut self.busy_once) {
            return self.native.deliver(r);
        }
        PROGRAM.with(|slot| *slot.borrow_mut() = Some(stub(self.native.config, r)));
        let lock = team_db(&self.native.config.state);
        lock.execute_batch("BEGIN IMMEDIATE").unwrap();
        let outcome = self.native.deliver(r);
        lock.execute_batch("ROLLBACK").unwrap();
        outcome
    }
}
fn pass(f: &mut Fixture, busy_once: bool) {
    let mut native = Native::new(&f.own.config, &f.own.b.machine, &f.stop, &mut f.store).unwrap();
    native.capture = Some((f.row.plan.snapshot.clone(), f.raw.clone()));
    let mut ports = Seeded {
        native,
        snapshot: f.row.plan.snapshot.clone(),
        busy_once,
    };
    PROGRAM.with(|slot| *slot.borrow_mut() = Some(f.stub.clone()));
    let _reset = Reset;
    engine::step(
        &mut ports,
        &f.own.b,
        if busy_once {
            "first-engine-pass"
        } else {
            "second-engine-pass"
        },
        if busy_once { 200 } else { 300 },
    )
    .unwrap();
}
fn row(f: &Fixture) -> Record {
    Ledger::open(&f.own.config.state)
        .unwrap()
        .get(&f.row.plan.key)
        .unwrap()
        .unwrap()
}
fn journal(f: &Fixture) -> (String, String) {
    let db = crate::approval_log::open(&f.own.config.state.join("approval-audit")).unwrap();
    db.query_row(
        "SELECT status,detail FROM attempts WHERE request_id=?1",
        [&f.row.audit.as_ref().unwrap().request],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}
fn waiting(kind: Kind) -> Fixture {
    let mut f = Fixture::new(kind);
    // Remove only the fixture's unused reservations. The first engine pass
    // must reserve its own effect and audit before encountering the real lock.
    let effects =
        rusqlite::Connection::open(f.own.config.state.join("teams/standing-v1/journal.sqlite3"))
            .unwrap();
    effects.execute("DELETE FROM standing_effects", []).unwrap();
    let audit = crate::approval_log::open(&f.own.config.state.join("approval-audit")).unwrap();
    audit.execute("DELETE FROM attempts", []).unwrap();
    pass(&mut f, true);
    f.row = row(&f);
    assert_eq!(f.row.state, "waiting");
    assert_eq!(
        f.row.detail,
        "Admission lock busy; the original child never started"
    );
    assert_eq!(journal(&f), ("prepared".into(), String::new()));
    assert!(!f.called());
    f
}
#[test]
fn wp17_own_successful_retry_must_not_journal_a_nonstart() {
    for kind in [Kind::Trust, Kind::Resume] {
        let mut f = waiting(kind);
        let audit = f.row.audit.clone();
        let original = f.row.original.clone();
        pass(&mut f, false);
        let after = row(&f);
        assert!(f.called());
        assert_eq!(after.audit, audit);
        assert_ne!(after.original, original);
        let (status, detail) = journal(&f);
        assert_eq!(status, "approved");
        assert!(
            !detail.contains("never started"),
            "a successful original is recorded as never started: {detail}"
        );
        assert!(
            !detail.is_empty(),
            "the final receipt should explain the successful outcome"
        );
        assert_eq!(detail, after.detail);
    }
}
#[derive(Clone, Copy, Debug)]
enum Final {
    Refused,
    Revoked,
    LateRevoked,
    Unknown,
}
fn change(f: &Fixture, outcome: Final) -> (&'static str, &'static str, bool) {
    match outcome {
        Final::Refused => {
            let path = f.own.config.state.join("stub-receipt");
            let mut receipt: Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            receipt["status"] = json!("rejected");
            private_write(&path, receipt.to_string().as_bytes());
            ("failed", "refused the exact question", true)
        }
        Final::Revoked => {
            revoke(&f.own.config.state);
            ("cancelled", "delegation is off", false)
        }
        Final::LateRevoked => {
            let state = f.own.config.state.clone();
            AFTER_FACTS.with(|slot| *slot.borrow_mut() = Some(Box::new(move || revoke(&state))));
            ("cancelled", "nothing started", false)
        }
        Final::Unknown => {
            let source = std::fs::read_to_string(&f.stub).unwrap();
            std::fs::write(&f.stub, &source[..source.find("folder=root/").unwrap()]).unwrap();
            ("unknown", "receipt is missing", true)
        }
    }
}
fn revoke(state: &Path) {
    team_db(state).execute("UPDATE teams SET settings=json_set(settings,'$.delegations.confirm_trust',json('false'))", []).unwrap();
}
#[test]
fn other_final_outcomes_after_a_wait_journal_their_own_reason() {
    for outcome in [
        Final::Refused,
        Final::Revoked,
        Final::LateRevoked,
        Final::Unknown,
    ] {
        let mut f = waiting(Kind::Trust);
        let (expected, reason, called) = change(&f, outcome);
        pass(&mut f, false);
        let (status, detail) = journal(&f);
        assert_eq!(status, expected, "{outcome:?}");
        assert!(detail.contains(reason), "{outcome:?}: {detail}");
        assert!(
            !detail.contains("Admission lock busy"),
            "stale wait detail: {detail}"
        );
        assert_eq!(detail, row(&f).detail);
        assert_eq!(f.called(), called);
    }
}
