use super::*;
use crate::desktop::teams::confirm::{policy::tests::sample, resumed::resumed_binding};

#[test]
fn reservation_tombstones_and_hourly_quota_survive_database_reopens() {
    let temp = tempfile::tempdir().unwrap();
    let (b, _, s) = sample();
    let mut last = None;
    for n in 0..4 {
        let mut db = Ledger::open(temp.path()).unwrap();
        let mut b = b.clone();
        b.run = format!("run-{n}");
        let mut s = s.clone();
        s.run = b.run.clone();
        let p = engine::candidate(&b, &s, Kind::Resume, serde_json::Value::Null);
        let c = db
            .claim(&p, &format!("epoch-{n}"), 1000 + n * 1000)
            .unwrap();
        assert!(c.fresh);
        assert_eq!(c.limited, n == 3);
        assert!(!db.claim(&p, "replacement-epoch", 10000).unwrap().fresh);
        last = Some(p);
    }
    let mut db = Ledger::open(temp.path()).unwrap();
    let mut p = last.unwrap();
    p.key.push('x');
    p.binding.run = "later-run".into();
    assert!(!db.claim(&p, "later", 3_605_000).unwrap().limited);
}

#[test]
fn unknown_never_publishes_a_binding_and_completed_receipt_publishes_exactly_one_fact() {
    let temp = tempfile::tempdir().unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(temp.path(), &alias).unwrap();
    let (b, _, mut s) = sample();
    s.conversation = "conversation-1".into();
    assert!(resumed_binding(temp.path(), &b.team, &b.member, &b.launch)
        .unwrap()
        .is_none());
    let mut db = Ledger::open(temp.path()).unwrap();
    let p = engine::candidate(&b, &s, Kind::Resume, serde_json::Value::Null);
    let mut c = db.claim(&p, "epoch-1", 100).unwrap().record;
    c.state = "unknown".into();
    let mut incomplete = s.clone();
    incomplete.run = "unconfirmed-run".into();
    incomplete.receipt_at = Some(123000);
    c.result = Some(incomplete);
    db.save(&c).unwrap();
    assert!(db.binding(&b).unwrap().is_none());
    assert!(resumed_binding(temp.path(), &b.team, &b.member, &b.launch)
        .unwrap()
        .is_none());
    c.state = "approved".into();
    assert!(
        db.save(&c).is_err(),
        "A terminal Unknown cannot change to Approved"
    );
    c.plan.key.push('x');
    let mut c = db.claim(&c.plan, "epoch-2", 200).unwrap().record;
    c.state = "approved".into();
    let mut target = s;
    target.run = "new-run".into();
    target.conversation = "conversation-1".into();
    target.receipt_at = Some(123456);
    c.result = Some(target);
    db.save(&c).unwrap();
    let fact = resumed_binding(&alias, &b.team, &b.member, &b.launch)
        .unwrap()
        .unwrap();
    assert_eq!(fact.run_id, "new-run");
    assert_eq!(fact.conversation_id, "conversation-1");
    assert_eq!(fact.session_id, b.session);
    assert_eq!(fact.receipt_at, 123456);
    assert!(
        resumed_binding(temp.path(), &b.team, &b.member, "another-launch")
            .unwrap()
            .is_none()
    );
    let brief = db.pending_brief(&b).unwrap().unwrap();
    assert_eq!(brief.binding.run, fact.run_id);
    assert_eq!(brief.binding.conversation, fact.conversation_id);
    assert_eq!(brief.kind, Kind::Brief);
    drop(db);
    let path = temp.path().join("teams/standing-v1/journal.sqlite3");
    let saved = path.with_extension("saved");
    std::fs::rename(&path, &saved).unwrap();
    std::os::unix::fs::symlink(&saved, &path).unwrap();
    assert!(resumed_binding(&alias, &b.team, &b.member, &b.launch).is_err());
}

#[test]
fn claim_identity_is_immutable_and_pending_reads_are_indexed_and_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let (b, _, s) = sample();
    let mut db = Ledger::open(temp.path()).unwrap();
    let p = engine::candidate(&b, &s, Kind::Trust, s.questions[0].clone());
    let row = db.claim(&p, "epoch-1", 100).unwrap().record;
    let mut changed = row.clone();
    changed.original.push('x');
    assert!(db.save(&changed).is_err());
    changed = row.clone();
    changed.plan.binding.session.push('x');
    assert!(db.save(&changed).is_err());
    assert_eq!(db.unsettled(&b).unwrap().unwrap(), row);
    let query="EXPLAIN QUERY PLAN SELECT body FROM standing_effects WHERE team=?1 AND member=?2 AND launch=?3 AND pending=1 ORDER BY key LIMIT 1";
    let plan: String =
        db.0.query_row(query, params![b.team, b.member, b.launch], |r| r.get(3))
            .unwrap();
    assert!(plan.contains("standing_pending"), "{plan}");
    let plan:String=db.0.query_row("EXPLAIN QUERY PLAN SELECT 1 FROM standing_effects WHERE team=?1 AND member=?2 AND kind='resume' AND at>?3 LIMIT 3",params![b.team,b.member,100],|r|r.get(3)).unwrap();
    assert!(plan.contains("standing_quota"), "{plan}");
}

