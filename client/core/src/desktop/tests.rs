use super::*;
const NONCE: &str = "11111111-1111-4111-8111-111111111111";
const EPOCH: &str = "22222222-2222-4222-8222-222222222222";
fn responder() -> Responder {
    Responder::new(
        "com.hgdev.hgs-tray".into(),
        Build {
            version: "1.2.3-dev.4".into(),
            source_commit: "b".repeat(40),
            source_dirty: Some(false),
        },
        EPOCH.into(),
        true,
        false,
    )
}
fn hello() -> Hello {
    let responder = responder();
    assert!(responder.configure_once("a".repeat(64)));
    serde_json::from_slice(&responder.reply(&query(NONCE).unwrap()[..58]).unwrap()).unwrap()
}
#[test]
fn query_fits_the_original_line_and_has_an_exact_nonce() {
    let query = query(NONCE).unwrap();
    assert_eq!(query.len(), 59);
    assert!(query.len() <= 64);
    assert_eq!(query_nonce(&query[..query.len() - 1]), Some(NONCE));
    for bad in [
        "show",
        "sessions",
        "desktop-delegation-v2 11111111-1111-4111-8111-111111111111",
        "desktop-delegation-v1  11111111-1111-4111-8111-111111111111",
        "desktop-delegation-v1 11111111-1111-4111-8111-111111111111 ",
    ] {
        assert!(query_nonce(bad.as_bytes()).is_none());
    }
}
#[test]
fn identity_requires_exact_configuration_nonce_production_build_and_live_instance() {
    let good = hello();
    assert!(good.identity_matches(NONCE, &"a".repeat(64)));
    assert!(!good.identity_matches(EPOCH, &"a".repeat(64)));
    assert!(!good.identity_matches(NONCE, &"c".repeat(64)));
    let mut bad = good.clone();
    bad.version = 2;
    assert!(!bad.identity_matches(NONCE, &"a".repeat(64)));
    let mut bad = good.clone();
    bad.application = "dev.zerus.next".into();
    assert!(!bad.identity_matches(NONCE, &"a".repeat(64)));
    let mut bad = good.clone();
    bad.replay = true;
    assert!(!bad.identity_matches(NONCE, &"a".repeat(64)));
    let mut bad = good.clone();
    bad.live = false;
    assert!(!bad.identity_matches(NONCE, &"a".repeat(64)));
    let mut bad = good.clone();
    bad.build.source_dirty = None;
    assert!(!bad.identity_matches(NONCE, &"a".repeat(64)));
    let mut bad = good.clone();
    bad.build.source_dirty = Some(true);
    assert!(!bad.identity_matches(NONCE, &"a".repeat(64)));
    let mut bad = good.clone();
    bad.instance_epoch = "stale".into();
    assert!(!bad.identity_matches(NONCE, &"a".repeat(64)));
    let mut bad = serde_json::to_value(good).unwrap();
    bad["unsolicited"] = true.into();
    assert!(serde_json::from_value::<Hello>(bad).is_err());
}
#[test]
fn configuration_is_set_once_and_an_unready_reply_grants_nothing() {
    let responder = responder();
    let request = query(NONCE).unwrap();
    let before: Hello = serde_json::from_slice(&responder.reply(&request[..58]).unwrap()).unwrap();
    assert!(!before.identity_matches(NONCE, &"a".repeat(64)));
    assert!(!responder.configure_once("invalid".into()));
    assert!(responder.configure_once("a".repeat(64)));
    assert!(!responder.configure_once("c".repeat(64)));
    let after: Hello = serde_json::from_slice(&responder.reply(&request[..58]).unwrap()).unwrap();
    assert!(after.identity_matches(NONCE, &"a".repeat(64)));
    assert_eq!(after.writer_epoch, None);
    assert!(responder.reply(b"show").is_none());
    assert!(responder.reply(b"sessions").is_none());
}

