use super::{testing::*, *};

fn held() -> OwnedLock {
    UNLOCK_ATTEMPTS.with(|attempts| attempts.set(0));
    FAIL_UNLOCK.with(|fail| fail.set(false));
    let file = tempfile::tempfile().unwrap();
    fs2::FileExt::lock_exclusive(&file).unwrap();
    OwnedLock::acquired(file)
}

fn attempts() -> u32 {
    UNLOCK_ATTEMPTS.with(std::cell::Cell::get)
}

#[test]
fn release_unlocks_once_and_drop_does_not_retry() {
    let lock = held();
    assert!(lock.release().is_ok());
    assert_eq!(attempts(), 1);
}

#[test]
fn a_failed_release_is_not_retried_by_drop() {
    let lock = held();
    FAIL_UNLOCK.with(|fail| fail.set(true));
    let error = lock.release().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "injected unlock failure");
    assert_eq!(attempts(), 1);
    FAIL_UNLOCK.with(|fail| fail.set(false));
    assert_eq!(attempts(), 1);
}

#[test]
fn a_plain_drop_unlocks_once() {
    drop(held());
    assert_eq!(attempts(), 1);
}

#[test]
fn a_foreign_owner_never_unlocks_on_drop() {
    let mut lock = held();
    set_owner_pid(&mut lock, std::process::id().wrapping_add(1));
    drop(lock);
    assert_eq!(attempts(), 0);
}
