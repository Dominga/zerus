use super::*;
mod conversion;
mod cross;
mod drafts;
mod guards;
mod holder;
mod member_sessions;
#[cfg(unix)]
mod paths;
mod planning;
mod stage;
mod timeline;

#[test]
fn missing_store_is_lazy_and_preview_is_required() {
    let temp = tempfile::tempdir().unwrap();
    assert_eq!(Store::open(temp.path()).unwrap_err().code, "teams_inactive");
    assert!(!temp.path().join("teams").exists());
    std::fs::create_dir(temp.path().join("teams")).unwrap();
    std::fs::write(temp.path().join("teams/preview"), "on").unwrap();
    let store = Store::open(temp.path()).unwrap();
    assert!(store.team("missing").unwrap().is_none());
    assert!(!temp.path().join("teams/teams-v1.sqlite3").exists());
}

use crate::teams::ops::{
    self,
    tests::{create, person, refused, request, route, service, task, Harness, TEAM},
    Operation, Request,
};

#[test]
fn empty_published_store_recovers_schema_initialization() {
    let mut h = Harness::empty();
    let path = h.temp.path().join("teams/teams-v1.sqlite3");
    #[cfg(unix)]
    crate::sqlite_file::create(&path).unwrap();
    #[cfg(windows)]
    drop(
        crate::platform::private_open(std::fs::OpenOptions::new().create_new(true).write(true))
            .open(&path)
            .unwrap(),
    );
    h.store = Store::open(h.temp.path()).unwrap();
    h.run(&person(), create()).unwrap();
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, 1);
}
/// A version-2 store from before notes keeps its rows and gains the three
/// additive columns on its next open; no schema version bump is involved.
#[test]
fn a_version_two_store_from_before_notes_gains_the_columns_and_keeps_rows() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, ops::TaskStage::InReview);
    h.report(node, "m1");
    let db = h.store.db.as_ref().unwrap();
    db.execute_batch(
        "ALTER TABLE reports DROP COLUMN notes;
         ALTER TABLE inbox DROP COLUMN notes;
         ALTER TABLE cards DROP COLUMN notes;",
    )
    .unwrap();
    for table in ["reports", "inbox", "cards"] {
        let found: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name='notes')",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!found, "{table} still has notes before reopen");
    }
    drop(h.store);
    let mut store = Store::open(h.temp.path()).unwrap();
    for table in ["reports", "inbox", "cards"] {
        let found: bool = store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name='notes')",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(found, "{table} did not gain notes");
    }
    let report = store
        .report(TEAM, node, 1)
        .unwrap()
        .expect("the old row is kept");
    assert_eq!(report.text, "Implemented and checked");
    assert!(report.notes.is_empty());
    // A second open is a no-op and still finds the columns.
    drop(store);
    store = Store::open(h.temp.path()).unwrap();
    assert_eq!(
        store.report(TEAM, node, 1).unwrap().unwrap().text,
        "Implemented and checked"
    );
}

