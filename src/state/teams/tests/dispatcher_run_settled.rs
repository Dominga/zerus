// Task 36 (audit task 29, P8): a member that ends its turn waiting for its
// own `hgs team run` is not done. Nothing told it when the run settled, and
// report_timeout_seconds later the dispatcher captured "waiting for my build"
// as the report or marked No report. Now the run it waits on is remembered,
// its end wakes the member once, and only a member that still does nothing
// after the wake gets No report.
use super::*;

const RUN: &str = "6b1f2c3d-0000-4000-8000-000000000036";

/// The audit's repro: dev took #1 at 10, started its turn at 20, claimed a
/// team run and ended its turn at 50 saying it waits for the build.
fn waiting_for_its_build() -> (Team, Seen) {
    let mut team = team();
    team.id = "33fbafeb-0000-4000-8000-000000000036".into();
    delivered(&mut team);
    team.task_mut(1).unwrap().attempts[0].started_at = Some(20.0);
    let mut dev = session("codex/x/dev", "r1", true);
    dev.reply_at = Some(50.0);
    dev.reply_id = "r7".into();
    dev.building = true;
    dev.build_run = RUN.into();
    dev.build_label = "cargo-test".into();
    (team, dev)
}

fn settled(dev: &Seen) -> Seen {
    let mut done = dev.clone();
    done.building = false;
    done.build_run.clear();
    done.build_label.clear();
    done
}

#[test]
fn a_member_waiting_for_its_own_team_run_is_woken_not_captured() {
    let (mut team, dev) = waiting_for_its_build();
    let timeout = team.settings.report_timeout_seconds as f64;
    // While the run is claimed the session is Building: the run is remembered
    // and the turn is not captured.
    let found = steps(&team, std::slice::from_ref(&dev), 60.0);
    assert!(found.contains(&Step::RunAwaited { task: 1, run: RUN.into(), label: "cargo-test".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::TurnEnded { .. } | Step::NoReport { .. })), "{found:?}");
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.awaited_run = RUN.into();
        attempt.awaited_label = "cargo-test".into();
    }
    // Remembered once: the next tick asks for nothing new.
    assert!(!steps(&team, std::slice::from_ref(&dev), 70.0).iter().any(|step| matches!(step, Step::RunAwaited { .. })));
    // The run settles and the session stays idle. Before this task the quiet
    // session became a TurnEnded at once and, report_timeout_seconds later, a
    // No report that captured "waiting for my build". Now the member is woken
    // and nothing is captured, however long the tick comes after.
    let done = settled(&dev);
    for at in [80.0, 80.0 + timeout, 50.0 + 10.0 * timeout] {
        let found = steps(&team, std::slice::from_ref(&done), at);
        assert!(found.contains(&Step::RunSettled { task: 1, wake: true }), "{at}: {found:?}");
        assert!(!found.iter().any(|step| matches!(step, Step::TurnEnded { .. } | Step::NoReport { .. } | Step::GateOpen { .. })), "{at}: {found:?}");
    }
    // The same holds when the turn end was already recorded before the run
    // was seen (an old record whose timeout already ran out).
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.turn_ended_at = Some(50.0);
        attempt.turn_reply = "r7".into();
        attempt.report_ready_at = Some(55.0);
    }
    let found = steps(&team, std::slice::from_ref(&done), 55.0 + timeout);
    assert!(found.contains(&Step::RunSettled { task: 1, wake: true }) && !found.contains(&Step::NoReport { task: 1 }), "{found:?}");
    // A member that waited inside its turn (hgs team run --wait) works on when
    // the run settles: the run is forgotten, nobody is woken.
    let mut busy = done.clone();
    busy.idle = false;
    busy.free = false;
    busy.activity = "busy".into();
    let found = steps(&team, &[busy], 90.0);
    assert!(found.contains(&Step::RunSettled { task: 1, wake: false }), "{found:?}");
}

