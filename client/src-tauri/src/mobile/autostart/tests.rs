use super::*;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use zerus_client_core::mobile::autostart::Line;

const NODE: &str = "node-a";
const ARMED: &str = r#"{"version":1,"node_id":"node-a","armed":true}"#;
const GENERATION: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const LATER: &str = "ffffffffffffffffffffffffffffffff";

fn read_with(extra: Value) -> Workspace {
    let mut value = serde_json::json!({
        "state": "active", "encrypted": true, "config_revision": "a".repeat(64),
        "server_url": "https://relay.example.invalid", "node_id": NODE,
        "running": false, "connector_state": "stopped", "generation": null,
    });
    for (key, field) in extra.as_object().unwrap() {
        value[key] = field.clone();
    }
    mobile::workspace(value.as_object().unwrap())
}

fn stopped() -> Workspace {
    read_with(serde_json::json!({}))
}

fn running_as(generation: &str) -> Workspace {
    let mut workspace = read_with(serde_json::json!({
        "running": true, "connector_state": "running", "generation": generation,
    }));
    workspace.stop_supported = true;
    workspace
}

fn running() -> Workspace {
    running_as(GENERATION)
}

fn stopping() -> Workspace {
    read_with(serde_json::json!({
        "running": true, "connector_state": "stopping", "generation": GENERATION,
    }))
}

type Hook = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct Log {
    /// Status reads through the packaged connector, by automatic start.
    automatic_reads: AtomicUsize,
    /// Status reads through the page's own selection.
    page_reads: AtomicUsize,
    starts: AtomicUsize,
    saved: Mutex<Vec<String>>,
    /// File writes that have begun.
    save_entered: AtomicUsize,
    /// Each status read's answer, oldest first; the last one repeats.
    answers: Mutex<VecDeque<Result<Workspace, String>>>,
    during_automatic_read: Mutex<Option<Hook>>,
    /// Holds the next file write: says it began, then waits to be released.
    save_gate: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    start_fails: AtomicBool,
    save_fails: AtomicBool,
    unconfigured: AtomicBool,
    unpackaged: AtomicBool,
}

impl Log {
    fn answer(&self) -> Result<Workspace, String> {
        let mut answers = self.answers.lock().unwrap();
        if answers.len() > 1 {
            answers.pop_front().unwrap()
        } else {
            answers.front().cloned().unwrap()
        }
    }
    fn counts(&self) -> (usize, usize) {
        (
            self.automatic_reads.load(Ordering::SeqCst),
            self.starts.load(Ordering::SeqCst),
        )
    }
    fn saved(&self) -> Vec<Value> {
        self.saved
            .lock()
            .unwrap()
            .iter()
            .map(|text| serde_json::from_str(text).unwrap())
            .collect()
    }
}

fn packaged() -> PathBuf {
    PathBuf::from("packaged/zerus-mobile-connector.exe")
}

fn fixture(
    stored: Option<&str>,
    answers: Vec<Result<Workspace, String>>,
) -> (Arc<Inner>, Arc<Log>) {
    fixture_with(false, stored, answers)
}

