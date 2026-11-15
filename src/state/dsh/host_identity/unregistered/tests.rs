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

#[test]
fn review_registered_inner_host_is_the_nearest_custodian_boundary() {
    let mut ancestors = chain();
    ancestors[1].identity.parent = 30;
    ancestors[1].top = false;
    ancestors.push(Ancestor {
        pid: 30,
        identity: Process {
            parent: 1,
            birth: Some("outer-legacy-host".into()),
        },
        top: true,
    });
    let answer = detect(
        &record(Some("native-host")),
        &ancestors,
        |pid| (pid == 30).then(|| args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js")),
        native_host,
        || Ok(()),
    );
    assert_eq!(
        answer,
        Ok(false),
        "a registered current host must retain its own proof even when an older unregistered host launched it"
    );
}
#[test]
fn review_node_no_warnings_does_not_hide_the_same_manual_host() {
    let mut command = args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js");
    assert!(native_host(&command));
    command.insert(1, b"--no-warnings".to_vec());
    assert!(
        native_host(&command),
        "the same Node CLI entry and server argv should remain observable with a harmless Node runtime flag"
    );
}

#[test]
fn an_unregistered_host_below_a_registered_host_still_refuses() {
    let mut ancestors = chain();
    ancestors[1].identity.parent = 30;
    ancestors[1].top = false;
    ancestors.push(Ancestor {
        pid: 30,
        identity: Process {
            parent: 1,
            birth: Some("registered-outer".into()),
        },
        top: true,
    });
    let mut records = record(Some("registered-outer"));
    records[0].named.pid = 30;
    assert_eq!(
        detect(
            &records,
            &ancestors,
            |pid| (pid == 20).then(|| args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js")),
            native_host,
            || Ok(())
        ),
        Ok(true)
    );
}
#[test]
fn node_option_values_and_eval_modes_cannot_be_the_entry() {
    let original = args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js");
    for options in [
        vec!["--trace-warnings"],
        vec!["--require", "/fixture/preload.cjs"],
        vec!["--import=/fixture/preload.mjs"],
        vec!["--"],
    ] {
        let mut command = original.clone();
        command.splice(1..1, options.iter().map(|v| v.as_bytes().to_vec()));
        assert!(native_host(&command), "{options:?}");
    }
    for options in [
        vec!["--eval", "0"],
        vec!["--print=0"],
        vec!["-e0"],
        vec!["--test"],
        vec!["--require"],
    ] {
        let mut command = original.clone();
        command.splice(1..1, options.iter().map(|v| v.as_bytes().to_vec()));
        assert!(!native_host(&command), "{options:?}");
    }
}
