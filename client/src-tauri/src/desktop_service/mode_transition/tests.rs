//! Real original Python child/pipe tests with a private transport fixture.
//! Native helper caller/Grant and manager policy are tested in the Python
//! helper suite; this fixture does not manufacture a native auth positive.
use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

const CONFIG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
struct Fixture {
    root: PathBuf,
    cli: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("zerus-mode-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("bin")).unwrap();
        let dir = root.join("lib/zerus");
        fs::create_dir_all(&dir).unwrap();
        let cli = root.join("bin/hgs");
        fs::write(&cli, b"#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(dir.join("install-linux-desktop-service.py"), SCRIPT).unwrap();
        for name in ["desktop-revoke-hold.py", "desktop-service-activation.py"] {
            fs::write(dir.join(name), b"# private transport fixture\n").unwrap();
        }
        let fixture = Self { root, cli };
        fixture.config("default", "passive", false);
        fixture
    }
    fn config(&self, selection: &str, mode: &str, lose: bool) {
        fs::write(
            self.root.join("configuration.json"),
            serde_json::to_vec(&json!({"selection":selection,"mode":mode,"lose":lose})).unwrap(),
        )
        .unwrap();
    }
    fn effects(&self) -> usize {
        fs::read_to_string(self.root.join("effects"))
            .unwrap_or_default()
            .lines()
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn maintained_cli_links_pin_the_same_original_target() {
    let f = Fixture::new();
    let target = f.root.join("original-cli");
    fs::rename(&f.cli, &target).unwrap();
    std::os::unix::fs::symlink(&target, &f.cli).unwrap();
    let route = Route::select(&f.cli).unwrap();
    route.check().unwrap();
    fs::rename(&target, target.with_extension("old")).unwrap();
    fs::write(&target, b"changed CLI").unwrap();
    assert!(route.check().is_err());
}

#[tokio::test]
async fn automatic_activation_rechecks_a_later_explicit_passive_selection() {
    let f = Fixture::new();
    let owner = Controller::new();
    owner.observe(f.cli.clone(), CONFIG.into()).await;
    assert!(owner.automatic());
    f.config("explicit", "passive", false);
    owner.activate_default(f.cli.clone(), CONFIG.into()).await;
    assert_eq!(f.effects(), 0);
    assert!(!owner.pending());
    assert!(owner.view().needs_resume);
}

#[tokio::test]
async fn selection_provenance_is_explicit_and_read_only_reconnect_never_activates() {
    let f = Fixture::new();
    let owner = Controller::new();
    for provenance in ["default", "explicit", "retained"] {
        f.config(provenance, "passive", false);
        owner.observe(f.cli.clone(), CONFIG.into()).await;
        assert_eq!(owner.automatic(), provenance == "default");
        assert!(owner.view().needs_resume);
        assert!(!owner.pending());
        assert_eq!(f.effects(), 0);
    }
    f.config("explicit", "background", false);
    owner.activate(f.cli.clone(), CONFIG.into()).await;
    assert_eq!(f.effects(), 0, "already active is a no-op");
}

#[tokio::test]
async fn original_activation_settles_and_reconnect_never_repeats_the_helper() {
    let f = Fixture::new();
    let owner = Controller::new();
    owner.activate(f.cli.clone(), CONFIG.into()).await;
    assert!(!owner.pending());
    assert_eq!(owner.view().code, "");
    assert_eq!(f.effects(), 1);
    for _ in 0..3 {
        owner.observe(f.cli.clone(), CONFIG.into()).await;
    }
    owner.activate(f.cli.clone(), CONFIG.into()).await;
    assert_eq!(
        f.effects(),
        1,
        "reconnection/already active must not repeat the helper transition"
    );
}

#[tokio::test]
async fn lost_reply_and_restart_recover_only_the_original_activation_id() {
    let f = Fixture::new();
    f.config("default", "passive", true);
    let owner = Controller::new();
    owner.activate(f.cli.clone(), CONFIG.into()).await;
    assert!(owner.pending());
    let original = fs::read(f.root.join("intent.json")).unwrap();
    for _ in 0..2 {
        owner.activate(f.cli.clone(), CONFIG.into()).await;
    }
    let restarted = Controller::new();
    restarted.observe(f.cli.clone(), CONFIG.into()).await;
    restarted.activate(f.cli.clone(), CONFIG.into()).await;
    assert!(restarted.pending());
    assert_eq!(fs::read(f.root.join("intent.json")).unwrap(), original);
    assert_eq!(f.effects(), 1);
    assert!(!restarted.automatic());
}

#[tokio::test]
async fn helper_replacement_refuses_before_an_effect_and_keeps_the_original_lookup() {
    let f = Fixture::new();
    let route = Route::select(&f.cli).unwrap();
    let helper = route.helper.clone();
    let bytes = fs::read(&helper).unwrap();
    fs::rename(&helper, helper.with_extension("previous")).unwrap();
    fs::write(&helper, bytes).unwrap();
    assert!(route.check().is_err());
    assert_eq!(f.effects(), 0);
}

const SCRIPT: &str = r#"import json,sys
from pathlib import Path
root=Path(__file__).resolve().parents[2]
config=json.loads((root/'configuration.json').read_text())
scope='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
args=sys.argv[1:]
if '--help' in args:
 print('Desktop service activation ABI: desktop-service-activation-v1');sys.exit(0)
intent=root/'intent.json'
stored=json.loads(intent.read_text()) if intent.exists() else None
request=args[args.index('--activation-id')+1] if '--activation-id' in args else None
if 'activation-status' in args:
 result=stored if stored and stored['request_id']==request else dict(abi='desktop-service-activation-v1',request_id=request,configuration=scope,read_only=True,phase='missing',result=None)
elif '--mode' in args:
 with (root/'effects').open('a') as out:out.write(request+'\n')
 result=dict(abi='desktop-service-activation-v1',request_id=request,configuration=scope,read_only=True,phase='unknown' if config['lose'] else 'settled',result=None if config['lose'] else dict(ok=True,code='ready'))
 intent.write_text(json.dumps(result))
 if config['lose']:sys.exit(1)
 config['mode']='background';(root/'configuration.json').write_text(json.dumps(config))
else:
 result=dict(schema=1,configuration=scope,selected_mode=config['mode'],running_mode=config['mode'],ipc_ready=True,mode_selection=config['selection'],pending_restart=False,transition=None,activation_pending=stored['request_id'] if stored and stored['phase']=='unknown' else None)
print(json.dumps(result))
"#;

struct Running {
    service: Arc<Service>,
    _cache: crate::desktop_service::frontend::tests::native::CacheServer,
    source: crate::desktop_service::tests::Fixture,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.service.stop();
    }
}
fn start_native(helpers: &Fixture, config: Value) -> Running {
    use crate::desktop_service::frontend::tests::native::{responder, CacheServer};
    let source = crate::desktop_service::tests::Fixture::new(config);
    fs::remove_file(source.0.join("hgs")).unwrap();
    let code = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    for (leaf, target) in [
        ("hgs", "desktop_ownership/fake_hgs.py"),
        ("catalog-fixture.py", "desktop_service/fake_hgs.py"),
    ] {
        std::os::unix::fs::symlink(code.join(target), source.0.join(leaf)).unwrap();
    }
    let responder = responder();
    assert!(responder
        .configure_ownership_mode_once(zerus_client_core::desktop::ownership::Mode::Service));
    let cache = CacheServer::start(&source.0, responder.clone());
    let pump = crate::pump::Pump::default();
    let (sink, changes) = std::sync::mpsc::sync_channel(256);
    let drain = pump.drain_service(changes, |_| {});
    let service = Service::start_observed(
        source.0.join("hgs"),
        pump,
        sink,
        true,
        drain,
        Some(responder),
        Observers {
            mode_helpers: Some(helpers.root.join("lib/zerus")),
            ..Default::default()
        },
    );
    Running {
        service,
        _cache: cache,
        source,
    }
}
fn mutations(running: &Running) -> Vec<String> {
    running
        .source
        .calls()
        .iter()
        .filter_map(|call| {
            call["value"]["request"]["action"]
                .as_str()
                .map(str::to_owned)
        })
        .collect()
}
#[test]
fn actual_startup_then_automatic_reconnect_never_repeats_grant_or_helper_activation() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        let running = start_native(&helpers, json!({"ready":true,"restart_watch":true}));
        crate::desktop_service::tests::eventually("startup Grant then original activation", || {
            running.service.snapshot().connected
                && helpers.effects() == 1
                && running.service.shared.ownership.granted()
        })
        .await;
        assert_eq!(mutations(&running), vec!["grant"]);
        // Force a newly observed default-passive status during reconnect. Even
        // that positive policy observation must not make reconnect activate.
        helpers.config("default", "passive", false);
        fs::write(running.source.0.join("restart-watch"), []).unwrap();
        crate::desktop_service::tests::eventually("a new automatic read stream", || {
            running
                .source
                .calls()
                .iter()
                .filter(|c| c["args"] == json!(["desktop", "watch", "--json"]))
                .count()
                >= 2
        })
        .await;
        running.service.join().await.unwrap();
        assert_eq!(mutations(&running), vec!["grant"]);
        assert_eq!(helpers.effects(), 1);
    });
}
#[test]
fn actual_explicit_resume_rearms_only_when_revoked_and_then_activates_once() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for revoked in [false, true] {
            let helpers = Fixture::new();
            helpers.config("explicit", "passive", false);
            let running = start_native(
                &helpers,
                json!({"ready":true,"state":if revoked {"revoked"} else {"passive"},"rearm_required":revoked}),
            );
            crate::desktop_service::tests::eventually("limited consumer is available", || {
                // Connected is the preflight projection. The original watch is
                // spawned only after ownership and orchestration have settled.
                running.service.snapshot().connected
                    && running
                        .source
                        .calls()
                        .iter()
                        .any(|call| call["args"] == json!(["desktop", "watch", "--json"]))
            })
            .await;
            assert_eq!(
                helpers.effects(),
                0,
                "explicit passive never resumes on mount"
            );
            if revoked {
                assert!(mutations(&running).is_empty());
            } else {
                assert_eq!(mutations(&running), vec!["grant"]);
            }
            running.service.ownership_rearm().await;
            crate::desktop_service::tests::eventually(
                "Resume reconnects read-only after its one mutation",
                || {
                    running
                        .source
                        .calls()
                        .iter()
                        .filter(|call| call["args"] == json!(["desktop", "watch", "--json"]))
                        .count()
                        >= 2
                },
            )
            .await;
            running.service.join().await.unwrap();
            assert_eq!(
                mutations(&running),
                if revoked {
                    vec!["rearm"]
                } else {
                    vec!["grant"]
                }
            );
            assert_eq!(helpers.effects(), 1);
        }
    });
}

#[test]
fn actual_unconfirmed_grant_offers_only_original_lookup_and_never_activates() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        let running = start_native(&helpers, json!({"ready":true,"pending":true}));
        crate::desktop_service::tests::eventually(
            "original pending Grant settled to the read stream",
            || {
                running.service.snapshot().connected
                    && running
                        .source
                        .calls()
                        .iter()
                        .any(|call| call["args"] == json!(["desktop", "watch", "--json"]))
            },
        )
        .await;
        assert!(running.service.shared.ownership.view().pending);
        assert_eq!(mutations(&running), vec!["grant"]);
        assert_eq!(helpers.effects(), 0);
        let original = fs::read(running.source.0.join("original.json")).unwrap();
        running.service.ownership_rearm().await;
        running.service.join().await.unwrap();
        assert_eq!(mutations(&running), vec!["grant"]);
        assert_eq!(helpers.effects(), 0);
        assert_eq!(
            fs::read(running.source.0.join("original.json")).unwrap(),
            original
        );
    });
}
