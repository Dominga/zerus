//! Linux transport/dialog models. No Windows Caller, task or PC qualification.
use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
const CONFIG: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const EPOCH: &str = "11111111-1111-4111-8111-111111111111";
const SCRIPT: &str = r#"#!/usr/bin/python3
import json, pathlib, sys
root = pathlib.Path(__file__).resolve().parent
p = root/'state.json'
s = json.loads(p.read_text())
a = sys.argv[1:]
with (root/'calls').open('a') as log: log.write(json.dumps(a)+'\n')
def save(): p.write_text(json.dumps(s))
if a == ['--help']:
    if not s.get('old'): print('Desktop registration refresh ABI: desktop-registration-refresh-v1')
elif a == ['desktop','registration','status','--json']:
    print(json.dumps(dict(schema=1, configuration=s['target']['configuration'],pending_transition=s.get('pending'),first_choice_lost=s.get('lost'))))
elif a == ['desktop','registration','refresh-info','--json']:
    if s.get('denied'): sys.exit(1)
    s['reads'] = s.get('reads',0)+1
    print(json.dumps(dict(schema=1,refresh_abi='desktop-registration-refresh-v1',read_only=True,
       configuration=s['target']['configuration'],target=s['target'],drained=s.get('drained',True))))
    if s.get('drift_after') == s['reads']: s['target']['selection'] = 'b'*64
    save()
elif a[:5] == ['desktop','registration','refresh','--json','--request-id']:
    request = a[5]
    target = json.loads(a[7])
    assert a[6]=='--scope' and target == s['target']
    scope = dict(target)
    original = scope.pop('original'); scope.pop('configuration'); scope['kind']='original_instance_refresh'
    s['pending']=dict(request_id=request,original=original,refresh=scope)
    s['request']=request
    if s.get('unknown'):
        save(); sys.exit(1)
    s['receipt']=dict(request=request,original=original,scope=scope,phase='settled',
      ready=dict(instance=dict(guid='new',pid=9,birth=101,epoch='22222222-2222-4222-8222-222222222222'),generation=8))
    s['pending']=None
    save()
    print(json.dumps(dict(ok=True)))
elif a[:5] == ['desktop','registration','refresh-result','--json','--request-id']:
    receipt=s.get('receipt')
    if s.get('forged') and receipt: receipt['scope']['owner_epoch']='different'
    print(json.dumps(dict(schema=1,refresh_abi='desktop-registration-refresh-v1',read_only=True,
      configuration=s['target']['configuration'], request_id=a[5], receipt=receipt,pending=s.get('pending'),
      outcome='settled' if receipt else 'unknown',lost_original=s.get('lost'))))
