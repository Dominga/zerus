use super::*;
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicU64;
use std::sync::{mpsc, Arc};
use std::thread;
use zbus::blocking::Connection as BlockingConnection;
use zbus::zvariant::OwnedValue;

static DIRECTORY: AtomicU64 = AtomicU64::new(0);
const BUDGET: Duration = Duration::from_secs(2);

struct Bus {
    child: Child,
    address: String,
}
impl Bus {
    fn new() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        assert!(address.starts_with("unix:"));
        Self {
            child,
            address: address.trim().into(),
        }
    }
    fn connect(&self) -> BlockingConnection {
        // Latch async-io outside any runtime even when the qualification build
        // also enables zbus/tokio. The blocking builder's block_on may otherwise
        // create a Tokio-backed fixture whose later object_server call has no
        // entered runtime. The production port never uses that blocking builder.
        future::block_on(
            zbus::connection::Builder::address(self.address.as_str())
                .unwrap()
                .method_timeout(BUDGET)
                .build(),
        )
        .unwrap()
        .into()
    }
    fn port(&self) -> Transport {
        Transport::connect(&self.address, BUDGET, &AtomicBool::new(false))
            .ok()
            .unwrap()
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug)]
struct Posted {
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
    posted: mpsc::Sender<Posted>,
    closed: mpsc::Sender<u32>,
    mode: AtomicU64,
    delay_ms: AtomicU64,
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
        self.0
            .posted
            .send(Posted {
                app,
                replaces,
                icon,
                title,
                body,
                actions,
                hints,
                timeout,
            })
            .unwrap();
        thread::sleep(Duration::from_millis(
            self.0.delay_ms.load(Ordering::Acquire),
        ));
        match self.0.mode.load(Ordering::Acquire) {
            1 => Err(zbus::fdo::Error::Failed("private daemon details".into())),
            2 => Ok(0),
            _ => Ok(7),
        }
    }
    fn close_notification(&self, id: u32) {
        self.0.closed.send(id).unwrap();
    }
}
struct Malformed;
#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Malformed {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        _app: String,
        _replaces: u32,
        _icon: String,
        _title: String,
        _body: String,
        _actions: Vec<String>,
        _hints: BTreeMap<String, OwnedValue>,
        _timeout: i32,
    ) -> String {
        "unexpected return signature".into()
    }
    fn close_notification(&self, _id: u32) -> String {
        "unexpected return signature".into()
    }
}
struct Server {
    connection: BlockingConnection,
    control: Arc<Control>,
    posted: mpsc::Receiver<Posted>,
    closed: mpsc::Receiver<u32>,
}
impl Server {
    fn new(bus: &Bus) -> Self {
        let connection = bus.connect();
        let (tx, posted) = mpsc::channel();
        let (cx, closed) = mpsc::channel();
        let control = Arc::new(Control {
            posted: tx,
            closed: cx,
            mode: AtomicU64::new(0),
            delay_ms: AtomicU64::new(0),
        });
        connection
            .object_server()
            .at(PATH, Fake(control.clone()))
            .unwrap();
        connection.request_name(SERVICE).unwrap();
        Self {
            connection,
            control,
            posted,
            closed,
        }
    }
}
fn namespace(port: &mut Transport) -> Namespace {
    port.namespace(BUDGET, &AtomicBool::new(false)).unwrap()
}
fn post(port: &mut Transport, scope: &Namespace) -> Outcome<NativeId> {
    port.post(
        scope,
        "Session needs input",
        "Агент <ready> & \"answer\"",
        BUDGET,
        &AtomicBool::new(false),
    )
}
fn drained(port: &mut Transport) {
    assert_eq!(port.shutdown(BUDGET), Drain::Drained);
    assert!(port.connection.is_none());
    assert!(port.executor.is_empty());
    assert_eq!(port.shutdown(Duration::ZERO), Drain::Drained);
}

