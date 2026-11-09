//! Real original Python child/pipe tests with a private transport fixture.
//! Native helper caller/Grant and manager policy are tested in the Python
//! helper suite; this fixture does not manufacture a native auth positive.
use super::*;
use std::fs;

mod recovery;

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
        crate::write_executable(&cli, b"#!/bin/sh\nexit 1\n");
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
    fn patch(&self, key: &str, value: Value) {
        let path = self.root.join("configuration.json");
        let mut config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config[key] = value;
        fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    }
    fn reviews(&self) -> usize {
        fs::read_to_string(self.root.join("reviews"))
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
    if crate::test_context::desktop_fixture() {
        return;
    }
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

#[test]
fn absent_startup_environment_requires_a_read_only_owned_registration() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let f = Fixture::new();
    for mode in ["passive", "background"] {
        for registered in [false, true] {
            fs::write(
                f.root.join("configuration.json"),
                serde_json::to_vec(&json!({"selection":"explicit","mode":mode,"lose":false,"registered":registered})).unwrap(),
            ).unwrap();
            assert_eq!(super::super::startup_mode(None, true, &f.cli), registered);
            assert_eq!(
                f.effects(),
                0,
                "default selection never activates a manager"
            );
        }
    }
    fs::write(f.root.join("configuration.json"), b"invalid").unwrap();
    assert!(!super::super::startup_mode(None, true, &f.cli));
    assert!(!super::super::startup_mode(
        Some(std::ffi::OsStr::new("0")),
        true,
        &f.cli
    ));
    assert!(super::super::startup_mode(
        Some(std::ffi::OsStr::new("1")),
        true,
        &f.cli
    ));
    assert!(!super::super::startup_mode(None, false, &f.cli));
}