#[test]
fn newer_schema_is_refused_without_modifying_its_version() {
    let h = Harness::new();
    h.store
        .db
        .as_ref()
        .unwrap()
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    refused(Store::open(h.temp.path()), code::INVALID_STATE);
    let version: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION + 1);
}
#[test]
fn version_one_is_refused_as_an_unreleased_store() {
    let h = Harness::new();
    let db = h.store.db.as_ref().unwrap();
    db.pragma_update(None, "user_version", 1).unwrap();
    let error = Store::open(h.temp.path()).unwrap_err();
    assert_eq!(error.code, code::INVALID_STATE);
    assert!(error.message.contains("pre-release"));
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        1
    );
}
#[test]
fn an_empty_handle_can_refresh_after_another_writer_creates_a_team() {
    let mut h = Harness::empty();
    let mut writer = Store::open(h.temp.path()).unwrap();
    ops::apply(&mut writer, &person(), &request(0, create()), 1).unwrap();
    h.store.refresh().unwrap();
    assert!(h.store.team(TEAM).unwrap().is_some());
}
#[test]
fn queue_and_journal_pages_exclude_additions_after_the_first_page() {
    let mut h = Harness::new();
    for n in 1..=5 {
        h.add(&person(), n);
    }
    let first = h.store.queue(TEAM, 2, None).unwrap();
    let journal = h.store.journal(TEAM, 2, None).unwrap();
    let old_count = h.store.journal(TEAM, 200, None).unwrap().rows.len();
    h.add(&person(), 6);
    let mut ids = first.rows.iter().map(|n| n.id).collect::<Vec<_>>();
    let mut cursor = first.next;
    while let Some(c) = cursor {
        let p = h.store.queue(TEAM, 2, Some(&c)).unwrap();
        ids.extend(p.rows.iter().map(|n| n.id));
        cursor = p.next;
    }
    assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    let mut count = journal.rows.len();
    let mut cursor = journal.next;
    while let Some(c) = cursor {
        let p = h.store.journal(TEAM, 2, Some(&c)).unwrap();
        count += p.rows.len();
        cursor = p.next;
    }
    assert_eq!(count, old_count);
}
#[test]
fn node_rows_and_dependencies_share_one_snapshot_during_a_concurrent_route() {
    let mut h = Harness::new();
    for n in 1..=3 {
        h.add(&person(), n);
    }
    let mut old = route(1);
    if let Operation::Route { deps, writes, .. } = &mut old {
        deps.push(Dep {
            node: 2,
            kind: DepKind::Hard,
        });
        writes.push("old.rs".into());
    }
    h.run(&person(), old).unwrap();
    let mut writer = Store::open(h.temp.path()).unwrap();
    let p = h
        .store
        .node_page_with(TEAM, None, 200, None, || {
            let mut new = route(1);
            if let Operation::Route { deps, writes, .. } = &mut new {
                deps.push(Dep {
                    node: 3,
                    kind: DepKind::Hard,
                });
                writes.push("new.rs".into());
            }
            let rev = writer.team(TEAM).unwrap().unwrap().revision;
            ops::apply(&mut writer, &person(), &request(rev, new), 1).unwrap();
        })
        .unwrap();
    assert_eq!(p.rows[0].writes, vec!["old.rs"]);
    assert_eq!(p.rows[0].deps[0].node, 2);
    assert_eq!(h.node(1).writes, vec!["new.rs"]);
    assert_eq!(h.node(1).deps[0].node, 3);
}
#[test]
fn every_list_rejects_over_limit_and_foreign_cursors() {
    let mut h = Harness::new();
    h.add(&person(), 1);
    refused(h.store.queue(TEAM, 201, None), code::OVER_CAP);
    refused(h.store.members(TEAM, 0, None), code::OVER_CAP);
    let c = h.store.queue(TEAM, 1, None).unwrap().next.unwrap();
    refused(h.store.cards(TEAM, 1, Some(&c)), code::INVALID_STATE);
    refused(h.store.inbox(TEAM, 201, None), code::OVER_CAP);
    refused(h.store.effects(TEAM, 201, None), code::OVER_CAP);
    refused(h.store.journal(TEAM, 201, None), code::OVER_CAP);
}
#[test]
fn a_thousand_decided_nodes_leave_the_live_page_unchanged() {
    let mut h = Harness::new();
    h.add(&person(), 1);
    let before = h.store.queue(TEAM, 200, None).unwrap();
    for n in 2..=1001 {
        let node = h.add(&person(), n);
        h.cancel_fact(node);
    }
    assert_eq!(h.store.queue(TEAM, 200, None).unwrap(), before);
    assert_eq!(
        h.store
            .nodes_by_state(TEAM, NodeState::Cancelled, 200, None)
            .unwrap()
            .rows
            .len(),
        200
    );
    h.run(
        &person(),
        Operation::AddTask {
            task: task(1002),
            role: None,
            pipeline: None,
        },
    )
    .unwrap();
}
#[test]
fn journal_is_capped_and_archived_rows_keep_sequence_order() {
    let mut h = Harness::new();
    let tx = h.store.db.as_mut().unwrap().transaction().unwrap();
    for n in 0..=LIVE_JOURNAL_CAP {
        tx.execute("INSERT INTO journal(team,at_ms,actor,kind,request_id,text) VALUES(?1,0,?2,'fixture',?3,'Example')",rusqlite::params![TEAM,encode(&person()).unwrap(),format!("fixture-{n}")]).unwrap();
    }
    tx.commit().unwrap();
    h.run(&person(), Operation::Pause).unwrap();
    let count: u64 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM journal WHERE team=?1", [TEAM], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, LIVE_JOURNAL_CAP);
    assert_eq!(
        h.store
            .journal(TEAM, 3, None)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.seq)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}
