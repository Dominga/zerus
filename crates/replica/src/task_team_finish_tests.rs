use super::*;

#[test]
fn team_finish_history_is_readable_without_the_team_reader() {
    let mut doc =
        TaskDoc::from_bytes(include_bytes!("../tests/vectors/task-team-finished.loro")).unwrap();
    let view = doc.view().unwrap();
    assert_eq!(view.stage, Stage::Done);
    assert!(view
        .log
        .iter()
        .any(|entry| entry.text == "Finished by the team"));
    assert!(!view.log.iter().any(|entry| entry.kind == "report"));
    let team = string(&doc.fields(), "team").unwrap();
    doc.comment("Read by a client without team routing", &Actor::Person, 50)
        .unwrap();
    assert_eq!(string(&doc.fields(), "team").unwrap(), team);
}

#[cfg(not(feature = "test-without-team"))]
mod writer {
    use super::*;
    use crate::execution::{EventKind, NativeRequest, Origin, Phase, Receipt, ReportKind};
    const HOME: &str = "00000000-0000-4000-8000-000000000002";
    fn actor(name: &str) -> Actor {
        Actor::Agent(SessionKey {
            machine: HOME.into(),
            session_id: format!("launch_id:{name}"),
            session: format!("codex/{name}"),
        })
    }
    fn task() -> TaskDoc {
        let mut doc = TaskDoc::create(NewTask {
            project: "example-project",
            id: "T-abcdef",
            text: "Check this change",
            stage: Stage::Todo,
            target: Target::Any,
            author: actor("lead"),
            rank: "V",
            now: 1,
        })
        .unwrap();
        doc.set_team_named(
            Some(&crate::TaskTeam {
                team: "00000000-0000-4000-8000-000000000001".into(),
                home: HOME.into(),
            }),
            "Example team",
            &actor("lead"),
            2,
        )
        .unwrap();
        doc
    }
    fn sent(doc: &mut TaskDoc, submitted: bool) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let Actor::Agent(holder) = actor("holder") else {
            unreachable!()
        };
        let native = NativeRequest {
            machine: HOME.into(),
            name: holder.session.clone(),
            run_id: "example-run".into(),
            conversation_id: "example-conversation".into(),
            digest: "ab".repeat(32),
        };
        doc.reserve_attempt(
            &id,
            Origin::Send,
            &holder,
            native.clone(),
            &actor("lead"),
            3,
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
                &actor("lead"),
                4,
            )
            .unwrap();
        }
        id
    }
    fn refusal(doc: &mut TaskDoc) {
        let revision = doc.revision();
        let log = doc.view().unwrap().log;
        assert!(doc.finish_team_accepted(&actor("lead"), 20).is_err());
        assert_eq!(doc.revision(), revision);
        assert_eq!(doc.view().unwrap().log, log);
    }

    #[test]
    fn team_finish_accepts_review_submitted_and_running_as_the_lead_without_a_report() {
        for state in ["review", "submitted", "running"] {
            let mut doc = task();
            let id = sent(&mut doc, true);
            if state == "running" {
                doc.begin_attempt(&id, &actor("holder"), 5).unwrap();
            }
            if state == "review" {
                doc.report_attempt(
                    &id,
                    "Holder result",
                    ReportKind::Ready,
                    None,
                    &actor("holder"),
                    6,
                )
                .unwrap();
            }
            let schema = doc.schema();
            let reports = doc
                .view()
                .unwrap()
                .log
                .iter()
                .filter(|e| e.kind == "report")
                .count();
            doc.finish_team_accepted(&actor("lead"), 10).unwrap();
            let view = doc.view().unwrap();
            assert_eq!(view.stage, Stage::Done);
            assert_eq!(doc.schema(), schema);
            assert_eq!(view.execution.unwrap().current.unwrap().phase, Phase::Ready);
            assert_eq!(
                view.log.iter().filter(|e| e.kind == "report").count(),
                reports
            );
            let entry = view.log.last().unwrap();
            assert_eq!(entry.text, "Finished by the team");
            assert_eq!(entry.actor, actor("lead"));
            let (attempts, events) = doc.execution_records().unwrap();
            assert_eq!(attempts.len(), 1);
            if state != "review" {
                assert!(events
                    .iter()
                    .any(|e| e.kind == EventKind::Ready && e.actor == actor("lead")));
            }
            if state == "running" && std::env::var_os("ZERUS_BLESS").is_some() {
                std::fs::write(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/vectors/task-team-finished.loro"),
                    doc.snapshot().unwrap(),
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn team_finish_refuses_each_unproved_state_without_a_change() {
        for change in 0..12 {
            let mut doc = task();
            let id = sent(&mut doc, change != 6);
            match change {
                0 => doc.set_team(None, &Actor::Person, 5).unwrap(),
                1 => doc.set_raw_field("team", "not JSON"),
                2 => {
                    doc.fields().insert("cancelled_at", 5).unwrap();
                    doc.doc.commit();
                }
                3 => {
                    doc.fields().insert("stage", "todo").unwrap();
                    doc.doc.commit();
                }
                4 => {
                    doc.fields().insert("stage", "backlog").unwrap();
                    doc.doc.commit();
                }
                5 => {
                    doc.fields().insert("stage", "done").unwrap();
                    doc.doc.commit();
                }
                6 => {}
                7 => {
                    doc.push_event(&id, EventKind::Paused, &Actor::Person, 5)
                        .unwrap();
                    doc.doc.commit();
                }
                8 => {
                    doc.push_event(&id, EventKind::Cancelled, &Actor::Person, 5)
                        .unwrap();
                    doc.doc.commit();
                }
                9 => {
                    doc.set_floor_value_for_test(false);
                }
                10 => {
                    doc.meta().insert("schema", 6).unwrap();
                    doc.doc.commit();
                }
                11 => {
                    doc.meta().insert("schema", TASK_SCHEMA + 1).unwrap();
                    doc.doc.commit();
                }
                _ => unreachable!(),
            }
            refusal(&mut doc);
        }
        let mut absent = task();
        refusal(&mut absent);
        absent.floor().unwrap();
        refusal(&mut absent);
        let mut legacy = task();
        let Actor::Agent(holder) = actor("holder") else {
            unreachable!()
        };
        legacy
            .record_send(&holder, "legacy-dispatch", &actor("lead"), 3)
            .unwrap();
        refusal(&mut legacy);
        legacy
            .record_report("Legacy report", &actor("holder"), 4)
            .unwrap();
        refusal(&mut legacy);
    }

    #[test]
    fn team_finish_refuses_two_current_attempts_after_a_merge() {
        let mut one = task();
        let mut two = TaskDoc::from_bytes(&one.snapshot().unwrap()).unwrap();
        sent(&mut one, true);
        sent(&mut two, true);
        one.import(&two.snapshot().unwrap()).unwrap();
        assert!(one.view().unwrap().execution.unwrap().conflict);
        refusal(&mut one);
    }
    #[test]
    fn team_finish_refuses_a_review_without_a_ready_attempt_and_a_wrong_actor() {
        for change in 0..5 {
            let mut doc = task();
            let id = sent(&mut doc, true);
            if change == 1 {
                doc.begin_attempt(&id, &actor("holder"), 5).unwrap();
            }
            if change < 2 {
                doc.fields().insert("stage", "review").unwrap();
                doc.doc.commit();
            }
            let mut who = actor("lead");
            if change >= 2 {
                doc.report_attempt(
                    &id,
                    "Actual holder result",
                    ReportKind::Ready,
                    None,
                    &actor("holder"),
                    6,
                )
                .unwrap();
                if change == 2 {
                    who = Actor::Person;
                }
                if let Actor::Agent(k) = &mut who {
                    if change == 3 {
                        k.machine = "foreign".into();
                    } else {
                        k.session_id.clear();
                    }
                }
            }
            let view = doc.view().unwrap();
            let revision = doc.revision();
            assert!(doc.finish_team_accepted(&who, 10).is_err(), "{change}");
            assert_eq!(doc.revision(), revision);
            assert_eq!(doc.view().unwrap(), view);
        }
    }
}
