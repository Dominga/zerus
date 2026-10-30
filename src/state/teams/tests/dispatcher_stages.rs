use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

#[test]
fn a_waiting_stage_goes_to_its_first_free_member_never_the_implementer() {
    let mut t = staged("C:\\proj");
    let found = steps(&t, &free_sessions(), 100.0);
    assert!(found.contains(&Step::StageOut { task: 1, member: "rev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::StageOut { member, .. } if member == "dev")), "{found:?}");
    // A busy reviewer gets nothing; a second reviewer takes the stage.
    let busy = vec![session("claude/x/lead", "r0", true), session("codex/x/dev", "r1", true), session("claude/x/rev", "r2", false), session("kimi/x/qa", "r3", true)];
    assert!(!steps(&t, &busy, 100.0).iter().any(|step| matches!(step, Step::StageOut { .. })));
    t.members.push(Member { id: "rev2".into(), role: "Reviewer".into(), agent: "claude".into(), state: MemberState::Active,
        session: Some(SessionRef { name: "claude/x/rev2".into(), run_id: "r4".into(), ..Default::default() }), ..Default::default() });
    let mut seen = busy;
    seen.push(session("claude/x/rev2", "r4", true));
    assert!(steps(&t, &seen, 100.0).contains(&Step::StageOut { task: 1, member: "rev2".into() }));
    // Given out, its member is busy with it: no second stage and no task.
    super::super::pipeline::give_stage(&mut t, 1, "rev").unwrap();
    let found = steps(&t, &free_sessions(), 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::StageOut { .. })), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "rev")), "{found:?}");
}

/// #635 N1: a stage member's turn that ended after the hand-off with no
/// verdict gives the lead one stalled item, and the run stays Delivered.
#[test]
fn a_stage_member_whose_turn_ends_without_a_verdict_is_told_to_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().display().to_string();
    let mut t = staged(&folder);
    let run = super::super::pipeline::give_stage(&mut t, 1, "rev").unwrap();
    {
        let stage = t.task_mut(1).unwrap().pipeline_run.as_mut().unwrap().runs.iter_mut().find(|stage| stage.id == run).unwrap();
        stage.delivered_at = Some(100.0);
    }
    let lead = session("claude/x/lead", "r0", true);
    let dev = session("codex/x/dev", "r1", true);
    let mut rev = session("claude/x/rev", "r2", true);
    rev.reply_at = Some(200.0);
    let found = steps(&t, &[lead.clone(), dev.clone(), rev.clone()], 1000.0);
    assert!(found.contains(&Step::StageSilent { task: 1, run: run.clone() }), "{found:?}");
    // A member still working, or one with a stage question open, is not silent.
    let mut working = rev.clone();
    working.free = false;
    assert!(!steps(&t, &[lead.clone(), dev.clone(), working], 1000.0).iter().any(|step| matches!(step, Step::StageSilent { .. })));
    {
        let stage = t.task_mut(1).unwrap().pipeline_run.as_mut().unwrap().runs.iter_mut().find(|stage| stage.id == run).unwrap();
        stage.questions.push(Question { id: "q".into(), text: "Which line?".into(), at: 150.0, ..Default::default() });
    }
    assert!(!steps(&t, &[lead, dev, rev], 1000.0).iter().any(|step| matches!(step, Step::StageSilent { .. })));
    {
        let stage = t.task_mut(1).unwrap().pipeline_run.as_mut().unwrap().runs.iter_mut().find(|stage| stage.id == run).unwrap();
        stage.questions.clear();
    }
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), t.clone(), vec![]).unwrap();
    run_step(&mut store, &t, &Step::StageSilent { task: 1, run: run.clone() }, "").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let stage = saved.task(1).unwrap().pipeline_run.as_ref().unwrap().runs.iter().find(|stage| stage.id == run).unwrap();
    assert!(stage.silent.contains("without a verdict"), "{stage:?}");
    assert_eq!(stage.state, StageState::Delivered, "the run is not reset: {stage:?}");
    let items = crate::state::teams::inbox::items(&saved);
    assert!(items.iter().any(|item| item.kind == "stage_stalled" && item.key.ends_with(":silent")), "{items:?}");
    let page = store.events(&t.id, None, 50).unwrap();
    assert!(page["items"].as_array().unwrap().iter().any(|event| event["kind"] == "stage_silent"), "{page}");
}

