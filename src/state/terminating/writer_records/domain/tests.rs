use super::*;
use std::{
    cell::Cell,
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    time::Duration,
};

fn fixture() -> (tempfile::TempDir, Run, Instant) {
    (
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap(),
        Run::new(
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap(),
        Instant::now() + Duration::from_secs(5),
    )
}
fn root(path: &std::path::Path) -> Directory {
    Directory::open(&platform::canonical(path).unwrap()).unwrap()
}
fn path(d: &tempfile::TempDir, scope: &Run) -> PathBuf {
    d.path()
        .join("terminating-writers")
        .join(&scope.session)
        .join(&scope.run)
}
fn parents(d: &tempfile::TempDir, scope: &Run) {
    platform::private_dir(&path(d, scope).parent().unwrap()).unwrap();
}

#[test]
fn exclusive_creation_keeps_the_original_files_after_drop_and_refuses_reinitialization() {
    let (d, scope, until) = fixture();
    let value = seed(root(d.path()), &scope, until, || Ok(())).unwrap();
    value.current(&scope).unwrap();
    let ledger = fs::read(path(&d, &scope).join("pending.json")).unwrap();
    drop(value);
    assert!(seed(root(d.path()), &scope, until, || Ok(())).is_err());
    assert_eq!(
        fs::read(path(&d, &scope).join("pending.json")).unwrap(),
        ledger
    );
}

#[test]
fn existing_partial_special_linked_or_foreign_run_is_never_reset() {
    for kind in ["empty", "partial", "file", "fifo", "link", "foreign"] {
        let (d, scope, until) = fixture();
        parents(&d, &scope);
        let p = path(&d, &scope);
        match kind {
            "file" => fs::write(&p, b"retained").unwrap(),
            "fifo" => {
                use std::os::unix::ffi::OsStrExt;
                let name = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            }
            "link" => symlink(d.path(), &p).unwrap(),
            _ => {
                platform::private_dir(&p).unwrap();
                if kind == "partial" {
                    fs::write(p.join("pending.json"), b"retained").unwrap();
                }
                if kind == "foreign" {
                    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
                }
            }
        }
        assert!(
            seed(root(d.path()), &scope, until, || Ok(())).is_err(),
            "{kind}"
        );
        assert!(fs::symlink_metadata(&p).is_ok());
        if kind == "partial" {
            assert_eq!(fs::read(p.join("pending.json")).unwrap(), b"retained");
        }
    }
}

#[test]
fn linked_or_nonprivate_parents_are_refused_without_following_or_chmod() {
    for session in [false, true] {
        for linked in [false, true] {
            let (d, scope, until) = fixture();
            let foreign = tempfile::tempdir().unwrap();
            let base = d.path().join("terminating-writers");
            let selected = if session {
                platform::private_dir(&base).unwrap();
                base.join(&scope.session)
            } else {
                base
            };
            if linked {
                symlink(foreign.path(), &selected).unwrap();
            } else {
                fs::create_dir(&selected).unwrap();
                fs::set_permissions(&selected, fs::Permissions::from_mode(0o755)).unwrap();
            }
            assert!(seed(root(d.path()), &scope, until, || Ok(())).is_err());
            assert_eq!(fs::read_dir(foreign.path()).unwrap().count(), 0);
            if !linked {
                assert_eq!(
                    fs::metadata(selected).unwrap().permissions().mode() & 0o777,
                    0o755
                );
            }
        }
    }
}

#[test]
fn every_source_recheck_failure_keeps_partial_state_and_returns_no_resource() {
    for cut in 1..=14 {
        let (d, scope, until) = fixture();
        let calls = Cell::new(0);
        let result = seed(root(d.path()), &scope, until, || {
            calls.set(calls.get() + 1);
            if calls.get() == cut {
                Err("fixture original changed".into())
            } else {
                Ok(())
            }
        });
        assert!(result.is_err(), "cut {cut}, reached {}", calls.get());
        assert_eq!(calls.get(), cut);
        if path(&d, &scope).exists() {
            assert!(seed(root(d.path()), &scope, until, || Ok(())).is_err());
        }
    }
}

#[test]
fn changed_readback_or_original_expiry_after_publication_keeps_the_original_unknown() {
    for expires in [false, true] {
        let (d, scope, until) = fixture();
        let end = if expires {
            Instant::now() + Duration::from_millis(100)
        } else {
            until
        };
        let p = path(&d, &scope);
        let altered = Cell::new(false);
        let result = seed(root(d.path()), &scope, end, || {
            if p.join("pending.json").exists() && !altered.replace(true) {
                if expires {
                    std::thread::sleep(end.saturating_duration_since(Instant::now()));
                } else {
                    fs::write(p.join("pending.json"), b"{}").unwrap();
                }
            }
            Ok(())
        });
        assert!(altered.get());
        assert!(result.is_err());
        assert!(p.join("pending.json").exists());
        assert!(seed(root(d.path()), &scope, until, || Ok(())).is_err());
    }
}

#[test]
fn actual_namespace_sync_failure_never_returns_a_domain_or_removes_partial_publication() {
    let (d, scope, until) = fixture();
    parents(&d, &scope);
    let parent = path(&d, &scope).parent().unwrap().to_path_buf();
    crate::state::repositories::directory::fail_publication_sync(Some(parent));
    let result = seed(root(d.path()), &scope, until, || Ok(()));
    crate::state::repositories::directory::fail_publication_sync(None);
    assert!(result.is_err());
    assert!(
        path(&d, &scope).is_dir(),
        "published directory is retained after uncertain durability"
    );
    assert!(seed(root(d.path()), &scope, until, || Ok(())).is_err());
}

#[test]
fn retained_domain_refuses_replaced_immutable_files_and_unreadable_or_oversize_ledger() {
    for kind in ["gate", "lock", "oversize", "fifo", "corrupt", "run"] {
        let (d, scope, until) = fixture();
        let domain = seed(root(d.path()), &scope, until, || Ok(())).unwrap();
        let p = path(&d, &scope);
        if kind == "run" {
            fs::rename(&p, p.with_extension("old")).unwrap();
            platform::private_dir(&p).unwrap();
        } else {
            let leaf = if matches!(kind, "gate" | "lock") {
                kind
            } else {
                "pending.json"
            };
            let bytes = fs::read(p.join(leaf)).unwrap();
            fs::remove_file(p.join(leaf)).unwrap();
            if kind == "fifo" {
                use std::os::unix::ffi::OsStrExt;
                let name = std::ffi::CString::new(p.join(leaf).as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            } else {
                let replacement = match kind {
                    "oversize" => vec![b' '; BYTES + 1],
                    "corrupt" => b"{}".to_vec(),
                    _ => bytes,
                };
                fs::write(p.join(leaf), replacement).unwrap();
                fs::set_permissions(p.join(leaf), fs::Permissions::from_mode(0o600)).unwrap();
            }
        }
        assert!(domain.current(&scope).is_err(), "{kind}");
    }
}

#[test]
fn an_expired_original_budget_cannot_create_even_the_base_directory() {
    let (d, scope, _) = fixture();
    assert!(seed(root(d.path()), &scope, Instant::now(), || Ok(())).is_err());
    assert_eq!(fs::read_dir(d.path()).unwrap().count(), 0);
}