#[test]
fn unknown_default_registration_and_old_helper_never_enable_the_consumer() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let f = Fixture::new();
    for wrong in [
        json!({"registered": "true"}),
        json!({"operation": "install"}),
        json!({"schema": 2}),
        json!({"label": "other.service"}),
        json!({"configuration": "A".repeat(64)}),
        json!({"selected_mode": "unknown"}),
    ] {
        fs::write(
            f.root.join("configuration.json"),
            serde_json::to_vec(&json!({
                "selection":"explicit","mode":"passive","lose":false,"status_overrides":wrong
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(!super::super::startup_mode(None, true, &f.cli));
        assert_eq!(f.effects(), 0);
    }
    f.config("explicit", "passive", false);
    let helper = f.root.join("lib/zerus/install-linux-desktop-service.py");
    fs::write(
        &helper,
        "import json,sys\nfrom pathlib import Path\np=Path(__file__).resolve().parents[2]\nwith (p/'old-calls').open('a') as f:f.write(json.dumps(sys.argv[1:])+'\\n')\nif '--help' in sys.argv:print('old helper')\nelse:print(json.dumps(dict(schema=1,operation='status',registered=True,label='hgs-desktop.service',configuration='a'*64,selected_mode='passive')))\n",
    )
    .unwrap();
    assert!(!super::super::startup_mode(None, true, &f.cli));
    assert_eq!(f.effects(), 0);
    let calls: Vec<Value> = fs::read_to_string(f.root.join("old-calls"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(calls, [json!(["--help"])]);
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
scope=config.get('scope','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')
args=sys.argv[1:]
if '--help' in args:
 if config.get('classification_old_help'): print('old helper without classification');sys.exit(0)
 if config.get('classification_hold_help'):
  import time
  (root/'classification-help-entered').touch()
  until=time.monotonic()+15
  while not (root/'classification-help-release').exists() and time.monotonic()<until: time.sleep(.005)
 print('Desktop service activation ABI: desktop-service-activation-v1\nreview-activation\nDesktop service mode choice scope ABI: desktop-service-mode-choice-scope-v1');sys.exit(0)
intent=root/'intent.json'
stored=json.loads(intent.read_text()) if intent.exists() else None
review_path=root/'review.json'
reviewed=json.loads(review_path.read_text()) if review_path.exists() else None
request=args[args.index('--activation-id')+1] if '--activation-id' in args else None
if 'review-activation' in args:
 if not stored or stored['request_id']!=request or stored['phase']!='unknown':sys.exit(1)
 disposition=stored.get('disposition')
 boot=config.get('boot','first-native-boot-model')
 if disposition and disposition['state']=='reboot_required' and disposition['boot']==boot:sys.exit(1)
 stored['disposition']=dict(state='reviewed_unknown' if disposition else 'reboot_required',request_id=request,binding=dict(configuration=scope),boot=disposition['boot'] if disposition else boot)
 intent.write_text(json.dumps(stored));review_path.write_text(json.dumps(stored))
 with (root/'reviews').open('a') as out:out.write(request+'\n')
 if config.get('lose_review'):sys.exit(1)
 result=dict(stored,read_only=False)
elif 'activation-status' in args:
 result=stored if stored and stored['request_id']==request else reviewed if reviewed and reviewed['request_id']==request else dict(abi='desktop-service-activation-v1',request_id=request,configuration=scope,read_only=True,phase='missing',result=None)
elif '--mode' in args:
 if reviewed and config.get('refuse_choice'):sys.exit(1)
 if reviewed and ('--after-review' not in args or args[args.index('--after-review')+1]!=reviewed['request_id'] or reviewed['disposition']['state']!='reviewed_unknown'):sys.exit(1)
 with (root/'effects').open('a') as out:out.write(request+'\n')
 result=dict(abi='desktop-service-activation-v1',request_id=request,configuration=scope,read_only=True,phase='unknown' if config['lose'] else 'settled',result=None if config['lose'] else dict(ok=True,code='ready'))
 if reviewed:
  result['after_review']=reviewed['request_id']
  if not config['lose']:
   result['scope']=dict(journal=config.get('journal','11111111-1111-4111-8111-111111111111'),owner_epoch=config.get('owner_epoch','22222222-2222-4222-8222-222222222222'),service_epoch=config.get('service_epoch','33333333-3333-4333-8333-333333333333'),generation=config.get('service_generation',1))
 intent.write_text(json.dumps(result))
 if config['lose']:sys.exit(1)
 config['mode']='background';config['selection']='explicit';(root/'configuration.json').write_text(json.dumps(config))
else:
 pending=stored['request_id'] if stored and stored['phase']=='unknown' and stored.get('disposition',{}).get('state')!='reviewed_unknown' else None
 result=dict(schema=1,operation='status',registered=config.get('registered',True),label='hgs-desktop.service',configuration=scope,selected_mode=config['mode'],running_mode=config['mode'],ipc_ready=True,mode_selection=config['selection'],pending_restart=False,transition=None,activation_pending=config.get('pending',pending),activation_review=dict(request_id=reviewed['request_id'],phase='unknown',result=None,disposition=reviewed['disposition']) if reviewed else None,activation_automatic_blocked=reviewed is not None,mode_choice=dict(request=stored['request_id'],reviewed=stored['after_review'],desired='background',phase=stored['phase'],scope=stored.get('scope')) if stored and 'after_review' in stored else None,service_epoch=config.get('service_epoch'),service_generation=config.get('service_generation'))
 result.update(config.get('status_overrides',{}))
print(json.dumps(result))
"#;

// Confirmation callbacks below model the native user's response. They do not
// qualify the real dialog, native helper admission or a Windows manager effect.
async fn interrupted(f: &Fixture) -> Arc<Controller> {
    f.config("default", "passive", true);
    let owner = Controller::new();
    owner.activate(f.cli.clone(), CONFIG.into()).await;
    owner.observe(f.cli.clone(), CONFIG.into()).await;
    assert_eq!(
        owner.view().review.unwrap().disposition,
        review::Disposition::Required
    );
    owner
}
async fn acknowledge_model_boot(f: &Fixture, owner: &Arc<Controller>) {
    let original = owner.view().review.unwrap();
    let first = owner
        .confirm(original, ReviewAction::Review, |_, _| true)
        .await
        .unwrap()
        .unwrap();
    let _ = owner.apply(first).await;
    assert_eq!(
        owner.view().review.unwrap().disposition,
        review::Disposition::RebootRequired
    );
    f.patch("boot", json!("different-native-boot-model"));
    let second = owner
        .confirm(
            owner.view().review.unwrap(),
            ReviewAction::Review,
            |_, _| true,
        )
        .await
        .unwrap()
        .unwrap();
    let _ = owner.apply(second).await;
    assert_eq!(
        owner.view().review.unwrap().disposition,
        review::Disposition::ReviewedUnknown
    );
}

#[tokio::test]
async fn native_confirmation_cancel_and_stale_scope_perform_no_review_or_choice() {
    let f = Fixture::new();
    let owner = interrupted(&f).await;
    let display = owner.view().review.unwrap();
    assert!(owner
        .confirm(display.clone(), ReviewAction::Review, |_, _| false)
        .await
        .unwrap()
        .is_none());
    assert_eq!((f.effects(), f.reviews()), (1, 0));
    let mut stale = display.clone();
    stale.configuration = "b".repeat(64);
    assert!(owner
        .confirm(stale, ReviewAction::Review, |_, _| panic!(
            "stale view reached native confirmation"
        ))
        .await
        .is_err());
    let path = f.root.join("configuration.json");
    assert!(owner
        .confirm(display, ReviewAction::Review, move |_, _| {
            let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["scope"] = json!("b".repeat(64));
            fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
            true
        })
        .await
        .is_err());
    assert_eq!((f.effects(), f.reviews()), (1, 0));
    assert!(owner.blocks_resume());
}

#[tokio::test]
async fn review_lost_reply_and_reconnect_are_reads_until_a_second_confirmation() {
    let f = Fixture::new();
    let owner = interrupted(&f).await;
    f.patch("lose_review", json!(true));
    acknowledge_model_boot(&f, &owner).await;
    assert_eq!((f.effects(), f.reviews()), (1, 2));
    let archived = fs::read(f.root.join("review.json")).unwrap();
    for _ in 0..2 {
        owner.observe(f.cli.clone(), CONFIG.into()).await;
        owner.activate(f.cli.clone(), CONFIG.into()).await;
    }
    let restarted = Controller::new();
    restarted.observe(f.cli.clone(), CONFIG.into()).await;
    restarted.activate(f.cli.clone(), CONFIG.into()).await;
    assert!(restarted.blocks_resume());
    assert!(!restarted.automatic());
    assert_eq!((f.effects(), f.reviews()), (1, 2));
    f.patch("lose", json!(false));
    let choice = restarted
        .confirm(
            restarted.view().review.unwrap(),
            ReviewAction::Background,
            |_, _| true,
        )
        .await
        .unwrap()
        .unwrap();
    let _ = restarted.apply(choice).await;
    assert_eq!(f.effects(), 2);
    assert_eq!(fs::read(f.root.join("review.json")).unwrap(), archived);
    assert!(!restarted.view().pending);
    assert!(restarted.view().automatic_blocked);
    assert!(restarted.blocks_resume());
    assert_eq!(
        restarted
            .close()
            .wait_until(std::time::Instant::now() + Duration::from_secs(1)),
        zerus_client_core::desktop::drain::Observation::Idle
    );
}

#[tokio::test]
async fn second_confirmation_refuses_changed_selection_and_lost_choice_never_rotates_its_id() {
    let f = Fixture::new();
    let owner = interrupted(&f).await;
    acknowledge_model_boot(&f, &owner).await;
    let path = f.root.join("configuration.json");
    let display = owner.view().review.unwrap();
    assert!(owner
        .confirm(display, ReviewAction::Background, move |_, _| {
            let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["selection"] = "retained".into();
            fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
            true
        })
        .await
        .is_err());
    assert_eq!((f.effects(), f.reviews()), (1, 2));
    let choice = owner
        .confirm(
            owner.view().review.unwrap(),
            ReviewAction::Background,
            |_, _| true,
        )
        .await
        .unwrap()
        .unwrap();
    let _ = owner.apply(choice).await;
    let original = fs::read(f.root.join("intent.json")).unwrap();
    let id = owner.view().request_id.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&original).unwrap()["request_id"],
        id
    );
    for _ in 0..3 {
        owner.observe(f.cli.clone(), CONFIG.into()).await;
        owner.activate(f.cli.clone(), CONFIG.into()).await;
    }
    let restarted = Controller::new();
    restarted.observe(f.cli.clone(), CONFIG.into()).await;
    assert_eq!(restarted.view().request_id.as_deref(), Some(id.as_str()));
    assert!(restarted.view().pending);
    assert_eq!(fs::read(f.root.join("intent.json")).unwrap(), original);
    assert_eq!((f.effects(), f.reviews()), (2, 2));
}

#[tokio::test]
async fn a_borrowed_missing_result_never_claims_the_original_effect_child_joined() {
    let f = Fixture::new();
    let original = uuid::Uuid::new_v4().to_string();
    f.patch("pending", json!(original));
    let owner = Controller::new();
    owner.observe(f.cli.clone(), CONFIG.into()).await;
    owner.activate(f.cli.clone(), CONFIG.into()).await;
    assert!(owner.view().pending);
    assert_eq!(owner.view().request_id.as_deref(), Some(original.as_str()));
    assert_eq!(f.effects(), 0);
}

#[tokio::test]
async fn helper_replacement_during_confirmation_keeps_the_original_selection_and_has_no_effect() {
    let f = Fixture::new();
    let owner = interrupted(&f).await;
    let helper = f.root.join("lib/zerus/install-linux-desktop-service.py");
    let display = owner.view().review.unwrap();
    assert!(owner
        .confirm(display.clone(), ReviewAction::Review, move |_, _| {
            let bytes = fs::read(&helper).unwrap();
            fs::rename(&helper, helper.with_extension("original")).unwrap();
            fs::write(helper, bytes).unwrap();
            true
        })
        .await
        .is_err());
    assert_eq!(owner.view().review.unwrap(), display);
    assert_eq!((f.effects(), f.reviews()), (1, 0));
}

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
    let current_epoch = config
        .get("runtime_epoch")
        .and_then(Value::as_str)
        .unwrap_or("00000000-0000-4000-8000-000000000001")
        .to_owned();
    let generation = config
        .get("status_generation")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let hold_preflight = config["hold_mode_preflight"] == true;
    let hold_observation = config["hold_observation"] == true;
    let source = crate::desktop_service::tests::Fixture::new(config);
    fs::write(source.0.join("runtime-epoch"), &current_epoch).unwrap();
    if hold_preflight {
        fs::write(source.0.join("mode-preflight-hold"), []).unwrap();
    }
    if hold_observation {
        fs::write(source.0.join("observation-hold"), []).unwrap();
    }
    // Native protocol models agree on the same current incarnation. These
    // constants do not establish a production/native manager positive.
    helpers.patch("journal", json!("00000000-0000-4000-8000-000000000005"));
    helpers.patch("owner_epoch", json!("00000000-0000-4000-8000-000000000006"));
    helpers.patch("service_epoch", json!(current_epoch));
    helpers.patch("service_generation", json!(generation));
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
async fn confirm_when_idle(owner: &Arc<Controller>, action: ReviewAction) -> Prepared {
    let until = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let display = owner.view().review.unwrap();
        match owner.confirm(display, action, |_, _| true).await {
            Ok(Some(prepared)) => return prepared,
            Err("activation_original_resources_pending") if std::time::Instant::now() < until => {
                tokio::time::sleep(Duration::from_millis(10)).await
            }
            _ => panic!("original confirmation did not become available"),
        }
    }
}
#[test]
fn actual_unavailable_preflight_projects_the_interrupted_original_and_resume_is_only_a_read() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        let original = interrupted(&helpers).await;
        let id = original.view().request_id.unwrap();
        assert_eq!(
            original
                .close()
                .wait_until(std::time::Instant::now() + Duration::from_secs(1)),
            zerus_client_core::desktop::drain::Observation::Idle
        );
        let running = start_native(
            &helpers,
            json!({"produced_status":{"running":false,"configuration":CONFIG}}),
        );
        crate::desktop_service::tests::eventually("offline original is reviewable", || {
            let snapshot = running.service.snapshot();
            snapshot.code == "service_unavailable"
                && snapshot
                    .notification_ownership
                    .as_ref()
                    .is_some_and(|v| v["activation"]["review"]["request_id"] == id)
        })
        .await;
        assert!(mutations(&running).is_empty());
        let review = confirm_when_idle(
            &running.service.shared.mode_transition,
            ReviewAction::Review,
        )
        .await;
        running
            .service
            .apply_mode_action(Some(review))
            .await
            .unwrap();
        assert_eq!(helpers.reviews(), 1);
        running.service.ownership_rearm().await;
        running.service.retry_connection();
        crate::desktop_service::tests::eventually("read-only backoff checks the original", || {
            running
                .source
                .calls()
                .iter()
                .filter(|c| c["args"] == json!(["desktop", "status", "--json"]))
                .count()
                >= 2
        })
        .await;
        running.service.join().await.unwrap();
        assert!(mutations(&running).is_empty());
        assert!(running
            .source
            .calls()
            .iter()
            .all(|c| c["args"] != json!(["desktop", "watch", "--json"])));
        assert_eq!((helpers.effects(), helpers.reviews()), (1, 1));
    });
}

