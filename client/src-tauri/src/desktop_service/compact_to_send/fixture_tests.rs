//! "Compact and continue" past the capability probe, against the real
//! service fixture with a fake `hgs` that records every compact-to-send
//! request.
use super::tests::{original, receipt};
use super::*;
use crate::desktop_service::{
    task_message::fixture_tests::{connected, private_case},
    tests::Fixture,
};
use wire::Step::*;

const SESSION: &str = "claude/private-compact";

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
/// Every compact-to-send request the fake `hgs` saw.
fn calls(root: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("compact-calls"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}
/// How many requests of `op` the fake `hgs` saw.
fn count(root: &std::path::Path, op: &str) -> usize {
    calls(root)
        .iter()
        .filter(|call| call["value"]["request"]["op"] == op || call["value"]["op"] == op)
        .count()
}
/// Writes the service's answers into the fixture's config.
fn answers(root: &std::path::Path, admission: Option<Value>, receipts: &[Value]) {
    let path = root.join("fixture.json");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    if let Some(admission) = admission {
        config["compact_admission"] = admission;
    }
    config["compact_receipts"] = json!(receipts);
    std::fs::write(&path, config.to_string()).unwrap();
    let _ = std::fs::remove_file(root.join("compact-lookups"));
}
async fn offer(
    service: Arc<Service>,
    session: &str,
    original: (Checkpoint, Vec<u8>, Vec<u8>),
) -> CompactAnswer {
    offer_handing(service, session, original, true).await.0
}
/// Offers `original` with a hand-over hook that answers `hand`, and returns
/// the plans the hook saw.
async fn offer_handing(
    service: Arc<Service>,
    session: &str,
    original: (Checkpoint, Vec<u8>, Vec<u8>),
    hand: bool,
) -> (CompactAnswer, Vec<wire::Plan>) {
    let session = session.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let (checkpoint, compaction, send) = original;
        let seen = std::cell::RefCell::new(Vec::new());
        let answer = call(
            Some(service),
            &session,
            &checkpoint,
            &compaction,
            &send,
            &|plan| {
                seen.borrow_mut().push(plan.clone());
                hand
            },
            &AtomicBool::new(false),
        );
        (answer, seen.into_inner())
    })
    .await
    .unwrap()
}
fn settled_as(answer: SendAnswer) -> CompactAnswer {
    CompactAnswer::Settled(answer)
}
async fn checked(service: &Arc<Service>, plan: &wire::Plan) -> Checked {
    let (service, plan) = (service.clone(), plan.clone());
    tauri::async_runtime::spawn_blocking(move || check(Some(service), &plan))
        .await
        .unwrap()
}
/// The original's verification under the connected service's own scope.
async fn verified(
    service: &Arc<Service>,
    original: &(Checkpoint, Vec<u8>, Vec<u8>),
) -> wire::Verified {
    let (service, (checkpoint, compaction, send)) = (service.clone(), original.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let read = Read::new(Some(service)).unwrap();
        intent(&read.scope, SESSION, &checkpoint, &compaction, &send)
            .unwrap()
            .verify()
            .unwrap()
    })
    .await
    .unwrap()
}
fn admit(verified: &wire::Verified, receipt: Option<&wire::Receipt>, accepted: bool) -> Value {
    json!({"abi": wire::ABI, "request_id": verified.plan.compaction_request_id,
        "scope": verified.intent.scope, "accepted": accepted, "durable": accepted,
        "code": if accepted { "" } else { "compact_to_send_unavailable" }, "receipt": receipt})
}

#[test]
fn one_the_service_does_not_take_goes_direct_before_any_request() {
    if !private_case("desktop_service::compact_to_send::fixture_tests::one_the_service_does_not_take_goes_direct_before_any_request") {
        return;
    }
    tauri::async_runtime::block_on(async {
        // A service without the kind.
        let f = fixture(json!({"compact_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            offer(service.clone(), SESSION, original("hello")).await,
            CompactAnswer::Direct
        );
        service.join().await.unwrap();
        // The wire's own refusals before any offer: a DeepSeek session and
        // an original too large for the control frame.
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            offer(service.clone(), "dsh/example", original("hello")).await,
            CompactAnswer::Direct
        );
        // Each quote doubles in the stdin and again in the control line.
        assert_eq!(
            offer(service.clone(), SESSION, original(&"\"".repeat(20 << 10))).await,
            CompactAnswer::Direct
        );
        assert!(calls(&f.0).is_empty(), "nothing was offered");
        service.join().await.unwrap();
    });
}

/// A lost admission is looked up once offered and never becomes unsent;
/// the original goes only on the frontend lane, the lookups only on the
/// read-only request lane.
#[test]
fn a_lost_reply_is_looked_up_on_its_own_lane_and_never_becomes_unsent() {
    if !private_case("desktop_service::compact_to_send::fixture_tests::a_lost_reply_is_looked_up_on_its_own_lane_and_never_becomes_unsent") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let answer = offer(service.clone(), SESSION, original("hello")).await;
        assert_eq!(
            answer,
            settled_as(SendAnswer::Unconfirmed(UNCONFIRMED.into()))
        );
        let lane = |op: &str| -> Vec<String> {
            calls(&f.0)
                .iter()
                .filter(|c| c["value"]["request"]["op"] == op || c["value"]["op"] == op)
                .map(|c| c["args"][1].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(lane("compact_to_send"), ["frontend-request"]);
        assert_eq!(lane("compact_to_send_result"), ["request"]);
        service.join().await.unwrap();
    });
}

/// zerus-tlb9q: the service's exact refusal before admission never started:
/// Unsent with the draft kept, with no lookup and no second offer. The same
/// code for another request or scope, durable or with a receipt is no reply
/// of this original, so only the lookup decides.
#[test]
fn an_exact_not_admitted_refusal_is_unsent_without_a_lookup() {
    if !private_case("desktop_service::compact_to_send::fixture_tests::an_exact_not_admitted_refusal_is_unsent_without_a_lookup") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let refusal = |verified: &wire::Verified| {
            let mut reply = admit(verified, None, false);
            reply["code"] = json!(wire::NOT_ADMITTED);
            reply
        };
        let first = original("not admitted");
        let v = verified(&service, &first).await;
        answers(&f.0, Some(refusal(&v)), &[]);
        assert_eq!(
            offer(service.clone(), SESSION, first).await,
            settled_as(SendAnswer::Unsent(NOT_STARTED.into()))
        );
        assert_eq!(
            (
                count(&f.0, "compact_to_send"),
                count(&f.0, "compact_to_send_result")
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
                r["receipt"] = json!(receipt(v, NotStarted, NotStarted, true))
            }),
        ];
        for (n, (name, change)) in changes.into_iter().enumerate() {
            let next = original(&format!("changed {name}"));
            let v = verified(&service, &next).await;
            let mut reply = refusal(&v);
            change(&mut reply, &v);
            answers(&f.0, Some(reply), &[]);
            assert_eq!(
                offer(service.clone(), SESSION, next).await,
                settled_as(SendAnswer::Unconfirmed(UNCONFIRMED.into())),
                "{name}"
            );
            assert_eq!(count(&f.0, "compact_to_send_result"), n + 1, "{name}");
        }
        service.join().await.unwrap();
    });
}

