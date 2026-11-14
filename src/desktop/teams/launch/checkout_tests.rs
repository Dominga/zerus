use super::*;
use crate::desktop::teams::ports as p;
use crate::teams::ops::tests::{Harness, TEAM};
struct Verified(p::Worktree);
impl Effects for Verified {
    fn placement(&mut self, _: &str, _: &str) -> Result<p::Placement> {
        unreachable!()
    }
    fn branch_exists(&mut self, _: &BranchPlan) -> Result<bool> {
        unreachable!()
    }
    fn create_branch(&mut self, _: &BranchPlan) -> Effect<()> {
        unreachable!()
    }
    fn accounts(&mut self, _: &str) -> Result<Vec<zerus_team_plan::accounts::AccountUsage>> {
        unreachable!()
    }
    fn worktree(&mut self, _: &Plan, _: &str) -> Effect<Option<p::Worktree>> {
        Effect::Submitted(Some(self.0.clone()))
    }
    fn verify_worktree(&mut self, _: &Plan) -> Result<p::Worktree> {
        Ok(self.0.clone())
    }
    fn launch(&mut self, _: &Plan, _: &p::Worktree, _: &str) -> Effect<()> {
        unreachable!()
    }
    fn observe(&mut self, _: &Plan) -> Result<LaunchRead> {
        unreachable!()
    }
    fn parent(&mut self, _: &Plan, _: &p::ObservedSession, _: &str) -> String {
        unreachable!()
    }
}
fn plan(team: &Team, path: &Path) -> Plan {
    Plan {
        id: uuid::Uuid::new_v4().to_string(),
        team: team.id.clone(),
        person: team.owner.clone(),
        machine: team.home.clone(),
        project: team.project.clone(),
        member: team.lead.clone(),
        role: "lead".into(),
        agent: "claude".into(),
        account: "account-example".into(),
        model: String::new(),
        effort: String::new(),
        base_commit: team.base_commit.clone(),
        parent: None,
        launch_id: uuid::Uuid::new_v4().to_string(),
        session_name: String::new(),
        placement: p::Placement {
            source: path.to_str().unwrap().into(),
            git_dir: path.join("common").to_str().unwrap().into(),
            branch: "fixture".into(),
            destination: path.to_str().unwrap().into(),
        },
    }
}
#[test]
fn both_verified_worktree_transitions_record_the_original_directory_once() {
    for deferred in [false, true] {
        let h = Harness::new();
        let root = crate::platform::canonical(h.temp.path()).unwrap();
        let team = h.store.team(TEAM).unwrap().unwrap();
        let path = root.join("member");
        std::fs::create_dir(&path).unwrap();
        let plan = plan(&team, &path);
        let mut r = Runtime::open(h.temp.path(), &team.home, "epoch").unwrap();
        r.ledger = Some(Ledger::open(h.temp.path()).unwrap());
        let mut row = Row::new(
            &team,
            &team.lead,
            "epoch",
            if deferred {
                State::WorktreeVerify
            } else {
                State::WorktreeIntent
            },
        );
        row.plan = Some(plan.clone());
        r.ledger().insert(&row, 100).unwrap();
        r.admitted = Some((team.id.clone(), row.original.clone()));
        let tree = p::Worktree {
            path: path.to_str().unwrap().into(),
            ownership_nonce: uuid::Uuid::new_v4().to_string(),
        };
        let mut effects = Verified(tree.clone());
        if deferred {
            r.verify_worktree(&mut effects, &mut row, 101).unwrap();
        } else {
            r.worktree(&mut effects, &mut row, 101).unwrap();
        }
        let reopened = Ledger::open(h.temp.path()).unwrap();
        assert!(reopened
            .checkout(&team.id, &team.lead, &plan.launch_id)
            .unwrap()
            .unwrap()
            .matches(&tree.path, &plan.placement.git_dir, &tree.ownership_nonce));
        assert_eq!(row.state, State::WorktreeDone);
    }
}

/// The checkout module's four store calls share one mapping: a lock it could
/// not take is a busy non-start, while a broken store stays a storage
/// failure (zerus-63rru.35).
#[test]
fn checkout_store_errors_map_a_busy_lock_to_store_busy_and_a_broken_store_to_storage_failed() {
    for code in [
        rusqlite::ffi::SQLITE_BUSY,
        rusqlite::ffi::SQLITE_LOCKED,
    ] {
        let problem = crate::teams::store::begin_error(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(code),
            None,
        ));
        assert_eq!(problem.code, "store_busy", "{code}");
    }
    let problem = crate::teams::store::begin_error(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
        None,
    ));
    assert_eq!(problem.code, "storage_failed");
}

#[test]
fn a_busy_launch_ledger_records_no_checkout_and_answers_store_busy() {
    let h = Harness::new();
    let root = crate::platform::canonical(h.temp.path()).unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let path = root.join("member");
    std::fs::create_dir(&path).unwrap();
    let plan = plan(&team, &path);
    let tree = p::Worktree {
        path: path.to_str().unwrap().into(),
        ownership_nonce: uuid::Uuid::new_v4().to_string(),
    };
    let mut ledger = Ledger::open(h.temp.path()).unwrap();
    let writer = rusqlite::Connection::open(
        h.temp.path().join("teams/member-launch-v1.sqlite3"),
    )
    .unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let problem = ledger.record_checkout(&plan, &tree).unwrap_err();
    assert_eq!(problem.code, "store_busy", "{problem:?}");
    writer.execute_batch("ROLLBACK").unwrap();
    assert!(ledger.record_checkout(&plan, &tree).unwrap());
    assert!(ledger.checkout(&team.id, &team.lead, &plan.launch_id).unwrap().is_some());
}

/// Injected-error seam: a busy failure at each checkout statement's own result
/// answers `store_busy` and records no checkout. This is test-only injection,
/// not a real concurrent lock; the test above keeps the real BEGIN contention.
#[test]
fn an_injected_busy_checkout_statement_answers_store_busy() {
    use crate::desktop::teams::ledger::checkout::{injection, Statement};
    let h = Harness::new();
    let root = crate::platform::canonical(h.temp.path()).unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    let path = root.join("member");
    std::fs::create_dir(&path).unwrap();
    let tree = p::Worktree {
        path: path.to_str().unwrap().into(),
        ownership_nonce: uuid::Uuid::new_v4().to_string(),
    };
    let mut ledger = Ledger::open(h.temp.path()).unwrap();
    for statement in [Statement::Insert, Statement::Commit, Statement::Select] {
        let plan = plan(&team, &path);
        injection::fail(statement);
        let problem = ledger.record_checkout(&plan, &tree).unwrap_err();
        assert_eq!(problem.code, "store_busy", "{statement:?}: {problem:?}");
        assert!(
            ledger
                .checkout(&team.id, &team.lead, &plan.launch_id)
                .unwrap()
                .is_none(),
            "{statement:?} wrote a checkout"
        );
        assert!(
            ledger.record_checkout(&plan, &tree).unwrap(),
            "{statement:?} did not clear"
        );
    }
}