#[test]
fn actual_reviewed_archive_needs_a_second_confirmation_before_its_only_rearm_and_activation() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        let original = interrupted(&helpers).await;
        acknowledge_model_boot(&helpers, &original).await;
        assert_eq!(
            original
                .close()
                .wait_until(std::time::Instant::now() + Duration::from_secs(1)),
            zerus_client_core::desktop::drain::Observation::Idle
        );
        helpers.config("default", "passive", false);
        let running = start_native(&helpers, json!({"ready":true,"observation_abi":true}));
        crate::desktop_service::tests::eventually(
            "reviewed consumer opens its read stream",
            || {
                running.service.snapshot().connected
                    && running
                        .source
                        .calls()
                        .iter()
                        .any(|c| c["args"] == json!(["desktop", "watch", "--json"]))
            },
        )
        .await;
        running.service.ownership_rearm().await;
        assert!(mutations(&running).is_empty());
        assert_eq!((helpers.effects(), helpers.reviews()), (1, 2));
        let mode = &running.service.shared.mode_transition;
        assert!(mode.startup_permitted());
        let held = mode.gate.admit(Role::Fleet).unwrap();
        running.service.retry();
        crate::desktop_service::tests::eventually(
            "the reviewed connection defers on an unrelated busy read",
            || {
                running
                    .source
                    .calls()
                    .iter()
                    .filter(|c| c["args"] == json!(["desktop", "watch", "--json"]))
                    .count()
                    == 2
            },
        )
        .await;
        assert!(mode.classification_pending());
        assert!(!mode.startup_permitted());
        drop(held);
        crate::desktop_service::tests::eventually(
            "review classification recovers without scheduling consent",
            || !mode.classification_pending() && mode.gate.snapshot().unwrap().admitted == 0,
        )
        .await;
        assert!(
            !mode.startup_permitted(),
            "only the following B2 confirmation may clear this prior deferral"
        );
        let choice = confirm_when_idle(
            &running.service.shared.mode_transition,
            ReviewAction::Background,
        )
        .await;
        running
            .service
            .apply_mode_action(Some(choice))
            .await
            .unwrap();
        crate::desktop_service::tests::eventually(
            "the confirmed background scope permits existing work",
            || running.service.shared.business_permitted(),
        )
        .await;
        assert!(
            running.service.shared.mode_transition.startup_permitted(),
            "a confirmed B2 continuation must not defer on its own charge"
        );
        assert_eq!(mutations(&running), vec!["rearm", "grant"]);
        let count = mutations(&running).len();
        let watches_before_retry = running
            .source
            .calls()
            .iter()
            .filter(|c| c["args"] == json!(["desktop", "watch", "--json"]))
            .count();
        running.service.retry();
        crate::desktop_service::tests::eventually("Retry only reopens the read stream", || {
            running
                .source
                .calls()
                .iter()
                .filter(|c| c["args"] == json!(["desktop", "watch", "--json"]))
                .count()
                > watches_before_retry
        })
        .await;
        assert_eq!(
            mutations(&running).len(),
            count,
            "Retry cannot refresh a Grant"
        );
        // A previous settled receipt must not settle a later failed choice.
        helpers.patch("refuse_choice", json!(true));
        let refused = confirm_when_idle(
            &running.service.shared.mode_transition,
            ReviewAction::Background,
        )
        .await;
        running
            .service
            .apply_mode_action(Some(refused))
            .await
            .unwrap();
        assert!(
            !running.service.shared.business_permitted(),
            "a historical receipt is not THIS confirmed choice"
        );
        assert_eq!(
            mutations(&running).len(),
            count,
            "a missing new receipt must not refresh ownership"
        );
        assert_eq!(helpers.effects(), 2);
        let native = running.source.0.join("current.json");
        let mut replaced: Value = serde_json::from_slice(&fs::read(&native).unwrap()).unwrap();
        replaced["owner_epoch"] = uuid::Uuid::new_v4().to_string().into();
        fs::write(&native, serde_json::to_vec(&replaced).unwrap()).unwrap();
        running.service.ownership_result().await;
        assert!(
            !running.service.shared.business_permitted(),
            "a replacement Grant cannot use old reviewed history"
        );
        assert_eq!(
            running.service.snapshot().notification_ownership.unwrap()["activation"]
                ["business_ready"],
            false
        );
        assert_eq!(
            mutations(&running).len(),
            count,
            "an ownership check has zero effects"
        );
        helpers.config("explicit", "passive", false);
        running
            .service
            .shared
            .mode_transition
            .observe(running.source.0.join("hgs"), CONFIG.into())
            .await;
        assert!(
            !running.service.shared.business_permitted(),
            "a later passive selection closes business"
        );
        running.service.join().await.unwrap();
        assert_eq!(mutations(&running), vec!["rearm", "grant"]);
        assert_eq!((helpers.effects(), helpers.reviews()), (2, 2));
        assert!(
            running
                .service
                .shared
                .mode_transition
                .view()
                .automatic_blocked
        );
    });
}
#[test]
fn preflight_and_ownership_waits_cannot_rearm_a_changed_confirmed_original() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for lane in ["preflight", "ownership"] {
            let helpers = Fixture::new();
            let original = interrupted(&helpers).await;
            acknowledge_model_boot(&helpers, &original).await;
            assert_eq!(
                original
                    .close()
                    .wait_until(std::time::Instant::now() + Duration::from_secs(1)),
                zerus_client_core::desktop::drain::Observation::Idle
            );
            helpers.config("default", "passive", false);
            let running = start_native(&helpers, json!({"ready":true}));
            crate::desktop_service::tests::eventually("original read stream settled", || {
                running
                    .source
                    .calls()
                    .iter()
                    .any(|c| c["args"] == json!(["desktop", "watch", "--json"]))
                    && !running.service.shared.ownership.view().running
            })
            .await;
            let prepared = confirm_when_idle(
                &running.service.shared.mode_transition,
                ReviewAction::Background,
            )
            .await;
            let mode_id = running.service.shared.mode_transition.view().request_id;
            let record = fs::read(helpers.root.join("intent.json")).unwrap();
            let ownership_id = running.service.shared.ownership.view().request_id;
            fs::write(running.source.0.join(format!("mode-{lane}-hold")), []).unwrap();
            let owner = running.service.clone();
            let task = tauri::async_runtime::spawn(async move {
                owner.apply_mode_action(Some(prepared)).await
            });
            crate::desktop_service::tests::eventually(
                "exact slow native preflight boundary",
                || {
                    running
                        .source
                        .0
                        .join(format!("mode-{lane}-entered"))
                        .exists()
                },
            )
            .await;
            // Same configuration and original UUID; only its selected mode
            // changes after the initial opaque validation and before Rearm.
            helpers.config("explicit", "background", false);
            fs::write(running.source.0.join(format!("mode-{lane}-release")), []).unwrap();
            let result = tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .unwrap()
                .unwrap();
            running.service.join().await.unwrap();
            assert!(
                mutations(&running).is_empty(),
                "stale {lane} offered a frontend request"
            );
            assert_eq!(result.unwrap_err(), "activation_scope_changed", "{lane}");
            assert_eq!(
                running.service.shared.mode_transition.view().request_id,
                mode_id
            );
            assert_eq!(
                running.service.shared.ownership.view().request_id,
                ownership_id
            );
            assert_eq!(fs::read(helpers.root.join("intent.json")).unwrap(), record);

            assert!(!running.source.calls().iter().any(|c| c["args"][1]
                .as_str()
                .is_some_and(|s| s.starts_with("import-"))));
            assert_eq!((helpers.effects(), helpers.reviews()), (1, 2));
        }
    });
}

