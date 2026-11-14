use super::*;
use crate::teams::ops::tests::{Harness, TEAM};
#[test]
fn blocking_a_row_releases_its_live_admission_immediately() {
    let h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let mut r = Runtime::open(h.temp.path(), &t.home, "epoch").unwrap();
    let mut row = Row::new(&t, "lead", "epoch", State::WorktreeIntent);
    r.ledger = Some(Ledger::open(h.temp.path()).unwrap());
    r.ledger.as_mut().unwrap().insert(&row, 1).unwrap();
    r.admitted = Some((t.id, row.original.clone()));
    r.block(&mut row, uncertain("Synthetic interrupted effect"), 2)
        .unwrap();
    assert!(!r.busy());
}
#[test]
fn owner_card_text_is_bounded_by_bytes_at_a_utf8_boundary() {
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let mut r = Runtime::open(h.temp.path(), &t.home, "epoch").unwrap();
    let mut row = Row::new(&t, "lead", "epoch", State::Blocked);
    row.problem =
        Some(Problem::new(code::INVALID_STATE, "⚙".repeat(5000)).with_refs(["⏳".repeat(3000)]));
    r.ledger = Some(Ledger::open(h.temp.path()).unwrap());
    r.ledger.as_mut().unwrap().insert(&row, 1).unwrap();
    r.card(&mut h.store, &mut row, 2).unwrap();
    let card = h.store.card(TEAM, row.card_id.unwrap()).unwrap().unwrap();
    assert!(card.text.len() <= 3072);
    assert!(std::str::from_utf8(card.text.as_bytes()).is_ok());
}

/// zerus-63rru.91: the worker's SetMemberSession write keeps one request ID,
/// so a commit whose answer was lost is replayed, never repeated.
fn session_write(home: &str) -> (Operation, zerus_team_plan::model::SessionKey) {
    let key = zerus_team_plan::model::SessionKey {
        machine: home.into(),
        session_id: "launch_id:00000000-0000-4000-8000-000000000009".to_string(),
    };
    (
        Operation::SetMemberSession {
            member: "m2".into(),
            session: key.clone(),
            account: "dev".into(),
            worktree: "/tmp/claude-1000/launch-ids".into(),
            ownership_nonce: "nonce".into(),
        },
        key,
    )
}

#[test]
fn a_lost_answer_is_replayed_under_the_same_id_and_writes_once() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let mut h = Harness::new();
    h.bind("m1");
    let t = h.store.team(TEAM).unwrap().unwrap();
    let (op, key) = session_write(&t.home);
    let mut row = Row::new(&t, "m2", "epoch", State::Submitted);
    // The commit's result is lost: the ops answers storage_failed, the
    // reservation stays, and the same ID replays the write once.
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    h.store.db.as_ref().unwrap().commit_hook(Some(move || {
        observed.fetch_add(1, Ordering::SeqCst);
        true
    }));
    let failed = apply(&mut h.store, &t, op.clone(), &mut row.session_request, 1000).unwrap_err();
    h.store
        .db
        .as_ref()
        .unwrap()
        .commit_hook(None::<fn() -> bool>);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(failed.code, "storage_failed");
    let id = row.session_request.as_ref().unwrap().request_id.clone();
    apply(&mut h.store, &t, op, &mut row.session_request, 1001).unwrap();
    let saved: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM requests WHERE id=?1", [&id], |r| r.get(0))
        .unwrap();
    assert_eq!(saved, 1, "the write was recorded twice");
    assert_eq!(
        h.store.member(TEAM, "m2").unwrap().unwrap().session,
        Some(key)
    );
}

#[test]
fn storage_failed_keeps_the_id_and_the_retry_writes_once() {
    let mut h = Harness::new();
    h.bind("m1");
    let t = h.store.team(TEAM).unwrap().unwrap();
    let (op, _) = session_write(&t.home);
    let mut row = Row::new(&t, "m2", "epoch", State::Submitted);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("CREATE TRIGGER fail_write BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END;")
        .unwrap();
    let failed = apply(&mut h.store, &t, op.clone(), &mut row.session_request, 1000).unwrap_err();
    assert_eq!(failed.code, "storage_failed");
    assert!(
        row.session_request.is_some(),
        "a storage failure dropped the reserved request; the retry would take a new ID"
    );
    let id = row.session_request.as_ref().unwrap().request_id.clone();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_write")
        .unwrap();
    apply(&mut h.store, &t, op, &mut row.session_request, 1001).unwrap();
    let saved: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM requests WHERE id=?1", [&id], |r| r.get(0))
        .unwrap();
    assert_eq!(saved, 1);
}