fn fixture_with(
    blocked: bool,
    stored: Option<&str>,
    answers: Vec<Result<Workspace, String>>,
) -> (Arc<Inner>, Arc<Log>) {
    let log = Arc::new(Log::default());
    *log.answers.lock().unwrap() = answers.into();
    let mut calls = super::super::tests::service_calls_for_plans();
    let page = Arc::clone(&log);
    calls.status = Box::new(move || {
        page.page_reads.fetch_add(1, Ordering::SeqCst);
        page.answer()
    });
    let mut inner = Inner::new(blocked, calls, Box::new(|| {}));
    let (configured, selected, status, save, start) = (
        Arc::clone(&log),
        Arc::clone(&log),
        Arc::clone(&log),
        Arc::clone(&log),
        Arc::clone(&log),
    );
    inner.enable_autostart(
        stored.map(str::as_bytes),
        Io {
            configured: Box::new(move || !configured.unconfigured.load(Ordering::SeqCst)),
            packaged: Box::new(move || {
                (!selected.unpackaged.load(Ordering::SeqCst)).then(packaged)
            }),
            status: Box::new(move |connector| {
                assert_eq!(connector, packaged(), "a read used another connector");
                status.automatic_reads.fetch_add(1, Ordering::SeqCst);
                let hook = status.during_automatic_read.lock().unwrap().take();
                if let Some(hook) = hook {
                    hook();
                }
                status.answer()
            }),
            save: Box::new(move |bytes| {
                save.save_entered.fetch_add(1, Ordering::SeqCst);
                let gate = save.save_gate.lock().unwrap().take();
                if let Some((began, release)) = gate {
                    began.send(()).unwrap();
                    release.recv().unwrap();
                }
                if save.save_fails.swap(false, Ordering::SeqCst) {
                    return Err("store refused".into());
                }
                save.saved
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes.to_vec()).unwrap());
                Ok(())
            }),
            start: Box::new(move |connector, target| {
                assert_eq!(connector, packaged(), "a start used another connector");
                assert_eq!(target.node_id, NODE);
                start.starts.fetch_add(1, Ordering::SeqCst);
                if start.start_fails.load(Ordering::SeqCst) {
                    return Err("not confirmed".into());
                }
                Ok(core::Answer {
                    workspace: running(),
                    notice: "",
                })
            }),
        },
    );
    (Arc::new(inner), log)
}

impl Inner {
    fn answer_for_test(&self, workspace: Workspace) {
        assert!(self.page().read_workspace());
        self.answered(Ok(workspace));
    }
    /// A button action, past its lane and status admission.
    fn button(
        self: &Arc<Self>,
        kind: core::Kind,
        admitted: &Workspace,
        answer: Result<core::Answer, mobile::Failure>,
    ) -> Result<&'static str, String> {
        let target = core::admit(admitted, kind).unwrap();
        assert!(self.page().read_workspace());
        self.connector_run(&target, kind, |_, _| answer)
    }
}

fn stop_answer(
    workspace: Workspace,
    notice: &'static str,
) -> Result<core::Answer, mobile::Failure> {
    Ok(core::Answer { workspace, notice })
}

#[test]
fn an_eligible_connector_is_started_exactly_once_in_a_gui_run() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    inner.autostart_once();
    inner.autostart_once();
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 1));
    let view = inner.view(None);
    assert_eq!(view.autostart, Some(Line::Running));
    assert!(!view.workspace_busy);
    assert!(
        !inner.effects.load(Ordering::SeqCst),
        "the lane stayed taken"
    );
    assert!(log.saved().is_empty(), "an unchanged choice was saved");
}

#[test]
fn no_attempt_is_made_by_opening_refreshing_or_reading_status() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    inner.open();
    settled(&inner);
    inner.workspace();
    settled(&inner);
    inner.answer_for_test(stopped());
    assert_eq!(log.counts(), (0, 0));
    assert_eq!(inner.view(None).autostart, Some(Line::WillStart));
}

#[test]
fn a_workspace_nobody_started_here_is_left_stopped() {
    let (inner, log) = fixture(None, vec![Ok(stopped())]);
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 0));
    assert_eq!(inner.view(None).autostart, Some(Line::NotArmed));
}

#[test]
fn malformed_arm_state_starts_nothing() {
    let (inner, log) = fixture(Some("on:node-a"), vec![Ok(stopped())]);
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 0));
    assert_eq!(inner.view(None).autostart, Some(Line::NotArmed));
}

#[test]
fn a_connector_the_person_stopped_stays_stopped() {
    let stored = format!(
        r#"{{"version":1,"node_id":"node-a","armed":false,"stopped_generation":"{GENERATION}"}}"#
    );
    let (inner, log) = fixture(Some(&stored), vec![Ok(stopped())]);
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 0));
    assert_eq!(inner.view(None).autostart, Some(Line::StoppedByYou));
}

