use super::*;
use crate::desktop_service::{
    frontend::tests::native::{responder, CacheServer},
    tests::{eventually, Fixture},
};

fn private_case(case: &'static str) -> bool {
    const MARKER: &str = "ZERUS_CREATION_CLONE_CASE";
    if std::env::var(MARKER).as_deref() == Ok(case) {
        return true;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    let invocation = Invocation {
        args: ["--exact", case, "--nocapture", "--test-threads=1"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        stdin: None,
        timeout: Duration::from_secs(40),
        request_id: None,
    };
    match run::run_without(
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
    crate::write_executable(
        &f.0.join("hgs"),
        format!(
            "#!/usr/bin/env python3\n{}\n{}",
            include_str!("fixture.py"),
            include_str!("../fake_hgs.py")
        ),
    );
    f
}
fn operation() -> (String, Value) {
    let id = uuid::Uuid::new_v4().to_string();
    (
        id.clone(),
        json!({"id":id,"creation_schema":1,"created":1,"revision":4,"cmd":"codex","tag":"example","host":"", "state":"cloning","repository":true,
        "parent":"/srv","target":"/srv/repo","repository_name":"repo","url":"https://example.invalid/repo.git",
        "clone_id":uuid::Uuid::new_v4().to_string(),"expected_machine":"22222222-2222-4222-8222-222222222222"}),
    )
}
fn calls(f: &Fixture) -> Vec<Value> {
    std::fs::read_to_string(f.0.join("clone-calls"))
        .unwrap_or_default()
        .lines()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}
async fn connected(f: &Fixture) -> (Arc<Service>, CacheServer) {
    let responder = responder();
    assert!(responder
        .configure_ownership_mode_once(zerus_client_core::desktop::ownership::Mode::Service));
    let cache = CacheServer::start(&f.0, responder.clone());
    let (service, _) = f.start_with_responder(true, Some(responder));
    eventually("clone service connected", || service.snapshot().connected).await;
    (service, cache)
}
async fn reply(
    service: Arc<Service>,
    id: String,
    value: Value,
    fresh: bool,
    wait: bool,
) -> Option<creation::Answer> {
    tauri::async_runtime::spawn_blocking(move || {
        call(
            Some(service),
            &id,
            &value,
            fresh,
            wait,
            &AtomicBool::new(false),
        )
    })
    .await
    .unwrap()
}
async fn answer(service: Arc<Service>, id: String, value: Value, fresh: bool) -> creation::Answer {
    reply(service, id, value, fresh, false)
        .await
        .expect("a service answer, not the direct path")
}
#[test]
fn a_launch_help_failure_is_unsent_and_never_falls_back_or_dispatches() {
    if !private_case("desktop_service::creation_clone::tests::a_launch_help_failure_is_unsent_and_never_falls_back_or_dispatches") { return }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        // The service connects before only its launch capability probe fails.
        std::fs::write(
            f.0.join("fixture.json"),
            json!({"clone_help_fail": true}).to_string(),
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let value = json!({"id":id,"creation_schema":1,"created":1,"revision":7,
            "cmd":"codex","tag":"example","host":"","target":"/srv/repo",
            "state":"launching","launch_dispatched":true,"launch_id":id,
            "expected_machine":"22222222-2222-4222-8222-222222222222"});
        let selected = service.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            crate::desktop_service::creation_launch::call(
                Some(selected),
                &id,
                &value,
                true,
                &AtomicBool::new(false),
            )
        })
        .await
        .unwrap();
        assert_eq!(
            result,
            crate::desktop_service::creation_launch::LaunchAnswer::Unsent(
                crate::desktop_service::creation_launch::NOT_SENT.into(),
            )
        );
        assert!(calls(&f).is_empty(), "no effect request was sent");
        assert!(
            !f.calls()
                .iter()
                .any(|c| c["args"].as_array().is_some_and(|args| {
                    args.iter()
                        .any(|arg| arg == "codex" || arg == "frontend-request")
                })),
            "no direct launch or authenticated input"
        );
        service.join().await.unwrap();
    });
}
#[test]
fn original_clone_child_is_bound_and_lost_reply_only_allows_a_scoped_read() {
    if !private_case("desktop_service::creation_clone::tests::original_clone_child_is_bound_and_lost_reply_only_allows_a_scoped_read") {return}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"clone_lost":true}));
        let (service, _cache) = connected(&f).await;
        let (id, value) = operation();
        assert!(
            !answer(service.clone(), id.clone(), value.clone(), true)
                .await
                .ok
        );
        assert!(f.0.join("clone-bound").exists());
        let result = answer(service.clone(), id.clone(), value.clone(), false).await;
        assert_eq!(result.data["status"], "cloned");
        assert_eq!(
            calls(&f)
                .iter()
                .filter(|c| c["value"]["request"]["op"] == "creation_clone")
                .count(),
            1
        );
        assert_eq!(
            calls(&f)
                .iter()
                .filter(|c| c["value"]["op"] == "creation_clone_result")
                .count(),
            1
        );
        assert!(!f.calls().iter().any(|c| c["args"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "repository"))));
        service.join().await.unwrap();
    });
}
#[test]
fn a_sent_create_whose_reply_is_lost_stays_unconfirmed_for_clone_and_worktree() {
    if !private_case("desktop_service::creation_clone::tests::a_sent_create_whose_reply_is_lost_stays_unconfirmed_for_clone_and_worktree") {return}
    tauri::async_runtime::block_on(async {
        // The production create path (wait): once its admission request was
        // written, a lost reply is Unknown and keeps its request ID. It is
        // never the never-sent failure that retires the request.
        let unconfirmed = |answer: creation::Answer| {
            assert!(!answer.ok);
            assert_eq!(
                answer.error,
                zerus_client_core::repository::UNCONFIRMED_OPERATION
            );
            assert!(answer.data.is_null(), "{}", answer.data);
        };
        let f = fixture(json!({"clone_lost":true}));
        let (service, _cache) = connected(&f).await;
        let (id, value) = operation();
        unconfirmed(reply(service.clone(), id, value, true, true).await.unwrap());
        assert!(f.0.join("clone-bound").exists(), "the clone was sent");
        service.join().await.unwrap();

        let f = fixture(json!({"worktree_abi":true}));
        let (service, _cache) = connected(&f).await;
        let id = uuid::Uuid::new_v4().to_string();
        let value = json!({"id":id,"creation_schema":1,"created":1,"revision":4,"cmd":"codex",
            "tag":"example","host":"","state":"worktree","source":"/srv/repo","target":"/srv/repo",
            "worktree_source":"/srv/repo","branch":"work-1","destination":"/srv/repo-work-1",
            "worktree_common":"/srv/repo/.git","worktree_receipts":true,
            "worktree_id":uuid::Uuid::new_v4().to_string(),
            "expected_machine":"22222222-2222-4222-8222-222222222222"});
        let sent = service.clone();
        let answer = tauri::async_runtime::spawn_blocking(move || {
            crate::desktop_service::creation_worktree::call(
                Some(sent),
                &id,
                &value,
                true,
                true,
                &AtomicBool::new(false),
            )
        })
        .await
        .unwrap()
        .expect("a service answer, not the direct path");
        unconfirmed(answer);
        let written = std::fs::read_to_string(f.0.join("worktree-calls")).unwrap_or_default();
        assert_eq!(
            written.lines().count(),
            1,
            "the worktree intent was sent once"
        );
        service.join().await.unwrap();
    });
}
#[test]
fn unsupported_or_mismatched_clone_results_never_advance_the_native_host() {
    if !private_case("desktop_service::creation_clone::tests::unsupported_or_mismatched_clone_results_never_advance_the_native_host") {return}
    tauri::async_runtime::block_on(async {
        for (config, writes) in [
            (json!({"clone_help_fail":true}), 0),
            (json!({"clone_wrong_identity":true}), 1),
        ] {
            let f = fixture(json!({}));
            let (service, _cache) = connected(&f).await;
            std::fs::write(f.0.join("fixture.json"), config.to_string()).unwrap();
            let (id, value) = operation();
            let result = answer(service.clone(), id, value, true).await;
            assert!(!result.ok);
            assert_eq!(
                result.error,
                zerus_client_core::repository::UNCONFIRMED_OPERATION
            );
            assert_eq!(calls(&f).len(), writes);
            service.join().await.unwrap();
        }
    });
}

