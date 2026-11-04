//! The first-choice controller over a Windows-shaped route and a fake
//! registration CLI (Linux CI). The fake records every call, so each test can
//! assert which original requests reached the "native" side. The real helper's
//! caller, Grant and history checks are tested in the root and hosted suites.
use super::*;
use std::fs;

const CONFIG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FAKE: &str = r#"#!/usr/bin/python3
import json, pathlib, sys
root = pathlib.Path(__file__).resolve().parent
state = json.loads((root / 'state.json').read_text())
args = sys.argv[1:]
with (root / 'calls').open('a') as log:
    log.write(' '.join(args) + '\n')
def save():
    (root / 'state.json').write_text(json.dumps(state))
config = state['status']['configuration']
if args == ['--help']:
    print('Desktop registration mode review ABI: desktop-registration-mode-review-v1')
    print('Desktop service mode choice scope ABI: desktop-service-mode-choice-scope-v1')
    if state.get('first', True):
        print('Desktop registration first mode choice ABI: desktop-registration-mode-first-choice-v1')
elif args == ['desktop', 'registration', 'status', '--json']:
    state['status_calls'] = state.get('status_calls', 0) + 1
    print(json.dumps(state['status']))
    # A deterministic drift right after the Nth read (test control only).
    if state.get('drift_after') == state['status_calls']:
        state['status'].update(state['drift'])
    save()
elif args[:7] == ['desktop', 'registration', 'install', '--mode', 'background', '--json', '--request-id'] and len(args) == 8:
    request = args[7]
    if state.get('install') == 'journal_lost':
        # Production shape after the WHOLE mode journal is gone or replaced
        # (or the state folder wiped) while the independent anchor survives:
        # no pending, history or receipt, only first_choice_lost and the latch.
        state['status'].update(pending_transition=None, first_choice=None, mode_choice=None,
                               reviewed_transition=None, first_choice_lost=request,
                               activation_automatic_blocked=True)
        state['receipts'].pop(request, None)
        save()
        print(json.dumps(dict(abi='desktop-registration-mode-v1', schema=1, configuration=config,
                              request_id=request, ok=True, changed=True)))
        sys.exit(0)
    if state.get('install') in ('missing_ok', 'missing_fail', 'replaced'):
        # A simulated effect that started, then its original evidence went
        # missing (journal deleted) or was replaced by another request's.
        pending = dict(request_id=request, desired='background', phase='update_offered', original=None, started=None)
        state['status'].update(pending_transition=pending, pending_restart=True)
        if state['install'] == 'replaced':
            state['receipts'][request] = dict(request='33333333-3333-4333-8333-333333333333',
                                              desired='background', phase='settled')
        else:
            state['receipts'].pop(request, None)
        save()
        if state['install'] == 'missing_fail':
            sys.exit(1)
        print(json.dumps(dict(abi='desktop-registration-mode-v1', schema=1, configuration=config,
                              request_id=request, ok=True, changed=True)))
        sys.exit(0)
    if state.get('install') == 'lost':
        pending = dict(request_id=request, desired='background', phase='stop_offered', original=None, started=None)
        state['status'].update(pending_transition=pending, pending_restart=True)
        state['receipts'][request] = dict(request=request, desired='background', phase='unknown')
        save()
        sys.exit(1)
    receipt = dict(request=request, desired='background', phase='settled')
    state['receipts'][request] = receipt
    state['status'].update(selected_mode='background', running_mode='background', first_choice=receipt,
                           completed_request_id=request)
    save()
    print(json.dumps(dict(abi='desktop-registration-mode-v1', schema=1, configuration=config,
                          request_id=request, ok=True, changed=True)))
elif args[:5] == ['desktop', 'registration', 'mode-result', '--json', '--request-id'] and len(args) == 6:
    request = args[5]
    print(json.dumps(dict(abi='desktop-registration-mode-v1', schema=1, configuration=config, request_id=request,
                          read_only=True, receipt=state['receipts'].get(request))))
else:
    sys.exit(2)
"#;

