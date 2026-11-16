//! Optional exact-CLI-body control; the author owns/captures private live state.
use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Instant,
};

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires ZERUS_LISTING_CAPTURE from the genuine private live CLI control"]
async fn genuine_full_listing_survives_native_snapshot_encryption_and_phone_open() {
    use std::os::unix::fs::PermissionsExt;
    let path = std::path::PathBuf::from(
        std::env::var_os("ZERUS_LISTING_CAPTURE").expect("private CLI capture"),
    );
    let raw = std::fs::read(&path).unwrap();
    assert!(raw.len() <= crate::native::MAX_BYTES);
    let expected: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(expected["host"], "test");
    let count = expected["sessions"].as_array().unwrap().len();
    assert!(count >= 20);
    let expected_ids = |rows: &Value| -> BTreeMap<String, Value> {
        rows.as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["name"].as_str().unwrap().into(),
                    json!([row["session_id"], row["run_id"], row["conversation_id"]]),
                )
            })
            .collect()
    };
    let ids = expected_ids(&expected["sessions"]);
    assert_eq!(ids.len(), count);

    // The real production native pipe/JSON cap, without a live gateway/state.
    let mut f = Fixture::new();
    let source = f._temporary.path().join("listing.json");
    std::fs::write(&source, &raw).unwrap();
    let executable = f._temporary.path().join("native-fixture");
    std::fs::write(
        &executable,
        format!("#!/bin/sh\nexec cat '{}'\n", source.display()),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let native = crate::native::Process::new(executable.to_str().unwrap().into(), 5.0);
    let parsed = native
        .call(Call::new(["ls", "--json", "--local", "--no-archives"]))
        .await
        .unwrap()
        .json();
    assert_eq!(parsed, expected);
    *f.connector.native.listing.borrow_mut() = Some(parsed);
    *f.connector.native.observation.borrow_mut() = Some(Observation {
        machine: MACHINE.into(),
        change_after_listing: false,
        inventory_reads: 0,
    });
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut profile = workspace::load_active(&f.private_dir).unwrap();
    profile.server_url = format!("http://{}", listener.local_addr().unwrap());
    let store = Store::lock(&f.private_dir, false).unwrap();
    let old = store.read("active.json").unwrap().unwrap();
    store
        .replace(
            "active.json",
            &Store::revision(Some(&old)),
            &serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();
    drop(store);
    f.relay = workspace::relay(&profile, true).unwrap();
    let server = tokio::task::spawn_blocking(move || {
        let until = Instant::now() + Duration::from_secs(10);
        let mut requests = Vec::new();
        while requests.len() < 2 && Instant::now() < until {
            let (mut stream, _) = match listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            let start = loop {
                let mut block = [0; 4096];
                let n = stream.read(&mut block).unwrap();
                assert!(n > 0 && bytes.len() + n < MAX_SMALL);
                bytes.extend_from_slice(&block[..n]);
                if let Some(i) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                    break i + 4;
                }
            };
            let header = String::from_utf8(bytes[..start].to_vec()).unwrap();
            let length = header
                .lines()
                .find_map(|line| {
                    let (n, v) = line.split_once(':')?;
                    n.eq_ignore_ascii_case("content-length")
                        .then(|| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while bytes.len() < start + length {
                let mut block = [0; 4096];
                let n = stream.read(&mut block).unwrap();
                assert!(n > 0 && bytes.len() + n < MAX_SMALL);
                bytes.extend_from_slice(&block[..n]);
            }
            requests.push((
                header.lines().next().unwrap().to_owned(),
                serde_json::from_slice::<Value>(&bytes[start..start + length]).unwrap(),
            ));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .unwrap();
        }
        assert_eq!(requests.len(), 2);
        requests
    });
    let engine = f.engine();
    let snapshot = engine.observe_local(MACHINE).await.unwrap();
    assert_eq!(expected_ids(&snapshot["sessions"]), ids);
    assert!(snapshot["mobile_projects"].is_object());
    assert!(snapshot["mobile_accounts"].is_object());
    // Same receiver budget used for a forwarded peer snapshot, with extras.
    let mobile_bytes = encoded(&snapshot).unwrap().len();
    assert!(mobile_bytes <= crate::fleet::MAX_PEER_SNAPSHOT);
    engine
        .snapshot(
            &profile,
            MACHINE,
            &profile.identity.device.device_id,
            "Private computer",
            snapshot,
            Value::Null,
        )
        .await
        .unwrap();
    let requests = server.await.unwrap();
    assert!(requests[0].0.starts_with("POST /v2/node/snapshot "));
    assert!(requests[1].0.starts_with("POST /v2/node/signals "));
    let envelope: EncryptedEnvelope =
        serde_json::from_value(requests[0].1["envelope"].clone()).unwrap();
    let plain = crypto::open(
        &f.phone,
        &profile.identity.device,
        &envelope.header,
        &envelope,
    )
    .unwrap();
    assert!(plain.len() <= 1024 * 1024);
    let decoded: Value = serde_json::from_slice(&plain).unwrap();
    assert_eq!(decoded["machine_id"], MACHINE);
    assert_eq!(expected_ids(&decoded["snapshot"]["sessions"]), ids);
    assert!(decoded["snapshot"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["relay_scope_id"].is_string()));
    println!("{{\"sessions\":{count},\"cli_bytes\":{},\"mobile_snapshot_bytes\":{mobile_bytes},\"encrypted_plaintext_bytes\":{},\"posted_ciphertext_bytes\":{}}}",raw.len(),plain.len(),encoded(&requests[0].1).unwrap().len());

    // Same canonical body limit, no truncate-to-success or enlarged transport.
    let mut oversized = decoded["snapshot"].clone();
    oversized["fixture_padding"] = json!("x".repeat(1024 * 1024));
    assert!(engine
        .snapshot(
            &profile,
            MACHINE,
            &profile.identity.device.device_id,
            "Private computer",
            oversized,
            Value::Null
        )
        .await
        .is_err());
}
