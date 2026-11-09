//! "Resume and send" past the capability probe, against the real
//! service fixture with a fake `hgs` that records every session resume
//! request.
use super::tests::{original, receipt, RESUMED};
use super::*;
use crate::desktop_service::{
    task_message::fixture_tests::{connected, private_case},
    tests::Fixture,
};
use wire::Step::*;

const SESSION: &str = "claude/private-resume";

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
/// Every session resume request the fake `hgs` saw.
fn calls(root: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("resume-calls"))
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
        config["resume_admission"] = admission;
    }
    config["resume_receipts"] = json!(receipts);
    std::fs::write(&path, config.to_string()).unwrap();
    let _ = std::fs::remove_file(root.join("resume-lookups"));
}
async fn offer(
    service: Arc<Service>,
    session: &str,
    original: (Checkpoint, Vec<u8>, Vec<u8>),
) -> ResumeAnswer {
    offer_handing(service, session, original, true).await.0
}
/// Offers `original` with a hand-over hook that answers `hand`, and returns
/// the plans the hook saw.
async fn offer_handing(
    service: Arc<Service>,
    session: &str,
    original: (Checkpoint, Vec<u8>, Vec<u8>),
    hand: bool,
) -> (ResumeAnswer, Vec<wire::Plan>) {
    let session = session.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let (checkpoint, resume, send) = original;
        let seen = std::cell::RefCell::new(Vec::new());
        let answer = call(
            Some(service),
            &session,
            &checkpoint,
            &resume,
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
fn settled_as(answer: SendAnswer) -> ResumeAnswer {
    ResumeAnswer::Settled(answer, None)
}
/// Settled after a recorded resume: with the run its receipt returned.
fn resumed_as(answer: SendAnswer) -> ResumeAnswer {
    ResumeAnswer::Settled(answer, Some(RESUMED.into()))
}
async fn checked(service: &Arc<Service>, plan: &wire::Plan) -> (Checked, Option<String>) {
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
    let (service, (checkpoint, resume, send)) = (service.clone(), original.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let read = Read::new(Some(service)).unwrap();
        intent(&read.scope, SESSION, &checkpoint, &resume, &send)
            .unwrap()
            .verify()
            .unwrap()
    })
    .await
    .unwrap()
}
fn admit(verified: &wire::Verified, receipt: Option<&wire::Receipt>, accepted: bool) -> Value {
    json!({"abi": wire::ABI, "request_id": verified.plan.resume_request_id,
        "scope": verified.intent.scope, "accepted": accepted, "durable": accepted,
        "code": if accepted { "" } else { "session_resume_unavailable" }, "receipt": receipt})
}

#[test]
fn one_the_service_does_not_take_goes_direct_before_any_request() {
    if !private_case("desktop_service::session_resume::fixture_tests::one_the_service_does_not_take_goes_direct_before_any_request") {
        return;
    }
    tauri::async_runtime::block_on(async {
        // A service without the kind.
        let f = fixture(json!({"resume_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            offer(service.clone(), SESSION, original("hello")).await,
            ResumeAnswer::Direct
        );
        service.join().await.unwrap();
        // The wire's own refusals before any offer: a DeepSeek or OpenCode
        // session and an original too large for the control frame.
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        for session in ["dsh/example", "opencode/example"] {
            assert_eq!(
                offer(service.clone(), session, original("hello")).await,
                ResumeAnswer::Direct,
                "{session}"
            );
        }
        // Each quote doubles in the stdin and again in the control line.
        assert_eq!(
            offer(service.clone(), SESSION, original(&"\"".repeat(20 << 10))).await,
            ResumeAnswer::Direct
        );
        assert!(calls(&f.0).is_empty(), "nothing was offered");
        service.join().await.unwrap();
    });
}

/// zerus-5wmh.66.4.10, design-tima2's pin: a Codex resume-then-send
/// reaches the offer only on a service whose help shows that it observes a
/// resumed Codex. Against an older service it goes the direct way, decided
/// before the hand-over hook and before any request; the same original on
/// a service with the marker is handed over and offered.
#[test]
fn a_codex_resume_reaches_the_offer_only_with_the_services_readiness_marker() {
    if !private_case("desktop_service::session_resume::fixture_tests::a_codex_resume_reaches_the_offer_only_with_the_services_readiness_marker") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"resume_no_observe": true}));
        let (service, _cache) = connected(&f).await;
        let (answer, seen) =
            offer_handing(service.clone(), "codex/example", original("hello"), true).await;
        assert_eq!(answer, ResumeAnswer::Direct);
        assert!(seen.is_empty(), "never reached the hand-over");
        assert!(calls(&f.0).is_empty(), "nothing was offered");
        // A Claude session still goes to that service.
        let (_, seen) = offer_handing(service.clone(), SESSION, original("hello"), false).await;
        assert_eq!(seen.len(), 1);
        service.join().await.unwrap();

        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (answer, seen) =
            offer_handing(service.clone(), "codex/example", original("hello"), true).await;
        assert_eq!(seen.len(), 1, "handed over");
        assert_eq!(seen[0].session, "codex/example");
        assert_ne!(answer, ResumeAnswer::Direct);
        let offered: Vec<Value> = calls(&f.0)
            .into_iter()
            .filter(|call| call["value"]["request"]["op"] == "session_resume")
            .collect();
        assert_eq!(offered.len(), 1);
        assert_eq!(
            offered[0]["value"]["request"]["intent"]["session"],
            "codex/example"
        );
        service.join().await.unwrap();
    });
}

