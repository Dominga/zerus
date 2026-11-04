//! Real original child/cache/protocol with fake actor/effect responses. No real
//! Terminate, Root authentication, owner state or cleanup/removal qualification.
use super::*;
use crate::desktop_service::{
    frontend::tests::native::{responder, CacheServer},
    tests::{eventually, Fixture},
};
use zerus_client_core::{Action, Outcome, Target};

type RefusedObserver = Box<dyn FnOnce(&mut std::process::Child)>;
thread_local! {
    // Observe only a REAL failed binding, before the SAME child is returned to
    // run_owned. A hook never substitutes a binding result or supplies consent.
    static REFUSED_OBSERVER: std::cell::RefCell<Option<RefusedObserver>> = const { std::cell::RefCell::new(None) };
}
pub(super) fn observe_refused_binding(child: &mut std::process::Child) {
    if let Some(observer) = REFUSED_OBSERVER.with_borrow_mut(Option::take) {
        observer(child);
    }
}

fn fixture(config: Value) -> Fixture {
    let f = Fixture::new(config);
    std::fs::remove_file(f.0.join("hgs")).unwrap();
    std::fs::write(
        f.0.join("hgs"),
        format!(
            "#!/usr/bin/env python3\n{}\n{}",
            include_str!("fixture.py"),
            include_str!("../fake_hgs.py").replacen(
                "return {'abi': 'desktop-v1', 'configuration': 'a' * 64,",
                "return {'running': True, 'abi': 'desktop-v1', 'configuration': ('b' * 64 if args[:2] == ['desktop', 'status'] and config.get('termination_wrong_scope') and (root / 'termination-retained').exists() else 'a' * 64),",
                1,
            )
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(f.0.join("hgs"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        f.0.join("termination-capture"),
        serde_json::to_vec(&capture()).unwrap(),
    )
    .unwrap();
    f
}
fn capture() -> Capture {
    Capture {
        target: Target {
            computer: String::new(),
            name: "codex/example/original".into(),
            run_id: "original-run".into(),
            conversation_id: "original-conversation".into(),
        },
        machine: "22222222-2222-4222-8222-222222222222".into(),
        root: "/srv/example/work".into(),
        common_dir: "/srv/example/main/.git".into(),
        owner_id: "original-owner".into(),
    }
}
async fn connected(f: &Fixture) -> (Arc<Service>, CacheServer) {
    let responder = responder();
    assert!(responder
        .configure_ownership_mode_once(zerus_client_core::desktop::ownership::Mode::Service));
    let cache = CacheServer::start(&f.0, responder.clone());
    let (service, _) = f.start_with_responder(true, Some(responder));
    eventually("termination service connected", || {
        service.snapshot().connected
            && f.calls()
                .iter()
                .any(|call| call["args"] == json!(["desktop", "watch", "--json"]))
    })
    .await;
    (service, cache)
}
fn calls(f: &Fixture) -> Vec<Value> {
    std::fs::read_to_string(f.0.join("termination-calls"))
        .unwrap_or_default()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
fn requests(calls: &[Value], op: &str) -> Vec<Value> {
    calls
        .iter()
        .filter_map(|v| {
            let value = v.get("value")?;
            let request = value.get("request").unwrap_or(value);
            (request["op"] == op).then(|| request.clone())
        })
        .collect()
}
fn effects(calls: &[Value]) -> Vec<Value> {
    calls
        .iter()
        .filter(|v| {
            v["args"]
                .as_array()
                .is_some_and(|args| args.iter().any(|a| a == "session-action"))
        })
        .cloned()
        .collect()
}
fn actions(f: &Fixture) -> crate::actions::Actions {
    crate::actions::Actions::new(f.0.join("marks"))
}
fn selected(service: Arc<Service>) -> Selected {
    let original = capture();
    let (actual, selected) =
        Selected::inspect(service, &original.target, &original.machine, Arc::new(())).unwrap();
    assert_eq!(actual, Some(original));
    selected
}
fn confirm(service: Arc<Service>, pending: Arc<Pending>) -> Confirmation {
    Confirmation::new(
        uuid::Uuid::new_v4().to_string(),
        capture(),
        selected(service),
        pending,
    )
    .unwrap()
}
#[test]
fn the_offer_selection_cannot_be_replaced_before_confirmation() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for change in ["image", "connection", "cancel"] {
            let f = fixture(json!({}));
            let (service, _cache) = connected(&f).await;
            let read = Arc::new(());
            let old_read = Arc::downgrade(&read);
            let original = capture();
            let (actual, selected) =
                Selected::inspect(service.clone(), &original.target, &original.machine, read)
                    .unwrap();
            assert_eq!(actual, Some(original.clone()));
            assert!(old_read.upgrade().is_none(), "offer I/O must actually join");
            let pending = Arc::new(Pending::default());
            match change {
                "image" => {
                    // Identical bytes at the selected name are a different image.
                    std::fs::rename(f.0.join("hgs"), f.0.join("original-hgs")).unwrap();
                    std::fs::copy(f.0.join("original-hgs"), f.0.join("hgs")).unwrap();
                }
                "connection" => {
                    // Same path/configuration, but a different connection original.
                    *service.shared.relay.lock().unwrap() = Some(Arc::new(
                        relay::Connection::select(&f.0.join("hgs")).unwrap(),
                    ));
                }
                "cancel" => selected.0.cancel.store(true, Ordering::Release),
                _ => unreachable!(),
            }
            let native = Confirmation::new(
                uuid::Uuid::new_v4().to_string(),
                original,
                selected,
                pending.clone(),
            )
            .unwrap();
            assert!(native.bind(Arc::new(())).is_err(), "{change}");
            assert!(pending.0.lock().unwrap().is_empty());
            assert!(requests(&calls(&f), "termination_outcome").is_empty());
            assert!(effects(&calls(&f)).is_empty());
            service.join().await.unwrap();
        }
    });
}
#[test]
fn one_original_is_retained_before_the_same_scoped_native_call() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for (lost, wait) in [(false, false), (true, false), (false, true)] {
            let f = fixture(json!({"termination_rpc_lost":lost, "termination_effect_wait":wait}));
            let (service, _cache) = connected(&f).await;
            let pending = Arc::new(Pending::default());
            let native = confirm(service.clone(), pending.clone());
            let original = native.invocation().clone();
            let outcome = crate::actions::dispatch_cleanup_native(&actions(&f), native, None)
                .await
                .unwrap();
            assert!(matches!(outcome, Outcome::Done { .. }), "{outcome:?}");
            let calls = calls(&f);
            let offered = requests(&calls, "termination_outcome");
            assert_eq!(offered.len(), 1);
            assert_eq!(
                offered[0]["intent"]["request_id"],
                original.request_id.clone().unwrap()
            );
            assert_eq!(
                offered[0]["intent"]["original"],
                String::from_utf8(original.stdin.clone().unwrap()).unwrap()
            );
            assert_eq!(effects(&calls).len(), 1);
            assert_eq!(effects(&calls)[0]["args"], json!(original.args));
            assert_eq!(
                effects(&calls)[0]["value"],
                serde_json::from_slice::<Value>(original.stdin.as_ref().unwrap()).unwrap()
            );
            assert_eq!(
                requests(&calls, "termination_outcome_status").len(),
                usize::from(lost)
            );
            assert!(Confirmation::new(
                uuid::Uuid::new_v4().to_string(),
                capture(),
                selected(service.clone()),
                pending
            )
            .is_err());
            service.join().await.unwrap();
        }
    });
}
#[test]
fn corrupted_epoch_scope_or_changed_capture_never_dispatches_or_reoffers() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for scenario in [
            "termination_bad_first_epoch",
            "termination_wrong_scope",
            "termination_changed_capture",
        ] {
            let mut config = json!({});
            config[scenario] = json!(true);
            let f = fixture(config);
            let (service, _cache) = connected(&f).await;
            let pending = Arc::new(Pending::default());
            let native = confirm(service.clone(), pending.clone());
            let original_selection = native.selected.clone();
            let outcome = crate::actions::dispatch_cleanup_native(&actions(&f), native, None)
                .await
                .unwrap();
            assert!(
                !matches!(outcome, Outcome::Done { .. }),
                "{scenario}:{outcome:?}"
            );
            assert!(effects(&calls(&f)).is_empty());
            assert_eq!(requests(&calls(&f), "termination_outcome").len(), 1);
            let mut renamed = capture();
            renamed.target.name = "renamed/original".into();
            renamed.target.computer = "another-alias".into();
            renamed.target.conversation_id = "successor-conversation".into();
            assert!(Confirmation::new(
                uuid::Uuid::new_v4().to_string(),
                renamed,
                original_selection,
                pending
            )
            .is_err());
            service.join().await.unwrap();
        }
    });
}
#[test]
fn native_effect_loss_consumes_the_one_shot_and_keeps_original_correlation() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"termination_effect_lost":true}));
        let (service, _cache) = connected(&f).await;
        let pending = Arc::new(Pending::default());
        let native = confirm(service.clone(), pending.clone());
        let id = native.invocation().request_id.clone().unwrap();
        let outcome = crate::actions::dispatch_cleanup_native(&actions(&f), native, None)
            .await
            .unwrap();
        assert!(!matches!(outcome, Outcome::Done { .. }));
        assert_eq!(effects(&calls(&f)).len(), 1);
        assert_eq!(requests(&calls(&f), "termination_outcome").len(), 1);
        {
            let entries = pending.0.lock().unwrap();
            let retained = entries.values().next().unwrap();
            assert_eq!(retained.original.request_id, id);
            assert_eq!(
                retained.stage.load(Ordering::Acquire),
                Stage::DispatchBegun as u8
            );
        }
        assert!(Confirmation::new(
            uuid::Uuid::new_v4().to_string(),
            capture(),
            selected(service.clone()),
            pending
        )
        .is_err());
        service.join().await.unwrap();
    });
}
struct Release(std::path::PathBuf);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, []);
    }
}
#[test]
fn retained_pipe_keeps_the_original_actions_claim_and_refuses_native_effect() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"termination_pipe":true}));
        f.1.store(true, Ordering::Release);
        let _release = Release(f.0.join("termination-release"));
        let (service, _cache) = connected(&f).await;
        let pending = Arc::new(Pending::default());
        let actions = actions(&f);
        let outcome = crate::actions::dispatch_cleanup_native(
            &actions,
            confirm(service.clone(), pending),
            None,
        )
        .await
        .unwrap();
        assert!(!matches!(outcome, Outcome::Done { .. }));
        assert!(effects(&calls(&f)).is_empty());
        let target = capture().target;
        assert!(actions
            .begin(
                crate::actions::Lane::key(&target, &Action::Terminate),
                &Action::Terminate
            )
            .is_none());
        std::fs::write(f.0.join("termination-release"), []).unwrap();
        eventually("original retention pipes physically finish", || {
            actions
                .begin(
                    crate::actions::Lane::key(&target, &Action::Terminate),
                    &Action::Terminate,
                )
                .is_some()
        })
        .await;
        assert!(effects(&calls(&f)).is_empty());
        service.join().await.unwrap();
        f.1.store(false, Ordering::Release);
    });
}

