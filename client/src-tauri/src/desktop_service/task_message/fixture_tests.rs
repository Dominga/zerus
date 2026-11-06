//! The send path past the capability probe, against the real service fixture
//! with a fake `hgs` that records every task message request.
use super::*;
use crate::desktop_service::{
    frontend::tests::native::{responder, CacheServer},
    tests::{eventually, Fixture},
};
use std::sync::atomic::AtomicUsize;
use zerus_client_core::tasks_write::{input_digest, message_input, message_intent};

const TASK: &str = "11111111-1111-4111-8111-111111111111";

pub(in crate::desktop_service) fn private_case(case: &'static str) -> bool {
    const MARKER: &str = "ZERUS_TASK_MESSAGE_CASE";
    if std::env::var(MARKER).as_deref() == Ok(case) {
        return true;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    let invocation = zerus_client_core::Invocation {
        args: ["--exact", case, "--nocapture", "--test-threads=1"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        stdin: None,
        timeout: Duration::from_secs(60),
        request_id: None,
    };
    match zerus_client_core::run::run_without(
        &std::env::current_exe().unwrap(),
        &invocation,
        &[(MARKER, case)],
        &["HGS_SESSION", "HGS_RUN_ID"],
        64 << 10,
    )
    .unwrap()
    {
        zerus_client_core::Exit::Code {
            code,
            stdout,
            stderr,
        } => assert_eq!(code, 0, "{}\n{stderr}", String::from_utf8_lossy(&stdout)),
        other => panic!("{other:?}"),
    }
    false
}
fn fixture(config: Value) -> Fixture {
    let f = Fixture::new(config);
    std::fs::remove_file(f.0.join("hgs")).unwrap();
    std::fs::write(
        f.0.join("hgs"),
        format!(
            "#!/usr/bin/env python3\n{}\n{}",
            include_str!("fixture.py"),
            include_str!("../fake_hgs.py")
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(f.0.join("hgs"), std::fs::Permissions::from_mode(0o700)).unwrap();
    f
}
/// Every task message request the fake `hgs` saw, in order.
fn calls(root: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("message-calls"))
        .unwrap_or_default()
        .lines()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}
fn count(root: &std::path::Path, op: &str) -> usize {
    calls(root)
        .iter()
        .filter(|c| c["value"]["request"]["op"] == op || c["value"]["op"] == op)
        .count()
}
pub(in crate::desktop_service) async fn connected(f: &Fixture) -> (Arc<Service>, CacheServer) {
    let responder = responder();
    assert!(responder
        .configure_ownership_mode_once(zerus_client_core::desktop::ownership::Mode::Service));
    let cache = CacheServer::start(&f.0, responder.clone());
    let (service, _) = f.start_with_responder(true, Some(responder));
    eventually("task message service connected", || {
        service.snapshot().connected
    })
    .await;
    (service, cache)
}
/// A canonical record with one attachment of `size` raw bytes.
fn with_attachment(size: usize) -> Value {
    let shot = zerus_client_core::attachments::Attachment {
        name: "screen.png".into(),
        mime: "image/png".into(),
        bytes: (0..size)
            .map(|n| (n % 251) as u8)
            .collect::<Vec<_>>()
            .into(),
        reference: "[screen.png]".into(),
    };
    let intent = message_intent("see this", &[shot], "queue", "");
    json!({"id": uuid::Uuid::new_v4().to_string(), "intent": intent,
        "input_sha256": input_digest(&message_input(&intent))})
}
/// The staging folder the fake service names, private like the real one.
fn staging(root: &std::path::Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = root.join("staging");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}
/// A canonical record as the editor reserves it, with `padding` bytes of
/// text.
fn record(padding: usize) -> Value {
    let intent = message_intent(&"x".repeat(padding.max(1)), &[], "queue", "");
    json!({"id": uuid::Uuid::new_v4().to_string(), "intent": intent,
        "input_sha256": input_digest(&message_input(&intent))})
}
/// What one call did: its answer, the task message requests the fake had
/// seen when the handoff ran (`None`: the handoff never ran) and how often
/// the original was released.
struct Sent {
    answer: MessageAnswer,
    handoff: Option<usize>,
    released: usize,
}
async fn send(
    service: Arc<Service>,
    root: &std::path::Path,
    record: Value,
    fresh: bool,
    handoff_ok: bool,
) -> Sent {
    let root = root.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let seen = std::sync::Mutex::new(None);
        let released = AtomicUsize::new(0);
        let answer = call(
            Some(service),
            "example",
            TASK,
            &record,
            fresh,
            Outbox {
                handoff: &|| {
                    *seen.lock().unwrap() = Some(count(&root, "task_message"));
                    if handoff_ok {
                        Ok(())
                    } else {
                        Err("the handoff could not be recorded".into())
                    }
                },
                release: &|| {
                    released.fetch_add(1, Ordering::SeqCst);
                },
            },
            &AtomicBool::new(false),
        );
        Sent {
            answer,
            handoff: seen.into_inner().unwrap(),
            released: released.load(Ordering::SeqCst),
        }
    })
    .await
    .unwrap()
}

#[test]
fn an_original_refused_before_its_request_is_unsent_released_and_never_sent() {
    if !private_case("desktop_service::task_message::fixture_tests::an_original_refused_before_its_request_is_unsent_released_and_never_sent") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        // Stale consent: no handoff and no request.
        let sent = send(service.clone(), &f.0, record(10), false, true).await;
        assert_eq!(sent.answer, MessageAnswer::Unsent(NOT_SENT.into()));
        assert_eq!((sent.handoff, sent.released), (None, 1));
        // A handoff that cannot be recorded: no request.
        let sent = send(service.clone(), &f.0, record(10), true, false).await;
        assert_eq!(sent.answer, MessageAnswer::Unsent(NOT_SENT.into()));
        assert_eq!((sent.handoff, sent.released), (Some(0), 1));
        // Too large for the control frame: no handoff and no request.
        let sent = send(service.clone(), &f.0, record(70 << 10), true, true).await;
        assert_eq!(sent.answer, MessageAnswer::Unsent(TOO_LARGE.into()));
        assert_eq!((sent.handoff, sent.released), (None, 1));
        assert!(calls(&f.0).is_empty(), "nothing reached the service");
        service.join().await.unwrap();
    });
}

