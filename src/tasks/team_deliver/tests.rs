use super::*;
use crate::teams::ops::{
    self,
    tests::{person, Harness, TEAM},
};

fn setup() -> (Harness, Input) {
    let mut h = Harness::new();
    h.bind("lead");
    h.bind("m1");
    let node = h.add(&person(), 1);
    let id = h.intent(node, "m1");
    (
        h,
        Input {
            schema: 1,
            id,
            team: TEAM.into(),
            node,
            member: "m1".into(),
        },
    )
}

#[test]
fn exact_stored_effect_is_the_authority_without_a_caller() {
    let (h, body) = setup();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let before = h.store.team(TEAM).unwrap().unwrap().revision;
    let bound = permit(&h.store, &body, &home).unwrap();
    assert_eq!(bound.kind, "handout");
    assert_eq!(bound.task.uuid, ops::tests::task(1).uuid);
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, before);
}

#[test]
fn every_effect_binding_refusal_is_read_only() {
    for change in 0..13 {
        let (h, mut body) = setup();
        let mut home = h.store.team(TEAM).unwrap().unwrap().home;
        let db = h.store.db.as_ref().unwrap();
        match change {
            0 => body.id = uuid::Uuid::new_v4().to_string(),
            1 => body.team = uuid::Uuid::new_v4().to_string(),
            2 => body.node += 1,
            3 => body.member = "m2".into(),
            4 => {
                db.execute("UPDATE effects SET kind='other'", []).unwrap();
            }
            5 => {
                db.execute("UPDATE effects SET generation=generation+1", [])
                    .unwrap();
            }
            6 => {
                db.execute("UPDATE effects SET state='closed'", []).unwrap();
            }
            7 => home = uuid::Uuid::new_v4().to_string(),
            8 => {
                db.execute("UPDATE teams SET status='\"paused\"'", [])
                    .unwrap();
            }
            9 => {
                db.execute(
                    "UPDATE members SET data=json_remove(data,'$.session') WHERE id='m1'",
                    [],
                )
                .unwrap();
            }
            10 => {
                db.execute(
                    "UPDATE members SET data=json_set(data,'$.state','paused') WHERE id='m1'",
                    [],
                )
                .unwrap();
            }
            11 => {
                db.execute(
                    "UPDATE members SET data=json_remove(data,'$.session') WHERE id='lead'",
                    [],
                )
                .unwrap();
            }
            12 => {
                db.execute(
                    "UPDATE node_identity SET task_uuid='00000000-0000-4000-8000-000000000099'",
                    [],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = h.store.team(TEAM).unwrap().unwrap().revision;
        assert!(permit(&h.store, &body, &home).is_err(), "mutation {change}");
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, before);
    }
}

#[test]
fn request_rejects_unknown_fields_and_invalid_identity() {
    let (_, body) = setup();
    let mut value = serde_json::to_value(body).unwrap();
    value["actor"] = serde_json::json!({"kind":"service"});
    assert!(serde_json::from_value::<Input>(value).is_err());
}

#[cfg(unix)]
pub(super) mod processes;

#[test]
fn a_member_effect_cannot_deliver_to_the_lead() {
    let (h, mut body) = setup();
    body.member = "lead".into();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE effects SET member='lead' WHERE team=?1 AND request_id=?2",
            rusqlite::params![TEAM, body.id],
        )
        .unwrap();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    assert!(permit(&h.store, &body, &home).is_err());
}

/// Runs one ignored fixture of this module in its own process, with a private
/// state and configuration.
#[cfg(unix)]
fn isolated(fixture: &str) {
    let temp = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("tasks::team_deliver::tests::{fixture}"),
            "--ignored",
            "--test-threads=1",
        ])
        .env("HGS_STATE_DIR", temp.path().join("state"))
        .env("HGS_CONFIG_DIR", temp.path().join("config"))
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && text.contains("1 passed"),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// zerus-63rru.64: how a reopen's preparation binds the member's session to
/// the task's original dispatch. The general route (`SameRun`) keeps the
/// dispatch's run; the team route (`SameConversation`) continues a member
/// resumed into the same conversation, refuses a changed conversation with its
/// own sentence, and still needs the run when the receipt has no
/// conversation. The record is saved by the state's own writer.
#[cfg(unix)]
#[test]
fn the_team_route_continues_a_resumed_conversation_and_the_general_route_keeps_its_run() {
    isolated("continuation_fixture");
}

