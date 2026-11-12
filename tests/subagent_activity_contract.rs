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
    answered_with_archive(root, target, agent, None)
}
fn answered_with_archive(root: &Path, target: &Target, agent: &str, archive: Option<&str>) -> Exit {
    let call = subagent_identity_invocation(target, archive, agent).unwrap();
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

const CODEX_CHILD: &str = "33333333-3333-4333-8333-333333333333";
const ARCHIVE_ID: &str = "44444444-4444-4444-8444-444444444444";

fn isolated_root() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for folder in ["state", "config", "tmp", "tmux"] {
        std::fs::create_dir_all(root.join(folder)).unwrap();
        std::fs::set_permissions(root.join(folder), std::fs::Permissions::from_mode(0o700))
            .unwrap();
    }
    dir
}
fn codex_target() -> Target {
    Target {
        computer: String::new(),
        name: NAME.into(),
        run_id: RUN.into(),
        conversation_id: CONVERSATION.into(),
    }
}
fn codex_record(detail: Option<&str>) -> Value {
    json!({
        "version": 1, "name": NAME, "agent": "codex", "run_id": RUN, "conversation_id": CONVERSATION,
        "activity": "idle", "phase": "idle", "active_tools": {},
        "subagents": {CODEX_CHILD: {"state": "working", "name": "Explore", "model": "fixture-model", "updated": 1.0,
            "detail": detail.unwrap_or("")}},
    })
}
fn write_codex_session(root: &Path, detail: Option<&str>) {
    use sha2::{Digest, Sha256};
    std::fs::write(
        root.join("state")
            .join(format!("{:x}.json", Sha256::digest(NAME.as_bytes()))),
        codex_record(detail).to_string(),
    )
    .unwrap();
}
fn write_hooks(root: &Path, count: usize) {
    let db = rusqlite::Connection::open(root.join("state/events.sqlite3")).unwrap();
    db.execute_batch(
        "CREATE TABLE events(seq INTEGER PRIMARY KEY,name TEXT,conversation TEXT,payload TEXT);
         CREATE INDEX events_session ON events(name,conversation,seq)",
    )
    .unwrap();
    for at in 1..=count {
        let event = json!({"type":"PreToolUse","agent_id":CODEX_CHILD,"at":at,"detail":format!("hook {at}")});
        db.execute(
            "INSERT INTO events(name,conversation,payload) VALUES(?,?,?)",
            rusqlite::params![NAME, CONVERSATION, event.to_string()],
        )
        .unwrap();
    }
}
fn write_native(root: &Path, count: usize, start: usize) {
    let home = root.join(".codex");
    std::fs::create_dir_all(&home).unwrap();
    let rollout = home.join("rollout.jsonl");
    let db = rusqlite::Connection::open(home.join("state_5.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE threads(id TEXT PRIMARY KEY,rollout_path TEXT,cwd TEXT)")
        .unwrap();
    db.execute(
        "INSERT INTO threads(id,rollout_path,cwd) VALUES(?,?,?)",
        rusqlite::params![CODEX_CHILD, rollout.to_string_lossy(), "/srv/example/work"],
    )
    .unwrap();
    let mut text = String::new();
    text.push_str(&json!({"type":"session_meta","payload":{"id":CODEX_CHILD}}).to_string());
    text.push('\n');
    for at in start..start + count {
        let event = json!({"type":"event_msg","payload":{
            "type":"item_completed","completed_at_ms":at * 1000,
            "item":{"type":"AgentMessage","id":format!("native-{at}"),"phase":"commentary",
                "content":[{"type":"output_text","text":format!("native {at}")}]}
        }});
        text.push_str(&event.to_string());
        text.push('\n');
    }
    std::fs::write(rollout, text).unwrap();
}
fn codex_fixture(root: &Path, hooks: usize, native: usize) {
    codex_fixture_with_start(root, hooks, native, 1);
}
fn codex_fixture_with_start(root: &Path, hooks: usize, native: usize, start: usize) {
    write_codex_session(root, None);
    write_hooks(root, hooks);
    write_native(root, native, start);
}

#[test]
fn combined_codex_history_is_one_bounded_latest_page() {
    let dir = isolated_root();
    codex_fixture_with_start(dir.path(), 150, 150, 101);
    let target = codex_target();
    let exit = answered(dir.path(), &target, CODEX_CHILD);
    let Exit::Code { code, stderr, .. } = &exit else {
        panic!("hgs did not run: {exit:?}")
    };
    assert_eq!(*code, 0, "the CLI refused the synthetic call: {stderr}");
    let page = parse_subagent_activity(&target, CODEX_CHILD, exit).expect("combined Activity page");
    let events = page["events"].as_array().unwrap();
    assert_eq!(events.len(), 200);
    assert_eq!(page["history_truncated"], true);
    assert_eq!(events.first().unwrap()["at"], 101);
    assert_eq!(events.last().unwrap()["at"], 250.0, "{page}");
    assert_eq!(events.first().unwrap()["detail"], "hook 101");
    assert_eq!(events.last().unwrap()["detail"], "native 250");
    let hook = events
        .iter()
        .position(|event| event["at"].as_f64() == Some(101.0) && event["detail"] == "hook 101")
        .unwrap();
    let native = events
        .iter()
        .position(|event| event["at"].as_f64() == Some(101.0) && event["detail"] == "native 101")
        .unwrap();
    assert!(
        hook < native,
        "equal timestamps preserve hook then native order"
    );
    assert_eq!(page["parent_conversation_id"], CONVERSATION);
    assert_eq!(page["run_id"], RUN);
    assert_eq!(page["agent_id"], CODEX_CHILD);
    assert!(
        parse_subagent_activity(&target, "other", answered(dir.path(), &target, "other")).is_none()
    );
}

#[test]
fn exactly_two_hundred_and_native_truncation_have_distinct_truth() {
    let dir = isolated_root();
    codex_fixture(dir.path(), 100, 100);
    let target = codex_target();
    let page = parse_subagent_activity(
        &target,
        CODEX_CHILD,
        answered(dir.path(), &target, CODEX_CHILD),
    )
    .expect("exactly 200 events");
    assert_eq!(page["events"].as_array().unwrap().len(), 200);
    assert!(page.get("history_truncated").is_none());

    let dir = isolated_root();
    codex_fixture(dir.path(), 0, 201);
    let page = parse_subagent_activity(
        &target,
        CODEX_CHILD,
        answered(dir.path(), &target, CODEX_CHILD),
    )
    .expect("native source truncation");
    let events = page["events"].as_array().unwrap();
    assert_eq!(events.len(), 200);
    assert_eq!(page["history_truncated"], true);
    assert_eq!(events.first().unwrap()["at"], 2.0);
    assert_eq!(events.last().unwrap()["at"], 201.0);
}

#[test]
fn recorded_only_summary_external_and_archive_paths_stay_intact() {
    let dir = isolated_root();
    write_codex_session(dir.path(), None);
    write_hooks(dir.path(), 2);
    let target = codex_target();
    let page = parse_subagent_activity(
        &target,
        CODEX_CHILD,
        answered(dir.path(), &target, CODEX_CHILD),
    )
    .expect("recorded-only history");
    assert_eq!(page["events"].as_array().unwrap().len(), 2);
    assert_eq!(
        page["history_scope"],
        "Recorded events; native subagent history is unavailable"
    );
    assert!(page.get("history_truncated").is_none());

    let dir = isolated_root();
    write_codex_session(dir.path(), Some("synthetic summary"));
    let page = parse_subagent_activity(
        &target,
        CODEX_CHILD,
        answered(dir.path(), &target, CODEX_CHILD),
    )
    .expect("summary-only history");
    assert_eq!(page["events"].as_array().unwrap().len(), 1);
    assert_eq!(page["events"][0]["source"], "subagent_summary");
    assert_eq!(
        page["history_scope"],
        "Only the last reported subagent result is available"
    );

    let dir = isolated_root();
    let mut record = codex_record(None);
    record["subagents"][CODEX_CHILD] = json!({
        "state": "working", "name": "External", "model": "fixture-model", "updated": 1.0,
        "external": true, "provider": "codex", "conversation_id": "55555555-5555-4555-8555-555555555555",
        "transcript": "/srv/example/external.jsonl", "cwd": "/srv/example/work",
        "agent_home": dir.path().join(".codex"),
    });
    use sha2::{Digest, Sha256};
    std::fs::write(
        dir.path()
            .join("state")
            .join(format!("{:x}.json", Sha256::digest(NAME.as_bytes()))),
        record.to_string(),
    )
    .unwrap();
    write_hooks(dir.path(), 2);
    let page = parse_subagent_activity(
        &target,
        CODEX_CHILD,
        answered(dir.path(), &target, CODEX_CHILD),
    )
    .expect("external recorded fallback");
    assert_eq!(page["events"].as_array().unwrap().len(), 2);
    assert_eq!(page["history_scope"], "Recorded subagent activity");

    let dir = isolated_root();
    codex_fixture(dir.path(), 2, 2);
    let mut archive = codex_record(None);
    archive["archive_id"] = json!(ARCHIVE_ID);
    archive["archived_at"] = json!(1.0);
    std::fs::create_dir_all(dir.path().join("state/archives")).unwrap();
    std::fs::write(
        dir.path()
            .join("state/archives")
            .join(format!("{ARCHIVE_ID}.json")),
        archive.to_string(),
    )
    .unwrap();
    let page = parse_subagent_activity(
        &target,
        CODEX_CHILD,
        answered_with_archive(dir.path(), &target, CODEX_CHILD, Some(ARCHIVE_ID)),
    )
    .expect("archived Activity page");
    assert_eq!(page["archive_id"], ARCHIVE_ID);
    assert_eq!(page["state"], "archived");
    assert_eq!(page["events"].as_array().unwrap().len(), 4);
    assert_eq!(page["parent_conversation_id"], CONVERSATION);
    assert_eq!(page["run_id"], RUN);
}
