use super::*;
use crate::desktop::windows_registration_policy::{
    self as policy,
    mode::{Budget, Mode},
};
use anyhow::bail;
use std::{
    cell::{Cell, RefCell},
    fs,
    time::{Duration, Instant},
};

fn plan() -> policy::Plan {
    policy::Plan::parse(&serde_json::json!({"schema":1,"operation":"check","plan":{
        "schema":1,"operation":"preview","label":"hgs-desktop","configuration":"a".repeat(64),
        "user_sid":"S-1-5-21-1001","task_name":"hgs-desktop-S-1-5-21-1001",
        "definition":"synthetic definition",
        "argv":["C:/Example/hgs.exe","desktop","serve","--config-dir","C:/Example/config","--state-dir","C:/Example/state"],
        "session":{"kind":"interactive_user","allow_session_zero":false}
    }}), "S-1-5-21-1001").unwrap()
}
fn held() -> Record {
    let mut record = Record::empty(&plan()).unwrap();
    record
        .begin(
            Mode::Background,
            Some(policy::mode::ObservedInstance {
                guid: "11111111-1111-4111-8111-111111111111".into(),
                pid: 4242,
                birth: 17,
                epoch: "22222222-2222-4222-8222-222222222222".into(),
            }),
        )
        .unwrap();
    record.advance(Phase::Prepared, Phase::Held).unwrap();
    record
}
fn save(root: &Path, record: &Record, check: &mut dyn FnMut() -> Result<()>) -> Result<()> {
    publish(root, check, || record.encode(&plan()))
}
fn read(root: &Path) -> Record {
    Record::decode(&fs::read(root.join(RECORD)).unwrap(), &plan()).unwrap()
}