#[cfg(unix)]
#[test]
#[ignore = "run by its isolated wrapper"]
fn continuation_fixture() {
    use crate::state::Continuation::{SameConversation, SameRun};
    let state = std::path::PathBuf::from(std::env::var("HGS_STATE_DIR").unwrap());
    let launch = uuid::Uuid::new_v4().to_string();
    let conversation = uuid::Uuid::new_v4().to_string();
    let save = |run: &str, conversation: &str| {
        let mut record = json!({"name":"codex/example/member","agent":"codex","version":1,
            "run_id":run,"conversation_id":conversation,"launch_id":launch,"cwd":state,
            "created":1.0});
        let _ = crate::state::inspection_for_test(&mut record);
    };
    let to = SessionKey {
        machine: "00000000-0000-4000-8000-0000000000aa".into(),
        session: "codex/example/member".into(),
        session_id: format!("launch_id:{launch}"),
    };
    let dispatch = uuid::Uuid::new_v4().to_string();
    crate::platform::private_dir(&state.join("input_receipts")).unwrap();
    let receipt = |conversation: &str| {
        std::fs::write(
            state.join(format!("input_receipts/{dispatch}.json")),
            json!({"request_id":dispatch,"name":"codex/example/member","run_id":"run-1",
                "conversation_id":conversation,"status":"submitted"})
            .to_string(),
        )
        .unwrap();
    };
    let prepare = |mode| {
        let id = uuid::Uuid::new_v4().to_string();
        crate::state::prepare_task_reply(&to, &dispatch, &id, "Continue.", &[], mode)
    };
    let changed = "the task's dispatched run or conversation changed; refresh or hand off the task explicitly";
    // The dispatched run and conversation: both routes continue.
    save("run-1", &conversation);
    receipt(&conversation);
    assert!(prepare(SameRun).is_ok() && prepare(SameConversation).is_ok());
    // Resumed: a new run, the same launch and conversation.
    save("run-2", &conversation);
    assert_eq!(prepare(SameRun).unwrap_err(), changed);
    assert_eq!(
        prepare(SameConversation).unwrap()["expected_run_id"],
        "run-2"
    );
    // Cleared: a new conversation.
    save("run-2", &uuid::Uuid::new_v4().to_string());
    assert_eq!(prepare(SameRun).unwrap_err(), changed);
    assert_eq!(
        prepare(SameConversation).unwrap_err(),
        crate::state::CONVERSATION_GONE
    );
    // A receipt without a conversation binds only its run.
    receipt("");
    assert_eq!(prepare(SameConversation).unwrap_err(), changed);
    save("run-1", &conversation);
    assert!(prepare(SameConversation).is_ok());
}

/// `path` below `root`, its components joined with `/` on every platform:
/// `Path::display` would join them with `\` on Windows (zerus-63rru.114).
fn relative(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .components()
        .map(|part| part.as_os_str().to_str().unwrap())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(windows)]
#[test]
fn source_paths_are_named_with_slashes_on_windows() {
    let root = std::path::Path::new(r"C:\checkout\src");
    let path = root.join("tasks").join("team_deliver").join("tests.rs");
    assert!(path.display().to_string().contains('\\'));
    assert_eq!(relative(&path, root), "tasks/team_deliver/tests.rs");
}

/// zerus-63rru.64: only the team route reaches the comparison by
/// conversation. Every use of `Continuation::SameConversation` in the source
/// is in the comparison itself or in team-deliver; every other caller of the
/// preparation passes `SameRun`.
#[test]
fn only_the_team_route_reaches_the_conversation_comparison() {
    fn walk(dir: &std::path::Path, root: &std::path::Path, found: &mut Vec<(String, usize)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                let uses = text.matches("Continuation::SameConversation").count();
                if uses > 0 {
                    found.push((relative(&path, root), uses));
                }
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    walk(&root, &root, &mut found);
    found.sort();
    let files: Vec<&str> = found.iter().map(|(file, _)| file.as_str()).collect();
    assert_eq!(
        files,
        [
            "state/input.rs",
            "state/mod.rs",
            "tasks/team_deliver.rs",
            "tasks/team_deliver/tests.rs"
        ],
        "{found:?}"
    );
    assert_eq!(found.iter().find(|(file,_)| file == "tasks/team_deliver.rs").unwrap().1, 4,
        "team-deliver's two base renders and two continuation preparations");
    // A cancel notice has no dispatch to compare; it selects only the
    // hosted preparation branch and cannot continue a task's old dispatch.
    assert!(include_str!("../../state/mod.rs").contains("input::task_reply(session, \"\", &uuid::Uuid::new_v4().to_string()"));
}

#[test]
fn the_first_open_is_bounded_well_below_the_actors_child_deadline() {
    let now = Instant::now();
    let open_by = first_open_by(now + attempts::CHILD_DEADLINE);
    assert!(attempts::STORE_WAIT < attempts::CHILD_DEADLINE);
    assert!(
        open_by <= now + attempts::STORE_WAIT + std::time::Duration::from_millis(100),
        "the first open waited past its bound"
    );
}

#[test]
fn the_busy_store_answer_is_not_started_with_its_typed_cause() {
    let (_h, body) = setup();
    let reply = store_busy_reply(&body);
    assert_eq!(reply["state"], "not_started");
    assert_eq!(reply["cause"], TASK_STORE_BUSY);
    assert_eq!(
        reply["error"],
        "The task store was busy; nothing was reserved or sent."
    );
}

#[test]
fn a_busy_store_code_is_never_a_refusal() {
    assert!(!refusal(&Error::new(STORE_BUSY_CODE, "busy")));
    assert!(refusal(&Error::new(RULE, "rule")));
    assert!(refusal(&Error::new(PROJECT_LIMIT_CODE, "limit")));
}
