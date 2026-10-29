//! New session binding uses the same bounded listing as Qt's
//! HgsClient.cpp:184, 279 and SessionsWindow.cpp:3020-3070.
use serde_json::json;
use std::time::Duration;
use zerus_client_core::creation::{binding_invocation, binding_listing};
use zerus_client_core::{ComputerId, Exit};

fn reply(value: serde_json::Value) -> Exit {
    Exit::Code {
        code: 0,
        stdout: serde_json::to_vec(&value).unwrap(),
        stderr: String::new(),
    }
}

#[test]
fn binding_reads_only_live_rows_with_qts_arguments_and_timeout() {
    let local = binding_invocation(&ComputerId::local());
    assert_eq!(local.args, ["ls", "--json", "--local", "--no-archives"]);
    let peer = binding_invocation(&ComputerId::from("studio"));
    assert_eq!(peer.args, ["@studio", "ls", "--json", "--no-archives"]);
    for invocation in [local, peer] {
        assert_eq!(invocation.timeout, Duration::from_secs(15));
        assert!(invocation.stdin.is_none() && invocation.request_id.is_none());
    }
}

#[test]
fn bindings_keep_only_the_fields_needed_to_bind_and_old_peers_can_lack_launches() {
    let listing = binding_listing(reply(json!({"ok": true, "sessions": [
        {"launch_id": "launch", "cmd": "codex", "name": "codex/project/tag", "prompt": "ignored"},
        {"launch_id": "stopped", "cmd": "sh", "name": "sh/project/tag", "state": "stopped"},
        {"name": "sh/project/old"}, null
    ]})))
    .unwrap();
    assert!(listing.online);
    assert_eq!(listing.sessions.len(), 3);
    assert_eq!(listing.sessions[0].launch_id, "launch");
    assert_eq!(listing.sessions[0].cmd, "codex");
    assert_eq!(listing.sessions[0].name, "codex/project/tag");
    assert_eq!(listing.sessions[0].state, "running");
    assert_eq!(listing.sessions[1].state, "stopped");
    assert!(listing.sessions[2].launch_id.is_empty());
    assert!(listing.ended_launches.is_empty());
    assert!(
        !binding_listing(reply(json!({"sessions": []})))
            .unwrap()
            .online
    );
}

#[test]
fn ended_launches_are_bounded_and_keep_nullable_exit_codes() {
    let ended: Vec<_> = (0..33).map(|n| json!({
        "launch_id": format!("launch-{n}"), "name": "sh/project/tag", "code": if n == 0 {json!(null)} else {json!(7)},
        "output": "retained output", "ignored": true
    })).collect();
    let listing = binding_listing(reply(
        json!({"ok": true, "sessions": [], "ended_launches": ended}),
    ))
    .unwrap();
    assert_eq!(listing.ended_launches.len(), 32);
    assert_eq!(listing.ended_launches[0].launch_id, "launch-0");
    assert_eq!(listing.ended_launches[0].code, None);
    assert_eq!(listing.ended_launches[31].code, Some(7));
    assert_eq!(listing.ended_launches[31].output, "retained output");
}

#[test]
fn a_failed_malformed_or_oversized_listing_never_supplies_binding_evidence() {
    for stdout in [
        b"not JSON".to_vec(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        serde_json::to_vec(
            &json!({"sessions": [], "ignored": " ".repeat(zerus_client_core::reads::READ_LIMIT)}),
        )
        .unwrap(),
    ] {
        assert!(binding_listing(Exit::Code {
            code: 0,
            stdout,
            stderr: String::new()
        })
        .is_err());
    }
    assert!(binding_listing(Exit::Code {
        code: 1,
        stdout: b"{\"sessions\":[]}".to_vec(),
        stderr: "offline".into()
    })
    .is_err());
}