#[test]
fn private_bus_posts_exact_qt_payload_and_closes_exact_positive_id() {
    let bus = Bus::new();
    let server = Server::new(&bus);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    assert_eq!(
        scope.owner,
        server.connection.unique_name().unwrap().as_str()
    );
    let Outcome::Delivered(id) = post(&mut port, &scope) else {
        panic!("post did not deliver");
    };
    assert_eq!(id.id.get(), 7);
    assert_eq!(id.namespace, scope);
    let sent = server.posted.recv_timeout(BUDGET).unwrap();
    assert_eq!(sent.app, "Zerus");
    assert_eq!(sent.replaces, 0);
    assert_eq!(sent.icon, "hgs-zerus-swarm-symbolic");
    assert_eq!(sent.title, "Session needs input");
    assert_eq!(sent.body, "Агент &lt;ready&gt; &amp; &quot;answer&quot;");
    assert_eq!(
        sent.actions,
        ["default", "Open session", "open-session", "Open session"]
    );
    assert_eq!(sent.timeout, -1);
    assert_eq!(
        <&str>::try_from(&sent.hints["desktop-entry"]).unwrap(),
        "hgs-tray"
    );
    assert_eq!(u8::try_from(&sent.hints["urgency"]).unwrap(), 1);
    assert_eq!(
        <&str>::try_from(&sent.hints["category"]).unwrap(),
        "im.received"
    );
    assert_eq!(
        port.withdraw(&id, BUDGET, &AtomicBool::new(false)),
        Outcome::Delivered(())
    );
    assert_eq!(server.closed.recv_timeout(BUDGET).unwrap(), 7);
    drained(&mut port);
}

#[test]
fn invalid_budget_pre_cancel_and_text_limit_never_call_native_methods() {
    let bus = Bus::new();
    let server = Server::new(&bus);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    for budget in [Duration::ZERO, Duration::from_secs(6)] {
        assert!(matches!(
            Transport::connect(&bus.address, budget, &AtomicBool::new(false)),
            Err(Refusal::InvalidBudget)
        ));
        assert_eq!(
            port.post(&scope, "t", "b", budget, &AtomicBool::new(false)),
            Outcome::Refused(Refusal::InvalidBudget)
        );
    }
    assert_eq!(
        port.post(&scope, "t", "b", BUDGET, &AtomicBool::new(true)),
        Outcome::Refused(Refusal::Cancelled)
    );
    assert_eq!(
        port.post(
            &scope,
            "t",
            &"я".repeat(TEXT_LIMIT),
            BUDGET,
            &AtomicBool::new(false)
        ),
        Outcome::Refused(Refusal::TextLimit)
    );
    assert!(server.posted.try_recv().is_err());
    assert!(server.closed.try_recv().is_err());
    drained(&mut port);
}

#[test]
fn missing_service_and_wrong_namespace_refuse_before_post_or_close() {
    let bus = Bus::new();
    let mut port = bus.port();
    assert_eq!(
        port.namespace(BUDGET, &AtomicBool::new(false)),
        Err(Refusal::Unavailable)
    );
    let server = Server::new(&bus);
    let mut scope = namespace(&mut port);
    scope.bus_id = "00000000000000000000000000000000".into();
    assert_eq!(
        post(&mut port, &scope),
        Outcome::Refused(Refusal::NamespaceChanged)
    );
    let id = NativeId {
        namespace: scope,
        id: NonZeroU32::new(7).unwrap(),
    };
    assert_eq!(
        port.withdraw(&id, BUDGET, &AtomicBool::new(false)),
        Outcome::Refused(Refusal::NamespaceChanged)
    );
    assert!(server.posted.try_recv().is_err());
    assert!(server.closed.try_recv().is_err());
    drained(&mut port);
}

#[test]
fn daemon_error_is_unknown_without_leaking_details_or_automatic_retry() {
    let bus = Bus::new();
    let server = Server::new(&bus);
    server.control.mode.store(1, Ordering::Release);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    let outcome = post(&mut port, &scope);
    assert_eq!(outcome, Outcome::Unknown(Unknown::Transport));
    assert!(!format!("{outcome:?}").contains("private daemon"));
    server.posted.recv_timeout(BUDGET).unwrap();
    assert!(server.posted.try_recv().is_err());
    drained(&mut port);
}

