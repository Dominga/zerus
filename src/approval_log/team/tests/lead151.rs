//! Audit rows are reserved by begin_team, including on Windows.
use super::*;
#[test]
fn a_startup_brief_has_a_durable_audit_without_allowing_an_empty_resume() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = open(&dir.path().join("audit")).unwrap();
    let question = json!({"source":"team_resume","question_id":"brief:run-1",
        "question_hash":"ab".repeat(32),"run_id":"run-1","conversation_id":"",
        "can_answer":false,"questions":[{"id":"team_resume","question":"Read hgs team inbox","options":[]}]});
    let p = payload("claude", &question);
    assert_eq!(begin(&mut db, &p).unwrap()["eligible"], false);
    let first = begin_team(&mut db, &p, TeamKind::Brief).unwrap();
    let again = begin_team(&mut db, &p, TeamKind::Brief).unwrap();
    assert_eq!(first["fresh"], true);
    assert_eq!(again["fresh"], false);
    assert_eq!(first["request_id"], again["request_id"]);
    assert!(begin_team(&mut db, &p, TeamKind::Resume).is_err());
}
