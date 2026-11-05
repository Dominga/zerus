//! The card's answer past the capability probe, against the real service
//! fixture with a fake `hgs` that records every deferred answer request.
use super::tests::{original, question, SESSION};
use super::*;
use crate::desktop_service::{
    task_message::fixture_tests::{connected, private_case},
    tests::Fixture,
};
use zerus_client_core::questions::AnswerItem;

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
    std::fs::read_to_string(root.join("answer-calls"))
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
        config["answer_admission"] = admission;
    }
    config["answer_receipts"] = json!(receipts);
    std::fs::write(&path, config.to_string()).unwrap();
    let _ = std::fs::remove_file(root.join("answer-lookups"));
}
async fn offer(
    service: Arc<Service>,
    session: &str,
    (checkpoint, pinned, stdin): (Checkpoint, Value, Vec<u8>),
) -> AnswerResult {
    let session = session.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        call(
            Some(service),
            &session,
            &checkpoint,
            &pinned,
            &stdin,
            &AtomicBool::new(false),
        )
    })
    .await
    .unwrap()
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
    (checkpoint, pinned, stdin): &(Checkpoint, Value, Vec<u8>),
) -> wire::Verified {
    let (service, checkpoint, pinned, stdin) = (
        service.clone(),
        checkpoint.clone(),
        pinned.clone(),
        stdin.clone(),
    );
    tauri::async_runtime::spawn_blocking(move || {
        let read = Read::new(Some(service)).unwrap();
        intent(&read.scope, SESSION, &checkpoint, &pinned, &stdin)
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
            status: wire::ANSWERED.into(),
            input_sha256: verified.plan.input_sha256.clone(),
            open_terminal: true,
        }),
    })
}

#[test]
fn an_answer_the_service_does_not_take_goes_direct_before_any_request() {
    if !private_case("desktop_service::deferred_answer::fixture_tests::an_answer_the_service_does_not_take_goes_direct_before_any_request") {
        return;
    }
    tauri::async_runtime::block_on(async {
        // A service without the kind.
        let f = fixture(json!({"answer_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            offer(
                service.clone(),
                SESSION,
                original("claude_folder_trust", "yes")
            )
            .await,
            AnswerResult::Direct
        );
        assert_eq!(count(&f.0, "deferred_answer"), 0);
        service.join().await.unwrap();
        // The wire's own refusals before any offer: a DeepSeek session, a
        // source v1 keeps direct, and an original too large for the
        // control frame.
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            offer(
                service.clone(),
                "dsh/example",
                original("claude_folder_trust", "yes")
            )
            .await,
            AnswerResult::Direct
        );
        assert_eq!(
            offer(service.clone(), SESSION, original("codex_async", "yes")).await,
            AnswerResult::Direct
        );
        // A plan review's pinned identity carries the whole plan.
        let mut review = question("kimi_plan_review");
        review["questions"][0]["body"] = json!("x".repeat(64 << 10));
        let (checkpoint, _, _) = original("kimi_plan_review", "yes");
        let pinned = zerus_client_core::questions::consent_identity(&review);
        let answers = vec![AnswerItem::Choice {
            question_id: "q-1".into(),
            selected_option_ids: vec!["yes".into()],
            text: String::new(),
        }];
        let stdin = zerus_client_core::actions::answer_input(
            SESSION,
            &review,
            &answers,
            &checkpoint.request_id,
        )
        .unwrap();
        assert_eq!(
            offer(service.clone(), SESSION, (checkpoint, pinned, stdin)).await,
            AnswerResult::Direct
        );
        assert_eq!(count(&f.0, "deferred_answer"), 0, "nothing was offered");
        service.join().await.unwrap();
    });
}

#[test]
fn a_lost_reply_is_looked_up_once_offered_and_never_becomes_unsent() {
    if !private_case("desktop_service::deferred_answer::fixture_tests::a_lost_reply_is_looked_up_once_offered_and_never_becomes_unsent") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let answer = offer(
            service.clone(),
            SESSION,
            original("claude_folder_trust", "yes"),
        )
        .await;
        // One request, then the scoped lookup, which finds nothing.
        assert_eq!(count(&f.0, "deferred_answer"), 1);
        assert_eq!(count(&f.0, "deferred_answer_result"), 1);
        assert_eq!(answer, AnswerResult::Unconfirmed(UNCONFIRMED.into()));
        service.join().await.unwrap();
    });
}