#[test]
fn a_manual_refresh_discovered_after_confirmation_refuses_before_rearm_or_mode_effect() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let helpers = Fixture::new();
        let original = interrupted(&helpers).await;
        acknowledge_model_boot(&helpers, &original).await;
        original
            .close()
            .wait_until(std::time::Instant::now() + Duration::from_secs(1));
        helpers.config("default", "passive", false);
        let running = start_native(&helpers, json!({"ready":true}));
        crate::desktop_service::tests::eventually("reviewed read-only consumer", || {
            running.service.snapshot().connected
        })
        .await;
        let prepared = confirm_when_idle(
            &running.service.shared.mode_transition,
            ReviewAction::Background,
        )
        .await;
        fs::write(
            running.source.0.join("fixture.json"),
            json!({"manual_upgrade":true}).to_string(),
        )
        .unwrap();
        assert_eq!(
            running
                .service
                .apply_mode_action(Some(prepared))
                .await
                .unwrap_err(),
            "manual_refresh_required"
        );
        assert!(running.service.shared.read_checks_only());
        assert!(!running.service.shared.business_permitted());
        running.service.join().await.unwrap();
        assert!(mutations(&running).is_empty());
        assert_eq!((helpers.effects(), helpers.reviews()), (1, 2));
    });
}