#[test]
fn a_restart_between_the_intent_and_the_write_replays_the_same_id() {
    let mut h = Harness::new();
    h.bind("m1");
    let t = h.store.team(TEAM).unwrap().unwrap();
    let (op, _) = session_write(&t.home);
    let machine = t.home.clone();
    let mut r = Runtime::open(h.temp.path(), &machine, "epoch").unwrap();
    r.ledger = Some(Ledger::open(h.temp.path()).unwrap());
    let mut row = Row::new(&t, "m2", "epoch", State::Submitted);
    // The first pass reserves the ID against a store that fails the write.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("CREATE TRIGGER fail_write BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END;")
        .unwrap();
    let _ = apply(&mut h.store, &t, op.clone(), &mut row.session_request, 1000).unwrap_err();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_write")
        .unwrap();
    let id = row.session_request.as_ref().unwrap().request_id.clone();
    r.ledger.as_mut().unwrap().insert(&row, 1).unwrap();
    drop(r);
    // The restart reads the row and replays the persisted request.
    let mut r = Runtime::open(h.temp.path(), &machine, "epoch").unwrap();
    r.ledger = Some(Ledger::open(h.temp.path()).unwrap());
    let mut loaded = r.ledger.as_mut().unwrap().get(TEAM, "m2").unwrap().unwrap();
    apply(&mut h.store, &t, op, &mut loaded.session_request, 1001).unwrap();
    let replayed: String = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT id FROM requests WHERE id=?1", [&id], |r| r.get(0))
        .unwrap();
    assert_eq!(replayed, id, "the restart wrote under a new ID");
    assert!(loaded.session_request.is_none(), "a success kept the reservation");
    let saved: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM requests WHERE id=?1", [&id], |r| r.get(0))
        .unwrap();
    assert_eq!(saved, 1);
}

/// Round 2: a successful write clears the reservation, so the row's next
/// intent gets its own ID.
fn open_card(h: &mut Harness, n: u32) -> u64 {
    let t = h.store.team(TEAM).unwrap().unwrap();
    h.run(
        &Actor::Service {
            person: t.owner,
            team: TEAM.into(),
        },
        Operation::OpenCard {
            kind: "launch_blocked".into(),
            text: format!("card {n}"),
            node: None,
            decisions: Some(vec!["retry".into(), "acknowledged".into()]),
        },
    )
    .unwrap()
    .item
    .unwrap()
}

#[test]
fn two_cards_closed_one_after_the_other_on_one_row_are_both_closed() {
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let first = open_card(&mut h, 1);
    let second = open_card(&mut h, 2);
    let mut row = Row::new(&t, "m2", "epoch", State::Submitted);
    let close = |card: u64| Operation::CloseCard {
        card,
        reason: "the launch's state changed".into(),
    };
    apply(&mut h.store, &t, close(first), &mut row.close_request, 1000).unwrap();
    apply(&mut h.store, &t, close(second), &mut row.close_request, 1001).unwrap();
    assert_eq!(h.store.card(TEAM, first).unwrap().unwrap().state, "closed");
    assert_eq!(h.store.card(TEAM, second).unwrap().unwrap().state, "closed");
}

#[test]
fn a_relaunch_into_the_same_row_writes_the_new_session() {
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let key1 = zerus_team_plan::model::SessionKey {
        machine: t.home.clone(),
        session_id: "launch_id:00000000-0000-4000-8000-000000000001".into(),
    };
    let key2 = zerus_team_plan::model::SessionKey {
        machine: t.home.clone(),
        session_id: "launch_id:00000000-0000-4000-8000-000000000002".into(),
    };
    let session = |member: &str,
                    key: zerus_team_plan::model::SessionKey|
     -> Operation {
        Operation::SetMemberSession {
            member: member.to_owned(),
            session: key,
            account: "dev".into(),
            worktree: "/tmp/claude-1000/launch-ids".into(),
            ownership_nonce: "nonce".into(),
        }
    };
    let mut row = Row::new(&t, "m2", "epoch", State::Submitted);
    apply(
        &mut h.store,
        &t,
        session("m2", key1.clone()),
        &mut row.session_request,
        1000,
    )
    .unwrap();
    // A relaunch into the same row writes the next member's session under a
    // new request, never a replay of the first write.
    apply(
        &mut h.store,
        &t,
        session("m3", key2.clone()),
        &mut row.session_request,
        1001,
    )
    .unwrap();
    assert_eq!(h.store.member(TEAM, "m3").unwrap().unwrap().session, Some(key2));
}

