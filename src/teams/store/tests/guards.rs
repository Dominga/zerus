use super::*;
use crate::teams::store::Guard;

#[test]
fn every_literal_guard_has_its_definite_rule_class() {
    for (guard, expected) in [
        (Guard::MissingDirectory, code::INVALID_STATE),
        (Guard::DirectoryPrivacy, code::INVALID_STATE),
        (Guard::FilePrivacy, code::INVALID_STATE),
        (Guard::Dependencies, code::OVER_CAP),
        (Guard::LiveNodes, code::OVER_CAP),
        (Guard::ObservedTeam, code::INVALID_STATE),
        (Guard::RetargetedTeam, code::INVALID_STATE),
        (Guard::CatalogReply, code::INVALID_STATE),
        (Guard::LaunchDirectory, code::INVALID_STATE),
        (Guard::LaunchPrivacy, code::INVALID_STATE),
        (Guard::LaunchSchema, code::INVALID_STATE),
        (Guard::LaunchHealth, code::INVALID_STATE),
        (Guard::LaunchKey, code::INVALID_STATE),
        (Guard::LaunchRevision, code::OVER_CAP),
        (Guard::LaunchOriginal, code::INVALID_STATE),
        (Guard::LaunchRecord, code::OVER_CAP),
    ] {
        let problem = guard.problem();
        assert_eq!(problem.code, expected, "{guard:?}");
        assert!(!problem.message.is_empty());
    }
}

#[test]
fn stored_dependency_and_live_node_bounds_are_rule_refusals() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE dependencies(team,node,dependency,kind); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<201) INSERT INTO dependencies SELECT 'team',1,i,'\"hard\"' FROM n;").unwrap();
    assert_eq!(rows::deps(&db, "team", 1).unwrap_err().code, code::OVER_CAP);
    db.execute_batch("CREATE TABLE nodes(team,id,node_state); CREATE TABLE live_tasks(team,node); CREATE TABLE task_observations(team,node,data,attempt_conflict); CREATE TABLE decisions(team,node,state,kind); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<601) INSERT INTO nodes SELECT 'team',i,'accepted' FROM n;").unwrap();
    assert_eq!(
        rows::archive_decided(&db, "team", "{}").unwrap_err().code,
        code::OVER_CAP
    );
}
