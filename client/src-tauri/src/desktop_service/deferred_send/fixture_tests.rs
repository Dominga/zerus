//! The composer's Send past the capability probe, against the real service
//! fixture with a fake `hgs` that records every deferred send request.
use super::tests::original;
use super::*;
use crate::desktop_service::{
    task_message::fixture_tests::{connected, private_case},
    tests::Fixture,
};

const SESSION: &str = "claude/private-send";

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
/// How many requests of `op` the fake `hgs` saw.
fn count(root: &std::path::Path, op: &str) -> usize {
    std::fs::read_to_string(root.join("send-calls"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|call| call["value"]["request"]["op"] == op || call["value"]["op"] == op)
        .count()
}
/// Writes the service's answers into the fixture's config.
fn answers(root: &std::path::Path, admission: Option<Value>, receipts: &[Value]) {
    let path = root.join("fixture.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    if let Some(admission) = admission {
        config["send_admission"] = admission;
    }
    config["send_receipts"] = json!(receipts);
    std::fs::write(&path, config.to_string()).unwrap();
}
async fn send(
    service: Arc<Service>,
    session: &str,
    checkpoint: Checkpoint,
    stdin: Vec<u8>,
) -> SendAnswer {
    let session = session.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        call(
            Some(service),
            &session,
            &checkpoint,
            &stdin,
            &AtomicBool::new(false),
        )
    })
    .await
    .unwrap()
}
/// The original's verification under the connected service's own scope.
async fn verified(service: &Arc<Service>, checkpoint: &Checkpoint, stdin: &[u8]) -> wire::Verified {
    let (service, checkpoint, stdin) = (service.clone(), checkpoint.clone(), stdin.to_vec());
    tauri::async_runtime::spawn_blocking(move || {
        let read = Read::new(Some(service)).unwrap();
        intent(&read.scope, SESSION, &checkpoint, &stdin)
            .unwrap()
            .verify()
            .unwrap()
    })
    .await
    .unwrap()
}
fn receipt(verified: &wire::Verified, phase: wire::Phase, drained: bool, code: &str) -> Value {
    json!(wire::Receipt {
        binding: verified.binding.clone(),
        admission_epoch: verified.intent.scope.service_epoch.clone(),
        revision: 3,
        phase,
        drained,
        code: code.into(),
        outcome: (phase == wire::Phase::Recorded).then(|| wire::Outcome {
            request_id: verified.plan.request_id.clone(),
            status: wire::SUBMITTED.into(),
            input_sha256: verified.plan.input_sha256.clone(),
        }),
    })
}