#[test]
fn a_lost_reply_is_handed_off_before_its_one_request_and_stays_unconfirmed() {
    if !private_case("desktop_service::task_message::fixture_tests::a_lost_reply_is_handed_off_before_its_one_request_and_stays_unconfirmed") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let sent = send(service.clone(), &f.0, record(10), true, true).await;
        // The marker was written before any request, then exactly one
        // request and one scoped lookup; the answer is never "not sent".
        assert_eq!(sent.handoff, Some(0));
        assert_eq!(count(&f.0, "task_message"), 1);
        assert_eq!(count(&f.0, "task_message_result"), 1);
        assert_eq!(sent.answer, MessageAnswer::Unconfirmed(UNCONFIRMED.into()));
        assert_eq!(sent.released, 0, "a sent original is never released");
        service.join().await.unwrap();
    });
}

/// The staged copies of `record` that exist right now.
fn copies(root: &std::path::Path, record: &Value) -> usize {
    std::fs::read_dir(root.join("staging").join(record["id"].as_str().unwrap()))
        .map(|dir| dir.count())
        .unwrap_or(0)
}

#[test]
fn an_oversize_original_is_staged_handed_off_and_offered_by_reference() {
    if !private_case("desktop_service::task_message::fixture_tests::an_oversize_original_is_staged_handed_off_and_offered_by_reference") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"message_references": true, "message_staging": true}));
        staging(&f.0);
        let (service, _cache) = connected(&f).await;
        let record = with_attachment(70 << 10);
        let id = record["id"].as_str().unwrap().to_owned();
        let sent = send(service.clone(), &f.0, record.clone(), true, true).await;
        // Staged and handed off before the one request, which names the form.
        assert_eq!(sent.handoff, Some(0));
        assert_eq!(copies(&f.0, &record), 1, "the copy stays for the service");
        let requests: Vec<Value> = calls(&f.0)
            .into_iter()
            .filter(|c| c["value"]["request"]["op"] == "task_message")
            .collect();
        assert_eq!(requests.len(), 1);
        let intent = &requests[0]["value"]["request"]["intent"];
        assert_eq!(intent["form"], "reference-v1");
        assert_eq!(intent["request_id"], id.as_str());
        assert!(!intent["original"].as_str().unwrap().contains("data_base64"));
        assert_eq!(count(&f.0, "task_message_result"), 1);
        assert_eq!(sent.answer, MessageAnswer::Unconfirmed(UNCONFIRMED.into()));
        assert_eq!(sent.released, 0);
        // A later check of the same record asks for the same original by
        // its one plan, sends nothing, and an unsettled result stays open.
        let checked = {
            let (service, record) = (service.clone(), record.clone());
            tauri::async_runtime::spawn_blocking(move || {
                check(Some(service), "example", TASK, &record)
            })
            .await
            .unwrap()
        };
        assert_eq!(checked, None);
        let queries: Vec<Value> = calls(&f.0)
            .into_iter()
            .map(|c| match c["value"].get("request") {
                Some(request) => request.clone(),
                None => c["value"].clone(),
            })
            .filter(|request| request["op"] == "task_message_result")
            .map(|request| request["query"].clone())
            .collect();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0], queries[1]);
        assert_eq!(count(&f.0, "task_message"), 1);
        service.join().await.unwrap();
    });
}