#[test]
fn missing_unknown_corrupt_or_foreign_original_lookup_never_replays_or_dispatches() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for case in [
            "missing",
            "unknown",
            "corrupt",
            "wrong-original",
            "cleanup-authority",
        ] {
            let f = fixture(json!({"termination_rpc_lost":true,"termination_lookup_case":case}));
            let (service, _cache) = connected(&f).await;
            let pending = Arc::new(Pending::default());
            let native = confirm(service.clone(), pending.clone());
            let id = native.invocation().request_id.clone().unwrap();
            let outcome = crate::actions::dispatch_cleanup_native(&actions(&f), native, None)
                .await
                .unwrap();
            assert!(
                !matches!(outcome, Outcome::Done { .. }),
                "{case}:{outcome:?}"
            );
            let calls = calls(&f);
            assert_eq!(requests(&calls, "termination_outcome").len(), 1);
            let queries = requests(&calls, "termination_outcome_status");
            assert_eq!(queries.len(), 1);
            assert_eq!(queries[0]["original_request_id"], id);
            assert!(effects(&calls).is_empty());
            assert!(Confirmation::new(
                uuid::Uuid::new_v4().to_string(),
                capture(),
                selected(service.clone()),
                pending
            )
            .is_err());
            service.join().await.unwrap();
        }
    });
}
#[test]
fn the_live_dispatch_transition_is_one_way_even_without_a_native_spawn() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({}));
        let (service, _cache) = connected(&f).await;
        let pending = Arc::new(Pending::default());
        let mut controller = confirm(service.clone(), pending.clone())
            .bind(Arc::new(()))
            .unwrap();
        let invocation = controller.invocation().clone();
        controller.retain().unwrap();
        controller.begin_dispatch(&invocation).unwrap();
        assert!(controller.begin_dispatch(&invocation).is_err());
        assert!(controller.recheck_scope().is_err());
        assert!(matches!(controller.read(&invocation), Exit::StartFailed(_)));
        drop(controller);
        assert!(effects(&calls(&f)).is_empty());
        assert!(Confirmation::new(
            uuid::Uuid::new_v4().to_string(),
            capture(),
            selected(service.clone()),
            pending
        )
        .is_err());
        service.join().await.unwrap();
    });
}

