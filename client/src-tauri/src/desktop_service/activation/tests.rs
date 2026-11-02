use super::*;
use crate::desktop_service::{
    frontend::tests::native::{responder, CacheServer},
    tests::Fixture,
};
use std::{sync::mpsc, time::Instant};

// The native responder deliberately refuses an agent-attributed parent. Run
// the GUI-only fixture in an exact child without mutating this harness's env.
fn private_case(case: &'static str) -> bool {
    const MARKER: &str = "ZERUS_ACTIVATION_CONSUMER_CASE";
    if std::env::var(MARKER).as_deref() == Ok(case) {
        return true;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    let call = Invocation {
        args: [
            "--exact",
            case,
            "--nocapture",
            "--test-threads=1",
            "--color=never",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        stdin: None,
        timeout: Duration::from_secs(30),
        request_id: None,
    };
    let result = runner::run_without(
        &std::env::current_exe().unwrap(),
        &call,
        &[(MARKER, case)],
        &["HGS_SESSION", "HGS_RUN_ID"],
        64 << 10,
    )
    .unwrap();
    assert!(
        matches!(&result, zerus_client_core::Exit::Code { code: 0, .. }),
        "private GUI case {case}: {result:?}"
    );
    if let zerus_client_core::Exit::Code { stdout, .. } = &result {
        assert!(
            String::from_utf8_lossy(stdout)
                .contains("test result: ok. 1 passed; 0 failed; 0 ignored;"),
            "the exact private GUI case must execute once: {result:?}"
        );
    }
    false
}

fn start(
    fixture: &Fixture,
    responder: Arc<zerus_client_core::desktop::Responder>,
) -> (Arc<Service>, mpsc::Receiver<Delivery>) {
    let (tx, rx) = mpsc::sync_channel(1);
    let pump = crate::pump::Pump::default();
    let (sink, input) = mpsc::sync_channel(256);
    let drain = pump.drain_service(input, |_| {});
    let service = Service::start_observed(
        fixture.0.join("hgs"),
        pump,
        sink,
        true,
        drain,
        Some(responder),
        Observers {
            projections: vec![],
            activation: Some(Arc::new(move |delivery| {
                tx.try_send(delivery).ok();
            })),
        },
    );
    (service, rx)
}
async fn until(label: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(Instant::now() < deadline, "{label}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
fn waits(fixture: &Fixture) -> Vec<Value> {
    fixture
        .calls()
        .into_iter()
        .filter(|call| call["value"]["request"]["op"] == "activation_wait")
        .collect()
}
async fn receive(rx: &mpsc::Receiver<Delivery>) -> Delivery {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(value) = rx.try_recv() {
            return value;
        }
        assert!(Instant::now() < deadline, "activation delivery");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
fn consume(delivery: Delivery, available: bool) -> crate::desk::Outcome {
    use crate::desk::{Context, DeskInput, DeskLoop};
    use zerus_client_core::{Change, ComputerId, ComputerStatus, ViewRow};
    let target = delivery.open().target.clone();
    let row = ViewRow {
        name: "codex/renamed/exact-session".into(),
        state: if available { "running" } else { "archived" }.into(),
        run_id: Some(target.run),
        conversation_id: Some(target.conversation),
        ..ViewRow::default()
    };
    let computer = ComputerId(target.computer);
    let mut desk = DeskLoop::new(
        zerus_client_core::menubar::OpenMode::Automatic,
        None,
        Instant::now(),
    );
    let mut publish =
        |_: &zerus_client_core::menubar::Desk, _: zerus_client_core::menubar::OpenMode| {};
    let mut optional = || false;
    let mut marks = || None;
    let mut notice = || "fixture".to_owned();
    let mut cx = Context {
        publish: &mut publish,
        now: Instant::now(),
        unix: 0,
        focused: false,
        optional_questions: &mut optional,
        marks: &mut marks,
        notice_id: &mut notice,
    };
    desk.round(
        [
            DeskInput::Change(Box::new(Change::Replace {
                computer: computer.clone(),
                rows: vec![(row.name.clone(), row)],
            })),
            DeskInput::Change(Box::new(Change::Computer {
                computer,
                status: ComputerStatus::Live,
            })),
            DeskInput::ServiceClicked(Box::new(delivery)),
        ],
        &mut cx,
    )
}
#[test]
fn original_bound_child_and_one_held_delivery_keep_reads_serial_and_stop_invalidates_ui_work() {
    if !private_case("desktop_service::activation::tests::original_bound_child_and_one_held_delivery_keep_reads_serial_and_stop_invalidates_ui_work") { return; }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({"ready":true,"activation_abi":true}));
        let responder = responder();
        let _cache = CacheServer::start(&fixture.0, responder.clone());
        let (service, rx) = start(&fixture, responder);
        let delivery = receive(&rx).await;
        assert!(fixture.0.join("original-child-bound").exists());
        assert!(delivery.current());
        assert_eq!(delivery.open().target.run, "exact-run");
        assert_eq!(delivery.open().target.conversation, "exact-conversation");
        assert_eq!(
            delivery.open().post_request_id,
            "22222222-2222-4222-8222-222222222222"
        );
        assert!(
            matches!(&delivery.open().platform_context, Some(zerus_client_core::desktop::notifications::PlatformContext::Wayland { activation_token }) if activation_token == "ephemeral-fixture")
        );
        let outcome = consume(delivery, true);
        assert!(outcome.show);
        assert_eq!(outcome.activation.as_deref(), Some("ephemeral-fixture"));
        assert_eq!(
            outcome.open,
            Some((
                zerus_client_core::ComputerId("".into()),
                "codex/renamed/exact-session".into()
            ))
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(
            waits(&fixture).len(),
            1,
            "native-main completion still holds the one delivery"
        );
        drop(outcome);
        until("next fresh read after UI consumes the delivery", || {
            waits(&fixture).len() == 2
        })
        .await;
        let calls = waits(&fixture);
        assert_ne!(
            calls[0]["value"]["request"]["request_id"],
            calls[1]["value"]["request"]["request_id"]
        );
        service.join().await.unwrap();
        assert!(rx.try_recv().is_err());
    });
}
#[test]
fn a_gone_target_notice_also_retains_the_single_delivery_until_native_ui_consumes_it() {
    if !private_case("desktop_service::activation::tests::a_gone_target_notice_also_retains_the_single_delivery_until_native_ui_consumes_it") { return; }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({"ready":true,"activation_abi":true}));
        let responder = responder();
        let _cache = CacheServer::start(&fixture.0, responder.clone());
        let (service, rx) = start(&fixture, responder);
        let outcome = consume(receive(&rx).await, false);
        assert!(outcome.open.is_none());
        assert_eq!(outcome.notices.len(), 1);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(waits(&fixture).len(), 1);
        drop(outcome);
        until("new read after native notice completion", || {
            waits(&fixture).len() == 2
        })
        .await;
        service.join().await.unwrap();
    });
}