#[test]
fn a_changed_intent_settles_the_kept_write_first_and_writes_both() {
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let key = zerus_team_plan::model::SessionKey {
        machine: t.home.clone(),
        session_id: "launch_id:00000000-0000-4000-8000-000000000003".into(),
    };
    let mut row = Row::new(&t, "m2", "epoch", State::Submitted);
    // The session write's answer is lost: storage_failed keeps the request.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("CREATE TRIGGER fail_write BEFORE INSERT ON requests BEGIN SELECT RAISE(ABORT,'fixture receipt failure'); END;")
        .unwrap();
    let failed = apply(
        &mut h.store,
        &t,
        Operation::SetMemberSession {
            member: "m2".into(),
            session: key.clone(),
            account: "dev".into(),
            worktree: "/tmp/claude-1000/launch-ids".into(),
            ownership_nonce: "nonce".into(),
        },
        &mut row.session_request,
        1000,
    )
    .unwrap_err();
    assert_eq!(failed.code, "storage_failed");
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_write")
        .unwrap();
    // The same field now carries a different intent: the kept session write
    // is replayed first, then the card close gets its own request.
    let card = open_card(&mut h, 1);
    apply(
        &mut h.store,
        &t,
        Operation::CloseCard {
            card,
            reason: "the intent changed".into(),
        },
        &mut row.session_request,
        1001,
    )
    .unwrap();
    assert_eq!(h.store.member(TEAM, "m2").unwrap().unwrap().session, Some(key));
    assert_eq!(h.store.card(TEAM, card).unwrap().unwrap().state, "closed");
}

#[test]
fn a_crash_between_the_commit_and_the_ledger_save_makes_no_second_write() {
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let card = open_card(&mut h, 1);
    let mut row = Row::new(&t, "m2", "epoch", State::Submitted);
    // The close commits but the row (with its reservation) is lost.
    apply(
        &mut h.store,
        &t,
        Operation::CloseCard {
            card,
            reason: "the launch's state changed".into(),
        },
        &mut row.close_request,
        1000,
    )
    .unwrap();
    let journal = h.store.journal(TEAM, 200, None).unwrap().rows.len();
    // The restart's pass has the intent but no reservation; the caller's
    // pre-check sees the committed close and writes nothing.
    let mut restarted = Row::new(&t, "m2", "epoch", State::Submitted);
    restarted.card_id = Some(card);
    super::Runtime::open(h.temp.path(), &t.home, "epoch")
        .unwrap()
        .close_stale(&mut h.store, &t, &mut restarted, 1001)
        .unwrap();
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap().rows.len(), journal);
}

#[test]
fn a_row_with_no_reservation_serializes_without_the_keys() {
    let h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let row = Row::new(&t, "m2", "epoch", State::Submitted);
    let value = serde_json::to_value(&row).unwrap();
    let object = value.as_object().unwrap();
    assert!(!object.contains_key("session_request"), "{value}");
    assert!(!object.contains_key("close_request"), "{value}");
}

/// zerus-63rru.24 and zerus-63rru.63: a stopped team's launch row whose
/// failures are at the retry cap gets one service item, visible in
/// `hgs team status`, and the row's successful pass ends it.
fn failing_row(h: &mut Harness, member: &str, times: u32) -> Runtime {
    let t = h.store.team(TEAM).unwrap().unwrap();
    let machine = t.home.clone();
    let mut r = Runtime::open(h.temp.path(), &machine, "epoch").unwrap();
    let mut row = Row::new(&t, member, "epoch", State::Retry);
    r.ledger = Some(Ledger::open(h.temp.path()).unwrap());
    r.ledger.as_mut().unwrap().insert(&row, 1).unwrap();
    for n in 0..times {
        r.failure(
            &mut h.store,
            &mut row,
            Problem::new(zerus_team_plan::problem::code::INVALID_STATE, "The launch keeps failing"),
            1000 + i64::from(n) * 60_000,
            false,
        )
        .unwrap();
    }
    r
}
fn status_items(h: &Harness, now: i64) -> Vec<(String, String, String, u32)> {
    crate::desktop::teams::service_items(h.temp.path(), TEAM, Some(true), now)
        .unwrap()
        .items
        .into_iter()
        .map(|i| (i.kind.to_owned(), i.subject, i.reason, i.failures))
        .collect()
}