#[test]
fn a_new_legacy_helper_is_probed_before_any_new_subcommand() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for broken in ["missing", "substring", "frontend", "utf8"] {
            let f = fixture(json!({}));
            let (service, _cache) = connected(&f).await;
            eventually("original catalog published", || {
                service.shared.pump.live("")
            })
            .await;
            let mut help = format!(
                "{HELP}\n{}\n{}\n",
                wire::HELP_MARKER,
                zerus_client_core::desktop::ui_requests::HELP_MARKER
            )
            .into_bytes();
            match broken {
                "missing" => help = b"Legacy fixture CLI\n".to_vec(),
                "substring" => {
                    help =
                        format!("prefix {} suffix\n", String::from_utf8(help).unwrap()).into_bytes()
                }
                "frontend" => help = format!("{HELP}\n{}\n", wire::HELP_MARKER).into_bytes(),
                "utf8" => help.push(0xff),
                _ => unreachable!(),
            }
            // Service's original watch stays connected; this newly installed
            // image must prove its own ABI before any new desktop command.
            std::fs::rename(f.0.join("hgs"), f.0.join("original-hgs")).unwrap();
            std::fs::write(f.0.join("hgs"), format!(
                "#!/usr/bin/python3\nimport sys,json,os\nfrom pathlib import Path\np=Path(os.path.realpath(__file__)).parent\nwith (p/'legacy-calls').open('a') as f:f.write(json.dumps(sys.argv[1:])+'\\n')\nif sys.argv[1:] != ['--help']:sys.exit(2)\nsys.stdout.buffer.write(bytes({help:?}))\n"
            )).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f.0.join("hgs"), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            assert!(Selected::inspect(
                service.clone(),
                &capture().target,
                &capture().machine,
                Arc::new(())
            )
            .is_err());
            let actual: Vec<Value> = std::fs::read_to_string(f.0.join("legacy-calls"))
                .unwrap()
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect();
            assert_eq!(actual, vec![json!(["--help"])], "{broken}");
            assert!(requests(&calls(&f), "termination_outcome").is_empty());
            assert!(effects(&calls(&f)).is_empty());
            service.join().await.unwrap();
        }
    });
}