#[test]
fn actual_startup_then_automatic_reconnect_never_repeats_grant_or_helper_activation() {
    if crate::test_context::desktop_fixture() {
        return;
    }
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
    if crate::test_context::desktop_fixture() {
        return;
    }
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
    if crate::test_context::desktop_fixture() {
        return;
    }
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

#[test]
fn actual_modern_service_first_choice_keeps_its_confirmed_scheduling_continuation() {
    if crate::test_context::desktop_fixture() {
        return;
    }
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        for (outcome, previously_deferred) in [
            ("settle", false),
            ("settle", true),
            ("refused", false),
            ("lost", true),
        ] {
            let helpers = Fixture::new();
            let registration = super::first_tests::Fixture::new(outcome, true);
            let running = start_native(
                &helpers,
                json!({"ready":true,"state":"service",
                "existing_grant":true,"observation_abi":true,"hold_mode_preflight":true}),
            );
            let mode = &running.service.shared.mode_transition;
            mode.fixture_windows(Some(registration.cli.clone()));
            fs::write(running.source.0.join("mode-preflight-release"), []).unwrap();
            let watches = || {
                running
                    .source
                    .calls()
                    .iter()
                    .filter(|c| c["args"] == json!(["desktop", "watch", "--json"]))
                    .count()
            };
            crate::desktop_service::tests::eventually(
                "modern first-choice Service is ready",
                || {
                    watches() == 1
                        && running.service.shared.ownership.granted()
                        && mode.view().first.is_some()
                },
            )
            .await;
            assert!(mode.state.lock().unwrap().classification_required);
            assert!(mode.startup_permitted());
            if previously_deferred {
                // A genuine other-owner busy reconnect defers the connection.
                // Only the later native-confirmed choice may reopen it.
                let held = mode.gate.admit(Role::Fleet).unwrap();
                running.service.retry();
                crate::desktop_service::tests::eventually("busy reconnect is connected", || {
                    watches() == 2
                })
                .await;
                assert!(mode.classification_pending());
                assert!(!mode.startup_permitted());
                drop(held);
                crate::desktop_service::tests::eventually(
                    "passive offer recovers read-only",
                    || !mode.classification_pending() && mode.view().first.is_some(),
                )
                .await;
                assert!(!mode.startup_permitted());
            }
            let before = mutations(&running);
            let prepared = mode
                .confirm_first(mode.view().first.unwrap(), |_| true)
                .await
                .unwrap()
                .unwrap();
            assert!(
                mode.gate.admit(Role::Fleet).is_err(),
                "the SAME Prepared holds the charge"
            );
            let result = running
                .service
                .apply_first_choice(Some(prepared))
                .await
                .unwrap();
            let id = result["request_id"].as_str().unwrap();
            let settled = mode.first_settled(id);
            let view = mode.view();
            let startup = mode.startup_permitted();
            let scheduling = running.service.shared.business_scheduling_permitted();
            let after = mutations(&running);
            let installs = registration.installs();
            running.service.join().await.unwrap();
            if outcome == "settle" {
                assert!(settled && view.active && !view.pending);
                assert!(startup, "a confirmed first choice must not require a second Resume; previously deferred={previously_deferred}");
                assert!(!view.needs_resume, "the settled first choice is not paused");
                assert!(scheduling);
            } else if outcome == "refused" {
                assert!(!settled && !view.pending && !view.active);
                assert!(startup, "a native pre-manager refusal must not invent a deferral from Prepared's own charge");
                assert_eq!(view.code, "activation_not_started");
            } else {
                assert!(!settled && view.pending);
                assert!(
                    !startup && !scheduling,
                    "confirmation alone cannot clear an uncertain first choice"
                );
            }
            assert_eq!(
                before, after,
                "no extra Rearm/Grant to clear a scheduling deferral"
            );
            assert_eq!(
                installs,
                [format!(
                    "desktop registration install --mode background --json --request-id {id}"
                )]
            );
        }
    });
}