#[test]
fn a_connector_already_running_is_armed_and_not_started_again() {
    let (inner, log) = fixture(None, vec![Ok(running())]);
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 0));
    assert_eq!(
        log.saved(),
        [serde_json::json!({"version":1,"node_id":NODE,"armed":true})]
    );
    assert_eq!(inner.view(None).autostart, Some(Line::Running));
}

#[test]
fn every_status_that_is_not_definitively_startable_starts_nothing() {
    let cases = [
        read_with(serde_json::json!({"state": "retired"})),
        read_with(serde_json::json!({"state": "legacy"})),
        read_with(serde_json::json!({"state": "pending-activation"})),
        read_with(
            serde_json::json!({"state": "unconfigured", "config_revision": "", "node_id": ""}),
        ),
        read_with(serde_json::json!({"encrypted": false})),
        read_with(serde_json::json!({"pending": {"state": "pending"}})),
        read_with(serde_json::json!({"connector_state": "unknown"})),
        read_with(serde_json::json!({"config_revision": "not-a-revision"})),
        stopping(),
    ];
    for case in cases {
        let (inner, log) = fixture(Some(ARMED), vec![Ok(case.clone())]);
        inner.autostart_once();
        assert_eq!(log.counts(), (1, 0), "started {case:?}");
    }
}

#[test]
fn without_a_configuration_nothing_is_run() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    log.unconfigured.store(true, Ordering::SeqCst);
    inner.autostart_once();
    assert_eq!(log.counts(), (0, 0));
}

#[test]
fn a_replay_runs_nothing() {
    let (inner, log) = fixture_with(true, Some(ARMED), vec![Ok(stopped())]);
    inner.autostart_once();
    assert_eq!(log.counts(), (0, 0));
}

#[test]
fn an_unreadable_status_makes_no_attempt() {
    let (inner, log) = fixture(Some(ARMED), vec![Err("unconfirmed".into())]);
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 0));
    assert_eq!(
        inner.view(None).workspace_error.as_deref(),
        Some("unconfirmed")
    );
}

#[test]
fn a_missing_packaged_connector_reads_and_starts_nothing() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    log.unpackaged.store(true, Ordering::SeqCst);
    inner.autostart_once();
    assert_eq!(log.counts(), (0, 0));
    assert_eq!(log.page_reads.load(Ordering::SeqCst), 0);
    // The page's own read still reports it, through the button's selection.
    inner.answer_for_test(stopped());
    assert_eq!(inner.view(None).autostart, Some(Line::Unavailable));
}

#[test]
fn an_uncertain_start_gets_no_further_automatic_call_in_this_gui_run() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    log.start_fails.store(true, Ordering::SeqCst);
    inner.autostart_once();
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 1));
    assert_eq!(log.page_reads.load(Ordering::SeqCst), 0);
    let view = inner.view(None);
    assert!(!view.workspace_busy);
    assert!(!inner.effects.load(Ordering::SeqCst));
    assert_eq!(
        view.workspace_error.as_deref(),
        Some(mobile::WORKSPACE_UNCONFIRMED)
    );

    // Opening the section shows what really happened, and starts nothing.
    inner.workspace();
    settled(&inner);
    assert_eq!(log.counts(), (1, 1));
    assert_eq!(
        inner.view(None).autostart,
        Some(Line::Failed {
            reason: "not confirmed".into()
        })
    );
}

#[test]
fn a_slow_start_that_did_succeed_shows_running_at_the_next_read() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped()), Ok(running())]);
    log.start_fails.store(true, Ordering::SeqCst);
    inner.autostart_once();
    inner.workspace();
    settled(&inner);
    assert_eq!(log.counts(), (1, 1));
    assert_eq!(inner.view(None).autostart, Some(Line::Running));
}