/// #635 N3: an unconfirmed hand-off is stalled only with evidence it did
/// not arrive; a reply after delivery or a run claim is evidence it did.
#[test]
fn an_unconfirmed_stage_hand_off_is_stalled_only_with_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().display().to_string();
    let mut t = staged(&folder);
    let run = super::super::pipeline::give_stage(&mut t, 1, "rev").unwrap();
    {
        let stage = t.task_mut(1).unwrap().pipeline_run.as_mut().unwrap().runs.iter_mut().find(|stage| stage.id == run).unwrap();
        stage.unconfirmed = true;
        stage.delivered_at = Some(100.0);
    }
    let lead = session("claude/x/lead", "r0", true);
    let dev = session("codex/x/dev", "r1", true);
    let rev = session("claude/x/rev", "r2", true);
    let found = steps(&t, &[lead.clone(), dev.clone(), rev.clone()], 1000.0);
    assert!(found.contains(&Step::StageStalled { task: 1, run: run.clone(), reason: "the stage hand-off was not confirmed".into() }), "{found:?}");
    // A reply after the delivery shows the member had it.
    let mut answered = rev.clone();
    answered.reply_at = Some(200.0);
    assert!(!steps(&t, &[lead.clone(), dev.clone(), answered], 1000.0).iter().any(|step| matches!(step, Step::StageStalled { .. })));
    // A run claimed under the stage's id is evidence it arrived too.
    let member = t.member("rev").unwrap().clone();
    let qa_folder = member_folder_path(&t, &member).unwrap();
    super::super::outbox::write_running(
        Path::new(&qa_folder),
        &run,
        &json!({"request_id":run,"started":1000.0,"timeout_secs":1800,"label":"cargo-test"}),
    )
    .unwrap();
    assert!(!steps(&t, &[lead, dev, rev], 1000.0).iter().any(|step| matches!(step, Step::StageStalled { .. })));
    super::super::outbox::settle_running(Path::new(&qa_folder), &run, &json!({"status":"ran"})).unwrap();
}

#[test]
fn a_held_folder_keeps_the_implementers_next_task_back() {
    let mut t = staged("C:\\proj");
    // #538 N5: from the report the test stage ahead holds the folder, so
    // dev's #2 waits while the review runs too.
    let found = steps(&t, &free_sessions(), 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })), "{found:?}");
    let run = super::super::pipeline::give_stage(&mut t, 1, "rev").unwrap();
    let mut events = Vec::new();
    ops::apply(&mut t, "report", &json!({"task":1,"attempt":run,"verdict":"pass"}), &Actor::Member("rev".into()), &BTreeSet::new(), &mut events).unwrap();
    // The test holds the folder: #2 waits, the test goes to qa.
    let found = steps(&t, &free_sessions(), 100.0);
    assert!(found.contains(&Step::StageOut { task: 1, member: "qa".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })), "{found:?}");
    // An ended stage with a temporary folder is cleaned up.
    let run = super::super::pipeline::give_stage(&mut t, 1, "qa").unwrap();
    t.task_mut(1).unwrap().pipeline_run.as_mut().unwrap().open_run_mut().unwrap().temp = "C:\\gone".into();
    ops::apply(&mut t, "report", &json!({"task":1,"attempt":run,"verdict":"pass"}), &Actor::Member("qa".into()), &BTreeSet::new(), &mut events).unwrap();
    let found = steps(&t, &free_sessions(), 100.0);
    assert!(found.contains(&Step::StageCleanup { task: 1, run: run.clone() }), "{found:?}");
    assert!(found.contains(&Step::HandOut { task: 2, member: "dev".into() }), "the hold ended: {found:?}");
}

#[test]
fn a_stage_hand_out_names_the_folder_and_a_refusal_puts_it_back() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("proj");
    std::fs::create_dir_all(&folder).unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let t = staged(&folder.display().to_string());
    store.create("11111111-1111-4111-8111-111111111532", &json!({}), t.clone(), vec![]).unwrap();
    let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, String)>::new()));
    let record = sent.clone();
    let refuse = std::rc::Rc::new(std::cell::Cell::new(true));
    let refusing = refuse.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
        assert_eq!(args.first().copied(), Some("send"));
        let input = input.cloned().unwrap_or_default();
        record.borrow_mut().push((args.get(1).copied().unwrap_or_default().to_owned(), input["text"].as_str().unwrap_or_default().to_owned()));
        Ok(if refusing.get() { fake_output(1, "", "hgs: the session is busy") } else { fake_output(0, r#"{"status":"submitted"}"#, "") })
    })));
    let refused = stage_hand_out(&mut store, &t.id, 1, "rev", "");
    let after_refusal = store.load(&t.id).unwrap().unwrap();
    refuse.set(false);
    let delivered = stage_hand_out(&mut store, &t.id, 1, "rev", "");
    set_fake_hgs(None);
    refused.unwrap();
    delivered.unwrap();
    let run = after_refusal.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap().clone();
    assert_eq!((run.state, run.member.as_str(), run.folder.as_str()), (StageState::Waiting, "", ""), "a refused stage waits again");
    let saved = store.load(&t.id).unwrap().unwrap();
    let run = saved.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap().clone();
    assert_eq!((run.state, run.member.as_str()), (StageState::Delivered, "rev"));
    assert!(run.delivered_at.is_some());
    assert_eq!(std::path::PathBuf::from(&run.folder), folder);
    let sent = sent.borrow();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].0, "claude/x/rev");
    assert!(sent[1].1.contains("review stage") && sent[1].1.contains(&format!("Folder: {}", folder.display())), "{}", sent[1].1);
    assert!(sent[1].1.contains(&format!("\"attempt\":\"{}\"", run.id)), "{}", sent[1].1);
    // hgs team current names the stage for its member.
    let current = super::super::commands::current_view(&saved, "rev").unwrap();
    assert_eq!((current["attempt"].clone(), current["stage"].clone()), (json!(run.id), json!("review")));
    // The implementer is never given its own stage.
    assert!(stage_hand_out(&mut store, &t.id, 1, "dev", "").is_err());
}