#[test]
fn a_terminal_cleanup_item_appears_only_at_the_retry_cap_and_goes_on_success() {
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    // One row below the cap and one at it, failed while the team runs.
    let _ = failing_row(&mut h, "m2", 1);
    let mut at_cap = failing_row(&mut h, "m3", 5);
    h.run(&crate::teams::ops::tests::person(), Operation::Stop).unwrap();
    // The first pass after the stop writes the cap row's item, once, and
    // nothing for the row below the cap.
    at_cap.note_late(&t, 400_000).unwrap();
    let items = status_items(&h, 400_000);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(
        (items[0].0.as_str(), items[0].1.as_str(), items[0].3),
        ("launch_failing", "m3", 5)
    );
    assert!(items[0].2.contains("The launch keeps failing"), "{items:?}");
    // The row's successful pass clears the item.
    let mut row = at_cap
        .ledger
        .as_mut()
        .unwrap()
        .get(TEAM, "m3")
        .unwrap()
        .unwrap();
    at_cap.close_stale(&mut h.store, &t, &mut row, 460_000).unwrap();
    assert!(status_items(&h, 460_000).is_empty());
}

#[test]
fn a_stopped_teams_failing_row_is_visible_on_the_first_pass_and_never_written_twice() {
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let mut r = failing_row(&mut h, "m3", 5);
    h.run(&crate::teams::ops::tests::person(), Operation::Stop).unwrap();
    for now in [400_000, 460_000, 520_000, 580_000] {
        r.note_late(&t, now).unwrap();
        let items = status_items(&h, now);
        assert_eq!(items.len(), 1, "pass at {now}: {items:?}");
        assert_eq!(items[0].3, 5, "the pass at {now} wrote the item again");
    }
}

/// Round 2: the item's clear never blocks the row's success, and the next
/// pass's sweep removes an item that outlived a failed clear.
fn break_items_file(h: &Harness) {
    let path = h.temp.path().join("teams").join("actor-v1.sqlite3");
    let _ = std::fs::remove_file(&path);
    std::fs::create_dir(&path).unwrap();
}
fn repair_items_file(h: &Harness) {
    let path = h.temp.path().join("teams").join("actor-v1.sqlite3");
    let _ = std::fs::remove_dir_all(&path);
}

#[test]
fn an_unopenable_items_file_leaves_the_success_saved_and_the_next_pass_clears_the_item() {
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let mut r = failing_row(&mut h, "m3", 5);
    h.run(&crate::teams::ops::tests::person(), Operation::Stop).unwrap();
    r.note_late(&t, 400_000).unwrap();
    assert_eq!(status_items(&h, 400_000).len(), 1);
    break_items_file(&h);
    // The row's successful pass is saved; the failed clear is swallowed.
    let mut row = r
        .ledger
        .as_mut()
        .unwrap()
        .get(TEAM, "m3")
        .unwrap()
        .unwrap();
    r.close_stale(&mut h.store, &t, &mut row, 460_000).unwrap();
    r.ledger.as_mut().unwrap().save(&mut row, 460_000).unwrap();
    let saved = r
        .ledger
        .as_mut()
        .unwrap()
        .get(TEAM, "m3")
        .unwrap()
        .unwrap();
    assert_eq!(saved.failures, 0, "the success was not saved");
    repair_items_file(&h);
    // The next pass removes the lingering item.
    r.note_late(&t, 520_000).unwrap();
    assert!(status_items(&h, 520_000).is_empty());
}