#[test]
fn a_busy_lane_skips_the_attempt_and_it_is_not_queued() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    inner.effects.store(true, Ordering::SeqCst);
    inner.autostart_once();
    assert_eq!(log.counts(), (0, 0));
    assert!(
        inner.effects.load(Ordering::SeqCst),
        "another action's lane was released"
    );
    inner.effects.store(false, Ordering::SeqCst);
    inner.autostart_once();
    assert_eq!(log.counts(), (0, 0), "the skipped attempt came back");
}

#[test]
fn a_status_read_in_progress_skips_the_attempt_and_releases_the_lane() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    assert!(inner.page().read_workspace());
    inner.autostart_once();
    assert_eq!(log.counts(), (0, 0));
    assert!(
        !inner.effects.load(Ordering::SeqCst),
        "the lane stayed taken"
    );
    assert!(
        inner.view(None).workspace_busy,
        "another read lost its lane"
    );
}

#[test]
fn a_stop_admitted_during_the_status_read_cancels_the_automatic_plan() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    let during = Arc::clone(&inner);
    *log.during_automatic_read.lock().unwrap() = Some(Box::new(move || {
        during.page().autostart_stop_chosen(NODE, Some(GENERATION));
    }));
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 0), "started after the Stop");
    assert_eq!(inner.view(None).autostart, Some(Line::StoppedByYou));
}

#[test]
fn stop_is_saved_before_it_is_sent_and_a_draining_reply_does_not_rearm() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
    let during = Arc::clone(&log);
    let target = core::admit(&running(), core::Kind::Stop).unwrap();
    assert!(inner.page().read_workspace());
    let reply = inner.connector_run(&target, core::Kind::Stop, |_, _| {
        assert_eq!(during.saved().len(), 1, "sent before it was saved");
        assert_eq!(during.saved()[0]["armed"], false);
        stop_answer(stopping(), "draining")
    });
    assert_eq!(reply.unwrap(), "draining");
    assert_eq!(inner.view(None).autostart, None);
    inner.answer_for_test(stopping());
    inner.answer_for_test(running());
    inner.answer_for_test(stopped());
    assert_eq!(log.saved().len(), 1, "a reply changed the choice");
    assert_eq!(inner.view(None).autostart, Some(Line::StoppedByYou));
}

#[test]
fn a_failed_stop_stays_disarmed_against_the_same_generation() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
    let reply = inner.button(core::Kind::Stop, &running(), Err("refused".into()));
    assert_eq!(reply.unwrap_err(), "refused");
    assert_eq!(
        inner.view(None).workspace_error.as_deref(),
        Some(mobile::WORKSPACE_UNCONFIRMED)
    );
    inner.answer_for_test(running());
    assert_eq!(log.saved().len(), 1, "the same generation re-armed");
    assert_eq!(inner.view(None).autostart, Some(Line::RunningNotArmed));
}

// zerus-7rkq9: a definite refusal means nothing happened, so nothing the person
// chose before pressing Stop may change. The disarm is still saved first, for a crash.
#[test]
fn a_refused_stop_restores_the_armed_choice() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
    let reply = inner.button(
        core::Kind::Stop,
        &running(),
        Err(mobile::Failure::StopRefused),
    );
    assert_eq!(reply.unwrap_err(), mobile::STOP_REFUSED);
    let saved = log.saved();
    assert_eq!(saved.len(), 2, "disarm, then the restored choice");
    assert_eq!(saved[0]["armed"], false);
    assert_eq!(saved[1]["armed"], true);
    assert!(saved[1].get("stopped_generation").is_none());
    assert_eq!(
        inner.view(None).workspace_error.as_deref(),
        Some(mobile::STOP_REFUSED)
    );
    inner.answer_for_test(running());
    assert_eq!(inner.view(None).autostart, Some(Line::Running));
    inner.answer_for_test(stopped());
    assert_eq!(inner.view(None).autostart, Some(Line::WillStart));
    assert_eq!(log.saved().len(), 2, "a later read changed the choice");
}