#[test]
fn late_observations_keep_archived_journals_archived_and_record_node_references() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    h.run(&person(), Operation::Stop).unwrap();
    h.run(&person(), Operation::Archive).unwrap();
    h.cancel_fact(node);
    let count: u32 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM journal WHERE team=?1", [TEAM], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
    let rows = h.store.journal(TEAM, 200, None).unwrap().rows;
    assert!(rows.last().unwrap().refs.contains(&format!("node:{node}")));
}
#[cfg(unix)]
#[test]
fn opening_another_connection_keeps_sqlite_locks_and_refuses_symlinks() {
    let h = Harness::new();
    let path = h.temp.path().join("teams/teams-v1.sqlite3");
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("BEGIN IMMEDIATE;")
        .unwrap();
    assert!(crate::sqlite_file::foreign_lock_refused(&path));
    let reopened = Store::open(h.temp.path()).unwrap();
    assert!(crate::sqlite_file::foreign_lock_refused(&path));
    drop(reopened);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("ROLLBACK;")
        .unwrap();
    drop(h.store);
    let moved = h.temp.path().join("teams/saved.sqlite3");
    std::fs::rename(&path, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &path).unwrap();
    refused(Store::open(h.temp.path()), code::INVALID_STATE);
}

#[test]
fn integration_accepts_page_by_open_node_and_filter_authority_and_recorded_facts() {
    use ops::tests::{service, COMMIT, TEAM_B};
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let recorded = h.accept_decision(node);
    h.run(
        &service(),
        Operation::RecordIntegration {
            node,
            decision: recorded,
            commit: COMMIT.into(),
        },
    )
    .unwrap();
    let cancelled = h.add(&person(), 2);
    h.run(
        &lead,
        Operation::Cancel {
            node: cancelled,
            reason: "No work needed".into(),
        },
    )
    .unwrap();
    let ready = h.add(&person(), 3);
    h.submit(ready, "m1");
    h.report(ready, "m1");
    h.run(&lead, Operation::Accept { node: ready }).unwrap();
    let expected = h.accept_decision(ready);
    h.settings(|s| s.delegations.merge_accepted = false);
    assert!(h
        .store
        .integration_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    let card = h
        .store
        .waiting(200, None)
        .unwrap()
        .rows
        .into_iter()
        .find(|r| r.card.node == Some(ready))
        .unwrap()
        .card
        .seq;
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "approve".into(),
        },
    )
    .unwrap();
    let owner = h.add(&person(), 4);
    h.submit(owner, "m1");
    h.report(owner, "m1");
    h.run(&lead, Operation::Accept { node: owner }).unwrap();
    let first = h.store.integration_accepts(TEAM, 1, None).unwrap();
    assert!(first.rows.is_empty());
    let mut cursor = first.next.clone();
    assert!(cursor.is_some());
    let mut rows = Vec::new();
    while let Some(c) = cursor {
        let page = h.store.integration_accepts(TEAM, 1, Some(&c)).unwrap();
        assert!(page.rows.len() <= 1);
        rows.extend(page.rows);
        cursor = page.next;
    }
    assert_eq!(
        rows,
        vec![IntegrationAccept {
            seq: expected,
            node: ready,
            generation: 1
        }]
    );
    refused(h.store.integration_accepts(TEAM, 0, None), code::OVER_CAP);
    refused(h.store.integration_accepts(TEAM, 201, None), code::OVER_CAP);
    refused(
        h.store.integration_accepts(TEAM_B, 1, first.next.as_ref()),
        code::INVALID_STATE,
    );
    h.cancel_fact(ready);
    assert!(h
        .store
        .integration_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    h.settings(|s| s.integration = Integration::None);
    assert!(h
        .store
        .integration_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn integration_accept_discovery_uses_the_open_decision_index() {
    let h = Harness::new();
    let db = h.store.db.as_ref().unwrap();
    let mut s = db
        .prepare(&format!(
            "EXPLAIN QUERY PLAN {}",
            pages::integration_accepts_sql()
        ))
        .unwrap();
    let plan = s
        .query_map(
            rusqlite::params![TEAM, 0, 100, 1, DecisionState::Ready],
            |r| r.get::<_, String>(3),
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|line| line.contains("USING INDEX decision_pending")),
        "{plan:?}"
    );
    assert!(
        !plan.iter().any(|line| line.contains("TEMP B-TREE")),
        "{plan:?}"
    );
}

#[test]
fn settled_decision_history_does_not_delay_the_first_open_accept_page() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    let tx = h.store.db.as_mut().unwrap().transaction().unwrap();
    for _ in 0..4000 {
        tx.execute("INSERT INTO decisions(team,node,generation,actor,kind,text,state,stopped_ready) VALUES(?1,?2,1,?3,'accept','Historical acceptance',?4,1)",rusqlite::params![TEAM,node,encode(&person()).unwrap(),DecisionState::Settled]).unwrap();
    }
    tx.commit().unwrap();
    h.run(&lead, Operation::Accept { node }).unwrap();
    let seq: u64 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT seq FROM decisions WHERE team=?1 AND state=?2",
            rusqlite::params![TEAM, DecisionState::Ready],
            |r| r.get(0),
        )
        .unwrap();
    let page = h.store.integration_accepts(TEAM, 1, None).unwrap();
    assert_eq!(
        page.rows,
        vec![IntegrationAccept {
            seq,
            node,
            generation: 1
        }]
    );
}