#[test]
fn a_recorded_nonstart_can_be_reclaimed_once_but_an_unknown_cannot() {
    let temp = tempfile::tempdir().unwrap();
    let (b, _, s) = sample();
    let plan = engine::candidate(&b, &s, Kind::Trust, s.questions[0].clone());
    let mut db = Ledger::open(temp.path()).unwrap();
    let mut row = db.claim(&plan, "epoch-1", 100).unwrap().record;
    row.audit = Some(Audit {
        id: 1,
        request: uuid::Uuid::new_v4().to_string(),
        answers: serde_json::json!([]),
        fresh: true,
    });
    row.state = "waiting".into();
    db.save(&row).unwrap();
    drop(db);
    let mut db = Ledger::open(temp.path()).unwrap();
    let retry = db.claim(&plan, "epoch-2", 200).unwrap();
    assert!(
        retry.fresh,
        "a recorded nonstart may be admitted in a later pass"
    );
    assert_eq!(retry.record.state, "prepared");
    assert_ne!(retry.record.original, row.original);
    assert_eq!(retry.record.epoch, "epoch-2");
    assert_eq!(retry.record.at, 200);
    assert!(!db.claim(&plan, "another-writer", 300).unwrap().fresh);
    assert!(
        db.save(&row).is_err(),
        "the prior writer lost its original identity"
    );
    let mut unknown = retry.record;
    unknown.state = "unknown".into();
    db.save(&unknown).unwrap();
    assert!(!db.claim(&plan, "epoch-3", 400).unwrap().fresh);
    unknown.state = "waiting".into();
    assert!(
        db.save(&unknown).is_err(),
        "unknown cannot become retryable"
    );
}

#[test]
fn a_nonstart_reclaim_renews_and_rechecks_the_resume_quota() {
    let temp = tempfile::tempdir().unwrap();
    let (b, _, s) = sample();
    let mut db = Ledger::open(temp.path()).unwrap();
    let plan = engine::candidate(&b, &s, Kind::Resume, serde_json::Value::Null);
    let mut row = db.claim(&plan, "epoch-1", 100).unwrap().record;
    row.audit = Some(Audit {
        id: 1,
        request: uuid::Uuid::new_v4().to_string(),
        answers: serde_json::json!([]),
        fresh: true,
    });
    row.state = "waiting".into();
    db.save(&row).unwrap();
    for n in 0..3 {
        let mut b = b.clone();
        let mut s = s.clone();
        b.run = format!("other-{n}");
        s.run = b.run.clone();
        let p = engine::candidate(&b, &s, Kind::Resume, serde_json::Value::Null);
        assert!(!db.claim(&p, "epoch-2", 4_000_000 + n).unwrap().limited);
    }
    let retry = db.claim(&plan, "epoch-3", 4_000_010).unwrap();
    assert!(retry.fresh);
    assert!(
        retry.limited,
        "an old nonstart cannot evade the current hourly quota"
    );
    assert_eq!(retry.record.at, 4_000_010);
}

#[test]
fn a_waiting_claim_may_be_cancelled_but_never_settled_or_started_directly() {
    let temp = tempfile::tempdir().unwrap();
    let (b, _, s) = sample();
    let plan = engine::candidate(&b, &s, Kind::Trust, s.questions[0].clone());
    let mut db = Ledger::open(temp.path()).unwrap();
    let mut row = db.claim(&plan, "epoch-1", 100).unwrap().record;
    row.state = "waiting".into();
    assert!(
        db.save(&row).is_err(),
        "a missing audit cannot be made retryable"
    );
    row.audit = Some(Audit {
        id: 1,
        request: uuid::Uuid::new_v4().to_string(),
        answers: serde_json::json!([]),
        fresh: true,
    });
    db.save(&row).unwrap();
    let mut illegal = row.clone();
    illegal.state = "prepared".into();
    assert!(
        db.save(&illegal).is_err(),
        "only the atomic claim may start a waiting original"
    );
    illegal = row.clone();
    illegal.audit_done = true;
    assert!(
        db.save(&illegal).is_err(),
        "a finished audit cannot admit a retry"
    );
    row.state = "cancelled".into();
    db.save(&row).unwrap();
    assert!(!db.claim(&plan, "epoch-2", 200).unwrap().fresh);
}
