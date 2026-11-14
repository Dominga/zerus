use super::*;
use std::sync::mpsc;
use zerus_client_core::{
    actions::{Exit, Invocation},
    run,
    terminal::{Attach, AttachEvent, Target},
};

fn root_turn_excludes(action: impl FnOnce() + Send + 'static) {
    let turn = crate::platform::spawn_turn();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        action();
        done_tx.send(()).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let escaped = done_rx.recv_timeout(Duration::from_millis(200)).is_ok();
    drop(turn);
    if !escaped {
        done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }
    worker.join().unwrap();
    assert!(
        !escaped,
        "the client spawned while the root process held its spawn turn"
    );
}
#[test]
fn the_root_turn_also_excludes_the_core_helper() {
    root_turn_excludes(|| {
        let invocation = Invocation {
            args: vec![],
            stdin: None,
            timeout: Duration::from_secs(5),
            request_id: None,
        };
        assert!(matches!(
            run::run(Path::new("/usr/bin/true"), &invocation),
            Exit::Code { code: 0, .. }
        ));
    });
}
#[test]
fn the_root_turn_also_excludes_the_core_pty() {
    root_turn_excludes(|| {
        let target = Target {
            hgs: "/usr/bin/true".into(),
            host: String::new(),
            name: "spawn-fixture".into(),
            run_id: String::new(),
        };
        let (attach, events) = Attach::start(&target, 80, 24).unwrap();
        loop {
            if let AttachEvent::Finished { code } =
                events.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert_eq!(code, 0);
                break;
            }
        }
        drop(attach);
    });
}

#[test]
fn the_root_turn_also_excludes_core_endpoint_preparation() {
    root_turn_excludes(|| {
        let pair = zerus_client_core::unix_spawn::socket_pair().unwrap();
        drop(pair);
    });
}
