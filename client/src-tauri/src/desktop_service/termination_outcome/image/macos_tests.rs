//! Real Mac original-child refusal/custody controls. These never fabricate a
//! genuine pair or a positive Selection and do not qualify installed Terminate.
use super::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};
use zerus_client_core::{desktop::drain, run, Invocation};

#[test]
fn native_mac_unpaired_and_path_only_routes_remain_unavailable() {
    let path = Path::new("/bin/zsh");
    let connection = Arc::new(relay::Connection::select(path).unwrap());
    assert!(connection.original_helper_executable().is_err());
    assert_eq!(
        Image::selected(connection).err().unwrap().kind(),
        io::ErrorKind::Unsupported
    );
    assert_eq!(
        Image::capture(path).err().unwrap().kind(),
        io::ErrorKind::Unsupported
    );
}

#[test]
fn native_mac_current_routing_cannot_replace_original_observation_or_child_custody() {
    // Production selected() refuses this ordinary route BEFORE spawning. The
    // private test object reaches the actual Mac observation's negative branch;
    // no mocked positive or reusable constructor is exposed by the library.
    let path = PathBuf::from("/bin/zsh");
    let image = Arc::new(Image {
        requested: path.clone(),
        routing: Arc::new(relay::Connection::select(&path).unwrap()),
    });
    let gate = drain::Gate::new(1).unwrap();
    let charge = gate.admit(drain::Role::Cleanup).unwrap();
    let hold: Arc<dyn Send + Sync> = Arc::new((image.clone(), charge));
    let weak = Arc::downgrade(&hold);
    let pid = Arc::new(AtomicU32::new(0));
    let closed = Arc::new(AtomicBool::new(false));
    let retained = Arc::new(AtomicBool::new(false));
    let observed_pid = pid.clone();
    let observed_closed = closed.clone();
    let observed_retained = retained.clone();
    let observed_gate = gate.clone();
    let checked = image.clone();
    let invocation = Invocation {
        args: vec![
            "-f".into(),
            "-c".into(),
            "cat; printf claimed-success".into(),
        ],
        stdin: Some(b"private-fixture-frame\n".to_vec()),
        timeout: Duration::from_secs(3),
        request_id: None,
    };
    let result = run::run_owned(
        &path,
        &invocation,
        &[],
        &AtomicBool::new(false),
        move |command| {
            let mut child = command.spawn()?;
            observed_pid.store(child.id(), Ordering::Release);
            let original_live = child.try_wait().is_ok_and(|s| s.is_none());
            // Nominal routing freshness is not a native selection/permission.
            // Exercise the production observation boundary independently: if
            // that actual call is removed, this hostile child receives input.
            if observe_mac_child(&checked.routing, &mut child, || true).is_err() {
                drop(child.stdin.take());
            }
            observed_closed.store(child.stdin.is_none(), Ordering::Release);
            observed_retained.store(
                child.stdout.is_some()
                    && child.stderr.is_some()
                    && original_live
                    && observed_gate
                        .snapshot()
                        .is_ok_and(|s| s.count(drain::Role::Cleanup) == 1),
                Ordering::Release,
            );
            // No assertions/panic/Err can discard a spawned child's custody.
            Ok(child)
        },
        hold,
    );
    assert!(
        result.is_err(),
        "a hostile success reply cannot admit a refused input exchange"
    );
    assert!(closed.load(Ordering::Acquire), "private frame was withheld");
    assert!(
        retained.load(Ordering::Acquire),
        "same live child and both output pipes remain owned"
    );
    let until = Instant::now() + Duration::from_secs(2);
    while weak.upgrade().is_some() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        weak.upgrade().is_none(),
        "actual child/writer/readers settle before releasing charge"
    );
    assert_eq!(gate.snapshot().unwrap().admitted, 0);
    let pid = pid.load(Ordering::Acquire);
    assert!(pid > 0);
    let mut status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
}