#[test]
fn a_missing_marker_sends_every_clone_call_down_the_direct_path() {
    if !private_case("desktop_service::creation_clone::tests::a_missing_marker_sends_every_clone_call_down_the_direct_path") {return}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"clone_no_abi":true}));
        let (service, _cache) = connected(&f).await;
        for windows in [false, true] {
            let (id, mut value) = operation();
            if windows {
                value["parent"] = json!(r"C:\Work");
                value["target"] = json!(r"C:\Work\repo");
                assert!(
                    wire::Plan::from_operation(&id, &value).is_none(),
                    "the missing-marker routing precedes POSIX admission validation"
                );
            }
            // A fresh clone, a never-consented clone and a status read: the
            // completed probe found no kind, so each takes the direct path.
            for (fresh, wait) in [(true, true), (false, true), (false, false)] {
                assert!(
                    reply(service.clone(), id.clone(), value.clone(), fresh, wait)
                        .await
                        .is_none(),
                    "{fresh} {wait}"
                );
            }
        }
        assert!(calls(&f).is_empty(), "neither mutation nor lookup was sent");
        service.join().await.unwrap();
    });
}
#[test]
fn a_clone_that_never_reached_the_service_fails_definitely_and_a_read_does_not() {
    if !private_case("desktop_service::creation_clone::tests::a_clone_that_never_reached_the_service_fails_definitely_and_a_read_does_not") {return}
    tauri::async_runtime::block_on(async {
        let unsent = |answer: Option<creation::Answer>, value: &Value| {
            let answer = answer.unwrap();
            assert!(answer.ok, "a definite pre-send result");
            assert_eq!(answer.data["status"], "failed");
            assert_eq!(answer.data["effects_started"], false);
            assert_eq!(answer.data["request_id"], value["clone_id"]);
            assert_eq!(answer.data["path"], value["target"]);
            assert_eq!(answer.data["detail"], NOT_SENT);
        };
        // A supported service: a clone without fresh consent is never sent.
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let (id, value) = operation();
        unsent(
            reply(service.clone(), id.clone(), value.clone(), false, true).await,
            &value,
        );
        assert!(calls(&f).is_empty(), "nothing was sent");
        // A failed probe stops even a fresh clone before anything is sent;
        // a status read there proves nothing and stays unconfirmed.
        std::fs::write(
            f.0.join("fixture.json"),
            json!({"clone_help_fail":true}).to_string(),
        )
        .unwrap();
        unsent(
            reply(service.clone(), id.clone(), value.clone(), true, true).await,
            &value,
        );
        let read = reply(service.clone(), id, value, false, false)
            .await
            .unwrap();
        assert_eq!(
            read.error,
            zerus_client_core::repository::UNCONFIRMED_OPERATION
        );
        assert!(calls(&f).is_empty(), "nothing was sent");
        service.join().await.unwrap();
    });
}
#[test]
fn completed_original_child_with_held_pipes_keeps_the_creation_gate_charged() {
    if !private_case("desktop_service::creation_clone::tests::completed_original_child_with_held_pipes_keeps_the_creation_gate_charged") {return}
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"clone_hold":true}));
        let (service, _cache) = connected(&f).await;
        let (id, value) = operation();
        let reader = tauri::async_runtime::spawn(answer(service.clone(), id, value, true));
        eventually("original clone held", || f.0.join("clone-held").exists()).await;
        assert!(matches!(
            service.shared.creation_rpc.admit(Role::Fleet),
            Err(zerus_client_core::desktop::drain::Refused::Capacity)
        ));
        assert!(!reader.await.unwrap().ok);
        assert!(
            matches!(
                service.shared.creation_rpc.admit(Role::Fleet),
                Err(zerus_client_core::desktop::drain::Refused::Capacity)
            ),
            "original child exit/runner failure is not its descendants' pipe EOF"
        );
        std::fs::write(f.0.join("clone-release"), b"").unwrap();
        eventually("original creation charge released", || {
            service.shared.creation_rpc.admit(Role::Fleet).is_ok()
        })
        .await;
        assert_eq!(calls(&f).len(), 1);
        service.join().await.unwrap();
    });
}

#[test]
fn original_connection_and_helper_are_rechecked_before_each_clone_read() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _) = f.start(true);
        eventually("clone reader connection", || service.snapshot().connected).await;
        let read = Read::new(Some(service.clone())).unwrap();
        assert!(read.current());
        let original = service
            .shared
            .cancellation
            .lock()
            .unwrap()
            .replace(Arc::new(AtomicBool::new(false)));
        assert!(
            !read.current(),
            "same epoch/generation with a different original connection is stale"
        );
        *service.shared.cancellation.lock().unwrap() = original;
        assert!(read.current());
        let path = f.0.join("hgs");
        let next = f.0.join("replacement");
        std::fs::copy(&path, &next).unwrap();
        std::fs::rename(next, &path).unwrap();
        assert!(read.exchange(None, Value::Null, true, None).is_err());
        assert!(calls(&f).is_empty());
        service.join().await.unwrap();
        assert!(!read.current());
    });
}