#[test]
fn zero_identity_is_unknown_and_cannot_propose_a_numeric_close() {
    let bus = Bus::new();
    let server = Server::new(&bus);
    server.control.mode.store(2, Ordering::Release);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    assert_eq!(post(&mut port, &scope), Outcome::Unknown(Unknown::Protocol));
    server.posted.recv_timeout(BUDGET).unwrap();
    assert!(server.closed.try_recv().is_err());
    drained(&mut port);
}

#[test]
fn malformed_post_and_withdrawal_replies_remain_unknown() {
    let bus = Bus::new();
    let server = bus.connect();
    server.object_server().at(PATH, Malformed).unwrap();
    server.request_name(SERVICE).unwrap();
    let mut port = bus.port();
    let scope = namespace(&mut port);
    assert_eq!(post(&mut port, &scope), Outcome::Unknown(Unknown::Protocol));
    let original = NativeId {
        namespace: scope,
        id: NonZeroU32::new(7).unwrap(),
    };
    assert_eq!(
        port.withdraw(&original, BUDGET, &AtomicBool::new(false)),
        Outcome::Unknown(Unknown::Protocol)
    );
    drained(&mut port);
}

#[test]
fn method_deadline_and_actual_connection_drain_do_not_wait_for_daemon_reply() {
    let bus = Bus::new();
    let server = Server::new(&bus);
    server.control.delay_ms.store(250, Ordering::Release);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    let start = Instant::now();
    assert_eq!(
        port.post(
            &scope,
            "t",
            "b",
            Duration::from_millis(60),
            &AtomicBool::new(false)
        ),
        Outcome::Unknown(Unknown::Deadline)
    );
    assert!(start.elapsed() < Duration::from_secs(1));
    server.posted.recv_timeout(BUDGET).unwrap();
    drained(&mut port);
    thread::sleep(Duration::from_millis(270));
    assert!(server.posted.try_recv().is_err());
}

#[test]
fn cancellation_during_native_call_is_unknown_and_has_no_retry() {
    let bus = Bus::new();
    let server = Server::new(&bus);
    server.control.delay_ms.store(250, Ordering::Release);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let observer = thread::spawn(move || {
        server.posted.recv_timeout(BUDGET).unwrap();
        flag.store(true, Ordering::Release);
        server
    });
    assert_eq!(
        port.post(&scope, "t", "b", BUDGET, &cancel),
        Outcome::Unknown(Unknown::Cancelled)
    );
    let server = observer.join().unwrap();
    drained(&mut port);
    assert!(server.posted.try_recv().is_err());
}

#[test]
fn replaced_daemon_cannot_receive_a_close_for_the_previous_owner() {
    let bus = Bus::new();
    let old = Server::new(&bus);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    let Outcome::Delivered(id) = post(&mut port, &scope) else {
        panic!("post");
    };
    old.connection.release_name(SERVICE).unwrap();
    let replacement = Server::new(&bus);
    assert_ne!(scope.owner, namespace(&mut port).owner);
    assert_eq!(
        port.withdraw(&id, BUDGET, &AtomicBool::new(false)),
        Outcome::Refused(Refusal::NamespaceChanged)
    );
    assert!(old.closed.try_recv().is_err());
    assert!(replacement.closed.try_recv().is_err());
    drained(&mut port);
}

#[test]
fn another_bus_with_same_unique_owner_and_numeric_id_is_not_the_original_namespace() {
    let first = Bus::new();
    let old = Server::new(&first);
    let mut port = first.port();
    let scope = namespace(&mut port);
    let Outcome::Delivered(id) = post(&mut port, &scope) else {
        panic!("post");
    };
    drained(&mut port);
    let second = Bus::new();
    let new = Server::new(&second);
    let mut port = second.port();
    let current = namespace(&mut port);
    assert_eq!(scope.owner, current.owner);
    assert_ne!(scope.bus_id, current.bus_id);
    assert_eq!(
        port.withdraw(&id, BUDGET, &AtomicBool::new(false)),
        Outcome::Refused(Refusal::NamespaceChanged)
    );
    assert!(old.closed.try_recv().is_err());
    assert!(new.closed.try_recv().is_err());
    drained(&mut port);
}

