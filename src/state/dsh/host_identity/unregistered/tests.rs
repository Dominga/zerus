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

#[test]
fn a_registered_host_omitted_by_the_folder_bound_keeps_its_exact_proof() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("tmux")).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "state::dsh::host_identity::unregistered::tests::bounded_registration_probe",
            "--exact",
            "--nocapture",
        ])
        .env("HGS_STATE_DIR", root.path().join("state"))
        .env("HGS_CONFIG_DIR", root.path().join("config"))
        .env("TMUX_TMPDIR", root.path().join("tmux"))
        .env("HGS_TEST_BOUNDED_REGISTRATION", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bounded_registration_probe() {
    if std::env::var_os("HGS_TEST_BOUNDED_REGISTRATION").is_none() {
        return;
    }
    struct Children(Vec<std::process::Child>);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let mut children = Children(Vec::new());
    let state = crate::state::storage::root();
    let hosts = state.join("dsh/hosts");
    std::fs::create_dir_all(&hosts).unwrap();
    let mut homes = Vec::new();
    for index in 0..=recorded::HOST_FOLDERS {
        let home = format!("/fixture/native-home-{index}");
        let folder = hosts.join(crate::state::dsh::host_key(&home));
        std::fs::create_dir(&folder).unwrap();
        let child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        super::super::record_this_boot_for_test(&folder, child.id()).unwrap();
        homes.push((home, folder, child.id()));
        children.0.push(child);
    }
    let records = recorded::observable_records();
    assert_eq!(records.len(), recorded::HOST_FOLDERS);
    let (home, folder, pid) = homes
        .iter()
        .find(|(_, _, pid)| !records.iter().any(|r| r.named.pid == *pid))
        .unwrap();
    let exact = recorded::for_home(home).unwrap();
    assert_eq!(exact.len(), 1);
    let birth = exact[0].birth.clone().unwrap();
    assert_eq!(exact[0].pid, *pid);
    let chain = vec![Ancestor {
        pid: *pid,
        identity: Process {
            parent: 1,
            birth: Some(birth),
        },
        top: true,
    }];
    let mut tokens = args("/fixture/node_modules/@deepseek-ai/dsh/lib/bin.js");
    let at = tokens.iter().position(|v| v == b"--patch").unwrap() + 1;
    tokens[at] = folder
        .join("hgs.patch.json")
        .to_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    assert_eq!(
        detect(
            &records,
            &chain,
            |_| Some(tokens.clone()),
            native_host,
            || Ok(())
        ),
        Ok(false),
        "a prefix cannot establish that the exact recorded native host is unregistered"
    );
    let mut wrong_birth = chain.clone();
    wrong_birth[0].identity.birth = Some("different-birth".into());
    assert_eq!(
        detect(
            &records,
            &wrong_birth,
            |_| Some(tokens.clone()),
            native_host,
            || Ok(())
        ),
        Ok(true)
    );
    let mut wrong_pid = chain.clone();
    wrong_pid[0].pid = i32::MAX as u32;
    assert_eq!(
        detect(
            &records,
            &wrong_pid,
            |_| Some(tokens.clone()),
            native_host,
            || Ok(())
        ),
        Ok(true)
    );
    let mut foreign = tokens.clone();
    foreign[at] = state
        .join("outside")
        .join(folder.file_name().unwrap())
        .join("hgs.patch.json")
        .to_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    assert_eq!(
        detect(
            &records,
            &chain,
            |_| Some(foreign.clone()),
            native_host,
            || Ok(())
        ),
        Ok(true)
    );
    let file = folder.join("host.identity.json");
    std::fs::write(&file, b"{").unwrap();
    assert_eq!(
        detect(
            &records,
            &chain,
            |_| Some(tokens.clone()),
            native_host,
            || Ok(())
        ),
        Ok(true)
    );
    super::super::record_this_boot_for_test(folder, *pid).unwrap();
    let moved = state.join("held-host");
    std::fs::rename(folder, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, folder).unwrap();
    assert_eq!(
        detect(
            &records,
            &chain,
            |_| Some(tokens.clone()),
            native_host,
            || Ok(())
        ),
        Ok(true)
    );
    std::fs::remove_file(folder).unwrap();
    std::fs::rename(&moved, folder).unwrap();
    // The same bounded prefix must still refuse an actually unregistered host.
    std::fs::remove_file(folder.join("host.identity.json")).unwrap();
    assert_eq!(
        detect(
            &records,
            &chain,
            |_| Some(tokens.clone()),
            native_host,
            || Ok(())
        ),
        Ok(true)
    );
}
