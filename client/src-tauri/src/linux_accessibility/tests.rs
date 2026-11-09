use super::*;
use crate::platform::linux_test_bus::Bus;
use gtk::atk::prelude::AtkSocketExt;

const CHILD: &str = "ZERUS_ACCESSIBILITY_PROBE_TEST";

fn command(mode: &str, address: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "linux_accessibility::tests::probe_child",
            "--nocapture",
        ])
        .env(CHILD, mode)
        .env("AT_SPI_BUS_ADDRESS", address)
        .env("DBUS_SESSION_BUS_ADDRESS", address)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY");
    command
}

#[test]
fn probe_child() {
    match std::env::var(CHILD).ok().as_deref() {
        Some("native") => std::process::exit(native_probe()),
        Some("invalid") => std::process::exit(2),
        Some("opt-out") => {
            prepare_with(|| panic!("explicit opt-out must not probe"));
            assert_eq!(std::env::var("NO_AT_BRIDGE").unwrap(), "01explicit");
            std::process::exit(0);
        }
        Some("gtk") | Some("gtk-unguarded") => {
            if std::env::var(CHILD).unwrap() == "gtk" {
                prepare_with(|| {
                    check(
                        command("native", &std::env::var("AT_SPI_BUS_ADDRESS").unwrap()),
                        Instant::now() + BUDGET,
                    )
                });
                assert_eq!(
                    disabled(std::env::var_os("NO_AT_BRIDGE").as_deref()),
                    std::env::var("PROBE_EXPECT_DISABLED").unwrap() == "1"
                );
            }
            gtk::init().unwrap();
            // The same native AtkSocket embed reached by WebKit. In the
            // failed bridge, its installed hook dereferences freed state.
            let socket = gtk::atk::Socket::new();
            socket.embed(":1.999:/org/a11y/atspi/accessible/root");
            std::process::exit(0);
        }
        Some("hung") => {
            std::fs::write(
                std::env::var("PROBE_PID").unwrap(),
                std::process::id().to_string(),
            )
            .unwrap();
            loop {
                std::thread::park();
            }
        }
        _ => {}
    }
}

