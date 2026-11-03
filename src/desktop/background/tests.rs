use super::*;
use crate::desktop::{notifications::preparation, ownership};
use std::sync::mpsc;

#[test]
fn slow_replacement_keeps_draining_on_actor_turns_without_a_client_retry() {
    let _serial = preparation::TEST_SERIAL
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let config = Config {
        home: root.to_str().unwrap().into(),
        dir: root.join("config"),
        state: root.join("state"),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    std::fs::create_dir_all(&config.dir).unwrap();
    let location = Location::new(&config).unwrap();
    location.prepare().unwrap();
    let epoch = uuid::Uuid::new_v4().to_string();
    let mut store = Store::open(&location, &epoch).unwrap();
    let image = temp.path().join("hgs-tray");
    std::fs::write(&image, b"owned fixture image").unwrap();
    let origin = ownership::Origin::capture(
        &image,
        zerus_client_core::desktop::Build {
            version: "1.2.3".into(),
            source_commit: "a".repeat(40),
            source_dirty: Some(false),
        },
    )
    .unwrap();
    store
        .db
        .execute(
            "UPDATE desktop_ownership SET state='service',owner_epoch=?1,origin=?2 WHERE id=1",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                serde_json::to_string(&origin).unwrap()
            ],
        )
        .unwrap();
    let owner = Arc::new(location.lock().unwrap());
    let mut broker = feed::Broker::new(&epoch);
    let mut runtime = Runtime {
        fleet: fleet::Runtime::empty_for_test(&epoch),
        producer: broker.claim_internal().unwrap(),
        presence: None,
        delivery: delivery::Runtime::start(&config, &epoch).unwrap(),
        details: details::Adapter::new(&epoch),
        maintenance: maintenance::Runtime::start(&config, &epoch).unwrap(),
        cache: crate::desktop::cache_maintenance::Runtime::new(&location, &epoch, owner.clone()),
        attention: attention::Runtime::new(&epoch),
        activations: activation::Router::new(&location.configuration, &epoch),
        drained: false,
        stopping: false,
        notifications: notification_service::Runtime::new(
            &config,
            &location,
            &epoch,
            owner.clone(),
        ),
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        creation: crate::desktop::creation_clone::Runtime::new(&config, &epoch, owner.clone())
            .unwrap(),
    };
    let (release, resume) = mpsc::sync_channel(1);
    runtime.notifications.block_selection_for_test(resume);
    assert!(runtime.drain(&mut store, Duration::ZERO).is_err());
    assert!(!runtime.drained());
    assert!(location.handoff_lock().is_err());
    assert!(!replacement_permitted(true, runtime.drained(), &location, &store).unwrap());
    // No frontend call/retry follows the timeout. Only normal actor polling.
    release.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    let mut seq = 0;
    while !runtime.drained() {
        assert!(Instant::now() < until, "original selection did not join");
        runtime.poll(&mut store, &mut broker, &mut seq);
        thread::sleep(Duration::from_millis(2));
    }
    assert!(location.handoff_lock().is_ok());
    assert!(replacement_permitted(true, runtime.drained(), &location, &store).unwrap());
    assert!(
        !replacement_permitted(false, runtime.drained(), &location, &store).unwrap(),
        "passive serve cannot start background"
    );
    // Routine update changes only launch eligibility, not the retained grant.
    std::fs::write(&image, b"updated owned fixture image").unwrap();
    assert!(!ownership::service_granted(&store, &location.configuration).unwrap());
    assert!(replacement_permitted(true, true, &location, &store).unwrap());
    ownership::invalidate(&store.db).unwrap();
    assert!(
        !replacement_permitted(true, true, &location, &store).unwrap(),
        "revoke cannot be rearmed by a drained handle"
    );
    drop(runtime);
    drop(owner);
    assert!(location.lock().is_ok());
}