#[test]
fn pending_shutdown_retains_owned_resources_disables_calls_and_can_finish() {
    let bus = Bus::new();
    let server = Server::new(&bus);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    assert_eq!(port.shutdown(Duration::ZERO), Drain::Pending);
    assert!(port.connection.is_some());
    assert_eq!(post(&mut port, &scope), Outcome::Refused(Refusal::Closed));
    assert!(server.posted.try_recv().is_err());
    drained(&mut port);
}

#[test]
fn shutdown_deadline_does_not_release_a_still_owned_executor_task() {
    let bus = Bus::new();
    let mut port = bus.port();
    let task = port
        .executor
        .spawn(std::future::pending::<()>(), "private-drain-fixture");
    let start = Instant::now();
    assert_eq!(port.shutdown(Duration::from_millis(40)), Drain::Pending);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(port.connection.is_none());
    assert!(!port.executor.is_empty());
    drop(task);
    drained(&mut port);
}

#[test]
fn stalled_authentication_obeys_connection_deadline_without_an_internal_worker() {
    let dir = std::env::temp_dir().join(format!(
        "zerus-notify-auth-{}-{}",
        std::process::id(),
        DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let path: PathBuf = dir.join("bus");
    let listener = UnixListener::bind(&path).unwrap();
    let fixture = thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        thread::sleep(Duration::from_millis(200));
    });
    let start = Instant::now();
    let result = Transport::connect(
        &format!("unix:path={}", path.display()),
        Duration::from_millis(40),
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(Refusal::Deadline)));
    assert!(start.elapsed() < Duration::from_secs(1));
    fixture.join().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn sender_mismatch_is_unknown_even_for_a_well_formed_reply() {
    let bus = Bus::new();
    let _server = Server::new(&bus);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    let call = Message::method_call(PATH, "Notify")
        .unwrap()
        .build(&())
        .unwrap();
    let reply = Message::method_return(&call.header())
        .unwrap()
        .sender(":1.999")
        .unwrap()
        .build(&7_u32)
        .unwrap();
    assert!(matches!(
        port.method(
            &scope,
            Instant::now() + BUDGET,
            &AtomicBool::new(false),
            future::ready(Ok(reply))
        ),
        Outcome::Unknown(Unknown::Protocol)
    ));
    drained(&mut port);
}

#[test]
fn cancelled_or_expired_drive_does_not_poll_a_ready_native_future() {
    let polled = AtomicBool::new(false);
    let result = drive(
        None,
        Instant::now() + BUDGET,
        &AtomicBool::new(true),
        async { polled.store(true, Ordering::Release) },
    );
    assert!(matches!(result, Err(Interrupted::Cancelled)));
    assert!(!polled.load(Ordering::Acquire));
    let result = drive(
        None,
        Instant::now() - BUDGET,
        &AtomicBool::new(false),
        async { polled.store(true, Ordering::Release) },
    );
    assert!(matches!(result, Err(Interrupted::Deadline)));
    assert!(!polled.load(Ordering::Acquire));
}

#[test]
fn active_tokio_context_cannot_construct_call_or_falsely_drain_the_port() {
    let bus = Bus::new();
    let server = Server::new(&bus);
    let mut port = bus.port();
    let scope = namespace(&mut port);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    {
        let _entered = runtime.enter();
        assert!(matches!(
            Transport::connect(&bus.address, BUDGET, &AtomicBool::new(false)),
            Err(Refusal::Runtime)
        ));
        assert_eq!(post(&mut port, &scope), Outcome::Refused(Refusal::Runtime));
        assert_eq!(port.shutdown(BUDGET), Drain::Pending);
        assert!(port.connection.is_some());
    }
    assert!(server.posted.try_recv().is_err());
    drained(&mut port);
}