#[test]
fn a_send_the_service_does_not_take_goes_direct_before_any_request() {
    if !private_case("desktop_service::deferred_send::fixture_tests::a_send_the_service_does_not_take_goes_direct_before_any_request") {
        return;
    }
    tauri::async_runtime::block_on(async {
        // A service without the kind.
        let f = fixture(json!({"send_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        let (checkpoint, stdin) = original("hello");
        assert_eq!(
            send(service.clone(), SESSION, checkpoint, stdin).await,
            SendAnswer::Direct
        );
        assert_eq!(count(&f.0, "deferred_send"), 0);
        service.join().await.unwrap();
        // The wire's own refusals before any offer: a DeepSeek session and
        // an original too large for the control frame.
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (checkpoint, stdin) = original("hello");
        assert_eq!(
            send(service.clone(), "dsh/example", checkpoint, stdin).await,
            SendAnswer::Direct
        );
        // Each quote doubles in the stdin and again in the control line.
        let (checkpoint, stdin) = original(&"\"".repeat(20 << 10));
        assert_eq!(
            send(service.clone(), SESSION, checkpoint, stdin).await,
            SendAnswer::Direct
        );
        assert_eq!(count(&f.0, "deferred_send"), 0, "nothing was offered");
        service.join().await.unwrap();
    });
}

#[test]
fn a_lost_reply_is_looked_up_once_offered_and_never_becomes_unsent() {
    if !private_case("desktop_service::deferred_send::fixture_tests::a_lost_reply_is_looked_up_once_offered_and_never_becomes_unsent") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (checkpoint, stdin) = original("hello");
        let answer = send(service.clone(), SESSION, checkpoint, stdin).await;
        // One request, then the scoped lookup, which finds nothing.
        assert_eq!(count(&f.0, "deferred_send"), 1);
        assert_eq!(count(&f.0, "deferred_send_result"), 1);
        assert_eq!(answer, SendAnswer::Unconfirmed(UNCONFIRMED.into()));
        service.join().await.unwrap();
    });
}

#[test]
fn the_admission_or_the_lookup_settles_and_an_unsettled_send_is_pending() {
    if !private_case("desktop_service::deferred_send::fixture_tests::the_admission_or_the_lookup_settles_and_an_unsettled_send_is_pending") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let admit = |verified: &wire::Verified, receipt: Option<Value>, accepted: bool| {
            json!({"abi": wire::ABI, "request_id": verified.plan.request_id,
                "scope": verified.intent.scope, "accepted": accepted, "durable": accepted,
                "code": if accepted { "" } else { "deferred_send_unavailable" }, "receipt": receipt})
        };
        // A refusal is no reply of this original: only the lookup decides,
        // and nothing found is never "not sent".
        let (checkpoint, stdin) = original("refused");
        let v = verified(&service, &checkpoint, &stdin).await;
        answers(&f.0, Some(admit(&v, None, false)), &[]);
        assert_eq!(
            send(service.clone(), SESSION, checkpoint, stdin).await,
            SendAnswer::Unconfirmed(UNCONFIRMED.into())
        );
        assert_eq!(count(&f.0, "deferred_send_result"), 1);
        // Settled in the admission reply itself: the same original again.
        let (checkpoint, stdin) = original("again");
        let v = verified(&service, &checkpoint, &stdin).await;
        let recorded = receipt(&v, wire::Phase::Recorded, true, "");
        answers(&f.0, Some(admit(&v, Some(recorded), true)), &[]);
        assert!(matches!(
            send(service.clone(), SESSION, checkpoint, stdin).await,
            SendAnswer::Recorded(_)
        ));
        // Running, then never started, through the lookup.
        let (checkpoint, stdin) = original("not started");
        let v = verified(&service, &checkpoint, &stdin).await;
        let running = receipt(&v, wire::Phase::Running, false, "");
        answers(
            &f.0,
            Some(admit(&v, Some(running.clone()), true)),
            &[
                running.clone(),
                receipt(&v, wire::Phase::Failed, true, wire::NOT_STARTED),
            ],
        );
        let _ = std::fs::remove_file(f.0.join("send-lookups"));
        assert_eq!(
            send(service.clone(), SESSION, checkpoint, stdin).await,
            SendAnswer::Unsent(NOT_STARTED.into())
        );
        // Still running past the bound: pending, checked again read-only.
        let (checkpoint, stdin) = original("slow");
        let v = verified(&service, &checkpoint, &stdin).await;
        let running = receipt(&v, wire::Phase::Running, false, "");
        answers(
            &f.0,
            Some(admit(&v, Some(running.clone()), true)),
            &[running],
        );
        let _ = std::fs::remove_file(f.0.join("send-lookups"));
        let offered = count(&f.0, "deferred_send");
        let answer = send(service.clone(), SESSION, checkpoint, stdin).await;
        assert_eq!(answer, SendAnswer::Pending(v.plan.clone()));
        let checked = {
            let (service, plan) = (service.clone(), v.plan.clone());
            tauri::async_runtime::spawn_blocking(move || check(Some(service), &plan))
                .await
                .unwrap()
        };
        assert_eq!(checked, Checked::InProgress);
        answers(
            &f.0,
            None,
            &[receipt(
                &v,
                wire::Phase::Unknown,
                true,
                "deferred_send_receipt_missing",
            )],
        );
        let checked = {
            let (service, plan) = (service.clone(), v.plan.clone());
            tauri::async_runtime::spawn_blocking(move || check(Some(service), &plan))
                .await
                .unwrap()
        };
        assert_eq!(
            checked,
            Checked::Settled(SendAnswer::Unconfirmed(UNCONFIRMED.into()))
        );
        assert_eq!(
            count(&f.0, "deferred_send"),
            offered + 1,
            "a check never offers"
        );
        service.join().await.unwrap();
    });
}

