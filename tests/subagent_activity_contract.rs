//! zerus-mnck6: the desktop's child Activity read, end to end. The client
//! core builds the exact `hgs inspect` call, the real CLI answers it from an
//! isolated state, and the core's parsers adopt that answer for the target
//! the web UI asked for. `client/src/store/subagentActivity.test.ts` feeds
//! the pinned bridged answer to the store, so a flag or field one side
//! renames fails a test on one side or the other.
#![cfg(unix)]
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use zerus_client_core::actions::{Exit, Target};
use zerus_client_core::details::{
    parse_subagent_activity, parse_subagent_identity, subagent_identity_invocation,
};

const NAME: &str = "claude/example/review";
const RUN: &str = "11111111-1111-4111-8111-111111111111";
const CONVERSATION: &str = "22222222-2222-4222-8222-222222222222";
const CHILD: &str = "child-1";

/// A session record with one recorded subagent, as the hooks keep it.
fn session(state: &Path) {
    use sha2::{Digest, Sha256};
    let record = json!({
        "version": 1, "name": NAME, "agent": "claude", "run_id": RUN, "conversation_id": CONVERSATION,
        "activity": "idle", "phase": "idle", "active_tools": {},
        "subagents": {CHILD: {"state": "working", "name": "Explore", "model": "fixture-model", "updated": 1.0}},
    });
    let path = state.join(format!("{:x}.json", Sha256::digest(NAME.as_bytes())));
    std::fs::write(path, record.to_string()).unwrap();
}

/// The core's invocation, run by the real CLI against `root`.
fn answered(root: &Path, target: &Target, agent: &str) -> Exit {
    let call = subagent_identity_invocation(target, None, agent).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_hgs"))
        .args(&call.args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("HOME", root)
        .env("HGS_CONFIG_DIR", root.join("config"))
        .env("HGS_STATE_DIR", root.join("state"))
        .env("HGS_SELF", "example")
        .env("HGS_PEERS", "")
        .env("TMPDIR", root.join("tmp"))
        .env("TMUX_TMPDIR", root.join("tmux"))
        .output()
        .unwrap();
    Exit::Code {
        code: output.status.code().unwrap_or(-1),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn fixture_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("client/core/tests/fixtures/subagent-activity.json")
}

#[test]
fn the_core_s_child_activity_call_is_answered_for_its_own_target() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for folder in ["state", "config", "tmp", "tmux"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::set_permissions(root.join(folder), std::fs::Permissions::from_mode(0o700))
            .unwrap();
    }
    session(&root.join("state"));
    // The target the web UI builds from the selected row and the open child.
    let target = Target {
        computer: String::new(),
        name: NAME.into(),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
    };
    let exit = answered(root, &target, CHILD);
    let Exit::Code { code, stderr, .. } = &exit else {
        panic!("hgs did not run: {exit:?}")
    };
    assert_eq!(*code, 0, "the CLI refused the core's call: {stderr}");
    let identity =
        parse_subagent_identity(&target, CHILD, exit.clone()).expect("the child's identity");
    assert_eq!(identity.conversation_id, format!("{CONVERSATION}/{CHILD}"));
    let page = parse_subagent_activity(&target, CHILD, exit).expect("the child's Activity page");
    // The web UI's own check (client/src/store/subagentActivity.ts).
    assert_eq!(page["agent_id"], CHILD);
    assert_eq!(page["parent_conversation_id"], CONVERSATION);
    assert_eq!(page["run_id"], RUN);
    assert!(page["events"].is_array());
    // A child the parent's roster does not name is refused, never adopted.
    assert!(parse_subagent_activity(&target, "other", answered(root, &target, "other")).is_none());
    // The bridged answer and the target the web UI sends, pinned for its test.
    let pinned = json!({
        "target": {"id": format!("\nrun:{RUN}"), "computer": "", "name": NAME, "runId": RUN,
            "conversationId": CONVERSATION, "archiveId": null, "agentId": CHILD},
        "answer": page,
    });
    let current = serde_json::to_string_pretty(&pinned).unwrap() + "\n";
    if std::env::var_os("ZERUS_UPDATE_FIXTURES").is_some() {
        std::fs::write(fixture_file(), &current).unwrap();
    }
    let fixture: Value =
        serde_json::from_str(&std::fs::read_to_string(fixture_file()).unwrap()).unwrap();
    assert_eq!(
        fixture, pinned,
        "update with ZERUS_UPDATE_FIXTURES=1 and rerun the web UI test"
    );
}