#[test]
fn retained_delivery_refuses_a_new_epoch_and_shutdown_does_not_wait_for_a_stalled_view() {
    if !private_case("desktop_service::activation::tests::retained_delivery_refuses_a_new_epoch_and_shutdown_does_not_wait_for_a_stalled_view") { return; }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(json!({"ready":true,"activation_abi":true}));
        let responder = responder();
        let _cache = CacheServer::start(&fixture.0, responder.clone());
        let (service, rx) = start(&fixture, responder);
        let delivery = receive(&rx).await;
        assert!(delivery.current());
        service.shared.snapshot.lock().unwrap().epoch = Some(uuid::Uuid::new_v4().to_string());
        assert!(!delivery.current());
        let completion = delivery.completion();
        let outcome = consume(delivery, true);
        assert!(!outcome.show && outcome.open.is_none());
        service.join().await.unwrap();
        assert!(!completion.current());
        assert_eq!(waits(&fixture).len(), 1);
    });
}
#[test]
fn idle_reads_use_new_ids_and_no_click_is_inferred_from_none() {
    if !private_case("desktop_service::activation::tests::idle_reads_use_new_ids_and_no_click_is_inferred_from_none") { return; }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let fixture =
            Fixture::new(json!({"ready":true,"activation_abi":true,"activation_idle_first":true}));
        let responder = responder();
        let _cache = CacheServer::start(&fixture.0, responder.clone());
        let (service, rx) = start(&fixture, responder);
        let delivery = receive(&rx).await;
        assert_eq!(waits(&fixture).len(), 2);
        let calls = waits(&fixture);
        assert_ne!(
            calls[0]["value"]["request"]["request_id"],
            calls[1]["value"]["request"]["request_id"]
        );
        service.join().await.unwrap();
        assert!(!delivery.current());
    });
}
#[test]
fn missing_abi_and_invalid_original_reply_never_fall_back_or_replay() {
    if !private_case("desktop_service::activation::tests::missing_abi_and_invalid_original_reply_never_fall_back_or_replay") { return; }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for bad in [
            "missing-abi",
            "request",
            "epoch",
            "configuration",
            "target",
            "transport",
        ] {
            let fixture = Fixture::new(
                json!({"ready":true,"activation_abi":bad!="missing-abi","activation_invalid":bad}),
            );
            let responder = responder();
            let _cache = CacheServer::start(&fixture.0, responder.clone());
            let (service, rx) = start(&fixture, responder);
            until("connected source", || service.snapshot().connected).await;
            if bad != "missing-abi" {
                until("one original activation read", || {
                    waits(&fixture).len() == 1
                })
                .await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            assert_eq!(waits(&fixture).len(), usize::from(bad != "missing-abi"));
            assert!(rx.try_recv().is_err(), "{bad}");
            assert!(
                service.snapshot().connected,
                "missing click ABI does not replace the catalog"
            );
            assert!(!fixture.calls().iter().any(|call| call["args"]
                == json!(["desktop", "request", "--json"])
                && call["value"]["op"] == "activation_wait"));
            service.join().await.unwrap();
        }
    });
}
