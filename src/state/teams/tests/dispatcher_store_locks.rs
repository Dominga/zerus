//! Task 287 (owner, 2026-11-08): calls failed with "The team database stayed
//! busy for 60 s". A member's report ran git up to four times inside its
//! transaction, and the worker took the write lock once per step. The git
//! reads now run before the lock, and a team's steps of one tick are one
//! transaction.
use super::support::*;
use super::*;

fn git_under_lock() -> Vec<Value> {
    crate::state::teams::store::take_lock_lines().into_iter().filter(|line| line["process"] == "git").collect()
}

/// #1 delivered to dev, whose folder is a git repository, stored.
fn reporting_team(dir: &std::path::Path) -> (Store, Team, std::path::PathBuf) {
    set_git_ceiling(&dir.display().to_string());
    let repo = folder_repo(dir, "dev");
    let mut team = team();
    delivered(&mut team);
    team.folder = repo.display().to_string();
    let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.directory = repo.display().to_string();
    let mut store = Store::open_at(&dir.join("teams/teams.sqlite3"), true).unwrap();
    stored(&mut store, &team);
    (store, team, repo)
}

#[test]
fn a_report_reads_git_before_the_store_lock() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, team, repo) = reporting_team(dir.path());
    let input = json!({"task":1,"attempt":"00000000-0000-4000-8000-000000000001","text":"Done"});
    let actor = ops::Actor::Member("dev".into());
    crate::state::teams::store::take_lock_lines();
    with_git_answers(
        || ops::prefetch_git(&team, "report", &input, &actor),
        || {
            store.mutate(Some(("22222222-2222-4222-8222-222222222222", &input)), &team.id, None, |team, events| {
                ops::apply(team, "report", &input, &actor, &BTreeSet::new(), events)
            })
        },
    )
    .unwrap();
    assert!(git_under_lock().is_empty(), "no git ran while the write lock was held");
    let saved = store.load(&team.id).unwrap().unwrap();
    let attempt = saved.task(1).unwrap().attempts.last().unwrap().clone();
    assert_eq!(attempt.folder_head, git_in(&repo, &["rev-parse", "HEAD"]).trim(), "the report still records its folder's HEAD");
    assert!(!attempt.folder_head.is_empty());
}

#[test]
fn a_git_read_inside_a_transaction_is_logged() {
    let dir = tempfile::tempdir().unwrap();
    let (mut store, team, repo) = reporting_team(dir.path());
    let folder = repo.display().to_string();
    crate::state::teams::store::take_lock_lines();
    store
        .mutate(None, &team.id, None, |_, _| {
            let _ = git_at(&folder, &["rev-parse", "HEAD"]);
            Ok(Value::Null)
        })
        .unwrap();
    assert_eq!(git_under_lock().len(), 1, "the guard names a git under the lock");
    // The same read made before the transaction is served from memory inside it.
    with_git_answers(
        || {
            let _ = git_at(&folder, &["rev-parse", "HEAD"]);
        },
        || {
            store.mutate(None, &team.id, None, |_, _| {
                assert!(git_at(&folder, &["rev-parse", "HEAD"]).is_some_and(|head| !head.is_empty()));
                Ok(Value::Null)
            })
        },
    )
    .unwrap();
    assert!(git_under_lock().is_empty());
}

/// The worker's tick of one team: its steps' changes, and the unread summary
/// brought up to date from the rows, are one transaction.
#[test]
fn a_teams_tick_writes_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = team();
    // A stale summary: the member already read its message.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().unread =
        Some(Unread { count: 1, oldest: "gone".into(), oldest_at: 1.0, ..Default::default() });
    stored(&mut store, &team);
    let revision = store.load(&team.id).unwrap().unwrap().revision;
    crate::state::teams::store::take_lock_lines();
    store.begin_batch(&team.id).unwrap();
    refresh_unread(&mut store, &team).unwrap();
    change(&mut store, &team.id, |team, _| {
        team.stop_reason = "step one".into();
        Ok(())
    })
    .unwrap();
    change(&mut store, &team.id, |team, _| {
        team.stop_reason.push_str(", step two");
        Ok(())
    })
    .unwrap();
    store.end_batch().unwrap();
    let lines = crate::state::teams::store::take_lock_lines();
    assert_eq!(lines.len(), 1, "one transaction: {lines:?}");
    let saved = store.load(&team.id).unwrap().unwrap();
    assert_eq!(saved.revision, revision + 1);
    assert_eq!(saved.stop_reason, "step one, step two");
    assert!(saved.member("dev").unwrap().unread.is_none(), "the summary follows the rows");
}