#[test]
fn the_admission_or_the_lookup_settles_and_an_unsettled_answer_is_pending() {
    if !private_case("desktop_service::deferred_answer::fixture_tests::the_admission_or_the_lookup_settles_and_an_unsettled_answer_is_pending") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let admit = |verified: &wire::Verified, receipt: Option<Value>, accepted: bool| {
            json!({"abi": wire::ABI, "request_id": verified.plan.request_id,
                "scope": verified.intent.scope, "accepted": accepted, "durable": accepted,
                "code": if accepted { "" } else { "deferred_answer_unavailable" }, "receipt": receipt})
        };
        // A refusal is no reply of this original: only the lookup decides,
        // and nothing found is never "not sent".
        let original_of = |option: &str| original("claude_folder_trust", option);
        let refused = original_of("yes");
        let v = verified(&service, &refused).await;
        answers(&f.0, Some(admit(&v, None, false)), &[]);
        assert_eq!(
            offer(service.clone(), SESSION, refused).await,
            AnswerResult::Unconfirmed(UNCONFIRMED.into())
        );
        assert_eq!(count(&f.0, "deferred_answer_result"), 1);
        // Settled in the admission reply itself, the Terminal hand-off
        // carried as data.
        let again = original_of("no");
        let v = verified(&service, &again).await;
        let recorded = receipt(&v, wire::Phase::Recorded, true, "");
        answers(&f.0, Some(admit(&v, Some(recorded), true)), &[]);
        match offer(service.clone(), SESSION, again).await {
            AnswerResult::Recorded(outcome) => {
                assert!(outcome.matches(&v.plan) && outcome.open_terminal)
            }
            other => panic!("{other:?}"),
        }
        // Running, then never started or refused, through the lookup.
        for (code, unsent) in [(wire::NOT_STARTED, NOT_STARTED), (wire::REJECTED, REFUSED)] {
            let ended = original_of("yes");
            let v = verified(&service, &ended).await;
            let running = receipt(&v, wire::Phase::Running, false, "");
            answers(
                &f.0,
                Some(admit(&v, Some(running.clone()), true)),
                &[running, receipt(&v, wire::Phase::Failed, true, code)],
            );
            assert_eq!(
                offer(service.clone(), SESSION, ended).await,
                AnswerResult::Unsent(unsent.into()),
                "{code}"
            );
        }
        // Still running past the bound: pending, checked again read-only.
        let slow = original_of("yes");
        let v = verified(&service, &slow).await;
        let running = receipt(&v, wire::Phase::Running, false, "");
        answers(
            &f.0,
            Some(admit(&v, Some(running.clone()), true)),
            &[running],
        );
        let offered = count(&f.0, "deferred_answer");
        assert_eq!(
            offer(service.clone(), SESSION, slow).await,
            AnswerResult::Pending(Box::new(v.plan.clone()))
        );
        assert_eq!(checked(&service, &v.plan).await, Checked::InProgress);
        answers(
            &f.0,
            None,
            &[receipt(
                &v,
                wire::Phase::Unknown,
                true,
                "deferred_answer_receipt_missing",
            )],
        );
        assert_eq!(
            checked(&service, &v.plan).await,
            Checked::Settled(AnswerResult::Unconfirmed(UNCONFIRMED.into()))
        );
        assert_eq!(
            count(&f.0, "deferred_answer"),
            offered + 1,
            "a check never offers"
        );
        service.join().await.unwrap();
    });
}

/// A check against a service without the kind knows nothing, and each call
/// uses its own lane: the original only the frontend lane, the lookups only
/// the read-only request lane.
#[test]
fn a_check_without_the_kind_knows_nothing_and_each_call_uses_its_lane() {
    if !private_case("desktop_service::deferred_answer::fixture_tests::a_check_without_the_kind_knows_nothing_and_each_call_uses_its_lane") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"answer_no_abi": true}));
        let (service, _cache) = connected(&f).await;
        let (checkpoint, pinned, stdin) = original("claude_folder_trust", "yes");
        let plan = super::plan(SESSION, &checkpoint, &pinned, &stdin).unwrap();
        assert_eq!(
            checked(&service, &plan).await,
            Checked::Unavailable,
            "missing marker"
        );
        service.join().await.unwrap();
        // A lost admission: the original only on the frontend lane, the
        // lookups only on the read-only request lane.
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            offer(
                service.clone(),
                SESSION,
                original("claude_folder_trust", "yes")
            )
            .await,
            AnswerResult::Unconfirmed(UNCONFIRMED.into())
        );
        let calls: Vec<Value> = std::fs::read_to_string(f.0.join("answer-calls"))
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
        assert_eq!(lane("deferred_answer"), ["frontend-request"]);
        let lookups = lane("deferred_answer_result");
        assert!(!lookups.is_empty() && lookups.iter().all(|a| a == "request"));
        service.join().await.unwrap();
    });
}