#[test]
fn a_refused_stop_keeps_an_earlier_stop_of_another_generation() {
    let earlier = format!(
        r#"{{"version":1,"node_id":"node-a","armed":false,"stopped_generation":"{LATER}"}}"#
    );
    let (inner, log) = fixture(Some(&earlier), vec![Ok(running())]);
    let _ = inner.button(
        core::Kind::Stop,
        &running(),
        Err(mobile::Failure::StopRefused),
    );
    let saved = log.saved();
    assert_eq!(saved.last().unwrap()["armed"], false);
    assert_eq!(saved.last().unwrap()["stopped_generation"], LATER);
}

#[test]
fn any_other_stop_failure_is_not_a_refusal_and_stays_disarmed() {
    // The refusal's own words as text are still not the closed result.
    for error in [
        "refused",
        mobile::WORKSPACE_UNCONFIRMED,
        "timed out",
        mobile::STOP_REFUSED,
    ] {
        let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
        let _ = inner.button(core::Kind::Stop, &running(), Err(error.into()));
        assert_eq!(log.saved().len(), 1, "{error}");
        assert_eq!(log.saved()[0]["armed"], false, "{error}");
    }
}

#[test]
fn only_a_new_start_or_later_generation_arms_after_a_stop() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    inner
        .button(
            core::Kind::Stop,
            &running(),
            stop_answer(stopped(), "Connector stopped."),
        )
        .unwrap();
    inner
        .button(core::Kind::Start, &stopped(), stop_answer(running(), ""))
        .unwrap();
    assert_eq!(log.saved().len(), 2);
    assert_eq!(log.saved()[1]["armed"], true);
    assert_eq!(inner.view(None).autostart, Some(Line::Running));

    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    inner
        .button(
            core::Kind::Stop,
            &running(),
            stop_answer(stopped(), "Connector stopped."),
        )
        .unwrap();
    inner.answer_for_test(running_as(LATER));
    assert_eq!(log.saved()[1]["armed"], true);
}

#[test]
fn a_disarm_that_could_not_be_saved_is_surfaced_stops_anyway_and_is_saved_again() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
    log.save_fails.store(true, Ordering::SeqCst);
    let target = core::admit(&running(), core::Kind::Stop).unwrap();
    assert!(inner.page().read_workspace());
    let during = Arc::clone(&inner);
    let sent = Arc::clone(&log);
    inner
        .connector_run(&target, core::Kind::Stop, |_, _| {
            assert!(sent.saved().is_empty());
            assert!(during.view(None).autostart_save_failed, "not surfaced");
            stop_answer(stopped(), "Connector stopped.")
        })
        .unwrap();
    // The answer that ended the Stop saved it, and the warning goes.
    assert_eq!(log.saved().len(), 1);
    assert_eq!(log.saved()[0]["armed"], false);
    assert!(!inner.view(None).autostart_save_failed);
}

#[test]
fn an_arm_write_still_in_progress_cannot_land_after_a_stop() {
    use std::sync::mpsc::channel;
    let (inner, log) = fixture(None, vec![Ok(running())]);
    let (began, write_began) = channel();
    let (release, released) = channel();
    *log.save_gate.lock().unwrap() = Some((began, released));

    // A status read arms the connector, and its file write stalls.
    let arming = {
        let inner = Arc::clone(&inner);
        std::thread::spawn(move || inner.answer_for_test(running()))
    };
    write_began.recv().unwrap();

    // The person chooses Stop while that write is still on its way to the disk.
    let stopping = {
        let inner = Arc::clone(&inner);
        std::thread::spawn(move || {
            let answer = stop_answer(stopped(), "Connector stopped.");
            inner.button(core::Kind::Stop, &running(), answer).unwrap();
        })
    };
    // Prove the Stop was admitted before judging anything: it has disarmed in
    // memory, however slowly its thread was scheduled.
    let admitted = (0..5000).any(|_| {
        let disarmed = inner.view(None).autostart == Some(Line::RunningNotArmed);
        if !disarmed {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        disarmed
    });
    // From here, without ordered saves the Stop's write would begin at once.
    let overtaken = admitted
        && (0..200).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(1));
            log.save_entered.load(Ordering::SeqCst) > 1
        });

    // Always drain both threads, so a failure below reports instead of hanging.
    release.send(()).unwrap();
    arming.join().unwrap();
    stopping.join().unwrap();
    assert!(admitted, "the Stop was never admitted");
    assert!(
        !overtaken,
        "the Stop's write began before the older one ended"
    );
    let saved = log.saved();
    assert_eq!(
        saved.last().unwrap()["armed"],
        false,
        "the older arm landed last: {saved:?}"
    );
    assert_eq!(inner.view(None).autostart, Some(Line::StoppedByYou));
    assert!(!inner.view(None).autostart_save_failed);
}

