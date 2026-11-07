//! Task 176 (owner, 2026-11-07): the session list stayed an endless skeleton
//! while `hgs desktop status` refused a state folder that had inherited a
//! sandbox group's read. The startup check repairs it once through the
//! service start's own preparation; a refusal that stays is shown.
use super::*;

fn args(fixture: &Fixture) -> Vec<Value> {
    fixture.calls().into_iter().map(|call| call["args"].clone()).collect()
}

fn prepare() -> Value {
    json!(["desktop", "prepare-state", "--json"])
}

#[test]
fn an_inherited_read_on_the_state_folder_is_repaired_once_and_the_list_connects() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({"ready":true,"state_refused":true,"prepare_state_abi":true}));
        let (service, _) = fixture.start(true);
        eventually("connected after the repair", || service.snapshot().connected).await;
        assert_eq!(service.snapshot().state_refusal, None);
        service.join().await.unwrap();
        let calls = args(&fixture);
        assert_eq!(calls.iter().filter(|call| **call == prepare()).count(), 1, "{calls:?}");
        // Refused status, the repair, then the status that passes.
        let at = calls.iter().position(|call| *call == prepare()).unwrap();
        assert_eq!(calls[at - 1], json!(["desktop", "status", "--json"]));
        assert_eq!(calls[at + 1], json!(["desktop", "status", "--json"]));
    });
}

#[test]
fn a_refusal_that_stays_is_shown_with_its_folder_and_entry_never_as_loading() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(
            json!({"ready":true,"state_refused":true,"state_unrepairable":true,"prepare_state_abi":true}),
        );
        let (service, _) = fixture.start(true);
        eventually("the refusal is shown", || service.snapshot().code == state_refusal::CODE).await;
        let snapshot = service.snapshot();
        service.join().await.unwrap();
        assert!(!snapshot.connected);
        assert!(!snapshot.reconnecting, "no endless automatic retry; Retry stays explicit");
        let refusal = snapshot.state_refusal.clone().expect("the refusal");
        assert!(refusal.starts_with(state_refusal::MARKER), "{refusal}");
        assert!(refusal.contains("/fixture/state/sessions") && refusal.contains("CodexSandboxUsers"), "{refusal}");
        assert!(refusal.contains("read and execute (RX)") && refusal.contains("prepare-state"), "{refusal}");
        // The frontend receives it beside the code.
        let shown = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(shown["state_refusal"], json!(refusal));
        let calls = args(&fixture);
        assert_eq!(calls.iter().filter(|call| **call == prepare()).count(), 1, "one repair: {calls:?}");
        assert!(!calls.iter().any(|call| *call == json!(["desktop", "watch", "--json"])), "{calls:?}");
    });
}

#[test]
fn an_older_cli_without_the_preparation_shows_the_refusal_without_a_repair() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({"ready":true,"state_refused":true}));
        let (service, _) = fixture.start(true);
        eventually("the refusal is shown", || service.snapshot().code == state_refusal::CODE).await;
        assert!(service.snapshot().state_refusal.is_some());
        service.join().await.unwrap();
        assert!(!args(&fixture).contains(&prepare()));
    });
}