#[test]
fn a_review_and_a_test_stage_reach_their_members_through_the_dispatcher() {
    // U36 stage delivery, end to end through a tick: the planner names the
    // stage member, run_step gives it the stage and sends the hand-off, and
    // the member's verdict moves the task on; the implementer is never one.
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = staged(&folder.display().to_string());
        store.create("11111111-1111-4111-8111-111111111594", &json!({}), t.clone(), vec![]).unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
            assert_eq!(args.first().copied(), Some("send"));
            record.borrow_mut().push(args.get(1).copied().unwrap_or_default().to_owned());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        let tick = |store: &mut Store| -> anyhow::Result<Vec<Step>> {
            let team = store.load(&t.id)?.context("unknown team")?;
            let planned: Vec<Step> = steps(&team, &free_sessions(), now()).into_iter().filter(|step| matches!(step, Step::StageOut { .. })).collect();
            for step in &planned {
                run_step(store, &team, step, "")?;
            }
            Ok(planned)
        };
        let verdict = |store: &mut Store, member: &str| -> anyhow::Result<()> {
            let team = store.load(&t.id)?.context("unknown team")?;
            let current = super::super::commands::current_view(&team, member)?;
            let attempt = current["attempt"].as_str().context("no stage attempt")?.to_owned();
            store.mutate(None, &t.id, None, |team, events| {
                ops::apply(team, "report", &json!({"task":1,"attempt":attempt,"verdict":"pass","text":"Fine"}), &Actor::Member(member.into()), &BTreeSet::new(), events)
            })?;
            Ok(())
        };
        let review = tick(&mut store);
        let reviewed = verdict(&mut store, "rev");
        let after_review = store.load(&t.id).unwrap().unwrap();
        let test = tick(&mut store);
        let tested = verdict(&mut store, "qa");
        let again = tick(&mut store);
        set_fake_hgs(None);
        assert_eq!(review.unwrap(), [Step::StageOut { task: 1, member: "rev".into() }]);
        reviewed.unwrap();
        assert_eq!(after_review.task(1).unwrap().status, TaskStatus::InTest, "the review passed");
        assert_eq!(test.unwrap(), [Step::StageOut { task: 1, member: "qa".into() }]);
        tested.unwrap();
        assert!(again.unwrap().is_empty(), "no stage is left to give out");
        let saved = store.load(&t.id).unwrap().unwrap();
        let task = saved.task(1).unwrap();
        assert_eq!(task.status, TaskStatus::Submitted, "after its stages the task is the lead's");
        let runs = &task.pipeline_run.as_ref().unwrap().runs;
        assert_eq!(
            runs.iter().map(|run| (run.kind.as_str(), run.member.as_str(), run.state)).collect::<Vec<_>>(),
            [("review", "rev", StageState::Passed), ("test", "qa", StageState::Passed)]
        );
        assert!(runs.iter().all(|run| run.delivered_at.is_some()));
        assert_eq!(*sent.borrow(), ["claude/x/rev", "kimi/x/qa"], "each stage member got its own hand-off, the implementer none");
        let kinds: Vec<String> = store.events(&t.id, None, 200).unwrap()["items"].as_array().unwrap().iter().map(|item| item["kind"].as_str().unwrap().to_owned()).collect();
        assert_eq!(kinds.iter().filter(|kind| *kind == "stage_delivered").count(), 2, "{kinds:?}");
    });
}

