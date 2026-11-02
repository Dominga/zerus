use super::super::{
    ledger,
    wire::{Action, NativeId, Outcome, Platform},
    Identity, Kind, Target,
};
use super::*;
use std::sync::Mutex;

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
#[derive(Default)]
struct State {
    restore: bool,
    finish: bool,
    ready: bool,
    joined: bool,
    stopping: bool,
    pages: Vec<Vec<Activation>>,
    finishes: usize,
}
struct Fake {
    endpoint: Endpoint,
    state: Arc<Mutex<State>>,
}
impl Host for Fake {
    fn restore(&mut self, page: &[Activation]) -> Result<bool> {
        let mut state = self.state.lock().unwrap();
        if !state.restore {
            return Ok(false);
        }
        if !page.is_empty() {
            state.pages.push(page.to_vec());
        }
        Ok(true)
    }
    fn finish(&mut self) -> Result<bool> {
        let mut state = self.state.lock().unwrap();
        state.finishes += 1;
        Ok(state.finish)
    }
    fn ready(&self) -> Result<bool> {
        Ok(self.state.lock().unwrap().ready)
    }
    fn stop(&self) {
        self.state.lock().unwrap().stopping = true;
        while let Ok(work) = self.endpoint.try_next() {
            drop(work);
        }
    }
    fn drained(&mut self) -> bool {
        self.state.lock().unwrap().joined
    }
    fn status(&self) -> Value {
        json!({"fake":true})
    }
}
struct Fixture {
    _temp: tempfile::TempDir,
    store: Store,
    session: Session,
}

#[test]
fn temporary_host_readiness_loss_pauses_dispatch_without_replacing_the_policy() {
    let mut f = Fixture::new();
    let state = Arc::new(Mutex::new(State {
        restore: true,
        finish: true,
        ready: true,
        ..State::default()
    }));
    let mut runtime = f.attach(state.clone(), Arc::new(()));
    let mut attention = attention::Runtime::new(&f.session.service_epoch);
    for _ in 0..4 {
        runtime.poll(&mut f.store, &mut attention);
    }
    assert!(runtime.admitting());
    assert!(runtime.ready());
    state.lock().unwrap().ready = false;
    assert!(!runtime.ready());
    assert!(
        runtime.admitting(),
        "a transient status observation is not a policy mutation"
    );
    state.lock().unwrap().joined = true;
    runtime.request_stop();
    runtime.poll(&mut f.store, &mut attention);
    assert!(!runtime.admitting());
    assert!(runtime.drained());
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: temp.path().canonicalize().unwrap().join("state/desktop"),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let epoch = id();
        let writer = id();
        let mut store = Store::open(&location, &epoch).unwrap();
        ledger::authorize(&mut store.db, &writer, &epoch).unwrap();
        Self {
            _temp: temp,
            store,
            session: Session {
                configuration: location.configuration,
                service_epoch: epoch,
                writer_epoch: writer,
                adapter: id(),
                host_epoch: id(),
                lease: id(),
                platform: Platform::Macos,
            },
        }
    }
    fn seed(&mut self) -> String {
        let edge = id();
        let identity = Identity {
            kind: Kind::Post,
            target: Target::Session {
                computer: "peer.example".into(),
                run: "run".into(),
                conversation: "conversation".into(),
            },
            edge: edge.clone(),
        };
        let token = zerus_client_core::attention::encode_token(json!({"host":"peer.example","run":"run","conversation":"conversation","name":"fixture","notice":edge}).as_object().unwrap());
        let action = Action::Post {
            token: token.clone(),
            title: "Fixture".into(),
            body: "Body".into(),
        };
        let intent = ledger::put_intent(
            &mut self.store.db,
            &identity,
            1,
            &serde_json::to_value(&action).unwrap(),
        )
        .unwrap();
        let (mut outbox, endpoint) = Outbox::new(self.session.clone()).unwrap();
        outbox
            .offer_intent(&mut self.store, &intent, 1, Instant::now())
            .unwrap();
        endpoint
            .try_next()
            .unwrap()
            .begin()
            .unwrap()
            .finish(Outcome::Delivered {
                native_id: NativeId::Macos {
                    request_id: zerus_client_core::attention::request_id(&token),
                },
            });
        outbox.poll(&mut self.store, Instant::now());
        self.store.db.execute("INSERT INTO desktop_attention_edges(logical,session,intent,identity,payload,blocked,bytes) VALUES(?1,'fixture',?2,?3,?4,0,0)",rusqlite::params![edge,intent,serde_json::to_string(&identity).unwrap(),serde_json::to_string(&action).unwrap()]).unwrap();
        intent
    }
    fn attach(&self, state: Arc<Mutex<State>>, owner: Arc<dyn Send + Sync>) -> Runtime<Fake> {
        Runtime::with_factory(&self.store, self.session.clone(), owner, |_, endpoint| {
            Ok(Fake { endpoint, state })
        })
        .unwrap()
    }
}