#[test]
fn a_changed_workspace_inherits_nothing() {
    let other = read_with(serde_json::json!({"node_id": "node-b"}));
    let (inner, log) = fixture(Some(ARMED), vec![Ok(other)]);
    inner.autostart_once();
    assert_eq!(log.counts(), (1, 0), "B was started by A's choice");
    assert!(log.saved().is_empty());
    assert_eq!(inner.view(None).autostart, Some(Line::NotArmed));
}

#[test]
fn a_workspace_change_whose_stop_is_refused_restores_the_armed_choice() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
    inner.answer_for_test(running());
    let target = Target::capture(&running()).unwrap();
    inner.enrollment_disarm(&target);
    assert_eq!(log.saved()[0]["armed"], false);
    let _ = inner.enrollment_call_failed(&target, mobile::Failure::StopRefused);
    assert_eq!(log.saved().last().unwrap()["armed"], true);
    assert_eq!(
        inner.view(None).workspace_error.as_deref(),
        Some(mobile::STOP_REFUSED)
    );
}

#[test]
fn a_workspace_change_that_fails_any_other_way_stays_disarmed() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
    inner.answer_for_test(running());
    let target = Target::capture(&running()).unwrap();
    inner.enrollment_disarm(&target);
    let _ = inner.enrollment_call_failed(&target, "anything else".into());
    assert_eq!(log.saved().len(), 1);
    assert_eq!(
        inner.view(None).workspace_error.as_deref(),
        Some(mobile::WORKSPACE_UNCONFIRMED)
    );
}

#[test]
fn a_workspace_change_that_stops_the_connector_disarms_it_first() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
    let target = mobile::enrollment::Target::capture(&running()).unwrap();
    inner.enrollment_disarm(&target);
    assert_eq!(log.saved().len(), 1);
    assert_eq!(log.saved()[0]["armed"], false);
    assert_eq!(log.saved()[0]["stopped_generation"], GENERATION);

    // A stopped connector has nothing to stop, so its choice stays.
    let (inner, log) = fixture(Some(ARMED), vec![Ok(stopped())]);
    let target = mobile::enrollment::Target::capture(&stopped()).unwrap();
    inner.enrollment_disarm(&target);
    assert!(log.saved().is_empty());
}

#[test]
fn the_button_behaves_as_before_where_there_is_no_automatic_start() {
    let inner = Arc::new(Inner::new(
        false,
        super::super::tests::service_calls_for_plans(),
        Box::new(|| {}),
    ));
    inner.autostart_once();
    assert_eq!(inner.view(None).autostart, None);
    assert!(!inner.effects.load(Ordering::SeqCst));
    let reply = inner.button(
        core::Kind::Start,
        &stopped(),
        stop_answer(running(), "started"),
    );
    assert_eq!(reply.unwrap(), "started");
    assert_eq!(inner.view(None).autostart, None);
    let reply = inner.button(
        core::Kind::Stop,
        &running(),
        stop_answer(stopping(), "draining"),
    );
    assert_eq!(reply.unwrap(), "draining");
    let reply = inner.button(core::Kind::Stop, &running(), Err("refused".into()));
    assert_eq!(reply.unwrap_err(), "refused");
    assert_eq!(
        inner.view(None).workspace_error.as_deref(),
        Some(mobile::WORKSPACE_UNCONFIRMED)
    );
    assert!(!inner.view(None).autostart_save_failed);
}