#[test]
fn refused_or_expired_original_binding_withholds_input_and_keeps_real_custody() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    struct Owned(std::process::Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    tauri::async_runtime::block_on(async {
        for expired in [false, true] {
            let f = fixture(json!({"termination_refused_registration":true}));
            let (service, _cache) = connected(&f).await;
            let actions = Arc::new(actions(&f));
            let key = crate::actions::Lane::key(&capture().target, &Action::Terminate);
            let hold = actions.begin(key.clone(), &Action::Terminate).unwrap();
            let controller = confirm(service.clone(), Arc::new(Pending::default()))
                .bind(Arc::new(hold))
                .unwrap();
            let registration = service
                .shared
                .responder
                .as_ref()
                .unwrap()
                .prepare_ui_request(
                    uuid::Uuid::new_v4().to_string(),
                    Request::TerminationOutcome {
                        intent: Box::new(controller.intent.clone()),
                    },
                )
                .unwrap();
            let slot = registration.resources();
            let original_slot = Arc::downgrade(&slot);
            drop(slot);
            let other = if expired {
                // Expiration removes evidence, never the original resource slot.
                std::thread::sleep(
                    zerus_client_core::desktop::ui_requests::LIFETIME + Duration::from_millis(10),
                );
                None
            } else {
                let child = Owned(
                    std::process::Command::new("/bin/sleep")
                        .arg("20")
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .unwrap(),
                );
                assert!(registration.bind_original(&child.0));
                Some(child)
            };
            let input = serde_json::to_vec(registration.envelope()).unwrap();
            let context = controller.context.clone();
            drop(controller);
            let busy = context.busy.clone();
            let root = f.0.clone();
            let check_actions = actions.clone();
            let check_key = key.clone();
            let check_slot = original_slot.clone();
            let check_service = service.clone();
            let (observed, observation) = std::sync::mpsc::sync_channel(1);
            let result = tauri::async_runtime::spawn_blocking(move || {
                REFUSED_OBSERVER.with_borrow_mut(|hook| {
                    *hook = Some(Box::new(move |child| {
                        let until = std::time::Instant::now() + Duration::from_secs(2);
                        let path = root.join("termination-refused-input");
                        while !path.exists() && std::time::Instant::now() < until {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        // No panic in the spawn callback: return THIS child to the
                        // runner even when a guard mutation makes the control fail.
                        let checks = (
                            std::fs::read(path).is_ok_and(|raw| raw.is_empty()),
                            child.try_wait().is_ok_and(|s| s.is_none()),
                            check_actions.begin(check_key, &Action::Terminate).is_none(),
                            check_slot.upgrade().is_some() && busy.load(Ordering::Acquire),
                            check_service
                                .shared
                                .rpc
                                .snapshot()
                                .is_ok_and(|v| v.count(Role::Fleet) > 0),
                        );
                        let _ = observed.send(checks);
                    }))
                });
                context.rpc(
                    vec!["desktop".into(), "frontend-request".into(), "--json".into()],
                    Some(input),
                    Some(registration),
                )
            })
            .await
            .unwrap();
            assert!(result.is_err());
            assert_eq!(
                observation.try_recv().unwrap(),
                (true, true, true, true, true)
            );
            assert_eq!(
                std::fs::read(f.0.join("termination-refused-input")).unwrap(),
                b""
            );
            assert!(
                original_slot.upgrade().is_none(),
                "registration released only after physical stop/join"
            );
            assert!(actions.begin(key, &Action::Terminate).is_some());
            assert!(requests(&calls(&f), "termination_outcome").is_empty());
            assert!(effects(&calls(&f)).is_empty());
            drop(other);
            service.join().await.unwrap();
        }
    });
}

#[test]
fn an_oversized_native_receipt_is_bounded_and_never_restores_dispatch() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"termination_effect_oversize":true}));
        let (service, _cache) = connected(&f).await;
        let pending = Arc::new(Pending::default());
        let actions = actions(&f);
        let key = crate::actions::Lane::key(&capture().target, &Action::Terminate);
        let hold = actions.begin(key.clone(), &Action::Terminate).unwrap();
        let mut controller = confirm(service.clone(), pending.clone())
            .bind(Arc::new(hold))
            .unwrap();
        let invocation = controller.invocation().clone();
        controller.retain().unwrap();
        controller.begin_dispatch(&invocation).unwrap();
        let exit = controller.run_native(&invocation);
        assert!(
            matches!(&exit, Exit::StartFailed(_)),
            "the native runner must enforce the receipt cap"
        );
        assert!(!matches!(
            capture().completed(&invocation, exit),
            Outcome::Done { .. }
        ));
        assert_eq!(effects(&calls(&f)).len(), 1);
        assert!(Confirmation::new(
            uuid::Uuid::new_v4().to_string(),
            capture(),
            selected(service.clone()),
            pending
        )
        .is_err());
        eventually("oversize native resources physically joined", || {
            actions.begin(key.clone(), &Action::Terminate).is_some()
        })
        .await;
        service.join().await.unwrap();
    });
}

