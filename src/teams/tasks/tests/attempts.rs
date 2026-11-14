use super::*;
use zerus_replica::execution::{NativeRequest, Origin, Receipt, ReportKind};
use zerus_replica::Settlement;

fn reserve(
    doc: &mut TaskDoc,
    executor: &zerus_replica::SessionKey,
    origin: Origin,
    submitted: bool,
) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let native = NativeRequest {
        machine: executor.machine.clone(),
        name: executor.session.clone(),
        run_id: "fixture-run".into(),
        conversation_id: "fixture-conversation".into(),
        digest: "a".repeat(64),
    };
    doc.reserve_attempt(
        &id,
        origin,
        executor,
        native.clone(),
        &TaskActor::Person,
        10,
    )
    .unwrap();
    if submitted {
        doc.settle_attempt(
            &id,
            Settlement::Receipt(&Receipt {
                request_id: id.clone(),
                name: native.name,
                run_id: native.run_id,
                conversation_id: native.conversation_id,
                first_message: false,
                digest: native.digest,
                submitted: true,
            }),
            &TaskActor::Person,
            11,
        )
        .unwrap();
    } else {
        doc.settle_attempt(&id, Settlement::NotAdmitted, &TaskActor::Person, 11)
            .unwrap();
    }
    id
}
fn check(doc: &TaskDoc, number: u64, id: &str, conflict: bool) {
    let o = observation(doc, TEAM, "00000000-0000-4000-8000-0000000000aa").unwrap();
    assert_eq!(o.attempt, number, "{:?}", doc.view().unwrap().stage);
    assert_eq!(o.attempt_id, id);
    assert_eq!(o.attempt_conflict, conflict);
    assert_eq!(o.revision, doc.revision());
}
#[test]
fn observation_tracks_attempt_identity_in_every_stage_and_keeps_the_highest_claim() {
    let h = Harness::new();
    let reference = document(&h);
    let mut doc = task(&h, &reference);
    check(&doc, 0, "", false);
    doc.set_stage(Stage::Backlog, &TaskActor::Person, 2)
        .unwrap();
    check(&doc, 0, "", false);
    doc.set_stage(Stage::Todo, &TaskActor::Person, 3).unwrap();
    doc.floor().unwrap();
    let holder = zerus_replica::SessionKey {
        machine: "00000000-0000-4000-8000-0000000000aa".into(),
        session: "codex/holder".into(),
        session_id: "launch_id:fixture-holder".into(),
    };
    let baseline = doc.snapshot().unwrap();
    let id = reserve(&mut doc, &holder, Origin::Send, true);
    check(&doc, 1, &id, false);
    let active = doc.snapshot().unwrap();
    doc.begin_attempt(&id, &TaskActor::Agent(holder.clone()), 12)
        .unwrap();
    check(&doc, 1, &id, false);
    doc.report_attempt(
        &id,
        "The actual holder result",
        ReportKind::Ready,
        None,
        &TaskActor::Agent(holder.clone()),
        13,
    )
    .unwrap();
    check(&doc, 1, &id, false);
    doc.set_stage(Stage::Done, &TaskActor::Person, 14).unwrap();
    check(&doc, 1, &id, false);
    doc.set_stage(Stage::Todo, &TaskActor::Person, 15).unwrap();
    check(&doc, 1, &id, false);
    let mut cancelled = TaskDoc::from_bytes(&active).unwrap();
    cancelled
        .record_cancel("Cancelled by the person", &TaskActor::Person, 12)
        .unwrap();
    check(&cancelled, 1, &id, false);
    cancelled
        .set_stage(Stage::Todo, &TaskActor::Person, 13)
        .unwrap();
    check(&cancelled, 1, &id, false);
    cancelled
        .set_stage(Stage::Backlog, &TaskActor::Person, 14)
        .unwrap();
    check(&cancelled, 1, &id, false);
    let mut legacy = task(&h, &reference);
    legacy
        .record_send(&holder, "legacy-receipt", &TaskActor::Person, 4)
        .unwrap();
    check(&legacy, 0, "", false);
    let mut rejected = TaskDoc::from_bytes(&active).unwrap();
    let mut other = holder.clone();
    other.session = "codex/replacement".into();
    other.session_id = "launch_id:fixture-replacement".into();
    reserve(&mut rejected, &other, Origin::Handoff, false);
    let e = rejected.view().unwrap().execution.unwrap();
    assert_eq!(e.highest, 2);
    assert_eq!(e.current.unwrap().generation, 1);
    check(&rejected, 2, &id, false);
    let mut refused = TaskDoc::from_bytes(&baseline).unwrap();
    reserve(&mut refused, &holder, Origin::Send, false);
    check(&refused, 1, "", false);
    let mut a = TaskDoc::from_bytes(&baseline).unwrap();
    let mut b = TaskDoc::from_bytes(&baseline).unwrap();
    reserve(&mut a, &holder, Origin::Send, true);
    reserve(&mut b, &holder, Origin::Send, true);
    a.import(&b.snapshot().unwrap()).unwrap();
    let e = a.view().unwrap().execution.unwrap();
    assert_eq!(e.highest, 1);
    assert!(e.conflict);
    check(&a, 1, "", true);
}