/// Round 2: the bind's own success path behaves the same.
struct Never;
impl super::super::ports::Effects for Never {
    fn placement(&mut self, _: &str, _: &str) -> super::super::ports::Result<super::super::ports::Placement> {
        unreachable!("the observed session is already set")
    }
    fn branch_exists(&mut self, _: &super::super::ledger::BranchPlan) -> super::super::ports::Result<bool> {
        unreachable!("the observed session is already set")
    }
    fn create_branch(&mut self, _: &super::super::ledger::BranchPlan) -> super::super::ports::Effect<()> {
        unreachable!("the observed session is already set")
    }
    fn accounts(&mut self, _: &str) -> super::super::ports::Result<Vec<zerus_team_plan::accounts::AccountUsage>> {
        unreachable!("the observed session is already set")
    }
    fn worktree(&mut self, _: &super::super::ports::Plan, _: &str) -> super::super::ports::Effect<Option<super::super::ports::Worktree>> {
        unreachable!("the observed session is already set")
    }
    fn verify_worktree(&mut self, _: &super::super::ports::Plan) -> super::super::ports::Result<super::super::ports::Worktree> {
        unreachable!("the observed session is already set")
    }
    fn launch(&mut self, _: &super::super::ports::Plan, _: &super::super::ports::Worktree, _: &str) -> super::super::ports::Effect<()> {
        unreachable!("the observed session is already set")
    }
    fn observe(&mut self, _: &super::super::ports::Plan) -> super::super::ports::Result<super::super::ports::LaunchRead> {
        unreachable!("the observed session is already set")
    }
    fn parent(&mut self, _: &super::super::ports::Plan, _: &super::super::ports::ObservedSession, _: &str) -> String {
        unreachable!("the observed session is already set")
    }
}

#[test]
fn an_unopenable_items_file_leaves_the_bind_successful_and_saved() {
    use super::super::ports::{Placement, Worktree};
    let mut h = Harness::new();
    let t = h.store.team(TEAM).unwrap().unwrap();
    let machine = t.home.clone();
    let mut r = Runtime::open(h.temp.path(), &machine, "epoch").unwrap();
    r.ledger = Some(Ledger::open(h.temp.path()).unwrap());
    let tree = Worktree {
        path: "/tmp/claude-1000/launch-ids".into(),
        ownership_nonce: "nonce".into(),
    };
    let plan = super::super::ports::Plan {
        id: "plan".into(),
        team: TEAM.into(),
        person: "local".into(),
        machine: machine.clone(),
        project: "project".into(),
        member: "m2".into(),
        role: "developer".into(),
        agent: "claude".into(),
        account: "dev".into(),
        model: "model".into(),
        effort: "1x".into(),
        base_commit: "base".into(),
        placement: Placement {
            source: "source".into(),
            git_dir: "git".into(),
            branch: "branch".into(),
            destination: "destination".into(),
        },
        parent: None,
        launch_id: "00000000-0000-4000-8000-000000000009".into(),
        session_name: String::new(),
    };
    let observed = super::super::ports::ObservedSession {
        machine: machine.clone(),
        launch_id: "00000000-0000-4000-8000-000000000009".into(),
        agent: "claude".into(),
        account: "dev".into(),
        worktree: tree.clone(),
        parent: None,
        name: "member".into(),
        session_id: "not-a-canonical-id".into(),
        run_id: String::new(),
        conversation_id: String::new(),
    };
    let mut row = Row::new(&t, "m2", "epoch", State::Submitted);
    row.plan = Some(plan);
    row.worktree = Some(tree);
    row.observed = Some(observed);
    row.failures = 5;
    r.ledger.as_mut().unwrap().insert(&row, 1).unwrap();
    // The items file is a directory: every items write answers an error.
    let _ = std::fs::create_dir_all(h.temp.path().join("teams"));
    let items_path = h.temp.path().join("teams").join("actor-v1.sqlite3");
    std::fs::create_dir(&items_path).unwrap();
    let mut effects = Never;
    let bound = r.bind(&mut h.store, &mut effects, &t, &mut row, 1000).unwrap();
    assert!(bound, "the bind did not report its success");
    let saved = r
        .ledger
        .as_mut()
        .unwrap()
        .get(TEAM, "m2")
        .unwrap()
        .unwrap();
    assert_eq!(saved.state, State::Bound, "the bind's success was not saved");
    assert_eq!(saved.failures, 0);
    std::fs::remove_dir_all(&items_path).unwrap();
}
