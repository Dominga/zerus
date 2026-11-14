use super::*;
#[test]
fn a_team_handover_is_readable_without_the_team_reader() {
    let mut doc =
        TaskDoc::from_bytes(include_bytes!("../tests/vectors/task-team-handover.loro")).unwrap();
    let before = doc.view().unwrap();
    assert_eq!(before.stage, Stage::Active);
    assert_eq!(
        before.session.as_ref().unwrap().session_id,
        "launch_id:replacement"
    );
    assert!(before
        .log
        .iter()
        .any(|e| e.kind == "sent" && e.text == "handoff"));
    assert!(!before.log.iter().any(|e| e.kind == "report"));
    let key = string(&doc.fields(), "team").unwrap();
    doc.comment("An older client can comment", &Actor::Person, 50)
        .unwrap();
    assert_eq!(string(&doc.fields(), "team").unwrap(), key);
}

#[cfg(not(feature = "test-without-team"))]
mod writer {
    use super::*;
    use crate::execution::{EventKind, NativeRequest, Origin, Phase, Receipt, ReportKind};
    const HOME: &str = "00000000-0000-4000-8000-000000000002";
    fn agent(name: &str) -> Actor {
        Actor::Agent(SessionKey {
            machine: HOME.into(),
            session_id: format!("launch_id:{name}"),
            session: format!("codex/{name}"),
        })
    }
    fn key(name: &str) -> SessionKey {
        let Actor::Agent(k) = agent(name) else {
            unreachable!()
        };
        k
    }
    fn native(name: &str) -> NativeRequest {
        NativeRequest {
            machine: HOME.into(),
            name: format!("codex/{name}"),
            run_id: format!("run-{name}"),
            conversation_id: format!("conversation-{name}"),
            digest: "ab".repeat(32),
        }
    }
    fn settle(doc: &mut TaskDoc, id: &str, name: &str) {
        let n = native(name);
        doc.settle_attempt(
            id,
            Settlement::Receipt(&Receipt {
                request_id: id.into(),
                name: n.name,
                run_id: n.run_id,
                conversation_id: n.conversation_id,
                first_message: false,
                digest: n.digest,
                submitted: true,
            }),
            &agent("lead"),
            10,
        )
        .unwrap();
    }
    fn task(stage: Stage) -> (TaskDoc, String) {
        let mut d = TaskDoc::create(NewTask {
            project: "example-project",
            id: "T-abcdef",
            text: "Implement the change",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        d.set_team_named(
            Some(&crate::TaskTeam {
                team: "00000000-0000-4000-8000-000000000001".into(),
                home: HOME.into(),
            }),
            "Example",
            &agent("lead"),
            2,
        )
        .unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        d.reserve_attempt(
            &id,
            Origin::Send,
            &key("holder"),
            native("holder"),
            &agent("lead"),
            3,
        )
        .unwrap();
        settle(&mut d, &id, "holder");
        if stage != Stage::Todo {
            d.begin_attempt(&id, &agent("holder"), 11).unwrap();
        }
        if matches!(stage, Stage::Review | Stage::Done) {
            d.report_attempt(
                &id,
                "Actual result",
                ReportKind::Ready,
                None,
                &agent("holder"),
                12,
            )
            .unwrap();
        }
        if stage == Stage::Done {
            d.set_stage(Stage::Done, &Actor::Person, 13).unwrap();
        }
        (d, id)
    }
    #[test]
    fn team_handover_preserves_provenance_and_a_later_old_report_is_history_only() {
        for stage in [Stage::Todo, Stage::Active, Stage::Review, Stage::Done] {
            let (mut d, old) = task(stage);
            let id = uuid::Uuid::new_v4().to_string();
            let reports = d
                .view()
                .unwrap()
                .log
                .iter()
                .filter(|e| e.kind == "report")
                .count();
            let schema = d.schema();
            let attempt = d
                .reserve_team_handover(
                    &id,
                    &key("replacement"),
                    native("replacement"),
                    &agent("lead"),
                    20,
                )
                .unwrap();
            assert_eq!(attempt.origin, Origin::Handoff);
            assert_eq!(attempt.actor, agent("lead"));
            assert_eq!(d.attempt_phases().unwrap()[&old].1, Phase::Ready);
            settle(&mut d, &id, "replacement");
            let v = d.view().unwrap();
            assert_eq!(v.stage, Stage::Active);
            assert_eq!(v.session, Some(key("replacement")));
            assert_eq!(d.schema(), schema);
            assert_eq!(v.log.iter().filter(|e| e.kind == "report").count(), reports);
            if stage == Stage::Active && std::env::var_os("ZERUS_BLESS").is_some() {
                std::fs::write(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/vectors/task-team-handover.loro"),
                    d.snapshot().unwrap(),
                )
                .unwrap();
            }
            let outcome = d
                .report_attempt(
                    &old,
                    "Late earlier result",
                    ReportKind::Ready,
                    None,
                    &agent("holder"),
                    21,
                )
                .unwrap();
            assert!(!outcome.moves);
            assert!(outcome.stale.is_some());
            assert_eq!(d.view().unwrap().stage, Stage::Active);
            assert_eq!(d.view().unwrap().session, Some(key("replacement")));
        }
    }
    #[test]
    fn team_handover_refuses_every_unproved_state_without_a_change() {
        for change in 0..12 {
            let (mut d, old) = task(Stage::Active);
            let mut to = key("replacement");
            let mut who = agent("lead");
            match change {
                0 => d.set_team(None, &Actor::Person, 14).unwrap(),
                1 => {
                    d.fields().insert("team", "bad JSON").unwrap();
                    d.doc.commit();
                }
                2 => d.record_cancel("Cancelled", &Actor::Person, 14).unwrap(),
                3 => {
                    d.set_floor_value_for_test(false);
                }
                4 => to = key("holder"),
                5 => who = Actor::Person,
                6 => {
                    let Actor::Agent(ref mut k) = who else {
                        unreachable!()
                    };
                    k.machine = "another-home".into();
                }
                7 => {
                    d.fields().insert("stage", "todo").unwrap();
                    d.doc.commit();
                }
                8 => {
                    d.fields().delete("team").unwrap();
                    d.doc.commit();
                }
                9 => {
                    to.machine = "another-home".into();
                }
                10 => {
                    d.push_event(&old, EventKind::Paused, &Actor::Person, 14)
                        .unwrap();
                    d.doc.commit();
                }
                11 => {
                    let Actor::Agent(ref mut k) = who else {
                        unreachable!()
                    };
                    k.session_id.clear();
                }
                _ => unreachable!(),
            }
            let revision = d.revision();
            let view = d.view().unwrap();
            assert!(
                d.reserve_team_handover(
                    &uuid::Uuid::new_v4().to_string(),
                    &to,
                    native("replacement"),
                    &who,
                    20
                )
                .is_err(),
                "{change}"
            );
            assert_eq!(d.revision(), revision);
            assert_eq!(d.view().unwrap(), view);
        }
    }
    #[test]
    fn a_team_handover_converges_with_remote_edits_start_and_report() {
        for edit in 0..5 {
            let (mut home, old) = task(Stage::Todo);
            let mut remote = TaskDoc::from_bytes(&home.snapshot().unwrap()).unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            home.reserve_team_handover(
                &id,
                &key("replacement"),
                native("replacement"),
                &agent("lead"),
                20,
            )
            .unwrap();
            settle(&mut home, &id, "replacement");
            match edit {
                0 => remote
                    .comment("Remote comment", &Actor::Person, 21)
                    .unwrap(),
                1 => remote.set_team(None, &Actor::Person, 21).unwrap(),
                2 => remote
                    .record_cancel("Remote cancel", &Actor::Person, 21)
                    .unwrap(),
                3 => remote.begin_attempt(&old, &agent("holder"), 21).unwrap(),
                _ => {
                    remote
                        .report_attempt(
                            &old,
                            "Remote old report",
                            ReportKind::Ready,
                            None,
                            &agent("holder"),
                            21,
                        )
                        .unwrap();
                }
            }
            let mut a = TaskDoc::from_bytes(&home.snapshot().unwrap()).unwrap();
            a.import(&remote.snapshot().unwrap()).unwrap();
            let mut b = TaskDoc::from_bytes(&remote.snapshot().unwrap()).unwrap();
            b.import(&home.snapshot().unwrap()).unwrap();
            assert_eq!(a.view().unwrap(), b.view().unwrap());
            let v = a.view().unwrap();
            eprintln!(
                "merge {edit}: stage={:?} key={:?} cancelled={:?} executor={:?} phase={:?}",
                v.stage,
                v.team,
                v.cancelled_at,
                v.execution
                    .as_ref()
                    .and_then(|e| e.current.as_ref())
                    .and_then(|c| c.executor.as_ref()),
                v.execution
                    .as_ref()
                    .and_then(|e| e.current.as_ref())
                    .map(|c| c.phase)
            );
            assert_eq!(v.stage, Stage::Active);
            assert_eq!(
                v.execution
                    .as_ref()
                    .unwrap()
                    .current
                    .as_ref()
                    .unwrap()
                    .executor,
                Some(key("replacement"))
            );
            assert_eq!(
                v.log.iter().filter(|e| e.kind == "report").count(),
                usize::from(edit == 4)
            );
        }
    }
    #[test]
    fn team_handover_refuses_unknown_missing_and_competing_attempts() {
        let (base, _) = task(Stage::Todo);
        let mut unknown = TaskDoc::from_bytes(&base.snapshot().unwrap()).unwrap();
        unknown
            .reserve_attempt(
                &uuid::Uuid::new_v4().to_string(),
                Origin::Handoff,
                &key("replacement"),
                native("replacement"),
                &Actor::Person,
                18,
            )
            .unwrap();
        let mut absent = TaskDoc::create(NewTask {
            project: "example-project",
            id: "T-abcdef",
            text: "No attempt",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        absent
            .set_team_named(base.team().unwrap().as_ref(), "Example", &agent("lead"), 2)
            .unwrap();
        let mut one = TaskDoc::from_bytes(&base.snapshot().unwrap()).unwrap();
        let mut two = TaskDoc::from_bytes(&base.snapshot().unwrap()).unwrap();
        for doc in [&mut one, &mut two] {
            let id = uuid::Uuid::new_v4().to_string();
            doc.reserve_team_handover(
                &id,
                &key("replacement"),
                native("replacement"),
                &agent("lead"),
                20,
            )
            .unwrap();
            settle(doc, &id, "replacement");
        }
        one.import(&two.snapshot().unwrap()).unwrap();
        assert!(one.view().unwrap().execution.unwrap().conflict);
        for mut doc in [unknown, absent, one] {
            let revision = doc.revision();
            assert!(doc
                .reserve_team_handover(
                    &uuid::Uuid::new_v4().to_string(),
                    &key("replacement"),
                    native("replacement"),
                    &agent("lead"),
                    30
                )
                .is_err());
            assert_eq!(doc.revision(), revision);
        }
    }
}