#[test]
fn only_the_installed_windows_instance_owner_outside_a_replay_may_start_it() {
    assert!(allowed(true, true, false, true));
    for (windows, installed, replay, owner) in [
        (false, true, false, true),
        (true, false, false, true),
        (true, true, true, true),
        (true, true, false, false),
    ] {
        assert!(
            !allowed(windows, installed, replay, owner),
            "windows {windows} installed {installed} replay {replay} owner {owner}"
        );
    }
}

fn opened(inner: &Arc<Inner>, workspace: &Workspace, kind: core::Kind) -> String {
    inner.answer_for_test(workspace.clone());
    let target = core::admit(workspace, kind).unwrap();
    inner.connector_open(&target, kind).unwrap().handle
}

#[test]
fn a_stop_cannot_be_admitted_during_a_status_read_so_no_older_reply_can_follow_it() {
    let (inner, log) = fixture(Some(ARMED), vec![Ok(running())]);
    let handle = opened(&inner, &running(), core::Kind::Stop);
    assert!(inner.page().read_workspace());
    let refused = inner.connector_apply(&handle, |_, _| panic!("sent during a read"));
    assert_eq!(refused.unwrap_err(), core::CHANGED);
    assert!(log.saved().is_empty(), "disarmed without a Stop");
    assert!(
        !inner.effects.load(Ordering::SeqCst),
        "the lane stayed taken"
    );
}

#[test]
fn the_button_starts_and_stops_through_its_one_use_plan() {
    let (inner, log) = fixture(None, vec![Ok(stopped())]);
    let handle = opened(&inner, &stopped(), core::Kind::Start);
    let notice = inner
        .connector_apply(&handle, |target, kind| {
            assert!(kind == core::Kind::Start);
            assert_eq!(target.node_id, NODE);
            stop_answer(running(), "")
        })
        .unwrap();
    assert_eq!(notice, "");
    assert_eq!(log.saved()[0]["armed"], true);
    assert!(!inner.effects.load(Ordering::SeqCst));
    assert_eq!(
        inner
            .connector_apply(&handle, |_, _| panic!("a plan ran twice"))
            .unwrap_err(),
        core::CHANGED
    );

    let handle = opened(&inner, &running(), core::Kind::Stop);
    let notice = inner
        .connector_apply(&handle, |target, kind| {
            assert!(kind == core::Kind::Stop);
            assert_eq!(target.generation.as_deref(), Some(GENERATION));
            stop_answer(stopped(), "Connector stopped.")
        })
        .unwrap();
    assert_eq!(notice, "Connector stopped.");
    assert_eq!(log.saved()[1]["armed"], false);
    assert_eq!(inner.view(None).autostart, Some(Line::StoppedByYou));
    assert_eq!(log.counts(), (0, 0), "the button ran an automatic call");
}

#[test]
fn a_record_is_read_only_up_to_what_a_real_one_may_hold() {
    let path = std::env::temp_dir().join(format!(
        "zerus-autostart-record-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    assert_eq!(read_record(&path), None, "a missing record");
    std::fs::write(&path, vec![b'x'; MAX_RECORD * 4]).unwrap();
    assert_eq!(read_record(&path).unwrap().len(), MAX_RECORD + 1);
    std::fs::remove_file(&path).unwrap();
    let dir = Path::new("data");
    assert_eq!(record_path(dir), dir.join(FILE));
}

/// Waits until no status read runs.
fn settled(inner: &Inner) {
    for _ in 0..5000 {
        if !inner.view(None).workspace_busy {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("still reading");
}
