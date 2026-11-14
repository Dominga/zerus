use super::*;
use crate::teams::ops::tests::*;
#[test]
fn service_read_uses_open_state_index_and_keeps_cursor_scope() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    let page = h.store.open_effects(TEAM, 1, None).unwrap();
    assert_eq!(page.rows[0].request_id, id);
    assert!(h.store.open_effects(TEAM_B, 1, page.next.as_ref()).is_err());
    h.outcome(
        &id,
        crate::teams::ops::HandoutOutcome::Refused {
            reason: "Refused".into(),
        },
    );
    assert!(h
        .store
        .open_effects(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    let db = h.store.db.as_ref().unwrap();
    let mut s = db.prepare(&format!("EXPLAIN QUERY PLAN {OPEN}")).unwrap();
    let details = s
        .query_map(params![TEAM, 0, 100, 20], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        details.iter().any(|s| s.contains("effects_pending")
            && s.contains("state=?")
            && s.contains("seq>?")
            && s.contains("seq<?")),
        "{details:?}"
    );
}
#[test]
fn report_is_pinned_to_holder_and_generation() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, crate::teams::ops::TaskStage::InReview);
    h.run(
        &service(),
        crate::teams::ops::Operation::RecordReport {
            node,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: COMMIT.into(),
            notes: vec![
                crate::teams::ops::ReportNote::new("text_shortened"),
                crate::teams::ops::ReportNote::new("worktree_dirty"),
            ],
        },
    )
    .unwrap();
    let report = h.store.report(TEAM, node, 1).unwrap().unwrap();
    assert_eq!(report.member, "m1");
    assert_eq!(report.commit, COMMIT);
    assert_eq!(report.text, "Report");
    assert_eq!(
        report.notes,
        vec![
            crate::teams::ops::ReportNote::new("text_shortened"),
            crate::teams::ops::ReportNote::new("worktree_dirty")
        ]
    );
    assert!(h.store.report(TEAM, node, 2).unwrap().is_none());
    assert!(h.store.report(TEAM_B, node, 1).unwrap().is_none());
}

/// A row from before notes keeps the old appended lines in its text and reads
/// with no notes at all: nothing is reinterpreted or moved.
#[test]
fn an_old_report_row_reads_exactly_as_before() {
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, crate::teams::ops::TaskStage::InReview);
    let old = "All done\nWorktree has uncommitted changes.\n[Report text shortened.]";
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "INSERT INTO reports(team,node,generation,actor,text,commit_id) VALUES(?1,?2,1,'{}',?3,?4)",
            params![TEAM, node, old, COMMIT],
        )
        .unwrap();
    let report = h.store.report(TEAM, node, 1).unwrap().unwrap();
    assert_eq!(report.text, old);
    assert!(report.notes.is_empty());
}

