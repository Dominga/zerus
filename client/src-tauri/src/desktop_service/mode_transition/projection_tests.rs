//! Portable: one checked registration status projects the same view on every
//! read path (observe, Check result, reconnect) and after an activation
//! attempt, and a plain passive presentation never hides an interrupted
//! record, its latch or a transition. Native reads and effects are tested with
//! the helper fixtures; this covers only the projection they share.
use super::*;

const CONFIGURATION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A Windows registration status as `Route::status` maps it.
fn windows(patch: Value) -> Value {
    let mut status = json!({"schema":1,"configuration":CONFIGURATION,"task_name":"hgs-desktop-fixture",
        "selected_mode":"passive","running_mode":"passive","mode_selection":"explicit","ipc_ready":true,
        "pending_restart":false,"pending_transition":null,"transition":null,"activation_pending":null,
        "activation_automatic_blocked":false});
    for (key, value) in patch.as_object().unwrap() {
        status[key] = value.clone();
    }
    status
}
fn view(controller: &Controller) -> Value {
    serde_json::to_value(controller.view()).unwrap()
}
const MANUAL: &str = "activation_platform_manual";

#[test]
fn a_plain_passive_windows_selection_projects_the_same_view_on_every_read() {
    let controller = Controller::new();
    controller.project_for_test(&windows(json!({})), true);
    let first = view(&controller);
    assert_eq!(
        first,
        json!({"pending":false,"running":false,"needs_resume":false,"code":MANUAL,
            "review":null,"automatic_blocked":false,"request_id":null,"active":false})
    );
    // observe -> activation attempt (manual) -> observe, then repeated Check
    // result and reconnect reads: the same view each time.
    controller.platform_manual_for_test();
    assert_eq!(view(&controller), first);
    for _ in 0..3 {
        controller.project_for_test(&windows(json!({})), true);
        assert_eq!(view(&controller), first);
    }
}

#[test]
fn unix_passive_still_offers_resume_and_windows_never_does() {
    let unix = Controller::new();
    unix.project_for_test(&windows(json!({"mode_selection":"default"})), false);
    assert_eq!(unix.view().code, "activation_required");
    assert!(unix.view().needs_resume);
    let windows_controller = Controller::new();
    windows_controller.project_for_test(&windows(json!({})), true);
    assert!(!windows_controller.view().needs_resume);
}

#[test]
fn the_passive_presentation_never_hides_a_review_latch_or_transition() {
    let transition = json!({"request_id":"11111111-1111-4111-8111-111111111111","phase":"stop_offered",
        "desired":"background","original":{"pid":4242,"birth":1,"guid":"{0}","epoch":"e"}});
    for (case, patch) in [
        (
            "pending transition",
            json!({"pending_transition":transition,"transition":transition,
            "activation_pending":"11111111-1111-4111-8111-111111111111","pending_restart":true}),
        ),
        ("pending restart", json!({"pending_restart":true})),
        (
            "background selected, passive running",
            json!({"selected_mode":"background"}),
        ),
        (
            "background running",
            json!({"selected_mode":"background","running_mode":"background"}),
        ),
        (
            "service not running",
            json!({"running_mode":null,"ipc_ready":false}),
        ),
    ] {
        let controller = Controller::new();
        controller.project_for_test(&windows(patch), true);
        assert_eq!(controller.view().code, "", "{case}");
        assert!(!controller.view().needs_resume, "{case}");
    }
    // An interrupted record shown by B2 keeps its own presentation.
    let reviewed = Controller::new();
    reviewed.state.lock().unwrap().view.review = Some(review::View {
        request_id: "11111111-1111-4111-8111-111111111111".into(),
        configuration: CONFIGURATION.into(),
        selection: "passive".into(),
        disposition: review::Disposition::RebootRequired,
        passive_available: true,
    });
    reviewed.project_for_test(&windows(json!({})), true);
    assert_eq!(reviewed.view().code, "");
    assert!(reviewed.view().review.is_some());
    // The automatic latch is sticky: a later plain status read keeps it.
    let latched = Controller::new();
    latched.project_for_test(&windows(json!({"activation_automatic_blocked":true})), true);
    latched.project_for_test(&windows(json!({})), true);
    assert_eq!(latched.view().code, "");
    assert!(latched.view().automatic_blocked);
}
