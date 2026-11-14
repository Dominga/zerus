use super::*;
use crate::teams::ops::{self, tests::*, Operation};
use zerus_team_plan::model::TeamStatus;

#[test]
fn another_connections_commit_wakes_the_worker_before_the_safety_interval() {
    let mut h = Harness::new();
    for member in ["lead", "m1", "m2"] {
        h.bind(member);
    }
    let config = Config {
        home: String::new(),
        dir: h.temp.path().join("config"),
        state: h.temp.path().to_owned(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    let machine = h.store.team(TEAM).unwrap().unwrap().home;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let (wake, receive) = mpsc::sync_channel(1);
    let (sent, observed) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut passes = Passes {
            launcher: launch::Runtime::open(&config.state, &machine, "fixture-worker").unwrap(),
            store: Store::open(&config.state).unwrap(),
            native: Native {
                config: config.clone(),
                machine: machine.clone(),
                stop: stopped.clone(),
            },
            actor: actor::Runtime::open(&config.state, &machine).unwrap(),
            effects: actor::native::Native {
                config: config.clone(),
                machine,
                stop: stopped.clone(),
                epoch: "fixture-worker".into(),
            },
            confirmations: Default::default(),
            after_cycle: Some(Box::new(move |store| {
                let _ = sent.send(store.team(TEAM).unwrap().unwrap().status);
            })),
        };
        passes.run(&config, &stopped, receive);
    });
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            TeamStatus::Running
        );
        let t = h.store.team(TEAM).unwrap().unwrap();
        ops::apply(
            &mut h.store,
            &person(),
            &request(t.revision, Operation::Pause),
            1234,
        )
        .unwrap();
        // No fleet wake is sent: only data_version can cause this pass.
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(3)).unwrap(),
            TeamStatus::Paused
        );
    }));
    stop.store(true, Ordering::Release);
    let _ = wake.try_send(());
    worker.join().unwrap();
    outcome.unwrap();
}
