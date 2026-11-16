use super::*;

#[test]
fn actual_native_events_are_bounded_hints_and_never_file_watches() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("bindings")).unwrap();
    let mut feed = Changes::new(root.path()).unwrap().unwrap();
    let count = feed.watches.len();
    for n in 0..32 {
        fs::write(
            root.path()
                .join("bindings")
                .join(format!("{:064x}.json", n)),
            b"not read by a hint",
        )
        .unwrap();
    }
    let mut keys = BTreeSet::new();
    for _ in 0..128 {
        let batch = feed.drain(3).unwrap();
        assert!(batch.entries <= 3 && !batch.restart);
        keys.extend(batch.keys.into_iter().map(|key| key.0));
        if batch.caught_up {
            break;
        }
    }
    assert_eq!(keys.len(), 32);
    assert_eq!(feed.watches.len(), count);
    assert!(count <= 16);
    let info = fs::read_to_string(format!("/proc/self/fdinfo/{}", feed.fd.as_raw_fd())).unwrap();
    assert_eq!(
        info.lines()
            .filter(|line| line.starts_with("inotify wd:"))
            .count(),
        count
    );
    assert!(feed.drain(3).unwrap().caught_up);
}

#[test]
fn a_native_watch_loss_or_new_namespace_requires_a_new_census() {
    for lost in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut feed = Changes::new(root.path()).unwrap().unwrap();
        if lost {
            let wd = *feed.watches.keys().next().unwrap();
            assert_eq!(
                unsafe { libc::inotify_rm_watch(feed.fd.as_raw_fd(), wd) },
                0
            );
        } else {
            fs::create_dir(root.path().join("sessions")).unwrap();
        }
        let batch = feed.drain(256).unwrap();
        assert!(
            batch.restart && !batch.caught_up,
            "lost native watch claimed caught-up"
        );
    }
}

#[test]
fn native_quota_failure_closes_the_partially_installed_feed() {
    if std::env::var("ZERUS_LISTING_FD_CHILD").as_deref() != Ok("1") {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "state::listing_metadata::changes::linux::tests::native_quota_failure_closes_the_partially_installed_feed", "--test-threads=1", "--nocapture"])
            .env_clear().env("PATH",std::env::var_os("PATH").unwrap_or_default())
            .env("ZERUS_LISTING_FD_CHILD","1").output().unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        return;
    }
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) },
        0
    );
    limit.rlim_cur = limit.rlim_cur.min(64);
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("bindings")).unwrap();
    let mut fd = -1;
    let mut calls = 0;
    let outcome = Changes::install(root.path(), |native, path, mask| {
        fd = native;
        calls += 1;
        if calls == 2 {
            return Err(io::Error::from_raw_os_error(libc::ENOSPC));
        }
        let wd = unsafe { libc::inotify_add_watch(native, path.as_ptr(), mask) };
        if wd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(wd)
        }
    });
    assert!(
        outcome
            .err()
            .unwrap()
            .to_string()
            .contains("watch limit is exhausted")
    );
    assert_eq!(calls, 2);
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_GETFD) },
        -1,
        "quota leaked the partially installed native descriptor"
    );
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::EBADF));
    for _ in 0..256 {
        drop(Changes::new(root.path()).unwrap().unwrap());
    }
}

#[test]
fn every_serialized_native_loss_flag_taints_closure() {
    let root = tempfile::tempdir().unwrap();
    for mask in [
        libc::IN_Q_OVERFLOW,
        libc::IN_IGNORED,
        libc::IN_DELETE_SELF,
        libc::IN_MOVE_SELF,
        libc::IN_UNMOUNT,
    ] {
        let mut feed = Changes::new(root.path()).unwrap().unwrap();
        let event = libc::inotify_event {
            wd: *feed.watches.keys().next().unwrap(),
            mask,
            cookie: 0,
            len: 0,
        };
        unsafe {
            std::ptr::write_unaligned(
                feed.buffer.as_mut_ptr().cast::<libc::inotify_event>(),
                event,
            );
        }
        feed.offset = 0;
        feed.length = std::mem::size_of::<libc::inotify_event>();
        let batch = feed.drain(256).unwrap();
        assert!(
            batch.restart && !batch.caught_up,
            "native loss {mask} claimed a closed cut"
        );
    }
}