struct Fixture {
    root: PathBuf,
    cli: PathBuf,
}
impl Fixture {
    fn new(install: &str, first: bool) -> Self {
        let root = std::env::temp_dir().join(format!("zerus-first-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let cli = root.join("hgs");
        crate::write_executable(&cli, FAKE.as_bytes());
        let fixture = Self { root, cli };
        fixture.write(json!({"install":install,"first":first,"receipts":{},"status":{
            "schema":1,"configuration":CONFIG,"task_name":"hgs-desktop-fixture",
            "selected_mode":"passive","running_mode":"passive","pending_restart":false,
            "pending_transition":null,"reviewed_transition":null,"mode_choice":null,"first_choice":null,
            "completed_request_id":null,"cancelled_request_id":null,
            "activation_automatic_blocked":false,"service_epoch":"e","service_generation":1}}));
        fixture
    }
    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("state.json")).unwrap()).unwrap()
    }
    fn write(&self, state: Value) {
        fs::write(
            self.root.join("state.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
    }
    fn patch(&self, patch: Value) {
        let mut state = self.state();
        for (key, value) in patch.as_object().unwrap() {
            state["status"][key] = value.clone();
        }
        self.write(state);
    }
    fn installs(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("calls"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("desktop registration install"))
            .map(str::to_owned)
            .collect()
    }
    fn lookups(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("calls"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.strip_prefix("desktop registration mode-result --json --request-id "))
            .map(str::to_owned)
            .collect()
    }
    fn controller(&self) -> Arc<Controller> {
        let controller = Controller::new();
        controller.fixture_windows(Some(self.cli.clone()));
        tauri::async_runtime::block_on(controller.observe(self.cli.clone(), CONFIG.into()));
        controller
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_plain_passive_offer_settles_exactly_one_first_choice() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let fixture = Fixture::new("settle", true);
    let controller = fixture.controller();
    let display = controller.view().first.expect("offered");
    assert_eq!(display.configuration, CONFIG);
    assert_eq!(controller.view().code, "activation_platform_manual");
    let prepared = tauri::async_runtime::block_on(controller.confirm_first(display, |_| true))
        .unwrap()
        .unwrap();
    let id = tauri::async_runtime::block_on(controller.apply_first(prepared)).unwrap();
    assert_eq!(
        fixture.installs(),
        [format!(
            "desktop registration install --mode background --json --request-id {id}"
        )]
    );
    assert!(controller.first_settled(&id));
    let view = controller.view();
    assert!(view.first.is_none() && !view.pending && !view.running && view.active);
}

#[test]
fn cancel_and_a_stale_offer_change_nothing_and_allocate_nothing() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let fixture = Fixture::new("settle", true);
    let controller = fixture.controller();
    let display = controller.view().first.unwrap();
    let cancelled =
        tauri::async_runtime::block_on(controller.confirm_first(display.clone(), |_| false))
            .unwrap();
    assert!(cancelled.is_none());
    let stale = FirstView {
        selection: uuid::Uuid::new_v4().to_string(),
        ..display.clone()
    };
    let refused = tauri::async_runtime::block_on(controller.confirm_first(stale, |_| true));
    assert_eq!(refused.err(), Some("activation_scope_changed"));
    assert!(fixture.installs().is_empty());
    let view = controller.view();
    assert!(view.request_id.is_none() && !view.pending && !view.running);
    assert_eq!(view.first, Some(display), "the same offer stays");
}

#[test]
fn a_change_while_the_dialog_is_open_refuses_without_effects() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let fixture = Fixture::new("settle", true);
    let controller = fixture.controller();
    let display = controller.view().first.unwrap();
    for (case, patch) in [
        ("service restarted", json!({"service_generation":2})),
        (
            "automatic latch",
            json!({"activation_automatic_blocked":true}),
        ),
    ] {
        let changed = {
            let fixture = &fixture;
            let patch = patch.clone();
            let root = fixture.root.clone();
            move |_: &FirstView| {
                let path = root.join("state.json");
                let mut state: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                for (key, value) in patch.as_object().unwrap() {
                    state["status"][key] = value.clone();
                }
                fs::write(&path, serde_json::to_vec(&state).unwrap()).unwrap();
                true
            }
        };
        let _ = fixture;
        let refused =
            tauri::async_runtime::block_on(controller.confirm_first(display.clone(), changed));
        assert!(refused.is_err(), "{case}");
        assert!(fixture.installs().is_empty(), "{case}");
        assert_ne!(controller.view().first, Some(display.clone()), "{case}");
        fixture.patch(json!({"service_generation":1,"activation_automatic_blocked":false}));
    }
}

