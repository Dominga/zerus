//! Canonical rows come from the operation and swarm producers. Validated
//! fixtures model native originals; these tests do not attest a GUI gesture.
use super::*;
use crate::teams::ops::tests::{Harness, TEAM};
use zerus_client_core::teams::action::TeamAction;

fn sample() -> (Harness, Runtime, Arc<wire::Plan>, Arc<platform::FileLock>) {
    let h = Harness::new();
    let config = Config {
        home: h.temp.path().display().to_string(),
        dir: h.temp.path().join("config"),
        state: h.temp.path().to_owned(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    platform::private_dir(&config.dir).unwrap();
    crate::swarm::fixture_project(
        &config,
        "22222222-2222-4222-8222-222222222222",
        "example-project",
    );
    let scope = Scope {
        configuration: "a".repeat(64),
        service_epoch: uuid::Uuid::new_v4().to_string(),
    };
    let owner = Arc::new(platform::FileLock::exclusive(tempfile::tempfile().unwrap()).unwrap());
    let runtime = Runtime::new(
        &config,
        scope,
        owner.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    let plan = Arc::new(wire::Plan {
        request_id: uuid::Uuid::new_v4().to_string(),
        team: TEAM.into(),
        expected_revision: h.store.team(TEAM).unwrap().unwrap().revision,
        action: wire::Action::Team {
            change: Box::new(TeamAction::RenameTeam {
                name: "Reviewed native worker name".into(),
            }),
        },
    });
    (h, runtime, plan, owner)
}
fn take(queue: &Queue) -> Value {
    serde_json::from_slice(&queue.next().expect("worker response")).unwrap()
}
fn stage(runtime: &mut Runtime, plan: &wire::Plan) {
    let body = serde_json::to_vec(plan).unwrap();
    let piece = wire::Piece::from_bytes(
        uuid::Uuid::new_v4().to_string(),
        runtime.scope.clone(),
        plan.reference().unwrap(),
        0,
        &body,
    )
    .unwrap();
    let result = runtime.stage(Validated::fixture(
        &json!({"op":"team_person_plan_piece","piece":piece}),
    ));
    assert_eq!(result["ok"], true);
    assert_eq!(result["complete"], true);
}

#[test]
fn native_v2_worker_prepares_executes_and_only_reads_original_results() {
    let (h, mut runtime, plan, _owner) = sample();
    stage(&mut runtime, &plan);
    let queue = Arc::new(Queue::default());
    runtime.query(&json!({"op":"team_person_action_prepare","scope":runtime.scope,"plan":plan.reference()}),queue.clone(),1).unwrap();
    let value = take(&queue);
    let review: wire::Review = serde_json::from_value(value["value"]["review"].clone()).unwrap();
    assert_eq!(review.team_name, h.store.team(TEAM).unwrap().unwrap().name);
    assert_eq!(review.intent.plan, plan.reference().unwrap());
    let queue = Arc::new(Queue::default());
    runtime
        .action(
            Validated::fixture(&json!({"op":"team_person_action","intent":review.intent})),
            queue.clone(),
            2,
        )
        .unwrap();
    let original = take(&queue)["value"].clone();
    assert_eq!(original["reply"]["ok"], true, "{original}");
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().name,
        "Reviewed native worker name"
    );
    // Read-only result lookup needs no staged plan and may run in a new epoch.
    runtime.plans = Cache::new(runtime.scope.clone());
    runtime.scope.service_epoch = uuid::Uuid::new_v4().to_string();
    let queue = Arc::new(Queue::default());
    runtime
        .query(
            &json!({"op":"team_person_action_result","intent":review.intent}),
            queue.clone(),
            3,
        )
        .unwrap();
    assert_eq!(take(&queue)["value"], original);
    let mut foreign = review.intent;
    foreign.request = "e".repeat(64);
    let queue = Arc::new(Queue::default());
    runtime
        .query(
            &json!({"op":"team_person_action_result","intent":foreign}),
            queue.clone(),
            4,
        )
        .unwrap();
    assert_eq!(
        take(&queue)["value"]["code"],
        "team_person_action_evidence_unavailable"
    );
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().revision,
        plan.expected_revision + 1
    );
}

#[test]
fn native_v2_workers_are_bounded_and_one_wait_does_not_block_the_actor() {
    let (_h, mut runtime, _plan, owner) = sample();
    assert_eq!(
        Arc::strong_count(&owner),
        1,
        "an idle bridge must not prevent service retirement"
    );
    let mut releases = Vec::new();
    for index in 0..WORKERS {
        let (send, receive) = std::sync::mpsc::channel();
        releases.push(send);
        runtime
            .start(Arc::new(Queue::default()), index as u64, move || {
                receive.recv().unwrap();
                json!({"fixture":index})
            })
            .unwrap();
    }
    assert_eq!(runtime.workers.len(), WORKERS);
    assert_eq!(
        Arc::strong_count(&owner),
        WORKERS + 1,
        "each physical worker retains the service owner"
    );
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    assert_eq!(
        runtime
            .start(Arc::new(Queue::default()), 100, move || {
                observed.fetch_add(1, Ordering::SeqCst);
                Value::Null
            })
            .unwrap_err()
            .code,
        code::STORE_BUSY
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // No scheduler-time assertion: all original workers are still waiting
    // while the actor has already returned the bounded refusal above.
    for release in releases {
        release.send(()).unwrap();
    }
    drop(runtime);
    assert_eq!(Arc::strong_count(&owner), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
