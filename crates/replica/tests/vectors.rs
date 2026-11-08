//! Golden vectors: documents and updates written by earlier versions of this
//! crate and of Loro. Every build must still read and merge them. Running with
//! `ZERUS_BLESS=1` writes them anew; do that only to add a vector for a new
//! schema or Loro version, never to make a failing build pass.
use std::fs;
use std::path::PathBuf;
use zerus_replica::{
    Actor, NewTask, ProjectDoc, SessionKey, Settings, Stage, Target, TaskDoc, TaskView,
};

const TASK: &str = "task-schema1-loro1.16";

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors")
        .join(name)
}

fn read(name: &str) -> Vec<u8> {
    fs::read(path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn blessing() -> bool {
    std::env::var("ZERUS_BLESS").as_deref() == Ok("1")
}

fn api() -> SessionKey {
    SessionKey {
        machine: "00000000-0000-4000-8000-000000000001".into(),
        session: "codex/api".into(),
        ..Default::default()
    }
}

/// The base document, then two replicas that change it at the same time.
fn bless_task() {
    let mut base = TaskDoc::create(NewTask {
        project: "project-1",
        id: "T-a1b2c3",
        text: "Add rate limiting to POST /v1/orders.",
        stage: Stage::Todo,
        target: Target::Session(api()),
        author: Actor::Agent(api()),
        rank: "V",
        now: 1_760_000_000_000,
    })
    .unwrap();
    base.comment("Use the config value.", &Actor::Person, 1_760_000_001_000)
        .unwrap();
    let snapshot = base.snapshot().unwrap();
    let version = base.version();
    let mut a = TaskDoc::from_bytes(&snapshot).unwrap();
    a.record_send(
        &api(),
        "00000000-0000-4000-8000-000000000002",
        &Actor::Person,
        1_760_000_002_000,
    )
    .unwrap();
    a.record_report(
        "Token bucket in Redis; 6 tests.",
        &Actor::Agent(api()),
        1_760_000_003_000,
    )
    .unwrap();
    let mut b = TaskDoc::from_bytes(&snapshot).unwrap();
    b.comment("Limit per API key.", &Actor::Person, 1_760_000_002_500)
        .unwrap();
    let (update_a, update_b) = (
        a.export_since(&version).unwrap(),
        b.export_since(&version).unwrap(),
    );
    a.import(&update_b).unwrap();
    for (name, bytes) in [
        (format!("{TASK}.loro"), snapshot),
        (format!("{TASK}.version"), version),
        (format!("{TASK}-a.update"), update_a),
        (format!("{TASK}-b.update"), update_b),
    ] {
        fs::write(path(&name), bytes).unwrap();
    }
    let merged = serde_json::to_string_pretty(&a.view().unwrap()).unwrap() + "\n";
    fs::write(path(&format!("{TASK}-merged.json")), merged).unwrap();
}

/// Every value the stored view has, this build's view has too. Later builds
/// may add fields; they never drop or change the ones a vector recorded.
fn contains(actual: &serde_json::Value, expected: &serde_json::Value, path: &str) {
    use serde_json::Value;
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            for (key, value) in expected {
                let found = actual
                    .get(key)
                    .unwrap_or_else(|| panic!("{path}.{key} is missing"));
                contains(found, value, &format!("{path}.{key}"));
            }
        }
        (Value::Array(actual), Value::Array(expected)) => {
            assert_eq!(actual.len(), expected.len(), "{path} length");
            for (index, (found, value)) in actual.iter().zip(expected).enumerate() {
                contains(found, value, &format!("{path}[{index}]"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}

fn merged(first: &str, second: &str) -> TaskView {
    let mut task = TaskDoc::from_bytes(&read(&format!("{TASK}.loro"))).unwrap();
    task.import(&read(&format!("{TASK}-{first}.update")))
        .unwrap();
    task.import(&read(&format!("{TASK}-{second}.update")))
        .unwrap();
    task.view().unwrap()
}

#[test]
fn task_schema1_loro_1_16() {
    if blessing() {
        bless_task();
    }
    let expected: serde_json::Value =
        serde_json::from_slice(&read(&format!("{TASK}-merged.json"))).unwrap();
    // Concurrent updates merge to the same task in either order.
    for (first, second) in [("a", "b"), ("b", "a")] {
        let view = merged(first, second);
        contains(&serde_json::to_value(&view).unwrap(), &expected, "view");
    }
    // The history reads oldest first, whatever order the merge gave its entries.
    let times: Vec<i64> = merged("a", "b").log.iter().map(|e| e.at).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "{times:?}");
    // An export since the stored version carries both updates to a fresh copy.
    let mut both = TaskDoc::from_bytes(&read(&format!("{TASK}.loro"))).unwrap();
    both.import(&read(&format!("{TASK}-a.update"))).unwrap();
    both.import(&read(&format!("{TASK}-b.update"))).unwrap();
    let delta = both
        .export_since(&read(&format!("{TASK}.version")))
        .unwrap();
    let mut fresh = TaskDoc::from_bytes(&read(&format!("{TASK}.loro"))).unwrap();
    fresh.import(&delta).unwrap();
    contains(
        &serde_json::to_value(fresh.view().unwrap()).unwrap(),
        &expected,
        "view",
    );
    // The stored document still accepts this build's edits.
    both.set_stage(Stage::Done, &Actor::Person, 1_760_000_004_000)
        .unwrap();
    assert_eq!(both.view().unwrap().stage, Stage::Done);
}

#[test]
fn project_schema1_loro_1_16() {
    let (bytes, expected) = (
        "project-schema1-loro1.16.loro",
        "project-schema1-loro1.16.json",
    );
    if blessing() {
        let mut project = ProjectDoc::new();
        project
            .set_settings(&Settings {
                agents_other: false,
                agent_limit: 3,
                ..Settings::default()
            })
            .unwrap();
        fs::write(path(bytes), project.snapshot().unwrap()).unwrap();
        let settings = serde_json::to_string_pretty(&project.settings()).unwrap() + "\n";
        fs::write(path(expected), settings).unwrap();
    }
    let project = ProjectDoc::from_bytes(&read(bytes)).unwrap();
    let expected: Settings = serde_json::from_slice(&read(expected)).unwrap();
    assert_eq!(project.settings(), expected);
}

const ATTEMPTS: &str = "task-schema6-loro1.16";

/// A send reserved before its input, then two replicas at the same time: one
/// settles it with its receipt and its executor starts and reports progress,
/// the other adds a comment. Written only when missing.
fn bless_attempts() {
    use zerus_replica::execution::{NativeRequest, Origin, Receipt};
    let executor = SessionKey {
        session_id: "launch_id:00000000-0000-4000-8000-000000000003".into(),
        ..api()
    };
    let id = "00000000-0000-4000-8000-000000000004";
    let mut base = TaskDoc::create(NewTask {
        project: "project-1",
        id: "T-d4e5f6",
        text: "Add rate limiting to POST /v1/orders.",
        stage: Stage::Todo,
        target: Target::Session(executor.clone()),
        author: Actor::Person,
        rank: "V",
        now: 1_790_000_000_000,
    })
    .unwrap();
    let native = NativeRequest {
        machine: executor.machine.clone(),
        name: executor.session.clone(),
        run_id: "00000000-0000-4000-8000-000000000005".into(),
        conversation_id: "00000000-0000-4000-8000-000000000006".into(),
        digest: "0f".repeat(32),
    };
    base.reserve_attempt(
        id,
        Origin::Send,
        &executor,
        native.clone(),
        &Actor::Person,
        1_790_000_001_000,
    )
    .unwrap();
    let snapshot = base.snapshot().unwrap();
    let version = base.version();
    let mut a = TaskDoc::from_bytes(&snapshot).unwrap();
    let receipt = Receipt {
        request_id: id.into(),
        name: native.name,
        run_id: native.run_id,
        conversation_id: native.conversation_id,
        first_message: false,
        digest: native.digest,
        submitted: true,
    };
    a.settle_attempt(
        id,
        zerus_replica::Settlement::Receipt(&receipt),
        &Actor::Person,
        1_790_000_002_000,
    )
    .unwrap();
    a.record_start(&Actor::Agent(executor.clone()), 1_790_000_003_000)
        .unwrap();
    a.report(
        "Token bucket in place; tests next.",
        zerus_replica::execution::ReportKind::Progress,
        Some(1),
        &Actor::Agent(executor),
        1_790_000_004_000,
    )
    .unwrap();
    let mut b = TaskDoc::from_bytes(&snapshot).unwrap();
    b.comment("Limit per API key.", &Actor::Person, 1_790_000_002_500)
        .unwrap();
    let (update_a, update_b) = (
        a.export_since(&version).unwrap(),
        b.export_since(&version).unwrap(),
    );
    a.import(&update_b).unwrap();
    for (name, bytes) in [
        (format!("{ATTEMPTS}.loro"), snapshot),
        (format!("{ATTEMPTS}.version"), version),
        (format!("{ATTEMPTS}-a.update"), update_a),
        (format!("{ATTEMPTS}-b.update"), update_b),
    ] {
        fs::write(path(&name), bytes).unwrap();
    }
    let merged = serde_json::to_string_pretty(&a.view().unwrap()).unwrap() + "\n";
    fs::write(path(&format!("{ATTEMPTS}-merged.json")), merged).unwrap();
}

#[test]
fn task_schema6_attempts_loro_1_16() {
    if blessing() && !path(&format!("{ATTEMPTS}.loro")).exists() {
        bless_attempts();
    }
    let expected: serde_json::Value =
        serde_json::from_slice(&read(&format!("{ATTEMPTS}-merged.json"))).unwrap();
    let merged = |first: &str, second: &str| {
        let mut task = TaskDoc::from_bytes(&read(&format!("{ATTEMPTS}.loro"))).unwrap();
        task.import(&read(&format!("{ATTEMPTS}-{first}.update")))
            .unwrap();
        task.import(&read(&format!("{ATTEMPTS}-{second}.update")))
            .unwrap();
        task
    };
    // Concurrent updates merge to the same task and execution in either order.
    for (first, second) in [("a", "b"), ("b", "a")] {
        let view = merged(first, second).view().unwrap();
        contains(&serde_json::to_value(&view).unwrap(), &expected, "view");
        let current = view.execution.unwrap().current.unwrap();
        assert_eq!(
            (view.schema, view.writable, view.stage),
            (zerus_replica::TASK_SCHEMA, true, Stage::Active)
        );
        assert_eq!(
            serde_json::to_value(current.phase).unwrap(),
            serde_json::json!("running")
        );
    }
    // The stored base alone is a reservation: its delivery is unknown.
    let base = TaskDoc::from_bytes(&read(&format!("{ATTEMPTS}.loro"))).unwrap();
    assert!(base.held().unwrap().is_some());
    // The stored document still accepts this build's edits under its rules.
    let mut task = merged("a", "b");
    let executor = task.view().unwrap().session.unwrap();
    assert!(
        task.report_attempt(
            "00000000-0000-4000-8000-000000000004",
            "Done.",
            zerus_replica::execution::ReportKind::Ready,
            None,
            &Actor::Agent(executor),
            1_790_000_005_000,
        )
        .unwrap()
        .moves
    );
    assert_eq!(task.view().unwrap().stage, Stage::Review);
}