/// The base reader's exact SQL and reply shape ignore the notes column: an
/// older build selects named columns and never asks for it.
#[test]
fn an_older_reader_reads_a_notes_row_unchanged() {
    use serde_json::json;
    let mut h = Harness::new();
    h.bind("m1");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, crate::teams::ops::TaskStage::InReview);
    h.run(
        &service(),
        crate::teams::ops::Operation::RecordReport {
            node,
            generation: 1,
            member: "m1".into(),
            text: "Report".into(),
            commit: COMMIT.into(),
            notes: vec![crate::teams::ops::ReportNote::new("worktree_dirty")],
        },
    )
    .unwrap();
    let db = h.store.db.as_ref().unwrap();
    let (text, commit): (String, String) = db
        .query_row(
            "SELECT text, commit_id FROM reports WHERE team=?1 AND node=?2 AND generation=?3",
            params![TEAM, node, 1],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(text, "Report");
    assert_eq!(commit, COMMIT);
    let base = json!({"member": "m1", "text": text, "commit": commit});
    assert_eq!(
        base,
        json!({"member": "m1", "text": "Report", "commit": COMMIT})
    );
    assert_eq!(
        h.store.report(TEAM, node, 1).unwrap().unwrap().notes,
        vec![crate::teams::ops::ReportNote::new("worktree_dirty")]
    );
}
#[test]
fn change_token_sees_other_connections_only_and_missing_store_is_read_only() {
    let mut h = Harness::new();
    let before = h.store.change_token().unwrap();
    h.add(&person(), 1);
    assert_eq!(before, h.store.change_token().unwrap());
    let mut other = Store::open(h.temp.path()).unwrap();
    let team = other.team(TEAM).unwrap().unwrap();
    let r = crate::teams::ops::Request {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: TEAM.into(),
        expected_revision: team.revision,
        operation: crate::teams::ops::Operation::Pause,
    };
    crate::teams::ops::apply(&mut other, &person(), &r, 1234).unwrap();
    assert_ne!(before, h.store.change_token().unwrap());
    let missing = Harness::empty();
    assert!(missing.store.change_token().unwrap().is_none());
    assert!(!missing.temp.path().join("teams/teams-v1.sqlite3").exists());
}
#[test]
fn pending_reopens_use_the_partial_open_decision_index() {
    let mut h = Harness::new();
    h.bind("m1");
    let lead = h.bind("lead");
    let node = h.add(&person(), 1);
    h.submit(node, "m1");
    h.observe(node, crate::teams::ops::TaskStage::InReview);
    h.run(
        &lead,
        crate::teams::ops::Operation::Return {
            node,
            reason: "Please revise".into(),
        },
    )
    .unwrap();
    let page = h.store.pending_reopens(TEAM, 1, None).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].node, node);
    assert!(h
        .store
        .pending_reopens(TEAM_B, 1, page.next.as_ref())
        .is_err());
    let db = h.store.db.as_ref().unwrap();
    let mut s = db
        .prepare(&format!("EXPLAIN QUERY PLAN {REOPENS}"))
        .unwrap();
    let details = s
        .query_map(params![TEAM, 0, 100, 20], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        details.iter().any(|s| s.contains("decision_pending")
            && s.contains("node>?")
            && s.contains("node<?")),
        "{details:?}"
    );
}
#[test]
fn ready_accepts_without_merge_are_discovered_by_the_partial_decision_index() {
    let mut h = Harness::new();
    h.settings(|s| s.integration = zerus_team_plan::model::Integration::None);
    let (node, lead) = h.reported();
    h.run(&lead, crate::teams::ops::Operation::Accept { node })
        .unwrap();
    let page = h.store.pending_accepts(TEAM, 1, None).unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].seq, h.accept_decision(node));
    assert!(h
        .store
        .pending_accepts(TEAM_B, 1, page.next.as_ref())
        .is_err());
    assert!(h
        .store
        .integration_accepts(TEAM, 200, None)
        .unwrap()
        .rows
        .is_empty());
    let db = h.store.db.as_ref().unwrap();
    let mut s = db
        .prepare(&format!("EXPLAIN QUERY PLAN {ACCEPTS}"))
        .unwrap();
    let details = s
        .query_map(params![TEAM, 0, 100, 20], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        details.iter().any(|s| s.contains("decision_pending")
            && s.contains("node>?")
            && s.contains("node<?")),
        "{details:?}"
    );
}
#[test]
fn only_the_named_teams_saved_refusal_proves_no_intent_started() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let r = request(
        revision,
        crate::teams::ops::Operation::HandoutIntent {
            node,
            member: "m1".into(),
            effect_request_id: uuid::Uuid::new_v4().to_string(),
        },
    );
    assert!(!h.store.request_refused(TEAM, &r.request_id).unwrap());
    assert!(crate::teams::ops::apply(&mut h.store, &service(), &r, 100).is_err());
    assert!(h.store.request_refused(TEAM, &r.request_id).unwrap());
    assert!(!h.store.request_refused(TEAM_B, &r.request_id).unwrap());
    let r = request(revision, crate::teams::ops::Operation::Pause);
    crate::teams::ops::apply(&mut h.store, &person(), &r, 101).unwrap();
    assert!(!h.store.request_refused(TEAM, &r.request_id).unwrap());
}

#[test]
fn report_lookup_is_keyed_to_the_exact_node_and_generation() {
    let h = Harness::new();
    let db = h.store.db.as_ref().unwrap();
    let mut q = db.prepare(&format!("EXPLAIN QUERY PLAN {REPORT}")).unwrap();
    let plan = q
        .query_map(params![TEAM, 1, 1], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plan.iter()
            .any(|s| s.contains("effects_attempt")
                && s.contains("team=? AND node=? AND generation=?")),
        "{plan:?}"
    );
    assert!(
        !plan
            .iter()
            .any(|s| s.contains("SCAN") || s.contains("TEMP B-TREE")),
        "{plan:?}"
    );
}

#[test]
fn exact_ready_accept_is_keyed_and_false_after_demotion() {
    let mut h = Harness::new();
    let (node, lead) = h.reported();
    h.run(&lead, crate::teams::ops::Operation::Accept { node })
        .unwrap();
    let d = h.accept_decision(node);
    assert!(h.store.ready_accept(TEAM, node, 1, d).unwrap());
    assert!(!h.store.ready_accept(TEAM_B, node, 1, d).unwrap());
    assert!(!h.store.ready_accept(TEAM, node, 2, d).unwrap());
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE decisions SET state='awaiting_owner' WHERE seq=?1",
            [d],
        )
        .unwrap();
    assert!(!h.store.ready_accept(TEAM, node, 1, d).unwrap());
    let mut q = h
        .store
        .db
        .as_ref()
        .unwrap()
        .prepare("EXPLAIN QUERY PLAN SELECT 1 FROM decisions WHERE seq=?1 AND team=?2")
        .unwrap();
    let plan = q
        .query_map(params![d, TEAM], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert!(
        plan.iter().any(|p| p.contains("INTEGER PRIMARY KEY")),
        "{plan:?}"
    );
}