/// A lost admission is looked up once offered and never becomes unsent;
/// the original goes only on the frontend lane, the lookups only on the
/// read-only request lane.
#[test]
fn a_lost_reply_is_looked_up_on_its_own_lane_and_never_becomes_unsent() {
    if !private_case("desktop_service::session_resume::fixture_tests::a_lost_reply_is_looked_up_on_its_own_lane_and_never_becomes_unsent") {
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
        assert_eq!(lane("session_resume"), ["frontend-request"]);
        assert_eq!(lane("session_resume_result"), ["request"]);
        service.join().await.unwrap();
    });
}

/// zerus-tlb9q: the service's exact refusal before admission never started:
/// Unsent with the draft kept, with no lookup and no second offer. The same
/// code for another request or scope, durable or with a receipt is no reply
/// of this original, so only the lookup decides.
#[test]
fn an_exact_not_admitted_refusal_is_unsent_without_a_lookup() {
    if !private_case("desktop_service::session_resume::fixture_tests::an_exact_not_admitted_refusal_is_unsent_without_a_lookup") {
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
                count(&f.0, "session_resume"),
                count(&f.0, "session_resume_result")
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
            assert_eq!(count(&f.0, "session_resume_result"), n + 1, "{name}");
        }
        service.join().await.unwrap();
    });
}

#[test]
fn the_admission_or_the_lookup_settles_and_an_unsettled_one_is_pending() {
    if !private_case("desktop_service::session_resume::fixture_tests::the_admission_or_the_lookup_settles_and_an_unsettled_one_is_pending") {
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
        assert_eq!(count(&f.0, "session_resume_result"), 1);
        // Settled in the admission reply itself.
        let sent = original("sent");
        let v = verified(&service, &sent).await;
        let recorded = receipt(&v, Recorded, Recorded, true);
        answers(&f.0, Some(admit(&v, Some(&recorded), true)), &[]);
        // Recorded, with the run its resume returned (zerus-5wmh.66.4.9.3).
        assert!(matches!(
            offer(service.clone(), SESSION, sent).await,
            ResumeAnswer::Settled(SendAnswer::Recorded(_), Some(run)) if run == RESUMED
        ));
        // Resuming, then the wait, then a send that never started after a
        // recorded resume whose agent never became ready, through the lookup.
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
            resumed_as(SendAnswer::Unsent(NOT_READY.into()))
        );
        // A resume the CLI refused before launching anything.
        let refused_resume = original("refused resume");
        let v = verified(&service, &refused_resume).await;
        answers(
            &f.0,
            Some(admit(
                &v,
                Some(&receipt(&v, Failed, NotStarted, true)),
                true,
            )),
            &[],
        );
        assert_eq!(
            offer(service.clone(), SESSION, refused_resume).await,
            settled_as(SendAnswer::Unsent(NOT_RESUMED.into()))
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
        let offered = count(&f.0, "session_resume");
        assert_eq!(
            offer(service.clone(), SESSION, slow).await,
            ResumeAnswer::Pending(v.plan.clone())
        );
        assert_eq!(
            checked(&service, &v.plan).await,
            (Checked::InProgress, None)
        );
        answers(&f.0, None, &[json!(receipt(&v, Recorded, Unknown, true))]);
        assert_eq!(
            checked(&service, &v.plan).await,
            (
                Checked::Settled(SendAnswer::Unconfirmed(UNCONFIRMED.into())),
                Some(RESUMED.into())
            )
        );
        assert_eq!(
            count(&f.0, "session_resume"),
            offered + 1,
            "a check never offers"
        );
        service.join().await.unwrap();
        // A check against a service without the kind knows nothing.
        let f = fixture(json!({"resume_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            checked(&service, &v.plan).await,
            (Checked::Unavailable, None)
        );
        assert!(calls(&f.0).is_empty());
        service.join().await.unwrap();
    });
}

/// The hand-over hook runs once with the exact plan before anything reaches
/// the service; a refusal there sends nothing and never ran.
#[test]
fn a_refused_hand_over_sends_nothing_and_never_ran() {
    if !private_case("desktop_service::session_resume::fixture_tests::a_refused_hand_over_sends_nothing_and_never_ran") {
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
        assert_eq!(answer, ResumeAnswer::Direct);
        assert!(seen.is_empty());
        service.join().await.unwrap();
    });
}