#[test]
fn integration_accept_pages_use_node_order_and_keep_their_node_ceiling() {
    use ops::tests::service;
    let mut h = Harness::new();
    let (first_node, lead) = h.reported();
    h.run(&lead, Operation::Accept { node: first_node })
        .unwrap();
    let low = h.add(&person(), 2);
    let high = h.add(&person(), 3);
    for node in [high, low] {
        h.submit(node, "m1");
        h.report(node, "m1");
        h.run(&lead, Operation::Accept { node }).unwrap();
    }
    assert!(h.accept_decision(high) < h.accept_decision(low));
    let first = h.store.integration_accepts(TEAM, 1, None).unwrap();
    let added = h.add(&person(), 4);
    h.submit(added, "m1");
    h.report(added, "m1");
    h.run(&lead, Operation::Accept { node: added }).unwrap();
    let mut rows = first.rows;
    let mut cursor = first.next;
    while let Some(c) = cursor {
        let page = h.store.integration_accepts(TEAM, 1, Some(&c)).unwrap();
        rows.extend(page.rows);
        cursor = page.next;
    }
    assert_eq!(
        rows.iter().map(|r| r.node).collect::<Vec<_>>(),
        vec![first_node, low, high]
    );
    assert_eq!(
        h.store
            .integration_accepts(TEAM, 200, None)
            .unwrap()
            .rows
            .len(),
        4
    );
    // A service uses the listed identity directly in its subsequent fact.
    h.run(
        &service(),
        Operation::RecordIntegration {
            node: low,
            decision: rows[1].seq,
            commit: ops::tests::COMMIT.into(),
        },
    )
    .unwrap();
}

#[test]
fn integration_discovery_excludes_stage_and_no_branch_accepts() {
    for stage in [false, true] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        if stage {
            let mut n = h.node(node);
            n.kind = NodeKind::Stage;
            n.stage_of = Some(2);
            h.store
                .db
                .as_ref()
                .unwrap()
                .execute(
                    "UPDATE nodes SET data=?3 WHERE team=?1 AND id=?2",
                    rusqlite::params![TEAM, node, rows::node_payload(&n).unwrap()],
                )
                .unwrap();
        } else {
            h.settings(|s| s.integration = Integration::None);
        }
        let actor = if stage { person() } else { lead };
        h.run(&actor, Operation::Accept { node }).unwrap();
        assert!(h
            .store
            .integration_accepts(TEAM, 200, None)
            .unwrap()
            .rows
            .is_empty());
    }
}