#[test]
fn restore_backpressure_and_finish_ack_precede_retirement_admission() {
    let mut f = Fixture::new();
    let first = f.seed();
    for _ in 1..18 {
        f.seed();
    }
    let state = Arc::new(Mutex::new(State {
        ready: true,
        ..State::default()
    }));
    let mut runtime = f.attach(state.clone(), Arc::new(()));
    let mut attention = attention::Runtime::new(&f.session.service_epoch);
    attention.request_retirement();
    runtime.poll(&mut f.store, &mut attention);
    assert!(ledger::current(&f.store.db, &first, 1).unwrap());
    assert_eq!(runtime.status()["restored"], false);
    state.lock().unwrap().restore = true;
    runtime.poll(&mut f.store, &mut attention);
    runtime.poll(&mut f.store, &mut attention);
    for _ in 0..3 {
        runtime.poll(&mut f.store, &mut attention);
    }
    assert!(ledger::current(&f.store.db, &first, 1).unwrap());
    assert_eq!(
        state
            .lock()
            .unwrap()
            .pages
            .iter()
            .map(Vec::len)
            .collect::<Vec<_>>(),
        vec![16, 2]
    );
    state.lock().unwrap().finish = true;
    runtime.poll(&mut f.store, &mut attention);
    assert_eq!(runtime.status()["restored"], true);
    runtime.poll(&mut f.store, &mut attention);
    assert!(!ledger::current(&f.store.db, &first, 1).unwrap());
    runtime.request_stop();
    state.lock().unwrap().joined = true;
    for _ in 0..2 {
        runtime.poll(&mut f.store, &mut attention);
    }
    assert!(runtime.drained());
}

#[test]
fn actual_operation_holds_and_host_join_independently_pin_owner() {
    let mut f = Fixture::new();
    f.seed();
    let state = Arc::new(Mutex::new(State {
        restore: true,
        finish: true,
        ready: true,
        ..State::default()
    }));
    let owner = Arc::new(());
    let weak = Arc::downgrade(&owner);
    let mut runtime = f.attach(state.clone(), owner.clone());
    drop(owner);
    let mut attention = attention::Runtime::new(&f.session.service_epoch);
    attention.request_retirement();
    for _ in 0..4 {
        runtime.poll(&mut f.store, &mut attention);
    }
    let dispatch = runtime.host.endpoint.try_next().unwrap().begin().unwrap();
    let native_id = match &dispatch.command().action {
        Action::Withdraw { native_id, .. } => native_id.clone(),
        _ => panic!("exact withdrawal"),
    };
    let hold = dispatch.resources();
    dispatch.finish(Outcome::Delivered { native_id });
    runtime.request_stop();
    runtime.poll(&mut f.store, &mut attention);
    assert!(!runtime.drained());
    state.lock().unwrap().joined = true;
    runtime.poll(&mut f.store, &mut attention);
    assert!(
        !runtime.drained(),
        "physical host join does not release command callbacks"
    );
    assert!(weak.upgrade().is_some());
    drop(hold);
    runtime.poll(&mut f.store, &mut attention);
    assert!(runtime.drained());
    drop(runtime);
    assert!(weak.upgrade().is_none());
}