#[test]
fn history_a_latch_or_an_older_helper_never_offers() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let archived = json!({"original":{"request_id":"11111111-1111-4111-8111-111111111111",
        "review":{"state":"reviewed_unknown"}},"outcome":"unknown"});
    for (case, patch, first) in [
        (
            "B2 archive with a healthy passive status",
            json!({"reviewed_transition":archived,
            "activation_automatic_blocked":true}),
            true,
        ),
        (
            "automatic latch",
            json!({"activation_automatic_blocked":true}),
            true,
        ),
        (
            "background",
            json!({"selected_mode":"background","running_mode":"background"}),
            true,
        ),
        ("older helper", json!({}), false),
    ] {
        let fixture = Fixture::new("settle", first);
        fixture.patch(patch);
        let controller = fixture.controller();
        assert!(controller.view().first.is_none(), "{case}");
        assert!(fixture.installs().is_empty(), "{case}");
    }
}

#[test]
fn a_lost_reply_keeps_its_original_and_closes_another_offer() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let fixture = Fixture::new("lost", true);
    let controller = fixture.controller();
    let display = controller.view().first.unwrap();
    let prepared =
        tauri::async_runtime::block_on(controller.confirm_first(display.clone(), |_| true))
            .unwrap()
            .unwrap();
    let id = tauri::async_runtime::block_on(controller.apply_first(prepared)).unwrap();
    assert!(!controller.first_settled(&id));
    let view = controller.view();
    assert!(view.pending && view.first.is_none());
    assert_eq!(view.request_id.as_deref(), Some(id.as_str()));
    // Check result / a later observe reads only the retained original UUID.
    tauri::async_runtime::block_on(controller.observe(fixture.cli.clone(), CONFIG.into()));
    tauri::async_runtime::block_on(controller.observe(fixture.cli.clone(), CONFIG.into()));
    assert_eq!(fixture.installs().len(), 1);
    let lookups = fixture.lookups();
    assert!(
        lookups.len() >= 3 && lookups.iter().all(|l| l == &id),
        "{lookups:?}"
    );
    assert!(controller.view().first.is_none());
    let again = tauri::async_runtime::block_on(controller.confirm_first(display, |_| true));
    assert!(again.is_err());
    assert_eq!(fixture.installs().len(), 1);
    // A restarted consumer adopts only the stored original ID for lookup.
    let restarted = fixture.controller();
    assert!(restarted.view().pending && restarted.view().first.is_none());
    assert_eq!(restarted.view().request_id.as_deref(), Some(id.as_str()));
    assert_eq!(fixture.installs().len(), 1);
}

#[test]
fn a_drift_between_the_last_read_and_the_spawn_never_reaches_the_helper() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let fixture = Fixture::new("settle", true);
    let controller = fixture.controller();
    let display = controller.view().first.unwrap();
    let prepared = tauri::async_runtime::block_on(controller.confirm_first(display, |_| true))
        .unwrap()
        .unwrap();
    // Reads so far: observe 1, confirmation 2 and 3. apply_first's own
    // pre-reservation read is 4; the drift lands right after it, so only the
    // spawn-time read can see it.
    let mut state = fixture.state();
    assert_eq!(state["status_calls"], 3);
    state["drift_after"] = json!(4);
    state["drift"] = json!({"service_generation": 2});
    fixture.write(state);
    let id = tauri::async_runtime::block_on(controller.apply_first(prepared)).unwrap();
    assert!(
        fixture.installs().is_empty(),
        "the mutating child never spawned"
    );
    assert!(!controller.first_settled(&id));
    // The closed no-spawn fact is the positive not-started case.
    let view = controller.view();
    assert!(!view.pending, "{:?}", view.code);
    assert_eq!(view.code, "activation_not_started");
    // 5 is the spawn check; 6 is the final read after the lookup.
    assert_eq!(
        fixture.state()["status_calls"],
        6,
        "the spawn check read the registration"
    );
}