#[test]
fn an_oversize_original_that_cannot_be_staged_is_never_sent_and_leaves_no_copy() {
    if !private_case("desktop_service::task_message::fixture_tests::an_oversize_original_that_cannot_be_staged_is_never_sent_and_leaves_no_copy") {
        return;
    }
    tauri::async_runtime::block_on(async {
        // No reference support: refused as too large, nothing staged.
        let f = fixture(json!({}));
        staging(&f.0);
        let (service, _cache) = connected(&f).await;
        let record = with_attachment(70 << 10);
        let sent = send(service.clone(), &f.0, record.clone(), true, true).await;
        assert_eq!(sent.answer, MessageAnswer::Unsent(TOO_LARGE.into()));
        assert_eq!(
            (sent.handoff, sent.released, copies(&f.0, &record)),
            (None, 1, 0)
        );
        service.join().await.unwrap();
        // Supported, but too large even by reference: its text alone exceeds
        // the control frame. Refused before staging, nothing sent.
        let f = fixture(json!({"message_references": true, "message_staging": true}));
        staging(&f.0);
        let (service, _cache) = connected(&f).await;
        let mut record = with_attachment(70 << 10);
        record["intent"]["content"]["text"] = json!("x".repeat(70 << 10));
        record["input_sha256"] = json!(input_digest(&message_input(&record["intent"])));
        let sent = send(service.clone(), &f.0, record.clone(), true, true).await;
        assert_eq!(
            sent.answer,
            MessageAnswer::Unsent(TOO_LARGE_REFERENCE.into())
        );
        assert_eq!(
            (sent.handoff, sent.released, copies(&f.0, &record)),
            (None, 1, 0)
        );
        assert!(calls(&f.0)
            .iter()
            .all(|c| c["value"]["request"]["op"] != "task_message_staging"
                && c["value"]["op"] != "task_message_staging"));
        service.join().await.unwrap();
        // Supported, but a record frozen with its stdin instead of the
        // canonical digest has no reference form: refused before staging.
        let f = fixture(json!({"message_references": true, "message_staging": true}));
        staging(&f.0);
        let (service, _cache) = connected(&f).await;
        let mut record = with_attachment(70 << 10);
        let input = message_input(&record["intent"]);
        record.as_object_mut().unwrap().remove("input_sha256");
        record["input"] = json!(input);
        let sent = send(service.clone(), &f.0, record.clone(), true, true).await;
        assert_eq!(
            sent.answer,
            MessageAnswer::Unsent(TOO_LARGE_REFERENCE.into())
        );
        assert_eq!(
            (sent.handoff, sent.released, copies(&f.0, &record)),
            (None, 1, 0)
        );
        service.join().await.unwrap();
        // Supported, but no staging folder is named: never sent.
        let f = fixture(json!({"message_references": true}));
        staging(&f.0);
        let (service, _cache) = connected(&f).await;
        let record = with_attachment(70 << 10);
        let sent = send(service.clone(), &f.0, record.clone(), true, true).await;
        assert_eq!(sent.answer, MessageAnswer::Unsent(NOT_SENT.into()));
        assert_eq!((sent.handoff, copies(&f.0, &record)), (None, 0));
        service.join().await.unwrap();
        // Staged, then a failed handoff: exactly its copies go, nothing sent.
        let f = fixture(json!({"message_references": true, "message_staging": true}));
        staging(&f.0);
        let (service, _cache) = connected(&f).await;
        let record = with_attachment(70 << 10);
        let sent = send(service.clone(), &f.0, record.clone(), true, false).await;
        assert_eq!(sent.answer, MessageAnswer::Unsent(NOT_SENT.into()));
        assert_eq!(sent.handoff, Some(0));
        assert_eq!(copies(&f.0, &record), 0, "exactly its own copies went");
        assert!(!f
            .0
            .join("staging")
            .join(record["id"].as_str().unwrap())
            .exists());
        assert_eq!(count(&f.0, "task_message"), 0);
        service.join().await.unwrap();
    });
}

