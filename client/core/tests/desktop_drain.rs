//! Real thread/queue and optional owned-pipe lifetimes. No frontend or service
//! integration, role-coverage attestation, effect, store or runtime authority.
use std::{
    sync::{mpsc, Arc, Barrier},
    thread,
    time::{Duration, Instant},
};
use zerus_client_core::desktop::drain::{Gate, Observation, Refused, Role, MAX_ADMISSIONS};

#[test]
fn pressure_preserves_existing_work_and_role_counts() {
    assert!(matches!(Gate::new(0), Err(Refused::InvalidLimit)));
    assert!(matches!(
        Gate::new(MAX_ADMISSIONS + 1),
        Err(Refused::InvalidLimit)
    ));
    let gate = Gate::new(Role::ALL.len()).unwrap();
    let mut charges = Role::ALL.map(|role| Some(gate.admit(role).unwrap()));
    assert!(matches!(gate.admit(Role::Fleet), Err(Refused::Capacity)));
    let snapshot = gate.snapshot().unwrap();
    assert!(!snapshot.closed);
    assert_eq!(snapshot.admitted, Role::ALL.len());
    for role in Role::ALL {
        assert_eq!(snapshot.count(role), 1);
    }
    drop(charges[0].take());
    assert_eq!(gate.snapshot().unwrap().count(Role::Fleet), 0);
    let next = gate.admit(Role::Downloads).unwrap();
    assert_eq!(next.role(), Role::Downloads);
    let closed = gate.close();
    assert!(matches!(gate.admit(Role::Fleet), Err(Refused::Closed)));
    drop(next);
    drop(charges);
    assert_eq!(closed.wait_until(Instant::now()), Observation::Idle);
}

#[test]
fn queued_callback_outlives_its_caller_and_timeout_never_reopens() {
    let gate = Gate::new(1).unwrap();
    let charge = gate.admit(Role::Cleanup).unwrap();
    let callback = charge.clone();
    let (queued, ready) = mpsc::channel();
    let (done, end) = mpsc::channel();
    let worker = thread::spawn(move || {
        queued.send(()).unwrap();
        end.recv().unwrap();
        drop(callback);
    });
    ready.recv().unwrap();
    drop(charge);
    let closed = gate.close();
    assert_eq!(closed.snapshot().unwrap().count(Role::Cleanup), 1);
    assert_eq!(closed.wait_until(Instant::now()), Observation::Pending);
    assert_eq!(
        closed.wait_until(Instant::now() + Duration::from_millis(10)),
        Observation::Pending
    );
    drop(closed);
    assert!(matches!(gate.admit(Role::Cleanup), Err(Refused::Closed)));
    done.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(gate.close().wait_until(Instant::now()), Observation::Idle);
    assert!(matches!(gate.admit(Role::Cleanup), Err(Refused::Closed)));
}

#[test]
fn concurrent_close_and_admission_have_one_serial_order() {
    for _ in 0..64 {
        let gate = Gate::new(1).unwrap();
        let rendezvous = Arc::new(Barrier::new(2));
        let (send, received) = mpsc::channel();
        let other = gate.clone();
        let start = rendezvous.clone();
        let worker = thread::spawn(move || {
            start.wait();
            send.send(other.admit(Role::ReadDelivery)).unwrap();
        });
        rendezvous.wait();
        let closed = gate.close();
        worker.join().unwrap();
        let admitted = received.recv().unwrap();
        match &admitted {
            Ok(_) => {
                assert_eq!(closed.snapshot().unwrap().admitted, 1);
                assert_eq!(closed.wait_until(Instant::now()), Observation::Pending);
            }
            Err(Refused::Closed) => {
                assert_eq!(closed.snapshot().unwrap().admitted, 0);
                assert_eq!(closed.wait_until(Instant::now()), Observation::Idle);
            }
            _ => panic!("unexpected admission result"),
        }
        assert!(matches!(
            gate.admit(Role::ReadDelivery),
            Err(Refused::Closed)
        ));
        drop(admitted);
        assert_eq!(closed.wait_until(Instant::now()), Observation::Idle);
    }
}

#[test]
fn all_waiters_observe_last_clone_completion() {
    let gate = Gate::new(1).unwrap();
    let charge = gate.admit(Role::Operations).unwrap();
    let last = charge.clone();
    let closed = gate.close();
    let waiters: Vec<_> = (0..4)
        .map(|_| {
            let closed = closed.clone();
            thread::spawn(move || closed.wait_until(Instant::now() + Duration::from_secs(2)))
        })
        .collect();
    drop(charge);
    assert_eq!(closed.wait_until(Instant::now()), Observation::Pending);
    drop(last);
    for waiter in waiters {
        assert_eq!(waiter.join().unwrap(), Observation::Idle);
    }
}

#[cfg(unix)]
#[test]
fn owned_input_pipe_keeps_gate_charged_after_bounded_runner_return() {
    use std::{path::Path, sync::atomic::AtomicBool};
    use zerus_client_core::{
        run::{run_owned, OwnedError},
        Invocation,
    };

    let gate = Gate::new(1).unwrap();
    let charge = Arc::new(gate.admit(Role::ReadDelivery).unwrap());
    let inv = Invocation {
        args: vec!["-c".into(), "import subprocess,sys;subprocess.Popen([sys.executable,'-c','import time;time.sleep(1.5)'],stdin=sys.stdin,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);print('{}',flush=True)".into()],
        stdin: Some(vec![b'x'; 1 << 20]),
        timeout: Duration::from_millis(200),
        request_id: None,
    };
    // Already-admitted work may finish after close. The host fences any new
    // spawn separately; closing this accounting gate does not kill a child.
    let closed = gate.close();
    let result = run_owned(
        Path::new("python3"),
        &inv,
        &[],
        &AtomicBool::new(false),
        |command| command.spawn(),
        charge,
    );
    assert_eq!(result, Err(OwnedError::TimedOut));
    assert_eq!(closed.wait_until(Instant::now()), Observation::Pending);
    assert_eq!(
        closed.wait_until(Instant::now() + Duration::from_secs(4)),
        Observation::Idle
    );
    assert!(matches!(
        gate.admit(Role::ReadDelivery),
        Err(Refused::Closed)
    ));
}
