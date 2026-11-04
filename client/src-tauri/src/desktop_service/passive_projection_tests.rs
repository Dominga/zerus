//! The Windows passive GUI state, end to end from the actual producers.
//!
//! The root's passive status ("passive" in desktop-delivery-status.json,
//! extracted by client/scripts/generate-delivery-status-fixture.py from the
//! root projection) is parsed by client-core as the service connection does.
//! The ownership display adds the activation view the platform-manual route
//! leaves: on Windows the actual `observe()`, elsewhere the same `failed()`
//! path. The result must equal client/fixtures/desktop-service-windows-passive.json,
//! which the client's paused-state tests consume. Nothing here grants
//! activation authority: needs_resume stays false.
use super::*;

const CONFIGURATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn platform_activation() -> mode_transition::View {
    let controller = mode_transition::Controller::new();
    #[cfg(windows)]
    tauri::async_runtime::block_on(
        controller.observe(PathBuf::from("hgs.exe"), CONFIGURATION.into()),
    );
    #[cfg(not(windows))]
    controller.platform_manual_for_test();
    controller.view()
}

/// A current Service grant, as the GUI's startup grant leaves it while the
/// actor still runs passive (src/desktop/ownership.rs: Grant sets Service).
fn granted() -> crate::desktop_ownership::View {
    use zerus_client_core::desktop::ownership::{State, Status, ABI};
    crate::desktop_ownership::View {
        status: Some(Status {
            abi: ABI.into(),
            configuration: CONFIGURATION.into(),
            journal: Some("00000000-0000-4000-8000-000000000002".into()),
            revision: 1,
            state: State::Service,
            rearm_required: false,
            request_id: None,
            owner_epoch: Some("00000000-0000-4000-8000-000000000003".into()),
            identity_current: true,
        }),
        request_id: None,
        pending: false,
        running: false,
        code: String::new(),
    }
}

#[test]
fn the_platform_manual_route_keeps_its_code_and_offers_no_resume() {
    let view = serde_json::to_value(platform_activation()).unwrap();
    assert_eq!(
        view,
        json!({"pending":false,"running":false,"needs_resume":false,"code":"activation_platform_manual",
            "review":null,"automatic_blocked":false,"request_id":null,"active":false})
    );
}

#[test]
fn the_windows_passive_projection_is_the_client_fixture() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/desktop-delivery-status.json"
    )))
    .unwrap();
    let status = &cases
        .iter()
        .find(|case| case["case"] == "passive")
        .expect("the root passive case")["status"];
    let mut state = service::State::new(status["configuration"].as_str().unwrap().into()).unwrap();
    state.status(status).unwrap();
    let mut snapshot = state.snapshot().clone();
    // An unreviewed passive selection has no reviewed-choice business gate.
    snapshot.notification_ownership = ownership_display(granted(), platform_activation(), true);
    let mut produced = serde_json::to_value(&snapshot).unwrap();
    // native_recovery is a process-wide startup flag that other tests in this
    // binary toggle; a fresh GUI start without recovery shows false.
    assert!(produced["notification_ownership"]["native_recovery"].is_boolean());
    produced["notification_ownership"]["native_recovery"] = json!(false);
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../fixtures/desktop-service-windows-passive.json"
    )))
    .unwrap();
    assert_eq!(
        produced,
        fixture,
        "regenerate the client fixture from:\n{}",
        serde_json::to_string_pretty(&produced).unwrap()
    );
}