#[test]
fn card_lookup_is_scoped_and_reads_the_persons_decision_by_primary_key() {
    let mut h = Harness::new();
    let card = h
        .run(
            &person(),
            Operation::OpenCard {
                kind: "proposal".into(),
                text: "Wait for a person".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    let before = h.store.cards(TEAM, 1, None).unwrap().rows.pop().unwrap();
    assert_eq!(h.store.card(TEAM, card).unwrap(), Some(before));
    h.run(
        &person(),
        Operation::DecideCard {
            card,
            decision: "acknowledged".into(),
        },
    )
    .unwrap();
    let c = h.store.card(TEAM, card).unwrap().unwrap();
    assert_eq!(c.decision.as_deref(), Some("acknowledged"));
    assert_eq!(c.decided_actor.unwrap()["kind"], "person");
    assert!(h.store.card(TEAM, 9_999_999).unwrap().is_none());
    refused(h.store.card(ops::tests::TEAM_B, card), code::INVALID_STATE);
    let db = h.store.db.as_ref().unwrap();
    let mut s = db
        .prepare(&format!("EXPLAIN QUERY PLAN {}", rows::CARD_BY_SEQ_SQL))
        .unwrap();
    let plan = s
        .query_map([card], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    assert!(
        plan.iter().any(|r| r.contains("USING INTEGER PRIMARY KEY")),
        "{plan:?}"
    );
}

#[test]
fn card_lookup_is_lazy_and_honors_the_preview_gate() {
    let h = Harness::empty();
    assert!(h.store.card(TEAM, 1).unwrap().is_none());
    assert!(!h.temp.path().join("teams/teams-v1.sqlite3").exists());
    std::fs::remove_file(h.temp.path().join("teams/preview")).unwrap();
    refused(h.store.card(TEAM, 1), code::TEAMS_INACTIVE);
}

#[test]
fn observations_upgrade_version_two_and_keep_departures_released_after_reopen() {
    use ops::tests::service;
    use ops::{KeyState, TaskStage};
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let old = h.store.task_observation(TEAM, node).unwrap();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TABLE task_observations;")
        .unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(h.store.task_observation(TEAM, node).unwrap(), old);
    assert_eq!(old.unwrap().key, KeyState::Ours);
    let mut o = h.observation(node, TaskStage::Todo);
    o.key = KeyState::Absent;
    h.run(
        &service(),
        Operation::ObserveTask {
            node,
            observation: o.clone(),
        },
    )
    .unwrap();
    let before = h.node(node);
    h.store = Store::open(h.temp.path()).unwrap();
    let saved = h.store.task_observation(TEAM, node).unwrap().unwrap();
    assert_eq!(saved.key, KeyState::Absent);
    assert_eq!(saved.revision, o.revision);
    h.observe(node, TaskStage::Todo);
    assert_eq!(h.node(node), before);
    assert!(h.store.queue(TEAM, 200, None).unwrap().rows.is_empty());
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
}

/// zerus-63rru.53 (c): `member_handles` is a common batch of `CREATE ... IF
/// NOT EXISTS` objects. It moves neither the core version nor the layout
/// mark, the two values an older build refuses on, so an older build opens
/// the store as before and never reads or writes the table; a store from
/// before the table gains it at its next open by this build.
#[test]
fn member_handles_move_neither_the_version_nor_the_mark_an_older_build_checks() {
    use crate::teams::ops::tests::{Harness, TEAM};
    let h = Harness::new();
    let path = h.temp.path().join("teams/teams-v1.sqlite3");
    let read = |db: &rusqlite::Connection| -> (u32, u32, bool) {
        (
            db.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap(),
            db.query_row("SELECT version FROM timers_extension WHERE id=1", [], |r| r.get(0))
                .unwrap(),
            db.query_row(
                "SELECT count(*)=3 FROM sqlite_master WHERE name IN ('member_handles','member_handles_terminal_v1','member_handles_removed_v1')",
                [],
                |r| r.get(0),
            )
            .unwrap(),
        )
    };
    // The values this build writes: the core version and its current
    // layout mark (`timers::EXTENSION`, which later changes move on).
    let current = (2, super::timers::EXTENSION, true);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(read(&db), current);
    // A store as an older build leaves it: without the table.
    db.execute_batch(
        "DROP TRIGGER member_handles_terminal_v1; DROP TRIGGER member_handles_removed_v1; DROP TABLE member_handles;",
    )
    .unwrap();
    drop(db);
    let store = Store::open(h.temp.path()).unwrap();
    assert_eq!(store.member_handle(TEAM, "m1").unwrap(), None);
    drop(store);
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(read(&db), current);
}

#[test]
fn an_oversized_saved_request_is_refused_before_it_is_read() {
    let mut h = Harness::empty();
    h.run(&person(), create()).unwrap();
    let mut saved = request(0, create());
    if let Operation::Create { goal, .. } = &mut saved.operation {
        *goal = "x".repeat(300_000);
    }
    let id = saved.request_id.clone();
    let content = serde_json::to_string(&saved).unwrap();
    assert!(content.len() > 262_144);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "INSERT INTO requests(id,team,actor,content,result) VALUES(?1,?2,'{}',?3,'{}')",
            rusqlite::params![id, TEAM, content],
        )
        .unwrap();
    let error = h.store.creation_request(&id).unwrap_err();
    assert_eq!(error.code, code::OVER_CAP);
}

/// zerus-63rru.35 (1): a version-2 store from before the timers extension
/// creates that extension on its first open. When another writer holds the
/// store, the opener never begins, so its refusal is store_busy, not a
/// storage failure, and it writes no mark.
#[test]
fn an_extension_upgrade_that_cannot_begin_answers_store_busy() {
    let h = Harness::new();
    let path = h.temp.path().join("teams/teams-v1.sqlite3");
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DELETE FROM timers_extension")
        .unwrap();
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let error = Store::open(h.temp.path()).unwrap_err();
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(error.code, code::STORE_BUSY);
    let db = rusqlite::Connection::open(&path).unwrap();
    let mark: Option<u32> = db
        .query_row("SELECT version FROM timers_extension WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()
        .unwrap();
    assert_eq!(mark, None, "the refused open wrote no mark");
}

/// zerus-63rru.35 (6b): creating a store is one immediate transaction. A
/// writer that cannot begin it because another connection holds the store
/// answers store_busy, never a storage failure.
#[test]
fn a_new_store_creation_that_cannot_begin_answers_store_busy() {
    let temp = tempfile::tempdir().unwrap();
    crate::platform::private_dir(&temp.path().join("teams")).unwrap();
    std::fs::write(temp.path().join("teams/preview"), "on").unwrap();
    let path = temp.path().join("teams/teams-v1.sqlite3");
    let mut store = Store::open(temp.path()).unwrap();
    drop(
        crate::platform::private_open(std::fs::OpenOptions::new().create_new(true).write(true))
            .open(&path)
            .unwrap(),
    );
    let writer = rusqlite::Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let error = store.ensure().unwrap_err();
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(error.code, code::STORE_BUSY);
    let db = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 0, "the refused creation wrote no schema");
}

#[test]
fn an_older_card_row_without_parameters_still_reads() {
    // The compatibility contract (zerus-63rru.34): a row written before the
    // parameters field existed deserializes with none.
    let older = serde_json::json!({
        "seq": 1,
        "kind": "note",
        "node": null,
        "actor": {"kind": "person", "person": "local"},
        "text": "Tell the team the release moved to Monday.",
        "state": "open",
        "decision": null,
        "decided_actor": null,
        "created_ms": 1700000000000i64,
        "decisions": ["acknowledged"]
    });
    let card: crate::teams::store::records::Card = serde_json::from_value(older).unwrap();
    assert!(card.parameters.is_empty());
}

/// The concurrency schedule's own count of the additive-column read. Only the
/// test's armed threads count, so other tests opening old-shape stores in
/// parallel never join the schedule.
static WATCH_MISSING_COLUMN: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static MISSING_COLUMN_READS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
thread_local! {
    static MISSING_COLUMN_ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
pub(super) fn after_missing_column() {
    if WATCH_MISSING_COLUMN.load(std::sync::atomic::Ordering::SeqCst)
        && MISSING_COLUMN_ARMED.with(std::cell::Cell::get)
    {
        MISSING_COLUMN_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The legacy store shape is produced by dropping the additive column from a
/// product-written store (explicit setup); its team and card rows come from
/// the product's own writers, never from inserted JSON.
fn legacy_store_without_parameters() -> (Harness, std::path::PathBuf, u64) {
    let mut h = Harness::new();
    let old = h
        .run(
            &service(),
            Operation::OpenCard {
                kind: "note".into(),
                text: "Written before the parameters column existed".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    let state = h.temp.path().to_path_buf();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("ALTER TABLE cards DROP COLUMN parameters")
        .unwrap();
    drop(h.store.db.take());
    (h, state, old)
}

#[test]
fn an_older_store_gains_parameters_and_keeps_reading_its_own_cards() {
    let (h, state, old) = legacy_store_without_parameters();
    let mut store = Store::open(&state).unwrap();
    let page = store.waiting(200, None).unwrap();
    let kept = page.rows.iter().find(|row| row.card.seq == old).unwrap();
    assert!(kept.card.parameters.is_empty());
    let request = Request {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: TEAM.into(),
        expected_revision: store.team(TEAM).unwrap().unwrap().revision,
        operation: Operation::OpenCard {
            kind: "launch_blocked".into(),
            text: "New card on an older store".into(),
            node: None,
            decisions: None,
            parameters: Some(vec![("member".into(), "m1".into())]),
        },
    };
    let card = ops::apply(&mut store, &service(), &request, 2000)
        .unwrap()
        .item
        .unwrap();
    assert_eq!(store.card(TEAM, card).unwrap().unwrap().parameters[0].1, "m1");
    assert!(store.db.as_ref().unwrap().is_autocommit());
    drop(store);
    drop(h);
}

#[test]
fn a_newer_mark_after_the_fence_is_refused_before_the_column_is_added() {
    let (h, state, _) = legacy_store_without_parameters();
    let path = state.join("teams/teams-v1.sqlite3");
    let other_path = path.clone();
    super::timers::AFTER_FENCE.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            let other = Connection::open(&other_path).unwrap();
            other
                .execute_batch(&format!(
                    "BEGIN IMMEDIATE; UPDATE timers_extension SET version={}; COMMIT;",
                    super::timers::EXTENSION + 100
                ))
                .unwrap();
            Ok(())
        }));
    });
    let error = Store::open(&state).unwrap_err();
    assert_eq!(error.code, code::INVALID_STATE);
    assert!(error.message.contains("newer"), "{error:?}");
    let raw = Connection::open(&path).unwrap();
    let added: bool = raw
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('cards') WHERE name='parameters')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !added,
        "refusing a newer store must leave its schema unchanged"
    );
    drop(raw);
    drop(h);
}