#[test]
fn an_unsupported_final_status_never_reaches_the_helper() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let fixture = Fixture::new("settle", true);
    let controller = fixture.controller();
    let display = controller.view().first.unwrap();
    let prepared = tauri::async_runtime::block_on(controller.confirm_first(display, |_| true))
        .unwrap()
        .unwrap();
    let mut state = fixture.state();
    state["drift_after"] = json!(4);
    state["drift"] = json!({"schema": 2});
    fixture.write(state);
    let id = tauri::async_runtime::block_on(controller.apply_first(prepared)).unwrap();
    assert!(
        fixture.installs().is_empty(),
        "the mutating child never spawned"
    );
    assert!(!controller.first_settled(&id));
}

#[test]
fn missing_or_replaced_evidence_after_a_possible_start_stays_unknown() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    for case in ["missing_ok", "missing_fail", "replaced"] {
        let fixture = Fixture::new(case, true);
        let controller = fixture.controller();
        let display = controller.view().first.unwrap();
        let prepared =
            tauri::async_runtime::block_on(controller.confirm_first(display.clone(), |_| true))
                .unwrap()
                .unwrap();
        let id = tauri::async_runtime::block_on(controller.apply_first(prepared)).unwrap();
        assert_eq!(fixture.installs().len(), 1, "{case}");
        let view = controller.view();
        assert!(view.pending && !controller.first_settled(&id), "{case}");
        assert_ne!(view.code, "activation_not_started", "{case}");
        assert!(view.first.is_none(), "{case}");
        // Check result and a later observe read only the retained original.
        tauri::async_runtime::block_on(controller.observe(fixture.cli.clone(), CONFIG.into()));
        tauri::async_runtime::block_on(controller.observe(fixture.cli.clone(), CONFIG.into()));
        assert!(controller.view().pending, "{case}");
        assert_eq!(fixture.installs().len(), 1, "{case}");
        assert!(fixture.lookups().iter().all(|l| l == &id), "{case}");
        let again = tauri::async_runtime::block_on(controller.confirm_first(display, |_| true));
        assert!(again.is_err(), "{case}");
        // A restarted consumer adopts only the stored original ID.
        let restarted = fixture.controller();
        assert!(restarted.view().pending, "{case}");
        assert_eq!(
            restarted.view().request_id.as_deref(),
            Some(id.as_str()),
            "{case}"
        );
        assert_eq!(fixture.installs().len(), 1, "{case}");
    }
}

#[test]
fn a_lost_journal_never_offers_or_infers_not_started_after_a_restart() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let fixture = Fixture::new("journal_lost", true);
    let controller = fixture.controller();
    let display = controller.view().first.unwrap();
    let prepared =
        tauri::async_runtime::block_on(controller.confirm_first(display.clone(), |_| true))
            .unwrap()
            .unwrap();
    let id = tauri::async_runtime::block_on(controller.apply_first(prepared)).unwrap();
    assert_eq!(fixture.installs().len(), 1);
    assert!(controller.view().pending && !controller.first_settled(&id));
    assert_ne!(controller.view().code, "activation_not_started");
    // A restarted consumer has no intent and sees only the native loss.
    let restarted = fixture.controller();
    let view = restarted.view();
    assert!(view.first.is_none(), "no offer after a lost journal");
    assert!(view.automatic_blocked && !view.needs_resume && !view.pending);
    assert_eq!(view.code, "activation_first_choice_lost");
    let again = tauri::async_runtime::block_on(restarted.confirm_first(display, |_| true));
    assert!(again.is_err());
    assert_eq!(fixture.installs().len(), 1, "no new UUID or install");
}

#[test]
fn the_spawn_veto_admits_only_this_continuations_own_reservation() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    let fixture = Fixture::new("settle", true);
    let controller = fixture.controller();
    let display = controller.view().first.unwrap();
    let route = controller.checked_first(&display, None).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    controller.reserve(id.clone(), CONFIG.into(), route);
    assert_eq!(
        controller.checked_first(&display, None).err(),
        Some("activation_unconfirmed")
    );
    assert_eq!(
        controller.checked_first(&display, Some("other")).err(),
        Some("activation_unconfirmed")
    );
    assert!(controller.checked_first(&display, Some(&id)).is_ok());
}