#[test]
fn actual_gtk_embed_survives_a_missing_bus_and_keeps_a_healthy_bridge() {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    let mut display = ProbeChild(
        Command::new("Xvfb")
            .args([
                "-displayfd",
                "1",
                "-screen",
                "0",
                "640x480x24",
                "-nolisten",
                "tcp",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut output = display.0.stdout.take().unwrap();
    let mut ready = libc::pollfd {
        fd: output.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(
        unsafe { libc::poll(&mut ready, 1, 3000) },
        1,
        "private Xvfb readiness"
    );
    let mut bytes = [0; 32];
    let count = output.read(&mut bytes).unwrap();
    let number: u32 = std::str::from_utf8(&bytes[..count])
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let absent = format!("unix:path={}/absent", root.path().display());
    let launch = |mode, address: &str, disabled| {
        // Supply the private display only to this owned child, never mutate
        // the test runner's environment or use the owner's display/buses.
        let mut command = command(mode, address);
        command
            .env("DISPLAY", format!(":{number}"))
            .env("GDK_BACKEND", "x11")
            .env("GSETTINGS_BACKEND", "memory")
            .env("HOME", root.path())
            .env("XDG_STATE_HOME", root.path())
            .env_remove("NO_AT_BRIDGE")
            .env("PROBE_EXPECT_DISABLED", if disabled { "1" } else { "0" });
        command
    };
    assert_eq!(
        check(
            launch("gtk", &absent, true),
            Instant::now() + Duration::from_secs(5)
        ),
        Availability::Ready
    );
    let bus = Bus::new();
    assert_eq!(
        check(
            launch("gtk", &bus.address, false),
            Instant::now() + Duration::from_secs(5)
        ),
        Availability::Ready
    );
}

#[test]
fn real_bus_requires_native_authentication_and_hello() {
    let bus = Bus::new();
    assert_eq!(
        check(command("native", &bus.address), Instant::now() + BUDGET),
        Availability::Ready
    );
    assert_eq!(
        check(command("invalid", &bus.address), Instant::now() + BUDGET),
        Availability::ProbeFailed
    );
}

#[test]
fn unreachable_bus_does_not_enable_the_bridge() {
    let root = tempfile::tempdir().unwrap();
    let address = format!("unix:path={}/absent", root.path().display());
    let session = Bus::new();
    let mut command = command("native", &address);
    command.env("DBUS_SESSION_BUS_ADDRESS", &session.address);
    assert_eq!(
        check(command, Instant::now() + BUDGET),
        Availability::Unavailable
    );
}

#[test]
fn a_socket_without_a_bus_cannot_stall_startup() {
    use std::os::fd::AsRawFd;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("silent");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let child =
        ProbeChild::start(command("native", &format!("unix:path={}", path.display()))).unwrap();
    let mut ready = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    assert_eq!(
        unsafe { libc::poll(&mut ready, 1, 5000) },
        1,
        "native probe connected"
    );
    let (mut stream, _) = listener.accept().unwrap();
    let start = Instant::now();
    assert_eq!(
        wait(child, start + Duration::from_millis(200)),
        Availability::TimedOut
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    // The original helper is physically gone, not left authenticating in a
    // detached thread. Its socket has no writer after check returns.
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut stream, &mut bytes).unwrap();
}

#[test]
fn timed_out_original_child_is_killed_and_reaped() {
    let root = tempfile::tempdir().unwrap();
    let pid = root.path().join("pid");
    let mut command = command("hung", "unix:path=/unused");
    command.env("PROBE_PID", &pid);
    let child = ProbeChild::start(command).unwrap();
    let ready_until = Instant::now() + Duration::from_secs(5);
    while !pid.exists() {
        assert!(
            Instant::now() < ready_until,
            "original fixture published its PID"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        wait(child, Instant::now() + Duration::from_millis(200)),
        Availability::TimedOut
    );
    let pid: u32 = std::fs::read_to_string(pid).unwrap().parse().unwrap();
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
}

struct Discovery(String);
#[zbus::interface(name = "org.a11y.Bus")]
impl Discovery {
    fn get_address(&self) -> &str {
        &self.0
    }
}

#[test]
fn native_discovery_uses_the_session_launcher_when_no_address_was_given() {
    let session = Bus::new();
    let accessibility = Bus::new();
    let _service = zbus::blocking::connection::Builder::address(session.address.as_str())
        .unwrap()
        .name("org.a11y.Bus")
        .unwrap()
        .serve_at("/org/a11y/bus", Discovery(accessibility.address.clone()))
        .unwrap()
        .build()
        .unwrap();
    let mut command = command("native", &session.address);
    command.env_remove("AT_SPI_BUS_ADDRESS");
    assert_eq!(check(command, Instant::now() + BUDGET), Availability::Ready);
}

#[test]
fn explicit_opt_out_is_preserved_and_the_helper_accepts_no_other_role() {
    for value in ["1", "01", "  +1", "\u{b}1", "1explicit"] {
        assert!(disabled(Some(OsStr::new(value))));
    }
    for value in ["0", "", "true", "2", "-1", "184467440737095516161"] {
        assert!(!disabled(Some(OsStr::new(value))));
    }
    assert!(!disabled(None));
    assert_eq!(probe_entry(&[PROBE, "--sessions"]), Some(2));
    assert_eq!(probe_entry(&[PROBE, "--build-info"]), Some(2));
    assert_eq!(probe_entry(&["--build-info"]), None);
    let mut command = command("opt-out", "unix:path=/unused");
    command.env("NO_AT_BRIDGE", "01explicit");
    assert_eq!(check(command, Instant::now() + BUDGET), Availability::Ready);
}
