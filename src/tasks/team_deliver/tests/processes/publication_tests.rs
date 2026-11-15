//! Every row and document here comes from the product's writers. Only the
//! missing-file test deliberately removes its own private task files.
use super::*;
use ops::Operation;
mod completion;
mod recovery;

fn fixture() -> (
    Fixture,
    String,
    Vec<crate::teams::store::publication::PlannedTaskRequest>,
) {
    let mut f = Fixture::new();
    let first = f.h.store.next_draft_node(TEAM).unwrap();
    let draft = ops::publication::tests::input(&[first, first + 1]);
    f.h.run(&person(), Operation::SubmitDraftPhase { phase: 1, draft })
        .unwrap();
    f.h.run(
        &person(),
        Operation::ApprovePhase {
            phase: 1,
            generation: 1,
            confirm_initial: false,
        },
    )
    .unwrap();
    let p =
        f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .publication
            .unwrap()
            .id;
    let work = f.h.store.publication_work(TEAM).unwrap();
    (f, p, work)
}

fn write(
    f: &Fixture,
    r: &crate::teams::store::publication::PlannedTaskRequest,
) -> Result<&'static str> {
    super::super::super::planned::write(&f.config, TEAM, r.node.id, &r.task.uuid, false)
}

#[test]
fn real_named_tasks_publish_together_with_the_original_lead_criteria_and_ids() {
    let (mut f, p, requests) = fixture();
    let original = f.h.store.publication_work(TEAM).unwrap();
    assert_eq!(write(&f, &requests[0]).unwrap(), "created");
    assert!(f.h.store.node(TEAM, requests[0].node.id).unwrap().is_none());
    assert!(f
        .h
        .run(
            &ops::tests::service(),
            Operation::ReconcilePublication {
                phase: 1,
                publication: p.clone()
            }
        )
        .is_err());
    let mut versions = vec![];
    for r in &requests {
        assert_eq!(write(&f, r).unwrap(), "created");
        let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
        let doc = tasks.load(&r.task.uuid).unwrap();
        let view = doc.view().unwrap();
        assert_eq!(view.title, r.node.title);
        assert_eq!(view.text, r.text);
        assert_eq!(view.author, Actor::Agent(f.lead.clone()));
        assert_eq!(view.team.as_ref().unwrap().team, TEAM);
        assert_eq!(view.stage, Stage::Todo);
        assert_eq!(view.target, Target::Any);
        versions.push(doc.revision());
    }
    f.h.run(
        &ops::tests::service(),
        Operation::ReconcilePublication {
            phase: 1,
            publication: p,
        },
    )
    .unwrap();
    let phase = f.h.store.phase_draft(TEAM, 1).unwrap();
    assert_eq!(phase.phase.state, zerus_team_plan::PhaseState::Approved);
    let live = phase.live.unwrap();
    assert!(live.complete);
    assert_eq!(live.nodes.len(), 2);
    for (r, version) in original.iter().zip(versions) {
        assert_eq!(write(&f, r).unwrap(), "created");
        let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
        assert_eq!(tasks.load(&r.task.uuid).unwrap().revision(), version);
        assert_eq!(
            f.h.store
                .node(TEAM, r.node.id)
                .unwrap()
                .unwrap()
                .task
                .as_ref(),
            Some(&r.task)
        );
    }
}

struct OwnedChild {
    child: Child,
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn paused(
    f: &Fixture,
    r: &crate::teams::store::publication::PlannedTaskRequest,
    label: &str,
) -> OwnedChild {
    let args = vec![TEAM.into(), r.node.id.to_string(), r.task.uuid.clone()];
    let (child, _) = f.spawn_with(
        "planned-write",
        &args,
        "",
        false,
        &[("HGS_TEST_PUBLICATION_PAUSE", label)],
    );
    let mut owned = OwnedChild { child };
    let until = Instant::now() + Duration::from_secs(15);
    while !f.config.state.join(format!("{label}.ready")).exists() {
        assert!(
            owned.child.try_wait().unwrap().is_none(),
            "publication child ended before its pause"
        );
        assert!(
            Instant::now() < until,
            "publication child did not reach its pause"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    owned
}

#[test]
fn a_lost_answer_after_save_is_recovered_by_reading_without_a_second_write() {
    let (f, _, requests) = fixture();
    let r = &requests[0];
    let mut child = paused(&f, r, "publication_saved");
    child.child.kill().unwrap();
    child.child.wait().unwrap();
    drop(child);
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    let version = tasks.load(&r.task.uuid).unwrap().revision();
    drop(tasks);
    assert_eq!(write(&f, r).unwrap(), "created");
    assert_eq!(write(&f, r).unwrap(), "created");
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    assert_eq!(tasks.load(&r.task.uuid).unwrap().revision(), version);
}

#[test]
fn a_claim_without_a_task_stays_uncertain_and_never_reinserts() {
    let (f, _, requests) = fixture();
    let r = &requests[0];
    let mut child = paused(&f, r, "publication_claim");
    child.child.kill().unwrap();
    child.child.wait().unwrap();
    drop(child);
    assert!(write(&f, r)
        .unwrap_err()
        .message
        .contains("will not be recreated"));
    assert!(write(&f, r).is_err());
    assert!(!super::super::super::task_present(&f.config, &r.task).unwrap());
    assert_eq!(
        f.h.store
            .phase_draft(TEAM, 1)
            .unwrap()
            .publication
            .unwrap()
            .blocked,
        1
    );
}

#[test]
fn pause_and_stop_win_before_writing_and_stop_clears_an_unbound_created_task() {
    let (mut f, _, requests) = fixture();
    let r = &requests[0];
    f.h.run(&person(), Operation::Pause).unwrap();
    assert!(write(&f, r).is_err());
    assert!(!super::super::super::task_present(&f.config, &r.task).unwrap());
    f.h.run(&person(), Operation::Resume).unwrap();
    write(&f, r).unwrap();
    assert!(f.h.store.node(TEAM, r.node.id).unwrap().is_none());
    f.h.run(&person(), Operation::Stop).unwrap();
    assert_eq!(
        super::super::super::planned::write(&f.config, TEAM, r.node.id, &r.task.uuid, true)
            .unwrap(),
        "cleared"
    );
    assert!(write(&f, r).is_err());
    let tasks = crate::tasks::store(&f.config, &r.task.project).unwrap();
    assert!(tasks.load(&r.task.uuid).unwrap().team().unwrap().is_none());
}
