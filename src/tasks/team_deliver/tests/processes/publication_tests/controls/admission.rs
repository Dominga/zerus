//! Private real delivery children retain the product task/Teams writers.
//! The native input port records submissions instead of contacting an agent.
use super::*;
use std::time::{Duration, Instant};

fn prepare(
    f: &mut Fixture,
    requests: &[crate::teams::store::publication::PlannedTaskRequest],
) -> (Input, Vec<String>) {
    for r in requests {
        crate::teams::tasks::observe(&f.config.state, &r.task).unwrap();
    }
    let home = f.lead.machine.clone();
    f.bind("m2", &home);
    let task = &requests[0];
    let body = Input {
        schema: 1,
        id: f.h.intent(task.node.id, "m2"),
        team: TEAM.into(),
        node: task.node.id,
        member: "m2".into(),
    };
    (
        body,
        vec![
            task.task.uuid.clone(),
            "--project".into(),
            task.task.project.clone(),
            "--json".into(),
        ],
    )
}
fn wait(path: &std::path::Path) {
    let until = Instant::now() + Duration::from_secs(6);
    while !path.exists() {
        assert!(
            Instant::now() < until,
            "private child did not reach {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_phase_pause_winning_after_spawn_prevents_the_real_child_reservation() {
    let (mut f, _, requests) = published();
    let (body, args) = prepare(&mut f, &requests);
    let (child, reply) = f.spawn(
        "submitted",
        &args,
        &serde_json::to_string(&body).unwrap(),
        true,
    );
    wait(&f.config.state.join(format!("ready-{}", child.id())));
    let current = scope(&f, 1);
    f.h.run(
        &person(),
        Operation::PausePhase {
            scope: current,
            pause: None,
            reason: "Pause before the original child reserves.".into(),
        },
    )
    .unwrap();
    std::fs::write(f.config.state.join("release"), b"go").unwrap();
    let result = f.finish(child, reply);
    assert_ne!(result["reply"]["state"], "submitted", "{result}");
    assert_eq!(f.count(), 0, "a winning pause must exclude input: {result}");
    let doc = crate::tasks::store(&f.config, &requests[0].task.project)
        .unwrap()
        .load(&requests[0].task.uuid)
        .unwrap();
    assert!(doc.attempt_phases().unwrap().is_empty());
}

#[test]
fn final_reservation_custody_wins_before_a_later_pause_and_existing_work_can_report() {
    let (mut f, _, requests) = published();
    let (body, args) = prepare(&mut f, &requests);
    let (child, reply) = f.spawn_with(
        "submitted",
        &args,
        &serde_json::to_string(&body).unwrap(),
        false,
        &[("HGS_TEST_TEAM_LOCK_AT", "deliver")],
    );
    wait(&f.config.state.join("deliver-ready"));
    let request = ops::tests::request(
        f.h.store.team(TEAM).unwrap().unwrap().revision,
        Operation::PausePhase {
            scope: scope(&f, 1),
            pause: None,
            reason: "Retain existing work while paused.".into(),
        },
    );
    let mut writer = crate::teams::store::Store::open(&f.config.state).unwrap();
    std::thread::scope(|s| {
        let (started, ready) = std::sync::mpsc::channel();
        let request = &request;
        let join = s.spawn(move || {
            started.send(()).unwrap();
            ops::tests::apply_fixture(&mut writer, &person(), request, now())
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        std::fs::write(f.config.state.join("deliver-release"), b"go").unwrap();
        let result = f.finish(child, reply);
        assert_eq!(result["reply"]["state"], "submitted", "{result}");
        join.join().unwrap().unwrap();
    });
    assert_eq!(f.count(), 1);
    assert!(
        facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1)
            .unwrap()
            .paused
    );
    crate::teams::tasks::observe(&f.config.state, &requests[0].task).unwrap();
    assert!(
        facts::phase(f.h.store.db.as_ref().unwrap(), TEAM, 1)
            .unwrap()
            .paused
    );
    assert_eq!(
        f.h.store.node(TEAM, body.node).unwrap().unwrap().state,
        zerus_team_plan::NodeState::InProgress
    );
}