/// zerus-tlb9q: the service's exact refusal before admission never started:
/// Unsent with the draft kept, with no lookup and no second offer. The same
/// code for another request or scope, durable or with a receipt is no reply
/// of this original, so only the lookup decides.
#[test]
fn an_exact_not_admitted_refusal_is_unsent_without_a_lookup() {
    if !private_case("desktop_service::deferred_send::fixture_tests::an_exact_not_admitted_refusal_is_unsent_without_a_lookup") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let refusal = |verified: &wire::Verified| {
            json!({"abi": wire::ABI, "request_id": verified.plan.request_id,
                "scope": verified.intent.scope, "accepted": false, "durable": false,
                "code": wire::NOT_ADMITTED, "receipt": null})
        };
        let (checkpoint, stdin) = original("not admitted");
        let v = verified(&service, &checkpoint, &stdin).await;
        answers(&f.0, Some(refusal(&v)), &[]);
        assert_eq!(
            send(service.clone(), SESSION, checkpoint, stdin).await,
            SendAnswer::Unsent(NOT_STARTED.into())
        );
        assert_eq!(
            (
                count(&f.0, "deferred_send"),
                count(&f.0, "deferred_send_result")
            ),
            (1, 0)
        );
        type Change = fn(&mut Value, &wire::Verified);
        let changes: [(&str, Change); 5] = [
            ("request", |r, _| {
                r["request_id"] = json!("00000000-0000-4000-8000-0000000000ff")
            }),
            ("scope", |r, _| {
                r["scope"]["service_epoch"] = json!("00000000-0000-4000-8000-0000000000fe")
            }),
            ("durable", |r, _| r["durable"] = json!(true)),
            ("abi", |r, _| r["abi"] = json!("other")),
            ("receipt", |r, v| {
                r["receipt"] = receipt(v, wire::Phase::Failed, true, wire::NOT_STARTED)
            }),
        ];
        for (n, (name, change)) in changes.into_iter().enumerate() {
            let (checkpoint, stdin) = original(&format!("changed {name}"));
            let v = verified(&service, &checkpoint, &stdin).await;
            let mut reply = refusal(&v);
            change(&mut reply, &v);
            answers(&f.0, Some(reply), &[]);
            assert_eq!(
                send(service.clone(), SESSION, checkpoint, stdin).await,
                SendAnswer::Unconfirmed(UNCONFIRMED.into()),
                "{name}"
            );
            assert_eq!(count(&f.0, "deferred_send_result"), n + 1, "{name}");
        }
        service.join().await.unwrap();
    });
}

/// review-3 (N, L): a check against a service without the kind knows
/// nothing, and each call uses its own lane: the original only the frontend
/// lane, the lookups only the read-only request lane.
#[test]
fn a_check_without_the_kind_knows_nothing_and_each_call_uses_its_lane() {
    if !private_case("desktop_service::deferred_send::fixture_tests::a_check_without_the_kind_knows_nothing_and_each_call_uses_its_lane") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"send_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        let (checkpoint, stdin) = original("hello");
        let plan = super::plan(SESSION, &checkpoint, &stdin).unwrap();
        let s = service.clone();
        let checked = tauri::async_runtime::spawn_blocking(move || check(Some(s), &plan))
            .await
            .unwrap();
        assert_eq!(checked, Checked::Unavailable, "missing marker");
        service.join().await.unwrap();
        // A lost admission: the original only on the frontend lane, the
        // lookups only on the read-only request lane.
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (checkpoint, stdin) = original("hello");
        assert_eq!(
            send(service.clone(), SESSION, checkpoint, stdin).await,
            SendAnswer::Unconfirmed(UNCONFIRMED.into())
        );
        let calls: Vec<Value> = std::fs::read_to_string(f.0.join("send-calls"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let lane = |op: &str| -> Vec<String> {
            calls
                .iter()
                .filter(|c| c["value"]["request"]["op"] == op || c["value"]["op"] == op)
                .map(|c| c["args"][1].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(lane("deferred_send"), ["frontend-request"]);
        let lookups = lane("deferred_send_result");
        assert!(!lookups.is_empty() && lookups.iter().all(|a| a == "request"));
        service.join().await.unwrap();
    });
}
