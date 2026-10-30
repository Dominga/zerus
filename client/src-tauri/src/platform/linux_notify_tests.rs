use super::*;
use crate::platform::linux_test_bus::Bus;
use std::sync::atomic::{AtomicU32, AtomicU8};
use std::sync::Condvar;
use std::time::Instant;
use zbus::zvariant::{DynamicType, OwnedValue};

#[test]
fn a_linked_identity_file_is_preserved_without_reading_or_reseeding_it() {
    let dir = Directory::new();
    let target = dir.0.join("other-identities.json");
    let original = br#"{"version":1,"owner":":1.2","tokens":{"42":"foreign"}}"#;
    std::fs::write(&target, original).unwrap();
    let path = dir.0.join(FILE);
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(load(&path, Box::new(|| panic!("existing paths never reseed"))).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), original);
    assert!(std::fs::symlink_metadata(path)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[derive(Debug)]
struct Call {
    id: u32,
    app: String,
    replaces: u32,
    icon: String,
    title: String,
    body: String,
    actions: Vec<String>,
    hints: BTreeMap<String, OwnedValue>,
    timeout: i32,
}
struct Control {
    next: AtomicU32,
    failure: AtomicU8,
    held: Mutex<bool>,
    release: Condvar,
    calls: mpsc::Sender<Call>,
    closed: mpsc::Sender<u32>,
}
struct Fake(Arc<Control>);
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Fake {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app: String,
        replaces: u32,
        icon: String,
        title: String,
        body: String,
        actions: Vec<String>,
        hints: BTreeMap<String, OwnedValue>,
        timeout: i32,
    ) -> zbus::fdo::Result<u32> {
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        let _ = self.0.calls.send(Call {
            id,
            app,
            replaces,
            icon,
            title,
            body,
            actions,
            hints,
            timeout,
        });
        let mut held = lock(&self.0.held);
        while *held {
            let result = self
                .0
                .release
                .wait_timeout(held, Duration::from_secs(5))
                .unwrap();
            held = result.0;
            if result.1.timed_out() {
                return Err(zbus::fdo::Error::Failed(
                    "Fixture release was not received".into(),
                ));
            }
        }
        match self.0.failure.swap(0, Ordering::Relaxed) {
            1 => Err(zbus::fdo::Error::Failed("Test delivery failure".into())),
            2 => Err(zbus::fdo::Error::NoReply(
                "Delivery outcome is unknown".into(),
            )),
            _ => Ok(id),
        }
    }
    fn close_notification(&self, id: u32) {
        let _ = self.0.closed.send(id);
    }
}
struct Server {
    connection: Connection,
    control: Arc<Control>,
}
impl Server {
    fn new(bus: &Bus) -> (Self, mpsc::Receiver<Call>, mpsc::Receiver<u32>) {
        let (calls, called) = mpsc::channel();
        let (closed, closes) = mpsc::channel();
        let control = Arc::new(Control {
            next: AtomicU32::new(100),
            failure: AtomicU8::new(0),
            held: Mutex::new(false),
            release: Condvar::new(),
            calls,
            closed,
        });
        let connection = zbus::blocking::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(SERVICE)
            .unwrap()
            .serve_at(PATH, Fake(control.clone()))
            .unwrap()
            .build()
            .unwrap();
        (
            Self {
                connection,
                control,
            },
            called,
            closes,
        )
    }
    fn emit<T: serde::Serialize + DynamicType>(&self, signal: &str, body: &T) {
        self.connection
            .emit_signal(None::<&str>, PATH, SERVICE, signal, body)
            .unwrap();
    }
    fn release(&self) {
        *lock(&self.control.held) = false;
        self.control.release.notify_all();
    }
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(format!("zln-{}", uuid::Uuid::new_v4()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    poster: Poster,
    server: Server,
    bus: Bus,
    dir: Directory,
    calls: mpsc::Receiver<Call>,
    closed: mpsc::Receiver<u32>,
    events: mpsc::Receiver<Event>,
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(Instant::now() < end, "fixture condition timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn notice(token: &str) -> Notice {
    Notice {
        token: token.into(),
        title: "Zerus — Needs attention".into(),
        body: "<b>Literal & session</b> \"quoted\"".into(),
    }
}
fn poster(path: PathBuf, bus: &Bus, qt: QtSeed) -> (Poster, mpsc::Receiver<Event>) {
    let (tx, rx) = mpsc::channel();
    (
        Poster::spawn(
            path,
            qt,
            Some(bus.address.clone()),
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
        ),
        rx,
    )
}
impl Fixture {
    fn new() -> Self {
        let dir = Directory::new();
        let bus = Bus::new();
        let (server, calls, closed) = Server::new(&bus);
        let (poster, events) = poster(dir.0.join(FILE), &bus, Box::new(|| Ok(None)));
        wait(|| lock(&poster.shared.state).ready);
        Self {
            poster,
            server,
            bus,
            dir,
            calls,
            closed,
            events,
        }
    }
    fn post(&self, token: &str) -> Call {
        self.poster.deliver(Vec::new(), vec![notice(token)]);
        let call = self.calls.recv_timeout(Duration::from_secs(5)).unwrap();
        wait(|| {
            lock(&self.poster.shared.state)
                .saved
                .tokens
                .get(&call.id)
                .is_some_and(|shown| shown == token)
        });
        call
    }
    fn event(&self) -> Event {
        self.events.recv_timeout(Duration::from_secs(5)).unwrap()
    }
    fn restart(&mut self, qt: QtSeed) {
        self.poster.stop();
        wait(|| self.poster.shared.done.load(Ordering::Acquire));
        let (next, events) = poster(self.dir.0.join(FILE), &self.bus, qt);
        self.poster = next;
        self.events = events;
        wait(|| lock(&self.poster.shared.state).ready);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.release();
        self.poster.stop();
        let end = Instant::now() + Duration::from_secs(5);
        while !self.poster.shared.done.load(Ordering::Acquire) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn exact_qt_payload_tokens_restart_expiry_and_withdrawal_use_the_private_bus() {
    let mut f = Fixture::new();
    let first = f.post("first");
    let second = f.post("second");
    assert_eq!(
        (
            first.app,
            first.replaces,
            first.icon,
            first.title,
            first.timeout
        ),
        (
            "Zerus".into(),
            0,
            "hgs-zerus-swarm-symbolic".into(),
            "Zerus — Needs attention".into(),
            -1
        )
    );
    assert_eq!(
        first.body,
        "&lt;b&gt;Literal &amp; session&lt;/b&gt; &quot;quoted&quot;"
    );
    assert_eq!(
        first.actions,
        ["default", "Open session", "open-session", "Open session"]
    );
    assert_eq!(
        <&str>::try_from(&first.hints["desktop-entry"]).unwrap(),
        "hgs-tray"
    );
    assert_eq!(u8::try_from(&first.hints["urgency"]).unwrap(), 1);
    assert_eq!(
        <&str>::try_from(&first.hints["category"]).unwrap(),
        "im.received"
    );
    f.server
        .emit("ActivationToken", &(second.id, "wayland-click-token"));
    f.server.emit("ActionInvoked", &(second.id, "open-session"));
    assert_eq!(
        f.event(),
        Event::Activated {
            token: "second".into(),
            activation: "wayland-click-token".into()
        }
    );
    f.server.emit("ActionInvoked", &(999_u32, "default"));
    f.server.emit("ActionInvoked", &(first.id, "unknown"));
    f.server.emit("ActionInvoked", &(first.id, "default"));
    assert_eq!(
        f.event(),
        Event::Activated {
            token: "first".into(),
            activation: String::new()
        }
    );
    f.restart(Box::new(|| panic!("the client's file wins over Qt")));
    f.server.emit("NotificationClosed", &(first.id, 1_u32));
    f.server.emit("ActionInvoked", &(first.id, "default"));
    assert_eq!(
        f.event(),
        Event::Activated {
            token: "first".into(),
            activation: String::new()
        }
    );
    f.poster.deliver(vec!["first".into()], Vec::new());
    assert_eq!(
        f.closed.recv_timeout(Duration::from_secs(5)).unwrap(),
        first.id
    );
    f.server.emit("ActionInvoked", &(first.id, "default"));
    f.server.emit("ActionInvoked", &(second.id, "default"));
    assert_eq!(
        f.event(),
        Event::Activated {
            token: "second".into(),
            activation: String::new()
        }
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(f.dir.0.join(FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn withdraw_while_notify_waits_closes_the_returned_id_and_cannot_activate_it() {
    let f = Fixture::new();
    *lock(&f.server.control.held) = true;
    f.poster
        .deliver(Vec::new(), vec![notice("cancelled-before-reply")]);
    let call = f.calls.recv_timeout(Duration::from_secs(5)).unwrap();
    let saved: Saved = serde_json::from_slice(&std::fs::read(f.dir.0.join(FILE)).unwrap()).unwrap();
    assert!(
        saved.uncertain.contains("cancelled-before-reply"),
        "reserve before native delivery"
    );
    f.poster
        .deliver(vec!["cancelled-before-reply".into()], Vec::new());
    f.server.release();
    assert_eq!(
        f.closed.recv_timeout(Duration::from_secs(5)).unwrap(),
        call.id
    );
    assert!(lock(&f.poster.shared.state).saved.tokens.is_empty());
    let current = f.post("current");
    f.server.emit("ActionInvoked", &(call.id, "default"));
    f.server.emit("ActionInvoked", &(current.id, "default"));
    assert_eq!(
        f.event(),
        Event::Activated {
            token: "current".into(),
            activation: String::new()
        }
    );
}

#[test]
fn known_rejection_reports_qts_reason_and_permits_the_existing_retry() {
    let f = Fixture::new();
    f.server.control.failure.store(1, Ordering::Relaxed);
    f.poster
        .deliver(Vec::new(), vec![notice("failed-delivery")]);
    let _ = f.calls.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(f.event(),Event::Failed { token,detail } if token=="failed-delivery" && detail=="Test delivery failure")
    );
    let accepted = f.post("failed-delivery");
    assert_eq!(accepted.id, 101);
}

#[test]
fn unknown_delivery_stays_reserved_across_restart_without_native_replay() {
    let mut f = Fixture::new();
    f.server.control.failure.store(2, Ordering::Relaxed);
    f.poster.deliver(Vec::new(), vec![notice("unknown")]);
    let _ = f.calls.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(f.event(),Event::Uncertain(detail) if detail.contains("unconfirmed")));
    f.restart(Box::new(|| panic!("never reseed an existing file")));
    f.poster
        .deliver(Vec::new(), vec![notice("unknown"), notice("after-restart")]);
    let actual = f.calls.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(actual.id, 101, "only the new token posts");
    wait(|| {
        lock(&f.poster.shared.state)
            .saved
            .tokens
            .get(&actual.id)
            .is_some_and(|token| token == "after-restart")
    });
    assert!(lock(&f.poster.shared.state)
        .saved
        .uncertain
        .contains("unknown"));
}

#[test]
fn owner_only_qt_and_old_next_ids_suppress_but_cannot_activate_or_close() {
    for next_file in [false, true] {
        let dir = Directory::new();
        let bus = Bus::new();
        let (server, calls, closed) = Server::new(&bus);
        let saved = serde_json::json!({
            "version": 1, "owner": server.connection.unique_name().unwrap().as_str(),
            "tokens": {"42":"legacy-token"}, "uncertain": ["older-unknown"]
        });
        let path = dir.0.join(FILE);
        if next_file {
            std::fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        }
        let (poster, events) = poster(
            path,
            &bus,
            Box::new(move || {
                assert!(!next_file, "an existing Next file must never reseed");
                Ok(Some(serde_json::from_value(saved).unwrap()))
            }),
        );
        wait(|| lock(&poster.shared.state).ready);
        assert!(lock(&poster.shared.state).saved.tokens.is_empty());
        assert!(lock(&poster.shared.state)
            .saved
            .uncertain
            .contains("legacy-token"));
        server.emit("ActivationToken", &(42_u32, "legacy-activation"));
        server.emit("ActionInvoked", &(42_u32, "default"));
        poster.deliver(
            Vec::new(),
            vec![
                notice("legacy-token"),
                notice("older-unknown"),
                notice("new-token"),
            ],
        );
        let call = calls.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(call.id, 100, "only the new edge posts");
        wait(|| {
            lock(&poster.shared.state)
                .saved
                .tokens
                .contains_key(&call.id)
        });
        server.emit("ActionInvoked", &(call.id, "default"));
        assert_eq!(
            events.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Activated {
                token: "new-token".into(),
                activation: String::new()
            }
        );
        poster.deliver(
            vec!["legacy-token".into(), "older-unknown".into()],
            vec![notice("fence")],
        );
        let _ = calls.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            closed.try_recv().is_err(),
            "unproved legacy IDs must never close"
        );
        assert!(!lock(&poster.shared.state)
            .saved
            .uncertain
            .contains("legacy-token"));
        poster.stop();
        wait(|| poster.shared.done.load(Ordering::Acquire));
    }
}

#[test]
fn another_bus_cannot_reuse_retained_ids_even_with_the_same_unique_owner() {
    let f = Fixture::new();
    let old = f.post("old-bus-token");
    f.server.control.failure.store(2, Ordering::Relaxed);
    f.poster
        .deliver(Vec::new(), vec![notice("old-bus-unknown")]);
    let _ = f.calls.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(f.event(), Event::Uncertain(_)));
    let old_namespace = Namespace::saved(&lock(&f.poster.shared.state).saved);
    f.poster.stop();
    wait(|| f.poster.shared.done.load(Ordering::Acquire));
    let bus = Bus::new();
    let (server, calls, closed) = Server::new(&bus);
    let current = current_namespace(&server.connection).unwrap();
    assert_eq!(old_namespace.owner, current.owner);
    assert_ne!(old_namespace.bus_id, current.bus_id);
    let (poster, events) = poster(
        f.dir.0.join(FILE),
        &bus,
        Box::new(|| panic!("existing file")),
    );
    wait(|| lock(&poster.shared.state).ready);
    assert!(lock(&poster.shared.state)
        .saved
        .uncertain
        .contains("old-bus-token"));
    assert!(lock(&poster.shared.state)
        .saved
        .uncertain
        .contains("old-bus-unknown"));
    poster.deliver(
        Vec::new(),
        vec![
            notice("old-bus-token"),
            notice("old-bus-unknown"),
            notice("new-bus-token"),
        ],
    );
    let new = calls.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(old.id, new.id);
    wait(|| {
        lock(&poster.shared.state)
            .saved
            .tokens
            .contains_key(&new.id)
    });
    server.emit("ActionInvoked", &(new.id, "default"));
    assert_eq!(
        events.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Activated {
            token: "new-bus-token".into(),
            activation: String::new()
        }
    );
    close(&server.connection, &old_namespace, old.id);
    close(&server.connection, &current, 999);
    assert_eq!(
        closed.recv_timeout(Duration::from_secs(5)).unwrap(),
        999,
        "the old bus ID must never close the replacement banner"
    );
    poster.deliver(vec!["old-bus-token".into()], vec![notice("fence")]);
    let _ = calls.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(closed.try_recv().is_err());
    poster.stop();
    wait(|| poster.shared.done.load(Ordering::Acquire));
}

#[test]
fn a_replacement_daemon_cannot_reuse_the_old_owners_click_ids() {
    let f = Fixture::new();
    let old = f.post("old-token");
    f.server.control.failure.store(2, Ordering::Relaxed);
    f.poster
        .deliver(Vec::new(), vec![notice("unknown-before-owner-change")]);
    let _ = f.calls.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(f.event(), Event::Uncertain(_)));
    f.server.connection.release_name(SERVICE).unwrap();
    let (replacement, calls, _) = Server::new(&f.bus);
    let new_owner = replacement.connection.unique_name().unwrap().to_string();
    wait(|| lock(&f.poster.shared.state).saved.owner == new_owner);
    assert!(lock(&f.poster.shared.state).saved.tokens.is_empty());
    assert!(lock(&f.poster.shared.state)
        .saved
        .uncertain
        .contains("old-token"));
    assert!(lock(&f.poster.shared.state)
        .saved
        .uncertain
        .contains("unknown-before-owner-change"));
    f.poster.deliver(
        Vec::new(),
        vec![
            notice("old-token"),
            notice("unknown-before-owner-change"),
            notice("new-owner-token"),
        ],
    );
    let new = calls.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(old.id, new.id);
    wait(|| {
        lock(&f.poster.shared.state)
            .saved
            .tokens
            .get(&new.id)
            .is_some_and(|token| token == "new-owner-token")
    });
    f.server.emit("ActionInvoked", &(old.id, "default"));
    replacement.emit("ActionInvoked", &(new.id, "default"));
    assert_eq!(
        f.event(),
        Event::Activated {
            token: "new-owner-token".into(),
            activation: String::new()
        }
    );
}

#[test]
fn malformed_client_state_is_preserved_and_never_falls_back_to_qt_or_posts() {
    let dir = Directory::new();
    let bus = Bus::new();
    let (_server, calls, _) = Server::new(&bus);
    let path = dir.0.join(FILE);
    std::fs::write(&path, b"{unfinished").unwrap();
    let (poster, events) = poster(
        path.clone(),
        &bus,
        Box::new(|| panic!("malformed state never reseeds")),
    );
    wait(|| lock(&poster.shared.state).disabled.is_some());
    assert!(matches!(
        events.recv_timeout(Duration::from_secs(5)).unwrap(),
        Event::Uncertain(_)
    ));
    poster.deliver(Vec::new(), vec![notice("blocked")]);
    assert!(
        matches!(events.recv_timeout(Duration::from_secs(5)).unwrap(),Event::Failed { token,.. } if token=="blocked")
    );
    assert!(calls.try_recv().is_err());
    assert_eq!(std::fs::read(path).unwrap(), b"{unfinished");
    poster.stop();
    wait(|| poster.shared.done.load(Ordering::Acquire));
}

#[test]
fn a_notice_queued_before_seed_loading_is_rechecked_before_the_native_call() {
    for unknown in [false, true] {
        let dir = Directory::new();
        let bus = Bus::new();
        let (server, calls, _) = Server::new(&bus);
        let namespace = current_namespace(&server.connection).unwrap();
        let (wake, _) = mpsc::sync_channel(1);
        let mut saved = Saved {
            version: 1,
            bus_id: namespace.bus_id,
            owner: namespace.owner,
            ..Saved::default()
        };
        if unknown {
            saved.uncertain.insert("seeded".into());
        } else {
            saved.tokens.insert(42, "seeded".into());
        }
        // This is the state published when an early pending offer raced the
        // saved/Qt load. Invoke the actual effect-boundary function, no worker.
        let shared = Shared {
            state: Mutex::new(State {
                saved,
                ready: true,
                pending: BTreeMap::from([(
                    "seeded".into(),
                    Pending {
                        nonce: 1,
                        notice: notice("seeded"),
                        sending: true,
                    },
                )]),
                ..State::default()
            }),
            path: dir.0.join(FILE),
            wake,
            sink: Arc::new(|event| panic!("unexpected event {event:?}")),
            stopped: AtomicBool::new(false),
            done: AtomicBool::new(false),
            connection: Mutex::new(None),
        };
        post(&shared, &server.connection, 1, notice("seeded"));
        assert!(lock(&shared.state).pending.is_empty());
        assert!(calls.try_recv().is_err(), "a retained edge must never post");
    }
}
