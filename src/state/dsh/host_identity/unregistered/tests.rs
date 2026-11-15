use super::*;
use recorded::{Named, Process};

fn args(entry: &str) -> Vec<Vec<u8>> {
    [
        "node",
        entry,
        "--profile",
        "web",
        "--patch",
        "/fixture/manual.json",
        "--no-open",
        "--host",
        "127.0.0.1",
        "--port",
        "0",
    ]
    .into_iter()
    .map(|v| v.as_bytes().to_vec())
    .collect()
}
fn chain() -> Vec<Ancestor> {
    vec![
        Ancestor {
            pid: 10,
            identity: Process {
                parent: 20,
                birth: Some("caller".into()),
            },
            top: false,
        },
        Ancestor {
            pid: 20,
            identity: Process {
                parent: 1,
                birth: Some("native-host".into()),
            },
            top: true,
        },
    ]
}
fn record(birth: Option<&str>) -> Vec<Recorded> {
    vec![Recorded {
        named: Named {
            pid: 20,
            birth: birth.map(str::to_owned),
        },
        record: "/fixture/host.identity.json".into(),
    }]
}
#[test]
fn native_pid_birth_must_match_a_record_and_never_adopts_a_stale_pid() {
    let chain = chain();
    for birth in [None, Some("old-birth")] {
        assert_eq!(
            detect(
                &record(birth),
                &chain,
                |pid| (pid == 20)
                    .then(|| args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js")),
                native_host,
                || Ok(())
            ),
            Ok(true)
        );
    }
    assert_eq!(
        detect(
            &record(Some("native-host")),
            &chain,
            |pid| {
                assert_ne!(pid, 20, "the matched host keeps its ordinary proof");
                None
            },
            native_host,
            || Ok(())
        ),
        Ok(false)
    );
    assert_eq!(
        detect(
            &[],
            &chain,
            |pid| (pid == 20).then(|| args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js")),
            native_host,
            || Err(Refusal::UnreadableAncestry)
        ),
        Err(Refusal::UnreadableAncestry)
    );
}
#[test]
fn native_command_line_is_rechecked_after_the_bounded_identity_walk() {
    let reads = std::cell::Cell::new(0);
    let result = detect(
        &[],
        &chain(),
        |pid| {
            if pid != 20 {
                return None;
            }
            reads.set(reads.get() + 1);
            Some(args(if reads.get() == 1 {
                "/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js"
            } else {
                "/fixture/other.js"
            }))
        },
        native_host,
        || Ok(()),
    );
    assert_eq!(result, Err(Refusal::UnreadableAncestry));
}
#[test]
fn executable_names_and_similar_client_arguments_are_not_host_identity() {
    assert!(!native_host(&[b"dsh".to_vec()]));
    assert!(!native_host(&args("/fixture/dsh/lib/bin.js")));
    let mut other = args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js");
    other[0] = b"unrelated-server".to_vec();
    assert!(!native_host(&other));
    let mut client = args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js");
    client.push(b"--version".to_vec());
    assert!(!native_host(&client));
    assert!(!server_options(&vec![b"--no-open".to_vec(); 33]));
    assert!(!server_options(&[b"--port".to_vec(), b"65536".to_vec()]));
}
#[test]
fn a_non_npm_layout_needs_the_exact_bounded_package_entry() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("lib")).unwrap();
    let entry = tmp.path().join("lib/bin.js");
    std::fs::write(&entry, b"fixture module").unwrap();
    let manifest = tmp.path().join("package.json");
    std::fs::write(
        &manifest,
        br#"{"name":"@deepseek-ai/dsh","bin":{"dsh":"lib/bin.js"}}"#,
    )
    .unwrap();
    assert!(native_host(&args(entry.to_str().unwrap())));
    for bytes in [
        br#"{"name":"other","bin":{"dsh":"lib/bin.js"}}"#.to_vec(),
        br#"{"name":"@deepseek-ai/dsh","bin":{"dsh":"lib/other.js"}}"#.to_vec(),
        vec![b' '; MANIFEST_BYTES as usize + 1],
    ] {
        std::fs::write(&manifest, bytes).unwrap();
        assert!(!native_host(&args(entry.to_str().unwrap())));
    }
}

#[test]
fn special_or_linked_package_evidence_is_never_read() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("lib")).unwrap();
    let entry = tmp.path().join("lib/bin.js");
    std::fs::write(&entry, b"fixture module").unwrap();
    let manifest = tmp.path().join("package.json");
    let other = tmp.path().join("other.json");
    std::fs::write(&other, br#"{"name":"@deepseek-ai/dsh","bin":"lib/bin.js"}"#).unwrap();
    symlink(&other, &manifest).unwrap();
    assert!(!native_host(&args(entry.to_str().unwrap())));
    std::fs::remove_file(&manifest).unwrap();
    let name = std::ffi::CString::new(manifest.as_os_str().as_bytes()).unwrap();
    // SAFETY: a private test-owned pathname, valid until this call returns.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(!native_host(&args(entry.to_str().unwrap())));
}