// Original is a model only, never a native Running, Caller or Stop capability.
#[derive(Default)]
struct Original {
    stopped: bool,
}
#[derive(Default)]
struct Model {
    checks: Cell<usize>,
    stops: Cell<usize>,
    trace: RefCell<Vec<&'static str>>,
}
impl Model {
    fn check(
        &self,
        _: &Original,
        fence: &mut impl FnMut() -> Result<()>,
        refuse: usize,
    ) -> Result<()> {
        fence()?;
        let count = self.checks.get() + 1;
        self.checks.set(count);
        self.trace.borrow_mut().push("check original");
        ensure!(count != refuse, "selection changed");
        fence()
    }
    fn stop(&self, original: &mut Original, fence: &mut impl FnMut() -> Result<()>) -> Result<()> {
        // Model the still-separate original Running::stop self-check and fence.
        fence()?;
        ensure!(!original.stopped, "stop already offered");
        original.stopped = true;
        self.trace.borrow_mut().push("original Stop");
        self.stops.set(self.stops.get() + 1);
        fence()
    }
}
fn run(
    root: &Path,
    record: &mut Record,
    original: &mut Original,
    model: &Model,
    refuse: usize,
    until: Instant,
    lose_reply: bool,
) -> Result<()> {
    let mut fence = || {
        ensure!(Instant::now() < until, "deadline expired");
        Ok(())
    };
    offer(
        record,
        original,
        &mut fence,
        |original, fence| model.check(original, fence, refuse),
        |record, check| {
            save(root, record, check)?;
            model.trace.borrow_mut().push("durable StopOffered");
            if lose_reply {
                bail!("publication reply lost");
            }
            Ok(())
        },
    )?;
    checked(
        original,
        &mut fence,
        |original, fence| model.check(original, fence, refuse),
        |original, fence| model.stop(original, fence),
    )
}
#[test]
fn stable_original_is_checked_before_publication_and_again_before_its_one_stop() {
    let root = tempfile::tempdir().unwrap();
    let mut record = held();
    save(root.path(), &record, &mut || Ok(())).unwrap();
    let model = Model::default();
    let mut original = Original::default();
    run(
        root.path(),
        &mut record,
        &mut original,
        &model,
        0,
        Budget::Stop.deadline(Instant::now()),
        false,
    )
    .unwrap();
    assert_eq!(model.checks.get(), 5);
    assert_eq!(model.stops.get(), 1);
    assert_eq!(
        *model.trace.borrow(),
        vec![
            "check original",
            "check original",
            "check original",
            "check original",
            "durable StopOffered",
            "check original",
            "original Stop"
        ]
    );
    assert_eq!(read(root.path()).pending.unwrap().phase, Phase::StopOffered);
}
#[test]
fn early_selection_refusal_keeps_the_original_held_bytes_and_has_no_stop() {
    refusal(1, Phase::Held, Phase::Held);
}
#[test]
fn pre_publication_selection_refusal_keeps_durable_held_even_after_memory_advanced() {
    for at in [2, 3] {
        refusal(at, Phase::Held, Phase::StopOffered);
    }
}
#[test]
fn post_persist_callback_failure_keeps_actual_stop_offered_and_cannot_cancel() {
    refusal(4, Phase::StopOffered, Phase::StopOffered);
}
#[test]
fn final_pre_stop_selection_refusal_keeps_the_published_unknown_and_has_no_stop() {
    refusal(5, Phase::StopOffered, Phase::StopOffered);
}
fn refusal(at: usize, durable: Phase, memory: Phase) {
    let root = tempfile::tempdir().unwrap();
    let mut record = held();
    let original_record = record.clone();
    save(root.path(), &record, &mut || Ok(())).unwrap();
    let before = fs::read(root.path().join(RECORD)).unwrap();
    let model = Model::default();
    let mut original = Original::default();
    let error = run(
        root.path(),
        &mut record,
        &mut original,
        &model,
        at,
        Budget::Stop.deadline(Instant::now()),
        false,
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "selection changed",
        "deadline must not mask selection"
    );
    assert_eq!(model.stops.get(), 0);
    assert!(!original.stopped);
    assert_eq!(model.checks.get(), at);
    assert_eq!(record.pending.as_ref().unwrap().phase, memory);
    let mut reopened = read(root.path());
    let pending = reopened.pending.as_ref().unwrap();
    assert_eq!(pending.phase, durable);
    assert_eq!(
        pending.request_id,
        original_record.pending.as_ref().unwrap().request_id
    );
    assert_eq!(
        pending.original,
        original_record.pending.as_ref().unwrap().original
    );
    assert_eq!(reopened.configuration, original_record.configuration);
    assert_eq!(reopened.command, original_record.command);
    let id = pending.request_id.clone();
    if durable == Phase::Held {
        assert_eq!(fs::read(root.path().join(RECORD)).unwrap(), before);
        assert!(!reopened.cancellation_requires_grant(&id).unwrap());
        reopened.cancel(&id).unwrap();
        assert_eq!(reopened.cancelled.as_deref(), Some(id.as_str()));
    } else {
        let bytes = reopened.encode(&plan()).unwrap();
        assert!(reopened.cancel(&id).is_err());
        assert_eq!(reopened.encode(&plan()).unwrap(), bytes);
    }
}
#[test]
fn lost_publication_reply_does_not_restore_held_or_issue_a_stop() {
    let root = tempfile::tempdir().unwrap();
    let mut record = held();
    save(root.path(), &record, &mut || Ok(())).unwrap();
    let id = record.pending.as_ref().unwrap().request_id.clone();
    let model = Model::default();
    let mut original = Original::default();
    let error = run(
        root.path(),
        &mut record,
        &mut original,
        &model,
        0,
        Budget::Stop.deadline(Instant::now()),
        true,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "publication reply lost");
    assert_eq!(read(root.path()), record);
    assert_eq!(record.pending.as_ref().unwrap().phase, Phase::StopOffered);
    assert!(record.cancel(&id).is_err());
    assert_eq!(model.stops.get(), 0);
}
#[test]
fn an_expired_original_stop_deadline_refuses_before_changing_held() {
    let root = tempfile::tempdir().unwrap();
    let mut record = held();
    let before = record.clone();
    save(root.path(), &record, &mut || Ok(())).unwrap();
    let model = Model::default();
    let mut original = Original::default();
    let error = run(
        root.path(),
        &mut record,
        &mut original,
        &model,
        0,
        Instant::now() - Duration::from_millis(1),
        false,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "deadline expired");
    assert_eq!(record, before);
    assert_eq!(read(root.path()), before);
    assert_eq!(model.stops.get(), 0);
}
#[test]
fn historical_stop_offered_is_never_downgraded_or_reoffered() {
    let root = tempfile::tempdir().unwrap();
    let mut record = held();
    record.advance(Phase::Held, Phase::StopOffered).unwrap();
    save(root.path(), &record, &mut || Ok(())).unwrap();
    let before = fs::read(root.path().join(RECORD)).unwrap();
    let model = Model::default();
    let mut original = Original::default();
    assert!(run(
        root.path(),
        &mut record,
        &mut original,
        &model,
        0,
        Budget::Stop.deadline(Instant::now()),
        false
    )
    .is_err());
    assert_eq!(model.checks.get(), 0);
    assert_eq!(model.stops.get(), 0);
    assert_eq!(record.encode(&plan()).unwrap(), before);
    assert_eq!(fs::read(root.path().join(RECORD)).unwrap(), before);
    let id = record.pending.as_ref().unwrap().request_id.clone();
    assert!(record.cancel(&id).is_err());
    assert_eq!(record.encode(&plan()).unwrap(), before);
}

#[test]
fn post_persist_directory_sync_uncertainty_never_infers_held_or_stops() {
    let root = tempfile::tempdir().unwrap();
    let mut record = held();
    save(root.path(), &record, &mut || Ok(())).unwrap();
    let id = record.pending.as_ref().unwrap().request_id.clone();
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FAIL_DIRECTORY_SYNC.set(false);
        }
    }
    let _reset = Reset;
    FAIL_DIRECTORY_SYNC.set(true);
    let model = Model::default();
    let mut original = Original::default();
    let error = run(
        root.path(),
        &mut record,
        &mut original,
        &model,
        0,
        Budget::Stop.deadline(Instant::now()),
        false,
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "injected directory sync uncertainty");
    // The bytes now observed are StopOffered; no durability is claimed after
    // this injected sync error, and no inferred Held/no-call rewrite is made.
    assert_eq!(read(root.path()), record);
    assert_eq!(record.pending.as_ref().unwrap().phase, Phase::StopOffered);
    assert!(record.cancel(&id).is_err());
    assert_eq!(model.stops.get(), 0);
    assert!(!original.stopped);
}
