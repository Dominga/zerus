use super::*;
use crate::mobile::{connector_program, packaged_connector, workspace};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const NODE: &str = "node-a";
const GENERATION: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const LATER: &str = "ffffffffffffffffffffffffffffffff";

fn fields(extra: Value) -> Map<String, Value> {
    let mut value = json!({
        "state": "active",
        "encrypted": true,
        "config_revision": "a".repeat(64),
        "server_url": "https://relay.example.invalid",
        "node_id": NODE,
        "running": false,
        "connector_state": "stopped",
        "generation": null,
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    value.as_object().unwrap().clone()
}

fn stopped() -> Workspace {
    workspace(&fields(json!({})))
}

fn running_as(generation: &str) -> Workspace {
    workspace(&fields(json!({
        "running": true,
        "connector_state": "running",
        "generation": generation,
    })))
}

fn running() -> Workspace {
    running_as(GENERATION)
}

fn stopping() -> Workspace {
    workspace(&fields(json!({
        "running": true,
        "connector_state": "stopping",
        "generation": GENERATION,
    })))
}

fn other_node(connector: &str) -> Workspace {
    let running = connector != "stopped";
    workspace(&fields(json!({
        "node_id": "node-b",
        "running": running,
        "connector_state": connector,
        "generation": running.then_some(LATER),
    })))
}

fn armed() -> State {
    State::load(Some(br#"{"version":1,"node_id":"node-a","armed":true}"#))
}

fn stopped_by_person() -> State {
    let mut state = armed();
    state.stop_chosen(NODE, Some(GENERATION));
    state.take_unsaved().unwrap();
    state
}

fn saved(state: &mut State) -> Option<Value> {
    state
        .take_unsaved()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
}

#[test]
fn a_workspace_never_started_here_is_not_started_after_an_upgrade() {
    let state = State::load(None);
    assert!(state.target(&stopped()).is_none());
    assert_eq!(state.line(&stopped()), Some(Line::NotArmed));
}

#[test]
fn unreadable_or_malformed_arm_state_counts_as_unarmed() {
    let long = format!(
        r#"{{"version":1,"node_id":"{}","armed":true}}"#,
        "n".repeat(129)
    );
    let oversized = format!(
        r#"{{"version":1,"node_id":"node-a","armed":true,"x":"{}"}}"#,
        "x".repeat(MAX_RECORD)
    );
    let cases: &[&[u8]] = &[
        b"",
        b"on:node-a",
        b"[]",
        br#"{"version":2,"node_id":"node-a","armed":true}"#,
        br#"{"version":1,"node_id":"","armed":true}"#,
        br#"{"version":1,"armed":true}"#,
        br#"{"version":1,"node_id":"node-a"}"#,
        br#"{"version":1,"node_id":"node-a","armed":"yes"}"#,
        br#"{"version":1,"node_id":"node-a","armed":true,"extra":1}"#,
        br#"{"version":1,"node_id":"node-a","armed":true,"stopped_generation":"e"}"#,
        long.as_bytes(),
        oversized.as_bytes(),
    ];
    for bytes in cases {
        let state = State::load(Some(bytes));
        assert!(
            state.target(&stopped()).is_none(),
            "started from {:?}",
            String::from_utf8_lossy(bytes)
        );
        assert_eq!(state.line(&stopped()), Some(Line::NotArmed));
    }
}

#[test]
fn what_is_saved_loads_back_as_the_same_choice() {
    let mut state = State::load(None);
    state.observe(&running());
    let on = state.take_unsaved().unwrap();
    assert!(State::load(Some(&on)).target(&stopped()).is_some());

    state.stop_chosen(NODE, Some(GENERATION));
    let off = state.take_unsaved().unwrap();
    let mut loaded = State::load(Some(&off));
    assert!(loaded.target(&stopped()).is_none());
    assert_eq!(loaded.line(&stopped()), Some(Line::StoppedByYou));
    loaded.observe(&running());
    assert_eq!(saved(&mut loaded), None, "the stopped generation was lost");
}

#[test]
fn a_verified_running_connector_arms_its_workspace_and_is_saved_once() {
    let mut state = State::load(None);
    state.observe(&running());
    assert_eq!(
        saved(&mut state),
        Some(json!({"version":1,"node_id":NODE,"armed":true}))
    );
    state.observe(&running());
    assert_eq!(saved(&mut state), None, "unchanged choice saved again");
    assert_eq!(state.line(&running()), Some(Line::Running));
}

#[test]
fn an_armed_workspace_is_started_when_it_is_definitively_stopped() {
    let state = armed();
    assert_eq!(state.target(&stopped()).unwrap().node_id, NODE);
    assert_eq!(state.line(&stopped()), Some(Line::WillStart));
    assert!(state.target(&running()).is_none());
    assert!(state.target(&stopping()).is_none());
}

#[test]
fn stop_disarms_at_once_and_says_it_was_the_person() {
    let mut state = armed();
    state.stop_chosen(NODE, Some(GENERATION));
    assert_eq!(
        saved(&mut state),
        Some(json!({"version":1,"node_id":NODE,"armed":false,"stopped_generation":GENERATION}))
    );
    assert!(state.target(&stopped()).is_none());
    assert_eq!(state.line(&stopped()), Some(Line::StoppedByYou));
}

#[test]
fn a_draining_connector_does_not_undo_the_stop() {
    let mut state = stopped_by_person();
    state.observe(&stopping());
    assert_eq!(saved(&mut state), None);
    assert_eq!(state.line(&stopping()), None);
    state.observe(&stopped());
    assert_eq!(state.line(&stopped()), Some(Line::StoppedByYou));
}

#[test]
fn a_stale_or_still_running_original_generation_never_rearms() {
    let mut state = stopped_by_person();
    state.observe(&running());
    assert_eq!(saved(&mut state), None, "the original generation re-armed");
    assert!(state.target(&stopped()).is_none());
    assert_eq!(state.line(&running()), Some(Line::RunningNotArmed));
    assert_eq!(state.line(&stopped()), Some(Line::StoppedByYou));
}

#[test]
fn a_positively_later_running_start_arms_again() {
    let mut state = stopped_by_person();
    state.observe(&running_as(LATER));
    assert_eq!(
        saved(&mut state),
        Some(json!({"version":1,"node_id":NODE,"armed":true}))
    );
    assert_eq!(state.line(&running_as(LATER)), Some(Line::Running));
}

#[test]
fn an_explicit_successful_start_arms_again() {
    let mut state = stopped_by_person();
    state.person_started(&running());
    assert_eq!(
        saved(&mut state),
        Some(json!({"version":1,"node_id":NODE,"armed":true}))
    );
    assert!(state.target(&stopped()).is_some());
}

#[test]
fn a_successful_start_that_is_not_a_running_workspace_does_not_arm() {
    let mut state = State::load(None);
    state.person_started(&stopped());
    state.person_started(&stopping());
    assert_eq!(saved(&mut state), None);
}

#[test]
fn a_changed_workspace_does_not_inherit_the_choice() {
    let mut state = armed();
    assert!(state.target(&other_node("stopped")).is_none());
    assert_eq!(state.line(&other_node("stopped")), Some(Line::NotArmed));

    // Its own verified running connector arms only that workspace.
    state.observe(&other_node("running"));
    assert_eq!(
        saved(&mut state),
        Some(json!({"version":1,"node_id":"node-b","armed":true}))
    );
    assert!(state.target(&stopped()).is_none(), "A kept B's choice");

    let state = stopped_by_person();
    assert_eq!(state.line(&other_node("stopped")), Some(Line::NotArmed));
}

#[test]
fn only_an_active_encrypted_workspace_with_nothing_pending_is_started() {
    let cases = [
        fields(json!({"state": "retired"})),
        fields(json!({"state": "legacy"})),
        fields(json!({"state": "pending-activation"})),
        fields(json!({"state": "unconfigured", "config_revision": "", "node_id": ""})),
        fields(json!({"encrypted": false})),
        fields(json!({"pending": {"state": "pending"}})),
    ];
    for case in cases {
        let read = workspace(&case);
        assert!(armed().target(&read).is_none(), "started {case:?}");
        assert_eq!(armed().line(&read), None, "line for {case:?}");
    }
}

#[test]
fn a_running_connector_outside_a_verified_active_workspace_does_not_arm() {
    for extra in [
        json!({"state": "retired"}),
        json!({"encrypted": false}),
        json!({"node_id": ""}),
        json!({"node_id": "n".repeat(129)}),
        json!({"generation": null}),
        json!({"config_revision": "not-a-revision"}),
    ] {
        let mut read = fields(json!({
            "running": true,
            "connector_state": "running",
            "generation": GENERATION,
        }));
        for (key, field) in extra.as_object().unwrap() {
            read.insert(key.clone(), field.clone());
        }
        let mut state = State::load(None);
        state.observe(&workspace(&read));
        assert_eq!(saved(&mut state), None, "armed by {extra}");
    }
}

#[test]
fn an_attempt_shows_starting_then_its_failure_until_the_connector_runs() {
    let mut state = armed();
    state.started();
    assert_eq!(state.line(&stopped()), Some(Line::Starting));
    state.finished(Some("not confirmed".to_owned()));
    assert_eq!(
        state.line(&stopped()),
        Some(Line::Failed {
            reason: "not confirmed".to_owned()
        })
    );
    state.observe(&running());
    assert_eq!(state.line(&running()), Some(Line::Running));
    assert_eq!(state.line(&stopped()), Some(Line::WillStart));
}

#[test]
fn a_connector_seen_running_during_the_attempt_outranks_its_failure() {
    let mut state = armed();
    state.started();
    state.observe(&running());
    state.finished(Some("not confirmed".to_owned()));
    assert_eq!(state.line(&running()), Some(Line::Running));
}

#[test]
fn a_missing_packaged_connector_is_said_only_where_it_would_have_started() {
    let mut state = armed();
    state.unavailable();
    assert_eq!(state.line(&stopped()), Some(Line::Unavailable));
    state.observe(&running());
    assert_eq!(state.line(&running()), Some(Line::Running));

    let mut state = State::load(None);
    state.unavailable();
    assert_eq!(state.line(&stopped()), Some(Line::NotArmed));
}

#[test]
fn a_failed_save_is_offered_again_and_surfaced_until_it_succeeds() {
    let mut state = armed();
    state.stop_chosen(NODE, Some(GENERATION));
    assert!(!state.save_problem());
    let value = state.take_unsaved().unwrap();
    state.save_failed();
    assert!(state.save_problem());
    assert_eq!(state.take_unsaved(), Some(value));
    state.save_succeeded();
    assert!(!state.save_problem());
    assert_eq!(state.take_unsaved(), None);
}

#[test]
fn the_line_reaches_the_page_as_a_kind_and_a_reason() {
    for (line, kind) in [
        (Line::Running, "running"),
        (Line::RunningNotArmed, "running_not_armed"),
        (Line::Starting, "starting"),
        (Line::WillStart, "will_start"),
        (Line::NotArmed, "not_armed"),
        (Line::StoppedByYou, "stopped_by_you"),
        (Line::Unavailable, "unavailable"),
    ] {
        assert_eq!(serde_json::to_value(line).unwrap(), json!({"kind": kind}));
    }
    assert_eq!(
        serde_json::to_value(Line::Failed {
            reason: "x".to_owned()
        })
        .unwrap(),
        json!({"kind": "failed", "reason": "x"})
    );
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "zerus-core-autostart-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("client")).unwrap();
        std::fs::create_dir_all(path.join("staged")).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn connector_name() -> &'static str {
    if cfg!(windows) {
        "zerus-mobile-connector.exe"
    } else {
        "zerus-mobile-connector"
    }
}

fn executable_at(path: &std::path::Path) {
    std::fs::write(path, b"").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn only_the_connector_packaged_beside_this_app_is_usable() {
    let scratch = Scratch::new("packaged");
    let client = scratch.0.join("client");
    let hgs = scratch.0.join("staged/hgs");
    executable_at(&hgs);
    executable_at(&scratch.0.join("staged").join(connector_name()));

    // The button's selection falls back to the staged sibling; automatic start must not.
    assert_eq!(
        connector_program(&hgs, Some(&client)),
        scratch.0.join("staged").join(connector_name())
    );
    assert_eq!(packaged_connector(&client), None);

    // Something unusable under the packaged name is no connector either.
    std::fs::create_dir(client.join(connector_name())).unwrap();
    assert_eq!(packaged_connector(&client), None);
    std::fs::remove_dir(client.join(connector_name())).unwrap();

    executable_at(&client.join(connector_name()));
    assert_eq!(
        packaged_connector(&client),
        Some(client.join(connector_name()))
    );
}

#[test]
fn a_refused_stop_puts_back_exactly_what_it_found() {
    let mut state = armed();
    state.stop_chosen(NODE, Some(GENERATION));
    assert_eq!(saved(&mut state).unwrap()["armed"], false);
    state.stop_refused(NODE, Some(GENERATION));
    let restored = saved(&mut state).unwrap();
    assert_eq!(restored["armed"], true);
    assert!(restored.get("stopped_generation").is_none());
    // Nothing is left to restore a second time.
    state.stop_refused(NODE, Some(GENERATION));
    assert!(saved(&mut state).is_none());
}

// zerus-9ysdg.1: a refusal never mints permission to start automatically.
#[test]
fn a_refused_stop_with_no_earlier_choice_stays_unarmed() {
    for mut state in [
        State::default(),
        State::load(None),
        State::load(Some(b"not a record")),
    ] {
        state.stop_chosen(NODE, Some(GENERATION));
        let _ = saved(&mut state);
        state.stop_refused(NODE, Some(GENERATION));
        let record = saved(&mut state).unwrap();
        assert_eq!(record["armed"], false);
        assert!(record.get("stopped_generation").is_none(), "{record}");
        assert!(state.target(&stopped()).is_none());
        assert_eq!(state.line(&stopped()), Some(Line::NotArmed));
        assert_eq!(state.line(&running()), Some(Line::RunningNotArmed));
    }
}

#[test]
fn a_refused_stop_with_no_earlier_choice_survives_a_restart_as_never_started() {
    let mut state = State::default();
    state.stop_chosen(NODE, Some(GENERATION));
    let _ = saved(&mut state);
    state.stop_refused(NODE, Some(GENERATION));
    let bytes = state.take_unsaved().unwrap();
    let loaded = State::load(Some(&bytes));
    assert!(loaded.target(&stopped()).is_none());
    assert_eq!(loaded.line(&stopped()), Some(Line::NotArmed));
}

#[test]
fn only_a_later_running_read_arms_after_a_refusal_with_no_earlier_choice() {
    let mut state = State::default();
    state.stop_chosen(NODE, Some(GENERATION));
    let _ = saved(&mut state);
    state.stop_refused(NODE, Some(GENERATION));
    let _ = saved(&mut state);
    // A stopped reading proves nothing and arms nothing.
    state.observe(&stopped());
    assert!(saved(&mut state).is_none());
    assert!(state.target(&stopped()).is_none());
    // The connector is verified running: the same rule as with no record at all.
    state.observe(&running());
    assert_eq!(saved(&mut state).unwrap()["armed"], true);
    assert!(state.target(&stopped()).is_some());
}

#[test]
fn a_refusal_with_no_stop_chosen_changes_nothing() {
    let mut state = armed();
    state.stop_refused(NODE, Some(GENERATION));
    assert!(saved(&mut state).is_none());
    assert_eq!(state.line(&running()), Some(Line::Running));
}

#[test]
fn a_refusal_for_another_node_or_generation_changes_nothing() {
    for (node, generation) in [
        ("node-b", Some(GENERATION)),
        (NODE, Some(LATER)),
        (NODE, None),
    ] {
        let mut state = armed();
        state.stop_chosen(NODE, Some(GENERATION));
        let _ = saved(&mut state);
        state.stop_refused(node, generation);
        assert!(saved(&mut state).is_none(), "{node} {generation:?}");
        // The real refusal still finds its transition.
        state.stop_refused(NODE, Some(GENERATION));
        assert_eq!(saved(&mut state).unwrap()["armed"], true);
    }
}

#[test]
fn a_late_refusal_cannot_overwrite_a_later_arm() {
    let mut state = armed();
    state.stop_chosen(NODE, Some(GENERATION));
    let _ = saved(&mut state);
    // A later generation is running, and arms it.
    state.observe(&running_as(LATER));
    let armed_later = saved(&mut state).unwrap();
    assert_eq!(armed_later["armed"], true);
    state.stop_refused(NODE, Some(GENERATION));
    assert!(saved(&mut state).is_none());
}

#[test]
fn a_late_refusal_cannot_overwrite_a_later_stop() {
    let mut state = armed();
    state.stop_chosen(NODE, Some(GENERATION));
    let _ = saved(&mut state);
    state.stop_chosen(NODE, Some(LATER));
    let _ = saved(&mut state);
    state.stop_refused(NODE, Some(GENERATION));
    assert!(saved(&mut state).is_none(), "the older refusal changed it");
    // The later Stop's own refusal restores only what that Stop replaced.
    state.stop_refused(NODE, Some(LATER));
    let restored = saved(&mut state).unwrap();
    assert_eq!(restored["armed"], false);
    assert_eq!(restored["stopped_generation"], GENERATION);
}

#[test]
fn a_late_refusal_cannot_overwrite_a_workspace_the_person_changed_to() {
    let mut state = armed();
    state.stop_chosen(NODE, Some(GENERATION));
    let _ = saved(&mut state);
    state.observe(&other_node("running"));
    let _ = saved(&mut state);
    state.stop_refused(NODE, Some(GENERATION));
    assert!(saved(&mut state).is_none());
}

#[test]
fn a_restore_that_could_not_be_saved_is_offered_again_and_stays_visible() {
    let mut state = armed();
    state.stop_chosen(NODE, Some(GENERATION));
    let _ = saved(&mut state);
    state.stop_refused(NODE, Some(GENERATION));
    assert!(state.take_unsaved().is_some());
    state.save_failed();
    assert!(state.save_problem());
    assert_eq!(saved(&mut state).unwrap()["armed"], true);
}