#[cfg(unix)]
mod sockets {
    use super::*;
    use std::{
        fs,
        io::{Read, Write},
        os::unix::net::{UnixListener, UnixStream},
        path::PathBuf,
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::Instant,
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    const FIXTURE_WAIT: Duration = Duration::from_secs(10);
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "zdh-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn configure_fixture_stream(stream: &UnixStream) {
        // Darwin inherits the listener's nonblocking flag. Only accept polls;
        // the connected fixture uses its existing bounded blocking timeout.
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(FIXTURE_WAIT)).unwrap();
        stream.set_write_timeout(Some(FIXTURE_WAIT)).unwrap();
    }
    struct FixtureRead<'a> {
        stream: &'a mut UnixStream,
        until: Instant,
        interrupt_once: bool,
    }
    impl Read for FixtureRead<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            let remaining = self.until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "fixture challenge deadline expired",
                ));
            }
            self.stream.set_read_timeout(Some(remaining))?;
            if std::mem::take(&mut self.interrupt_once) {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            self.stream.read(bytes)
        }
    }
    fn read_challenge(reader: &mut impl Read, case: &str) {
        let expected = query(NONCE).unwrap();
        let mut bytes = Vec::with_capacity(expected.len());
        let mut byte = [0];
        let mut interruptions = 0;
        let ended = loop {
            match reader.read(&mut byte) {
                Ok(0) => break "EOF",
                Ok(_) => {
                    assert!(
                        bytes.len() < expected.len(),
                        "{case}: challenge exceeded its {} byte bound",
                        expected.len()
                    );
                    bytes.push(byte[0]);
                    if byte[0] == b'\n' {
                        break "newline";
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                    interruptions += 1;
                }
                Err(error) => panic!(
                    "{case}: challenge read failed ({:?}, os={:?}) after {} bytes and {interruptions} Interrupted reads: {error}",
                    error.kind(), error.raw_os_error(), bytes.len()
                ),
            }
        };
        assert_eq!(
            bytes, expected,
            "{case}: challenge ended at {ended} after {interruptions} Interrupted reads"
        );
    }
    fn peer(reply: Vec<u8>, silent: bool, slow: bool) -> Observation {
        measured_peer(reply, silent, slow, Duration::ZERO).0
    }
    fn peer_case(reply: Vec<u8>, case: &'static str, interrupt_once: bool) -> Observation {
        measured_peer_with_read(reply, false, false, Duration::ZERO, case, interrupt_once).0
    }
    fn measured_peer(
        reply: Vec<u8>,
        silent: bool,
        slow: bool,
        cleanup_delay: Duration,
    ) -> (Observation, Duration) {
        measured_peer_with_read(reply, silent, slow, cleanup_delay, "socket fixture", false)
    }
    fn measured_peer_with_read(
        reply: Vec<u8>,
        silent: bool,
        slow: bool,
        cleanup_delay: Duration,
        case: &'static str,
        interrupt_once: bool,
    ) -> (Observation, Duration) {
        let root = Scratch::new();
        let path = root.0.join("s");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let (done, completed) = mpsc::sync_channel(1);
        let task = thread::spawn(move || {
            let until = Instant::now() + FIXTURE_WAIT;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < until, "fixture was not connected");
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            };
            configure_fixture_stream(&stream);
            assert!(same_user(&stream));
            let mut reader = FixtureRead {
                stream: &mut stream,
                until,
                interrupt_once,
            };
            read_challenge(&mut reader, case);
            if silent {
                thread::sleep(Duration::from_millis(1100));
            } else if slow {
                for b in reply {
                    if stream.write_all(&[b]).is_err() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(30));
                }
            } else {
                let _ = stream.write_all(&reply);
            }
            // Fixture scheduling/cleanup is independent of the product probe.
            thread::sleep(cleanup_delay);
            done.send(()).unwrap();
        });
        let start = Instant::now();
        let result = probe_at(&path, NONCE, &"a".repeat(64));
        let elapsed = start.elapsed();
        completed
            .recv_timeout(FIXTURE_WAIT)
            .unwrap_or_else(|error| {
                panic!("{case}: socket fixture did not finish: {error}; probe={result:?}, elapsed={elapsed:?}")
            });
        task.join().unwrap();
        (result, elapsed)
    }

    #[test]
    fn cached_producer_offer_is_one_use_over_the_authenticated_socket() {
        use super::super::producer::{Challenge, Observation as Producer};
        let root = Scratch::new();
        let path = root.0.join("p");
        let listener = UnixListener::bind(&path).unwrap();
        let responder = responder();
        responder.configure_once("a".repeat(64));
        let expected = responder.hello(NONCE);
        let offer = Challenge {
            nonce: NONCE.into(),
            service_epoch: EPOCH.into(),
            producer_epoch: "33333333-3333-4333-8333-333333333333".into(),
        };
        let deadline = Instant::now() + producer::CHALLENGE_WAIT;
        assert!(responder.install_challenge(offer.clone(), Instant::now()));
        let task = thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                assert!(same_user(&stream));
                stream.set_read_timeout(Some(PROBE_WAIT)).unwrap();
                let mut line = Vec::new();
                let mut byte = [0];
                while stream.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {
                    line.push(byte[0]);
                    assert!(line.len() < 64);
                }
                stream.write_all(&responder.reply(&line).unwrap()).unwrap();
            }
        });
        let Producer::LiveNext { reply } = probe_producer_at(&path, NONCE, &"a".repeat(64)) else {
            panic!("missing exact reply")
        };
        assert!(reply.binds(&offer, &expected, deadline));
        let Producer::LiveNext { reply } = probe_producer_at(&path, NONCE, &"a".repeat(64)) else {
            panic!("missing diagnostic reply")
        };
        assert!(!reply.binds(&offer, &expected, deadline));
        assert!(reply.challenge.is_none());
        task.join().unwrap();
    }
    #[test]
    fn valid_same_user_challenge_is_identity_evidence_only() {
        let mut bytes = serde_json::to_vec(&hello()).unwrap();
        bytes.push(b'\n');
        assert!(matches!(
            peer(bytes, false, false),
            Observation::LiveNext { .. }
        ));
    }
    #[test]
    fn ignored_or_rejected_queries_are_live_unknown_never_absent() {
        assert_eq!(
            peer(Vec::new(), false, false),
            Observation::LiveLegacyOrUnknown
        );
        assert_eq!(
            peer(Vec::new(), true, false),
            Observation::LiveLegacyOrUnknown
        );
    }
    #[test]
    fn an_inherited_nonblocking_fixture_stream_reads_the_deferred_exact_challenge() {
        use std::os::fd::AsRawFd;
        struct DeferredQuery<'a> {
            reader: FixtureRead<'a>,
            client: UnixStream,
            unsent: bool,
        }
        impl Read for DeferredQuery<'_> {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                if std::mem::take(&mut self.unsent) {
                    // SAFETY: the original accepted socket remains borrowed.
                    let flags =
                        unsafe { libc::fcntl(self.reader.stream.as_raw_fd(), libc::F_GETFL) };
                    assert!(flags >= 0, "accepted fixture flags are readable");
                    if flags & libc::O_NONBLOCK != 0 {
                        // Model the peer reaching its first read before the
                        // original client gets scheduled to send its query.
                        let result = self.reader.read(bytes);
                        self.client.write_all(&query(NONCE).unwrap())?;
                        return result;
                    }
                    // A blocking read needs the client's deferred query to
                    // proceed. No sleep or global scheduling change is used.
                    self.client.write_all(&query(NONCE).unwrap())?;
                }
                self.reader.read(bytes)
            }
        }
        let root = Scratch::new();
        let path = root.0.join("s");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let client = UnixStream::connect(&path).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        assert!(same_user(&stream));
        // Darwin inherits this flag; model it explicitly on every Unix host.
        stream.set_nonblocking(true).unwrap();
        configure_fixture_stream(&stream);
        read_challenge(
            &mut DeferredQuery {
                reader: FixtureRead {
                    stream: &mut stream,
                    until: Instant::now() + FIXTURE_WAIT,
                    interrupt_once: false,
                },
                client,
                unsent: true,
            },
            "modeled inherited nonblocking flag",
        );
    }
    #[test]
    fn an_interrupted_fixture_read_preserves_the_exact_challenge_and_passive_reply() {
        assert_eq!(
            peer_case(b"{}\n{}\n".to_vec(), "injected Interrupted read", true),
            Observation::LiveLegacyOrUnknown
        );
    }
    #[test]
    #[should_panic(expected = "genuine EOF: challenge ended at EOF")]
    fn a_genuine_fixture_eof_still_refuses_the_missing_challenge() {
        read_challenge(&mut std::io::empty(), "genuine EOF");
    }
    #[test]
    #[should_panic(expected = "non-Interrupted error: challenge read failed (WouldBlock")]
    fn a_non_interrupted_fixture_error_is_not_retried_or_normalized_to_eof() {
        struct FailedRead(bool);
        impl Read for FailedRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                assert!(!std::mem::replace(&mut self.0, true), "unexpected retry");
                Err(std::io::ErrorKind::WouldBlock.into())
            }
        }
        read_challenge(&mut FailedRead(false), "non-Interrupted error");
    }
    #[test]
    #[should_panic(expected = "expired fixture deadline: challenge read failed (TimedOut")]
    fn an_expired_fixture_deadline_still_refuses_the_challenge_read() {
        let (mut stream, _peer) = UnixStream::pair().unwrap();
        read_challenge(
            &mut FixtureRead {
                stream: &mut stream,
                until: Instant::now(),
                interrupt_once: false,
            },
            "expired fixture deadline",
        );
    }
    #[test]
    fn stale_unsolicited_newer_and_oversized_replies_are_passive() {
        let mut wrong = hello();
        wrong.nonce = EPOCH.into();
        let mut bytes = serde_json::to_vec(&wrong).unwrap();
        bytes.push(b'\n');
        assert_eq!(
            peer_case(bytes, "stale nonce reply", false),
            Observation::LiveLegacyOrUnknown
        );
        assert_eq!(
            peer_case(vec![b'x'; REPLY_BYTES + 1], "oversized reply", false),
            Observation::LiveLegacyOrUnknown
        );
        assert_eq!(
            peer_case(b"{}\n{}\n".to_vec(), "unsolicited extra reply", false),
            Observation::LiveLegacyOrUnknown
        );
    }
    #[test]
    fn total_deadline_is_not_extended_by_slow_reply_bytes() {
        // This exceeds the elapsed assertion by itself, reproducing a delayed
        // fixture exit after a correct probe. Only probe_at is under test.
        let (result, elapsed) =
            measured_peer(vec![b'x'; 64], false, true, Duration::from_millis(1800));
        assert_eq!(result, Observation::LiveLegacyOrUnknown);
        assert!(
            elapsed < Duration::from_millis(1600),
            "probe took {elapsed:?}"
        );
    }
    #[test]
    fn only_a_missing_path_is_absent_and_existing_entries_are_untouched() {
        let root = Scratch::new();
        let path = root.0.join("s");
        assert_eq!(probe_at(&path, NONCE, &"a".repeat(64)), Observation::Absent);
        fs::write(&path, b"existing").unwrap();
        assert_eq!(
            probe_at(&path, NONCE, &"a".repeat(64)),
            Observation::Unknown
        );
        assert_eq!(fs::read(&path).unwrap(), b"existing");
        fs::remove_file(&path).unwrap();
        drop(UnixListener::bind(&path).unwrap());
        // The closed endpoint can fail at connect or at the subsequent exchange
        // (observed on XNU under the parallel harness). Both remain passive:
        // neither absence nor an authenticated live Next identity is acceptable.
        let observed = probe_at(&path, NONCE, &"a".repeat(64));
        assert!(
            matches!(
                observed,
                Observation::Unknown | Observation::LiveLegacyOrUnknown
            ),
            "{observed:?}"
        );
        assert!(path.exists());
    }
    #[test]
    fn a_full_backlog_never_turns_connect_into_an_unbounded_wait() {
        use socket2::{Domain, SockAddr, Socket, Type};
        let root = Scratch::new();
        let path = root.0.join("s");
        let server = Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
        server.bind(&SockAddr::unix(&path).unwrap()).unwrap();
        server.listen(1).unwrap();
        let mut clients = Vec::new();
        for _ in 0..4 {
            let client = Socket::new(Domain::UNIX, Type::STREAM, None).unwrap();
            client.set_nonblocking(true).unwrap();
            let _ = client.connect(&SockAddr::unix(&path).unwrap());
            clients.push(client);
        }
        let start = Instant::now();
        assert!(matches!(
            probe_at(&path, NONCE, &"a".repeat(64)),
            Observation::Unknown | Observation::LiveLegacyOrUnknown
        ));
        assert!(start.elapsed() < Duration::from_millis(1600));
    }

    #[test]
    fn selected_cli_is_probed_safely_before_a_desktop_command() {
        let root = Scratch::new();
        let hgs = root.0.join("hgs");
        std::os::unix::fs::symlink(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_hgs_import.py"),
            &hgs,
        )
        .unwrap();
        let env = [("TEST_IMPORT_ROOT", root.0.to_str().unwrap())];
        let options = root.0.join("options.json");
        let log = root.0.join("calls.jsonl");
        fs::write(
            &options,
            r#"{"help":"Usage: hgs\nDesktop service ABI: desktop-v1-suffix"}"#,
        )
        .unwrap();
        assert_eq!(selected_configuration(&hgs, &env), None);
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);
        fs::write(
            &options,
            r#"{"help":"Usage: hgs\n Desktop service ABI: desktop-v1"}"#,
        )
        .unwrap();
        assert_eq!(selected_configuration(&hgs, &env), Some("a".repeat(64)));
        assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 3);
        fs::write(
            &options,
            r#"{"help":"Desktop service ABI: desktop-v1","configuration":"wrong"}"#,
        )
        .unwrap();
        assert_eq!(selected_configuration(&hgs, &env), None);
    }
}