#[test]
fn overflow_keeps_original_dispatch_and_claim_until_the_actual_pipe_ends() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    struct Release(PathBuf);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = std::fs::write(self.0.join("termination-release"), b"release");
        }
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(
            json!({"termination_effect_oversize":true,"termination_effect_oversize_pipe":true}),
        );
        f.1.store(true, Ordering::Release);
        let _release = Release(f.0.clone());
        let (service, _cache) = connected(&f).await;
        let pending = Arc::new(Pending::default());
        let actions = actions(&f);
        let original = confirm(service.clone(), pending.clone());
        let target = original.capture().target.clone();
        let key = crate::actions::Lane::key(&target, &Action::Terminate);
        let outcome = crate::actions::dispatch_cleanup_native(&actions, original, None)
            .await
            .unwrap();
        assert!(!matches!(outcome, Outcome::Done { .. }));
        assert_eq!(effects(&calls(&f)).len(), 1);
        assert!(actions.begin(key.clone(), &Action::Terminate).is_none());
        assert!(Confirmation::new(
            uuid::Uuid::new_v4().to_string(),
            capture(),
            selected(service.clone()),
            pending
        )
        .is_err());
        std::fs::write(f.0.join("termination-release"), b"release").unwrap();
        eventually("overflow original pipe EOF", || {
            actions.begin(key.clone(), &Action::Terminate).is_some()
        })
        .await;
        service.join().await.unwrap();
        f.1.store(false, Ordering::Release);
    });
}