/// U36: what the desktop and Android read about a delivered stage, as
/// `hgs team status` writes it after a real hand-out: each member's
/// stage_queue and the run's folder and hold. With ZERUS_WRITE_FIXTURES=1
/// the test writes tests/fixtures/team-stage-delivery.json.
#[test]
fn the_shared_stage_delivery_fixture_is_what_status_writes() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = staged(&folder.display().to_string());
        store.create("11111111-1111-4111-8111-111111111537", &json!({}), t.clone(), vec![]).unwrap();
        set_fake_hgs(Some(Box::new(|_, _: &[&str], _: Option<&Value>| Ok(fake_output(0, r#"{"status":"submitted"}"#, "")))));
        let handed = (|| -> anyhow::Result<Team> {
            let team = store.load(&t.id)?.context("unknown team")?;
            run_step(&mut store, &team, &Step::StageOut { task: 1, member: "rev".into() }, "")?;
            let team = store.load(&t.id)?.context("unknown team")?;
            let review = super::super::commands::current_view(&team, "rev")?["attempt"].as_str().unwrap_or_default().to_owned();
            store.mutate(None, &t.id, None, |team, events| {
                ops::apply(team, "report", &json!({"task":1,"attempt":review,"verdict":"pass","text":"Read it"}), &Actor::Member("rev".into()), &BTreeSet::new(), events)
            })?;
            let team = store.load(&t.id)?.context("unknown team")?;
            run_step(&mut store, &team, &Step::StageOut { task: 1, member: "qa".into() }, "")?;
            store.load(&t.id)?.context("unknown team")
        })();
        set_fake_hgs(None);
        let team = handed.unwrap();
        let view = super::super::commands::view_with(&team, &[], None);
        // Only what the stage views read, with a synthetic folder and fixed
        // times, so the fixture is the same on every computer.
        let real = dir.path().display().to_string();
        let mut shared = json!({
            "members": view["members"].as_array().unwrap().iter()
                .map(|member| {
                    let mut kept = json!({"id":member["id"],"role":member["role"],"stage_queue":member["stage_queue"]});
                    // #635 N4: the stage that holds the member's folder.
                    if let Some(held) = member.get("held_by") {
                        kept["held_by"] = held.clone();
                    }
                    kept
                })
                .collect::<Vec<_>>(),
            "tasks": view["tasks"].as_array().unwrap().iter().filter(|task| task["id"] == 1)
                .map(|task| json!({"id":task["id"],"title":task["title"],"status":task["status"],"assignee":task["assignee"],
                    "pipeline":task["pipeline"],"pipeline_run":task["pipeline_run"]}))
                .collect::<Vec<_>>(),
        });
        fn normalize(value: &mut Value, real: &str) {
            match value {
                Value::Object(map) => {
                    for (key, item) in map.iter_mut() {
                        let time = key == "at" || key == "since" || key.ends_with("_at");
                        if time && item.is_number() {
                            *item = json!(1800000000.0);
                        } else if key == "temp" && item.as_str().is_some_and(|temp| !temp.is_empty()) {
                            // Named after the run; only that the test has one counts.
                            *item = json!("/srv/example/stage-temp");
                        } else if key == "id" && item.as_str().is_some_and(|id| id.starts_with("stage-") || id.len() == 36) {
                            *item = json!("stage-run");
                        } else {
                            normalize(item, real);
                        }
                    }
                }
                Value::Array(items) => items.iter_mut().for_each(|item| normalize(item, real)),
                Value::String(text) if text.starts_with(real) => *text = format!("/srv/example{}", text[real.len()..].replace('\\', "/")),
                _ => {}
            }
        }
        normalize(&mut shared, &real);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/team-stage-delivery.json");
        if std::env::var("ZERUS_WRITE_FIXTURES").as_deref() == Ok("1") {
            std::fs::write(&path, serde_json::to_string_pretty(&shared).unwrap() + "\n").unwrap();
        }
        let qa = shared["members"].as_array().unwrap().iter().find(|member| member["id"] == "qa").unwrap();
        assert_eq!(qa["stage_queue"][0], json!({"task":1,"stage":"test","round":1,"state":"delivered","since":1800000000.0}));
        let run = &shared["tasks"][0]["pipeline_run"]["runs"][1];
        assert_eq!((run["kind"].as_str(), run["state"].as_str(), run["folder"].as_str(), run["holds"].as_bool()),
            (Some("test"), Some("delivered"), Some("/srv/example/proj"), Some(true)), "{run}");
        let stored: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(shared, stored, "the fixture is what status writes; rewrite it with ZERUS_WRITE_FIXTURES=1");
    });
}

#[test]
fn a_command_stage_is_never_given_to_a_member() {
    // U47 with U36 delivery: a stage Zerus runs itself has no candidates,
    // so the dispatcher starts its command instead of a member hand-off.
    let mut t = staged("C:\\proj");
    let pipeline = t.task_mut(1).unwrap().pipeline_run.as_mut().unwrap();
    pipeline.stages[0] = PipelineStage {
        kind: "command".into(),
        command: Some(Check { command: "cargo test --locked".into(), expect_exit: 0, timeout_minutes: 5 }),
        ..Default::default()
    };
    let found = steps(&t, &free_sessions(), 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::StageOut { .. })), "{found:?}");
    assert!(found.iter().any(|step| matches!(step, Step::StageCommand { task: 1, .. })), "{found:?}");
}