#[test]
fn an_offered_original_that_never_started_keeps_its_copies_for_the_service() {
    if !private_case("desktop_service::task_message::fixture_tests::an_offered_original_that_never_started_keeps_its_copies_for_the_service") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"message_references": true, "message_staging": true}));
        staging(&f.0);
        let (service, _cache) = connected(&f).await;
        let record = with_attachment(70 << 10);
        // The service's drained not-started receipt for exactly this
        // original, offered by reference.
        let receipt = {
            let (service, record) = (service.clone(), record.clone());
            tauri::async_runtime::spawn_blocking(move || {
                let read = Read::new(Some(service)).unwrap();
                let (projection, _) = references::project(&record).unwrap();
                let verified = wire::Intent {
                    original: projection.to_string(),
                    form: wire::Form::ReferenceV1,
                    ..intent(&read.scope, "example", TASK, &record)
                }
                .verify()
                .unwrap();
                wire::Receipt {
                    binding: verified.binding,
                    admission_epoch: read.scope.service_epoch.clone(),
                    revision: 2,
                    phase: wire::Phase::Failed,
                    drained: true,
                    code: wire::NOT_STARTED.into(),
                    outcome: None,
                }
            })
            .await
            .unwrap()
        };
        let path = f.0.join("fixture.json");
        let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        config["message_receipt"] = serde_json::to_value(&receipt).unwrap();
        std::fs::write(&path, config.to_string()).unwrap();
        let sent = send(service.clone(), &f.0, record.clone(), true, true).await;
        // Never ran, so it is released; but it was offered, so its copies
        // are the service's to remove, never this client's.
        assert_eq!(sent.answer, MessageAnswer::Unsent(NOT_STARTED.into()));
        assert_eq!((sent.handoff, sent.released), (Some(0), 1));
        assert_eq!(copies(&f.0, &record), 1);
        assert_eq!(count(&f.0, "task_message"), 1);
        service.join().await.unwrap();
    });
}

#[test]
fn a_service_without_the_kind_takes_the_direct_path_without_a_handoff() {
    if !private_case("desktop_service::task_message::fixture_tests::a_service_without_the_kind_takes_the_direct_path_without_a_handoff") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"message_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        let sent = send(service.clone(), &f.0, record(10), true, true).await;
        assert_eq!(sent.answer, MessageAnswer::Direct);
        assert_eq!((sent.handoff, sent.released), (None, 0));
        assert!(calls(&f.0).is_empty());
        service.join().await.unwrap();
    });
}