#[test]
fn two_opens_that_saw_the_missing_column_never_fail_on_a_duplicate() {
    let (h, state, _) = legacy_store_without_parameters();
    let path = state.join("teams/teams-v1.sqlite3");
    // A writer holds the store so both opens read the missing column, then
    // both reach the locked add together.
    let blocker = Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
    WATCH_MISSING_COLUMN.store(true, std::sync::atomic::Ordering::SeqCst);
    MISSING_COLUMN_READS.store(0, std::sync::atomic::Ordering::SeqCst);
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let state = state.clone();
            std::thread::spawn(move || {
                MISSING_COLUMN_ARMED.with(|armed| armed.set(true));
                let opened =
                    Store::open(&state).map(|store| store.waiting(200, None).unwrap().rows.len());
                MISSING_COLUMN_ARMED.with(|armed| armed.set(false));
                opened
            })
        })
        .collect();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while MISSING_COLUMN_READS.load(std::sync::atomic::Ordering::SeqCst) < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "both opens must read the missing column before the writer releases"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    blocker.execute_batch("ROLLBACK;").unwrap();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    WATCH_MISSING_COLUMN.store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(
        results.iter().all(std::result::Result::is_ok),
        "an additive extension must be rechecked under its writer lock: {results:?}"
    );
    let raw = Connection::open(path).unwrap();
    let added: bool = raw
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('cards') WHERE name='parameters')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(added);
    drop(raw);
    drop(blocker);
    drop(h);
}