else: sys.exit(2)
"#;
struct Fixture {
    root: PathBuf,
    cli: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("zerus-refresh-ui-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let cli = root.join("hgs");
        fs::write(&cli, SCRIPT).unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        let own = Self { root, cli };
        own.write(json!({"target":{"configuration":CONFIG,"selection":"a".repeat(64),"mode":"background",
          "original":{"guid":"original","pid":8,"birth":18446744073709551614u64,"epoch":EPOCH},"original_generation":7,
          "journal":"33333333-3333-4333-8333-333333333333","owner_epoch":"44444444-4444-4444-8444-444444444444",
          "grant_binding":"d".repeat(64),"original_image":"e".repeat(48),"replacement_image":"f".repeat(48)}}));
        own
    }
    fn write(&self, value: Value) {
        fs::write(
            self.root.join("state.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
    }
    fn read(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join("state.json")).unwrap()).unwrap()
    }
    fn patch(&self, key: &str, value: Value) {
        let mut s = self.read();
        s[key] = value;
        self.write(s);
    }
    fn controller(&self) -> Arc<Controller> {
        let c = Controller::new();
        c.fixture_windows(Some(self.cli.clone()));
        c
    }
    fn observe(&self, c: &Arc<Controller>) -> View {
        tauri::async_runtime::block_on(c.observe_refresh(
            self.cli.clone(),
            CONFIG.into(),
            EPOCH.into(),
        ))
    }
    fn prepared(&self, c: &Arc<Controller>) -> Prepared {
        let offer = self.observe(c).offer.unwrap();
        tauri::async_runtime::block_on(c.confirm_refresh(offer, Arc::new(|| Ok(())), |_| true))
            .unwrap()
            .unwrap()
    }
    fn effects(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.root.join("calls"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let a: Vec<String> = serde_json::from_str(l).unwrap();
                (a.get(2).is_some_and(|op| op == "refresh")).then_some(a)
            })
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn observation_and_cancel_allocate_no_original_and_invoke_no_effect() {
    let f = Fixture::new();
    let c = f.controller();
    let view = f.observe(&c);
    assert!(view.offer.is_some());
    assert!(view.request_id.is_none());
    let result = tauri::async_runtime::block_on(c.confirm_refresh(
        view.offer.unwrap(),
        Arc::new(|| Ok(())),
        |_| false,
    ))
    .unwrap();
    assert!(result.is_none());
    assert!(c.refresh_view().request_id.is_none());
    assert!(f.effects().is_empty());
    assert!(
        c.startup_permitted(),
        "refresh reads cannot change scheduling eligibility"
    );
}
#[test]
fn missing_capability_revoked_grant_and_live_loans_offer_no_refresh() {
    for (key, value) in [
        ("old", json!(true)),
        ("denied", json!(true)),
        ("drained", json!(false)),
    ] {
        let f = Fixture::new();
        f.patch(key, value);
        let c = f.controller();
        assert!(f.observe(&c).offer.is_none());
        assert!(f.effects().is_empty());
        assert!(c.refresh_view().request_id.is_none());
    }
}
#[test]
fn dialog_and_post_dialog_scope_drift_refuse_before_an_effect() {
    for after_dialog in [false, true] {
        let f = Fixture::new();
        let c = f.controller();
        let offer = f.observe(&c).offer.unwrap();
        let file = f.root.join("state.json");
        let change = move || {
            let mut s: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
            s["target"]["selection"] = "b".repeat(64).into();
            fs::write(&file, serde_json::to_vec(&s).unwrap()).unwrap();
        };
        if !after_dialog {
            let result = tauri::async_runtime::block_on(c.confirm_refresh(
                offer,
                Arc::new(|| Ok(())),
                move |_| {
                    change();
                    true
                },
            ));
            assert!(matches!(result, Err(CHANGED)));
        } else {
            let prepared = tauri::async_runtime::block_on(c.confirm_refresh(
                offer,
                Arc::new(|| Ok(())),
                |_| true,
            ))
            .unwrap()
            .unwrap();
            change();
            let result = tauri::async_runtime::block_on(c.apply_refresh(prepared));
            assert!(matches!(result, Err(CHANGED)));
        }
        assert!(f.effects().is_empty());
        assert!(c.refresh_view().request_id.is_none());
    }
}
#[test]
fn spawn_veto_rechecks_after_slow_read_and_keeps_one_unknown_id() {
    let f = Fixture::new();
    let c = f.controller();
    let prepared = f.prepared(&c);
    f.patch("drift_after", json!(4));
    let view = tauri::async_runtime::block_on(c.apply_refresh(prepared)).unwrap();
    assert!(f.effects().is_empty());
    let id = view.request_id.unwrap();
    assert!(!view.settled);
    assert!(view.offer.is_none());
    assert_eq!(f.observe(&c).request_id.as_deref(), Some(id.as_str()));
    assert!(f.effects().is_empty());
}
#[test]
fn settled_refresh_reads_exact_original_receipt_and_never_opens_business() {
    let f = Fixture::new();
    let c = f.controller();
    let prepared = f.prepared(&c);
    c.state.lock().unwrap().observation_only = true;
    let view = tauri::async_runtime::block_on(c.apply_refresh(prepared)).unwrap();
    assert!(view.settled);
    assert!(
        !view.running,
        "the returned view must allow read-only result checks"
    );
    let id = view.request_id.unwrap();
    assert_eq!(f.effects().len(), 1);
    assert!(
        !c.startup_permitted(),
        "a refresh receipt is not a scheduling continuation"
    );
    for _ in 0..3 {
        let v = f.observe(&c);
        assert!(v.settled);
        assert_eq!(v.request_id.as_deref(), Some(id.as_str()));
    }
    assert_eq!(f.effects().len(), 1);
    assert_eq!(
        f.read()["receipt"]["original"]["birth"].as_u64(),
        Some(18446744073709551614)
    );
    f.patch("forged", json!(true));
    assert!(!f.observe(&c).settled);
}
#[test]
fn lost_reply_and_gui_restart_only_read_the_same_native_original() {
    let f = Fixture::new();
    f.patch("unknown", json!(true));
    let c = f.controller();
    let prepared = f.prepared(&c);
    let v = tauri::async_runtime::block_on(c.apply_refresh(prepared)).unwrap();
    let id = v.request_id.unwrap();
    assert!(!v.settled);
    assert!(!v.running);
    assert!(v.offer.is_none());
    drop(c);
    let restarted = f.controller();
    for _ in 0..3 {
        let v = f.observe(&restarted);
        assert!(!v.settled);
        assert!(v.offer.is_none());
        assert_eq!(v.request_id.as_deref(), Some(id.as_str()));
    }
    assert_eq!(f.effects().len(), 1);
}
#[test]
fn native_scope_fence_is_rechecked_after_confirmation() {
    let f = Fixture::new();
    let c = f.controller();
    let offer = f.observe(&c).offer.unwrap();
    let allowed = Arc::new(AtomicBool::new(true));
    let a = allowed.clone();
    let prepared = tauri::async_runtime::block_on(c.confirm_refresh(
        offer,
        Arc::new(move || {
            if a.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(CHANGED)
            }
        }),
        |_| true,
    ))
    .unwrap()
    .unwrap();
    allowed.store(false, Ordering::Release);
    assert!(matches!(
        tauri::async_runtime::block_on(c.apply_refresh(prepared)),
        Err(CHANGED)
    ));
    assert!(f.effects().is_empty());
    assert!(c.refresh_view().request_id.is_none());
}