#[test]
fn the_wake_goes_once_and_a_member_that_still_stops_gets_no_report() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let (mut team, dev) = waiting_for_its_build();
        {
            let attempt = &mut team.task_mut(1).unwrap().attempts[0];
            attempt.awaited_run = RUN.into();
            attempt.awaited_label = "cargo-test".into();
            attempt.turn_ended_at = Some(50.0);
            attempt.turn_reply = "r7".into();
            attempt.report_ready_at = Some(55.0);
        }
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team.clone(), vec![]).unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            assert_eq!(args.first().copied(), Some("send"));
            record.borrow_mut().push(input.and_then(|input| input["text"].as_str()).unwrap_or_default().to_owned());
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_step(&mut store, &team, &Step::RunSettled { task: 1, wake: true }, "").unwrap();
        // A second settle of the same run (a repeated tick) sends nothing.
        let saved = store.load(&team.id).unwrap().unwrap();
        run_step(&mut store, &saved, &Step::RunSettled { task: 1, wake: true }, "").unwrap();
        set_fake_hgs(None);
        assert_eq!(sent.borrow().len(), 1, "woken once: {:?}", sent.borrow());
        let text = sent.borrow()[0].clone();
        assert!(text.contains("build (cargo-test) for task #1 API has finished"), "{text}");
        assert!(text.contains(RUN) && text.contains("hgs team report") && text.contains("00000000-0000-4000-8000-000000000001"), "{text}");
        let saved = store.load(&team.id).unwrap().unwrap();
        let attempt = &saved.task(1).unwrap().attempts[0];
        assert!(attempt.awaited_run.is_empty() && attempt.awaited_label.is_empty());
        assert_eq!((attempt.turn_ended_at, attempt.report_ready_at, attempt.no_report), (None, None, false), "the finished turn starts again");
        assert!(event_kinds(&store, &team.id).contains(&"run_settled_woken".to_owned()));
        // The member truly stops: no new turn after the wake. Nothing wakes it
        // again; its old turn is recorded and, after the timeout, No report.
        let done = settled(&dev);
        let found = steps(&saved, std::slice::from_ref(&done), 200.0);
        assert!(!found.iter().any(|step| matches!(step, Step::RunSettled { .. } | Step::RunAwaited { .. })), "{found:?}");
        assert!(found.contains(&Step::TurnEnded { task: 1, at: 50.0, reply: "r7".into() }), "{found:?}");
        let mut stopped = saved.clone();
        {
            let attempt = &mut stopped.task_mut(1).unwrap().attempts[0];
            attempt.turn_ended_at = Some(50.0);
            attempt.turn_reply = "r7".into();
            attempt.report_ready_at = Some(200.0);
        }
        let timeout = stopped.settings.report_timeout_seconds as f64;
        assert!(!steps(&stopped, std::slice::from_ref(&done), 199.0 + timeout).contains(&Step::NoReport { task: 1 }));
        assert!(steps(&stopped, &[done], 200.0 + timeout).contains(&Step::NoReport { task: 1 }));
    });
}

#[test]
fn the_listing_names_the_run_a_session_builds_on() {
    let listing = json!([{"sessions":[
        {"name":"codex/x/dev","run_id":"r1","state":"running","activity":"idle","phase":"idle",
            "build":{"source":"team_run","request_id":RUN,"label":"cargo-test","command":"cargo test","since":40.0}},
        {"name":"claude/x/lead","run_id":"r0","state":"running","activity":"idle","phase":"idle","build":null}]}]);
    let listed = seen(&listing);
    assert_eq!((listed[0].build_run.as_str(), listed[0].build_label.as_str()), (RUN, "cargo-test"));
    assert!(listed[1].build_run.is_empty() && !listed[1].building);
    // A build the listing gives no request id still counts once, as "build".
    let (mut team, mut dev) = waiting_for_its_build();
    dev.build_run.clear();
    assert!(steps(&team, &[dev], 60.0).contains(&Step::RunAwaited { task: 1, run: "build".into(), label: "cargo-test".into() }));
    team.task_mut(1).unwrap().attempts[0].awaited_run = "build".into();
    let task = team.task(1).unwrap();
    assert!(run_settled_text(task, &task.attempts[0]).contains("Read its result, then go on"));
}
