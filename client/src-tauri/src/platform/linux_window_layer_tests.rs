use super::*;
use crate::platform::linux_test_bus::Bus;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const ID: &str = "{11111111-1111-4111-8111-111111111111}";
const APP: &str = "org.example.ZerusWindowLayerTest";
#[derive(Default)]
struct Control {
    config: Mutex<Option<Value>>,
    file: Mutex<Option<PathBuf>>,
    mode: AtomicUsize,
    unloaded: AtomicUsize,
}
struct Scripting(Arc<Control>);
#[zbus::interface(name = "org.kde.kwin.Scripting")]
impl Scripting {
    #[zbus(name = "loadScript")]
    fn load_script(&self, path: &str, _name: &str) -> i32 {
        let source = std::fs::read_to_string(path).unwrap();
        *self.0.config.lock().unwrap() = Some(
            serde_json::from_str(
                source
                    .strip_prefix("const config = ")
                    .unwrap()
                    .split_once(";\n")
                    .unwrap()
                    .0,
            )
            .unwrap(),
        );
        *self.0.file.lock().unwrap() = Some(path.into());
        if self.0.mode.load(Ordering::SeqCst) == 1 {
            -1
        } else {
            7
        }
    }
    #[zbus(name = "unloadScript")]
    fn unload_script(&self, _name: &str) -> bool {
        self.0.unloaded.fetch_add(1, Ordering::SeqCst);
        true
    }
}
struct Runner(Arc<Control>);
#[zbus::interface(name = "org.kde.kwin.Script")]
impl Runner {
    #[zbus(name = "run")]
    async fn run(&self, #[zbus(connection)] bus: &zbus::Connection) {
        if self.0.mode.load(Ordering::SeqCst) == 2 {
            return;
        }
        let config = self.0.config.lock().unwrap().clone().unwrap();
        let proxy = zbus::Proxy::new(
            bus,
            config["service"].as_str().unwrap(),
            config["path"].as_str().unwrap(),
            "org.hgdev.Zerus.WindowLayer",
        )
        .await
        .unwrap();
        proxy
            .call::<_, _, ()>(
                "Report",
                &(
                    config["token"].as_str().unwrap(),
                    ID,
                    config["desired"].as_bool().unwrap_or(false),
                    "",
                ),
            )
            .await
            .unwrap();
    }
}
struct Info(Mutex<HashMap<String, OwnedValue>>);
#[zbus::interface(name = "org.kde.KWin")]
impl Info {
    #[zbus(name = "getWindowInfo")]
    fn info(&self, _id: &str) -> HashMap<String, OwnedValue> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.try_clone().unwrap()))
            .collect()
    }
}
fn info() -> HashMap<String, OwnedValue> {
    HashMap::from([
        ("uuid".into(), zbus::zvariant::Str::from(ID).into()),
        (
            "resourceClass".into(),
            zbus::zvariant::Str::from(APP).into(),
        ),
        ("pid".into(), (std::process::id() as i64).into()),
        ("keepAbove".into(), true.into()),
    ])
}
fn server(bus: &Bus, control: &Arc<Control>) -> Connection {
    zbus::blocking::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.kde.KWin")
        .unwrap()
        .serve_at("/Scripting", Scripting(control.clone()))
        .unwrap()
        .serve_at("/Scripting/Script7", Runner(control.clone()))
        .unwrap()
        .serve_at("/KWin", Info(Mutex::new(info())))
        .unwrap()
        .build()
        .unwrap()
}
fn client(bus: &Bus) -> KWin {
    KWin::connect(
        zbus::blocking::connection::Builder::address(bus.address.as_str()).unwrap(),
        APP.into(),
    )
    .unwrap()
}
fn eventually(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn report(bus: &Connection, config: &Value, token: &str, id: &str) {
    Proxy::new(
        bus,
        config["service"].as_str().unwrap(),
        config["path"].as_str().unwrap(),
        "org.hgdev.Zerus.WindowLayer",
    )
    .unwrap()
    .call::<_, _, ()>("Report", &(token, id, true, ""))
    .unwrap();
}
#[test]
fn temporary_script_confirms_scoped_window_and_unloads_without_leaving_files() {
    let bus = Bus::new();
    let control = Arc::new(Control::default());
    let _server = server(&bus, &control);
    let client = client(&bus);
    let result = client.request("literal\"; throw Error('injection'); //\n", "", Some(true));
    assert_eq!(
        result,
        Reply {
            id: ID.into(),
            on: true,
            error: String::new()
        }
    );
    let config = control.config.lock().unwrap().clone().unwrap();
    assert_eq!(config["pid"], std::process::id());
    assert_eq!(config["appId"], APP);
    assert!(!control.file.lock().unwrap().as_ref().unwrap().exists());
    assert!(control.unloaded.load(Ordering::SeqCst) > 0);
    assert_eq!(
        client.inspect(ID),
        Reply {
            id: ID.into(),
            on: true,
            error: String::new()
        }
    );
}
#[test]
fn callbacks_require_compositor_owner_and_exact_nonce() {
    let bus = Bus::new();
    let control = Arc::new(Control::default());
    control.mode.store(2, Ordering::SeqCst);
    let server = server(&bus, &control);
    let stranger = bus.connect();
    let client = client(&bus);
    let _client_connection = client.bus.clone();
    let worker = std::thread::spawn(move || client.request("test", "", None));
    eventually(|| control.config.lock().unwrap().is_some());
    let config = control.config.lock().unwrap().clone().unwrap();
    let file = control.file.lock().unwrap().clone().unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    report(&stranger, &config, config["token"].as_str().unwrap(), ID);
    report(&server, &config, "wrong-nonce", ID);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!worker.is_finished());
    report(&server, &config, config["token"].as_str().unwrap(), ID);
    assert_eq!(worker.join().unwrap().id, ID);
    assert!(!file.exists());
    // A delayed callback after completion has no pending request to mutate.
    report(&server, &config, config["token"].as_str().unwrap(), ID);
}
#[test]
fn rejection_timeout_and_invalid_uuid_fail_closed_and_clean_up() {
    let bus = Bus::new();
    let control = Arc::new(Control::default());
    let server = server(&bus, &control);
    let client = client(&bus);
    for mode in [1, 2] {
        control.mode.store(mode, Ordering::SeqCst);
        assert_eq!(client.request("test", "", Some(true)).error, "unavailable");
        assert!(!control.file.lock().unwrap().as_ref().unwrap().exists());
    }
    *control.config.lock().unwrap() = None;
    let worker = std::thread::spawn(move || client.request("test", "", None));
    eventually(|| control.config.lock().unwrap().is_some());
    let config = control.config.lock().unwrap().clone().unwrap();
    report(
        &server,
        &config,
        config["token"].as_str().unwrap(),
        "not-a-uuid",
    );
    assert_eq!(worker.join().unwrap().error, "unsupported");
}
#[test]
fn inspection_rejects_foreign_pid_app_uuid_and_untyped_state() {
    let bus = Bus::new();
    let control = Arc::new(Control::default());
    let server = server(&bus, &control);
    let client = client(&bus);
    let interface = server
        .object_server()
        .interface::<_, Info>("/KWin")
        .unwrap();
    for (key, value) in [
        ("pid", OwnedValue::from(1i64)),
        ("resourceClass", zbus::zvariant::Str::from("other").into()),
        ("uuid", zbus::zvariant::Str::from("other").into()),
        ("keepAbove", zbus::zvariant::Str::from("true").into()),
    ] {
        let guard = interface.get();
        let mut map = guard.0.lock().unwrap();
        *map = info();
        map.insert(key.into(), value);
        drop(map);
        drop(guard);
        assert_eq!(
            client.inspect(ID).error,
            if key == "keepAbove" {
                "unsupported"
            } else {
                "missing"
            }
        );
    }
}