#[test]
fn actual_service_refresh_keeps_immutable_manual_binding_and_all_positive_paths_closed() {
    let _process = crate::PROCESS_TREE.read().unwrap();
    tauri::async_runtime::block_on(async {
        let source = crate::desktop_service::tests::Fixture::new(json!({"manual_upgrade":true}));
        let (service, _) = source.start(true);
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while service.shared.read_only_original.get().is_none() {
            assert!(std::time::Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let original = service.refresh_binding().unwrap();
        let fake = Fixture::new();
        let mut s = fake.read();
        s["target"]["configuration"] = original.configuration.clone().into();
        s["target"]["original"]["epoch"] = original.epoch.clone().into();
        fake.write(s);
        service
            .shared
            .mode_transition
            .fixture_windows(Some(fake.cli.clone()));
        let offer = service.refresh_status().await.unwrap().offer.unwrap();
        let fence = service.refresh_fence(&offer).unwrap();
        let prepared = service
            .shared
            .mode_transition
            .confirm_refresh(offer, fence, |_| true)
            .await
            .unwrap()
            .unwrap();
        let result = service
            .shared
            .mode_transition
            .apply_refresh(prepared)
            .await
            .unwrap();
        assert!(result.settled);
        assert!(service.shared.read_checks_only());
        assert!(!service.shared.business_permitted());
        assert!(!service.shared.business_scheduling_permitted());
        assert_eq!(
            service.shared.original_read_scope().unwrap().unwrap().epoch,
            original.epoch
        );
        // A healthy replacement cannot be accepted by this old window, even
        // after an explicit read-only Check. Reopening is a separate startup.
        fs::write(
            source.0.join("runtime-epoch"),
            "00000000-0000-4000-8000-000000000003",
        )
        .unwrap();
        fs::write(source.0.join("fixture.json"), json!({}).to_string()).unwrap();
        service.check_status(&original.epoch).unwrap();
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while service.snapshot().code != "original_service_changed" {
            assert!(std::time::Instant::now() < until);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            service.snapshot().epoch.as_deref(),
            Some(original.epoch.as_str())
        );
        assert!(service.shared.read_checks_only());
        service.join().await.unwrap();
        assert_eq!(fake.effects().len(), 1);
        assert!(source.calls().iter().all(|c| c["args"] == json!(["--help"])
            || c["args"] == json!(["desktop", "status", "--json"])));
    });
}

#[test]
fn an_unavailable_pre_admission_read_can_recover_a_fresh_offer_without_an_effect() {
    let f = Fixture::new();
    let c = f.controller();
    let before = f.observe(&c).offer.unwrap();
    f.patch("denied", json!(true));
    assert!(f.observe(&c).offer.is_none());
    f.patch("denied", json!(false));
    let after = f.observe(&c).offer.unwrap();
    assert_ne!(before.selection, after.selection);
    assert!(c.refresh_view().request_id.is_none());
    assert!(f.effects().is_empty());
}
