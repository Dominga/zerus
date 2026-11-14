use super::*;
use std::os::unix::fs::symlink;

#[test]
fn a_symlinked_state_parent_supports_lazy_creation_and_reopening() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    crate::platform::private_dir(&state.join("teams")).unwrap();
    std::fs::write(state.join("teams/preview"), "on").unwrap();
    let parent = temp.path().join("alias");
    symlink(temp.path(), &parent).unwrap();
    let alias = parent.join("state");
    let mut store = Store::open(&alias).unwrap();
    assert!(store.team(TEAM).unwrap().is_none());
    assert!(!state.join("teams/teams-v1.sqlite3").exists());
    ops::apply(&mut store, &person(), &request(0, create()), 1).unwrap();
    assert_eq!(store.state(), crate::platform::canonical(&state).unwrap());
    drop(store);
    let reopened = Store::open(&alias).unwrap();
    assert_eq!(reopened.team(TEAM).unwrap().unwrap().revision, 1);
}

#[test]
fn a_symlinked_database_is_refused_even_under_a_symlinked_state_parent() {
    let h = Harness::new();
    let parent = h.temp.path().join("alias");
    symlink(h.temp.path(), &parent).unwrap();
    drop(h.store);
    let path = h.temp.path().join("teams/teams-v1.sqlite3");
    let saved = h.temp.path().join("teams/saved.sqlite3");
    std::fs::rename(&path, &saved).unwrap();
    symlink(&saved, &path).unwrap();
    refused(Store::open(&parent), code::INVALID_STATE);
}