#[test]
fn abandoned_undrained_runtime_preserves_its_original_owner_hold() {
    let f = Fixture::new();
    let owner = Arc::new(());
    let weak = Arc::downgrade(&owner);
    let state = Arc::new(Mutex::new(State::default()));
    let runtime = f.attach(state.clone(), owner.clone());
    drop(owner);
    drop(runtime);
    assert!(state.lock().unwrap().stopping);
    assert!(
        weak.upgrade().is_some(),
        "no drain was observed, so owner remains retained"
    );
}

#[test]
fn restart_restores_retired_mapping_for_cleanup_without_click_or_post_authority() {
    let mut f = Fixture::new();
    let intent = f.seed();
    ledger::withdraw(&mut f.store.db, &intent, 1).unwrap();
    f.store
        .db
        .execute(
            "UPDATE desktop_attention_edges SET live=0,blocked=1,dirty=1 WHERE intent=?1",
            [&intent],
        )
        .unwrap();
    let (mut outbox, endpoint) = Outbox::new(f.session.clone()).unwrap();
    let before = f.store.db.total_changes();
    let head = outbox.restore_cleanup_head(&f.store).unwrap();
    let page = outbox.restore_cleanup_page(&f.store, 0, head).unwrap();
    assert!(page.complete);
    assert_eq!(page.mappings.len(), 1);
    assert_eq!(f.store.db.total_changes(), before, "restoration only reads");
    let activation = page.mappings[0].clone();
    assert!(activation.platform_context.is_none());
    assert!(
        outbox
            .current_activation(&f.store, &activation)
            .unwrap()
            .is_none(),
        "retired clicks stay refused"
    );
    assert!(endpoint.emit(super::super::Event::Clicked(activation.clone())));
    assert!(outbox.poll(&mut f.store, Instant::now()).is_empty());
    drop(endpoint);
    drop(outbox);

    let state = Arc::new(Mutex::new(State {
        restore: true,
        finish: true,
        ready: true,
        ..State::default()
    }));
    let mut runtime = f.attach(state.clone(), Arc::new(()));
    let mut attention = attention::Runtime::new(&f.session.service_epoch);
    attention.request_retirement();
    for _ in 0..5 {
        runtime.poll(&mut f.store, &mut attention);
    }
    assert_eq!(
        state.lock().unwrap().pages[0][0].request_id,
        activation.request_id
    );
    let dispatch = runtime.host.endpoint.try_next().unwrap().begin().unwrap();
    assert!(
        matches!(&dispatch.command().action,Action::Withdraw{token,native_id}
        if *token==activation.token && *native_id==activation.native_id)
    );
    // Even an unknown previous withdrawal consumes its original native attempt.
    dispatch.finish(Outcome::Unknown {
        code: super::super::wire::Uncertainty::HostLost,
    });
    runtime.poll(&mut f.store, &mut attention);
    let remaining = runtime
        .outbox
        .restore_cleanup_page(&f.store, 0, head)
        .unwrap();
    assert!(
        remaining.mappings.is_empty(),
        "no restoration for replay of a consumed withdrawal"
    );
    runtime.request_stop();
    state.lock().unwrap().joined = true;
    runtime.poll(&mut f.store, &mut attention);
    assert!(runtime.drained());
}

#[test]
fn cleanup_restore_pages_candidates_before_filtering_and_refuses_replaced_writer() {
    let mut f = Fixture::new();
    for _ in 0..18 {
        f.seed();
    }
    let (outbox, _endpoint) = Outbox::new(f.session.clone()).unwrap();
    let head = outbox.restore_cleanup_head(&f.store).unwrap();
    // All are live, hence no cleanup mappings, but candidates remain paged.
    let first = outbox.restore_cleanup_page(&f.store, 0, head).unwrap();
    assert!(first.mappings.is_empty());
    assert!(!first.complete);
    assert_eq!(first.after, 16);
    assert!(
        outbox
            .restore_cleanup_page(&f.store, first.after, head)
            .unwrap()
            .complete
    );
    ledger::authorize(&mut f.store.db, &id(), &f.session.service_epoch).unwrap();
    assert!(outbox.restore_cleanup_head(&f.store).is_err());
    assert!(outbox.restore_cleanup_page(&f.store, 0, head).is_err());
}
