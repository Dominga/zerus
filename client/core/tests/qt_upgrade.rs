//! Next interim identities survive the Qt descoping; no migration executes them.
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};
use zerus_client_core::{
    settings::Settings,
    task_start,
    tasks_write::{self, outbox},
};
const ORIGINAL: &str = "11111111-1111-4111-8111-111111111111";
const FRESH: &str = "22222222-2222-4222-8222-222222222222";
const TASK: &str = "33333333-3333-4333-8333-333333333333";
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("zerus-qt-upgrade-guards-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn notice(&self) {
        let settings = Settings::open(self.0.join("settings.json"), None);
        settings.acknowledge_qt_upgrade_notice().unwrap();
        assert!(settings.flush_within(Duration::from_secs(5)));
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn next_message_markers_and_service_handoff_keep_the_original_after_upgrade() {
    let s = Scratch::new();
    let own = s.0.join("drafts");
    let intent = tasks_write::message_intent("original unsent text", &[], "comment", "none");
    let saved = outbox::reserve(&own, "project-fixture", TASK, &intent, ORIGINAL)
        .unwrap()
        .record;
    outbox::hand_off(&own, "project-fixture", TASK, &saved).unwrap();
    let file = outbox::path(&own, "project-fixture", TASK);
    let bytes = fs::read(&file).unwrap();
    let marker = fs::read(file.with_extension("marker")).unwrap();
    // An old Qt outbox cannot override the independently named Next original.
    let qt = s.0.join("hgs-tray.conf.drafts");
    outbox::reserve(
        &qt,
        "project-fixture",
        TASK,
        &json!({"text":"Qt-only"}),
        FRESH,
    )
    .unwrap();
    let qt_file = outbox::path(&qt, "project-fixture", TASK);
    let qt_bytes = fs::read(&qt_file).unwrap();
    s.notice();
    let recovering = outbox::reserve(&own, "project-fixture", TASK, &intent, FRESH).unwrap();
    assert!(recovering.recovering);
    assert_eq!(recovering.record, saved);
    assert!(outbox::handed_off(&own, "project-fixture", TASK, &recovering.record).unwrap());
    let call =
        tasks_write::message_call("project-fixture", TASK, &recovering.record, true).unwrap();
    assert!(call.args.iter().any(|arg| arg == "--recover-only"));
    assert!(call.args.iter().any(|arg| arg == ORIGINAL));
    assert!(!call.args.iter().any(|arg| arg == FRESH));
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert_eq!(fs::read(file.with_extension("marker")).unwrap(), marker);
    assert_eq!(fs::read(qt_file).unwrap(), qt_bytes);
    // A lost Next record with its reservation still present refuses, not fresh.
    fs::remove_file(&file).unwrap();
    assert!(outbox::reserve(&own, "project-fixture", TASK, &intent, FRESH).is_err());
}

#[test]
fn next_start_markers_never_reoffer_a_fresh_start_on_restart_or_notice_ack() {
    for phase in ["delivering", "review"] {
        let s = Scratch::new();
        let own = s.0.join("drafts");
        let starts = own.join("task-start");
        let intent = json!({"plan":{"schema":1,"id":ORIGINAL,"project":"project-fixture","task":TASK,"request":{"request_id":ORIGINAL}}});
        let saved = outbox::reserve(&starts, "project-fixture", TASK, &intent, ORIGINAL)
            .unwrap()
            .record;
        let mut record = saved.clone();
        record["schema"] = json!(1);
        record["phase"] = json!(phase);
        assert!(
            outbox::replace_if_unchanged(&starts, "project-fixture", TASK, &saved, &record)
                .unwrap()
        );
        outbox::hand_off(&starts, "project-fixture", TASK, &record).unwrap();
        let file = outbox::path(&starts, "project-fixture", TASK);
        let bytes = fs::read(&file).unwrap();
        s.notice();
        let selection = task_start::Selection {
            host: String::new(),
            name: "fixture-session".into(),
            session_id: "original-session".into(),
            run_id: "original-run".into(),
            conversation_id: "original-conversation".into(),
        };
        let assignment = task_start::Assignment {
            kind: "any".into(),
            machine: String::new(),
            session_id: String::new(),
            name: String::new(),
        };
        let view = task_start::start(
            &own,
            "project-fixture",
            TASK,
            1,
            &assignment,
            &selection,
            FRESH,
            |_| -> Result<serde_json::Map<String, Value>, String> {
                panic!("an unresolved Start must not probe or execute a new plan")
            },
        )
        .unwrap();
        assert!(view.pending);
        assert_eq!(view.id, ORIGINAL);
        assert_eq!(view.phase, "review");
        assert_eq!(fs::read(&file).unwrap(), bytes);
        assert!(outbox::handed_off(&starts, "project-fixture", TASK, &record).unwrap());
        fs::remove_file(&file).unwrap();
        assert!(task_start::open(&own, "project-fixture", TASK, None).is_err());
    }
}