#[test]
fn the_admission_or_the_lookup_settles_and_an_unsettled_one_is_pending() {
    if !private_case("desktop_service::compact_to_send::fixture_tests::the_admission_or_the_lookup_settles_and_an_unsettled_one_is_pending") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        // A refusal is no reply of this original: only the lookup decides,
        // and nothing found is never "not sent".
        let refused = original("refused");
        let v = verified(&service, &refused).await;
        answers(&f.0, Some(admit(&v, None, false)), &[]);
        assert_eq!(
            offer(service.clone(), SESSION, refused).await,
            settled_as(SendAnswer::Unconfirmed(UNCONFIRMED.into()))
        );
        assert_eq!(count(&f.0, "compact_to_send_result"), 1);
        // Settled in the admission reply itself.
        let sent = original("sent");
        let v = verified(&service, &sent).await;
        let recorded = receipt(&v, Recorded, Recorded, true);
        answers(&f.0, Some(admit(&v, Some(&recorded), true)), &[]);
        assert!(matches!(
            offer(service.clone(), SESSION, sent).await,
            CompactAnswer::Settled(SendAnswer::Recorded(_))
        ));
        // Compacting, then the wait, then a send that never started after
        // a submitted compaction, through the lookup.
        let waited = original("waited");
        let v = verified(&service, &waited).await;
        answers(
            &f.0,
            Some(admit(&v, Some(&receipt(&v, Running, Queued, false)), true)),
            &[
                json!(receipt(&v, Recorded, Queued, false)),
                json!(receipt(&v, Recorded, NotStarted, true)),
            ],
        );
        assert_eq!(
            offer(service.clone(), SESSION, waited).await,
            settled_as(SendAnswer::Unsent(COMPACTION_UNCONFIRMED.into()))
        );
        // Still waiting past the bound: pending, checked again read-only.
        let slow = original("slow");
        let v = verified(&service, &slow).await;
        let waiting = receipt(&v, Recorded, Queued, false);
        answers(
            &f.0,
            Some(admit(&v, Some(&waiting), true)),
            &[json!(waiting)],
        );
        let offered = count(&f.0, "compact_to_send");
        assert_eq!(
            offer(service.clone(), SESSION, slow).await,
            CompactAnswer::Pending(v.plan.clone())
        );
        assert_eq!(checked(&service, &v.plan).await, Checked::InProgress);
        answers(&f.0, None, &[json!(receipt(&v, Recorded, Unknown, true))]);
        assert_eq!(
            checked(&service, &v.plan).await,
            Checked::Settled(SendAnswer::Unconfirmed(UNCONFIRMED.into()))
        );
        assert_eq!(
            count(&f.0, "compact_to_send"),
            offered + 1,
            "a check never offers"
        );
        service.join().await.unwrap();
        // A check against a service without the kind knows nothing.
        let f = fixture(json!({"compact_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(checked(&service, &v.plan).await, Checked::Unavailable);
        assert!(calls(&f.0).is_empty());
        service.join().await.unwrap();
    });
}

/// The hand-over hook runs once with the exact plan before anything reaches
/// the service; a refusal there sends nothing and never ran.
#[test]
fn a_refused_hand_over_sends_nothing_and_never_ran() {
    if !private_case("desktop_service::compact_to_send::fixture_tests::a_refused_hand_over_sends_nothing_and_never_ran") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let refused = original("refused");
        let v = verified(&service, &refused).await;
        let (answer, seen) = offer_handing(service.clone(), SESSION, refused, false).await;
        assert_eq!(answer, settled_as(SendAnswer::Unsent(NOT_SENT.into())));
        assert_eq!(seen, std::slice::from_ref(&v.plan));
        assert!(calls(&f.0).is_empty(), "nothing reached the service");
        // A direct decision never reaches the hook.
        let (answer, seen) =
            offer_handing(service.clone(), "dsh/example", original("x"), true).await;
        assert_eq!(answer, CompactAnswer::Direct);
        assert!(seen.is_empty());
        service.join().await.unwrap();
    });
}
