use super::*;
use std::time::Instant;
use std::{
    io::Write,
    os::unix::net::UnixStream,
    process::{Command, Stdio},
    sync::mpsc,
};

fn fixture() -> (tempfile::TempDir, native::Source) {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("state")).unwrap();
    std::fs::create_dir(d.path().join("config")).unwrap();
    let source =
        native::Source::capture(&d.path().join("state"), &d.path().join("config")).unwrap();
    (d, source)
}
fn binding(source: &native::Source) -> Value {
    json!({"owner":std::process::id(),"birth":source.birth(),"run":"original","nonce":"one"})
}
#[test]
fn retained_source_tolerates_unrelated_records_but_not_replaced_directories() {
    let (d, source) = fixture();
    std::fs::write(d.path().join("state/ordinary.json"), "{}").unwrap();
    assert!(source.current());
    std::fs::rename(d.path().join("config"), d.path().join("old-config")).unwrap();
    std::fs::create_dir(d.path().join("config")).unwrap();
    assert!(!source.current());
}
#[test]
fn one_exact_read_only_observation_matches_the_native_original_peer() {
    let (_d, source) = fixture();
    let channel = channel::Channel::new().unwrap();
    let binding = binding(&source);
    let path = channel.path();
    let expected = binding.clone();
    let birth = source.birth().to_owned();
    let (tx, rx) = mpsc::channel();
    let observer = std::thread::spawn(move || {
        tx.send(()).unwrap();
        channel::query(&path, &expected, std::process::id(), &birth).unwrap()
    });
    rx.recv().unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    while !observer.is_finished() {
        assert!(Instant::now() < end);
        channel.poll(
            &source,
            &binding,
            &json!({"binding":binding,"effect_authority":false}),
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let reply = observer.join().unwrap();
    assert_eq!(reply["effect_authority"], false);
}
#[test]
fn stale_request_and_partial_peer_do_not_renew_the_server_deadline() {
    let (_d, source) = fixture();
    let channel = channel::Channel::new().unwrap();
    let binding = binding(&source);
    let mut peer = UnixStream::connect(channel.path()).unwrap();
    peer.write_all(b"{").unwrap();
    let started = Instant::now();
    channel.poll(&source, &binding, &json!({}));
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(peer);
    let mut peer = UnixStream::connect(channel.path()).unwrap();
    peer.write_all(b"{\"observe\":{\"run\":\"other\"}}\n")
        .unwrap();
    channel.poll(&source, &binding, &json!({}));
    peer.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut byte = [0];
    assert_eq!(std::io::Read::read(&mut peer, &mut byte).unwrap(), 0);
}
#[test]
fn wrong_native_owner_or_birth_never_becomes_an_attestation() {
    for wrong_birth in [false, true] {
        let (_d, source) = fixture();
        let channel = channel::Channel::new().unwrap();
        let binding = binding(&source);
        let request = binding.clone();
        let path = channel.path();
        let mut other = Command::new("python3")
            .args(["-c", "import sys; sys.stdin.buffer.read()"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let owner = if wrong_birth {
            std::process::id()
        } else {
            other.id()
        };
        let birth = if wrong_birth {
            "wrong".into()
        } else {
            source.birth().to_owned()
        };
        let client = std::thread::spawn(move || channel::query(&path, &request, owner, &birth));
        let end = Instant::now() + Duration::from_secs(2);
        while !client.is_finished() {
            assert!(Instant::now() < end);
            channel.poll(
                &source,
                &binding,
                &json!({"binding":binding,"effect_authority":false}),
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let result = client.join().unwrap();
        drop(other.stdin.take());
        assert!(other.wait().unwrap().success());
        assert!(result.is_err());
    }
}

#[test]
fn legacy_census_refuses_owned_metadata_before_any_native_stop() {
    for owner in [json!({"version":1}), Value::Null, json!("malformed")] {
        let record = json!({"original_owner":owner,"pid":std::process::id()});
        let error = processes::Termination::prepare(&record).err().unwrap();
        assert!(error.contains("cohort-aware"));
        assert!(super::super::check_relaunch(&record)
            .unwrap_err()
            .contains("without replay"));
        assert!(archive::save(&record)
            .unwrap_err()
            .contains("cohort finalization"));
    }
}
#[test]
fn sealed_legacy_status_observers_share_the_original_positive_wait() {
    let mut child = Command::new("sh")
        .args(["-c", "exit 23"])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let first = ChildStatus::wait(&mut child).unwrap();
    assert_eq!(first.code(), Some(23));
    assert_eq!(ChildStatus::try_wait(&mut child).unwrap(), Some(first));
    assert_eq!(ChildStatus::wait(&mut child).unwrap(), first);
}
#[test]
fn production_gate_has_no_runtime_override() {
    assert!(!std::hint::black_box(super::super::ORIGINAL_OWNER));
}

#[test]
fn an_unrelated_native_image_cannot_observe_the_owner_even_with_the_json() {
    let (_d, source) = fixture();
    let channel = channel::Channel::new().unwrap();
    let binding = binding(&source);
    let bytes = format!("{}\n", json!({"observe":binding}));
    let code = "import socket,sys\ns=socket.socket(socket.AF_UNIX);s.connect(sys.argv[1]);s.sendall(sys.argv[2].encode());print('connected',flush=True)\ntry: assert s.recv(1)==b''\nexcept ConnectionResetError: pass";
    let mut child = Command::new("python3")
        .args(["-c", code])
        .arg(channel.path())
        .arg(bytes)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    std::io::BufRead::read_line(&mut out, &mut line).unwrap();
    assert_eq!(line, "connected\n");
    channel.poll(&source, &binding, &json!({"binding":binding}));
    assert!(child.wait().unwrap().success());
}

#[cfg(target_os = "macos")]
#[test]
fn owned_mac_facade_waits_for_and_caches_its_actual_original_child() {
    let (_d, source) = fixture();
    let channel = channel::Channel::new().unwrap();
    let binding = binding(&source);
    let root = Command::new("sh")
        .args(["-c", "exit 23"])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let mut original = Original {
        root,
        exit: None,
        channel,
        source,
        binding,
        startup: true,
        unresolved: false,
        _local: PhantomData,
    };
    let status = ChildStatus::wait(&mut original).unwrap();
    assert_eq!(status.code(), Some(23));
    assert_eq!(ChildStatus::try_wait(&mut original).unwrap(), Some(status));
    assert_eq!(ChildStatus::wait(&mut original).unwrap(), status);
}
