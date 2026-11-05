//! Unix file/lock/loan transition models. Native process/COM observations are
//! modeled, not Windows authentication or installed refresh qualification.
use super::*;
use crate::{desktop::windows_mode_stop, platform};
use std::{
    fs,
    sync::Arc,
    time::{Duration, Instant},
};

struct Model {
    root: tempfile::TempDir,
    plan: Plan,
    owner: Arc<platform::FileLock>,
    loan: Option<Arc<platform::FileLock>>,
    pipe: bool,
    operations: bool,
    current: bool,
    exited: bool,
    started: String,
    next: ObservedInstance,
    phase: Budget,
    deadlines: budget::Deadlines,
    base: Instant,
    elapsed: Duration,
    schedule: Option<Schedule>,
    reads: usize,
    stops: usize,
    starts: usize,
    released: bool,
    fail_stop: bool,
    fail_start: bool,
    fail_final_post_persist: bool,
    trace: Vec<String>,
}
#[derive(Clone, Copy, Default)]
struct Schedule {
    pause_ms: u64,
    offer_ms: u64,
    stop_ms: u64,
    exit_polls: usize,
    stopped_fence_ms: u64,
}
impl Model {
    fn new() -> Self {
        let base = Instant::now();
        Self {
            root: tempfile::tempdir().unwrap(),
            plan: super::super::tests::plan(),
            owner: Arc::new(platform::FileLock::exclusive(tempfile::tempfile().unwrap()).unwrap()),
            loan: None,
            pipe: false,
            operations: false,
            current: true,
            exited: true,
            started: super::super::tests::id(),
            next: super::super::tests::instance(24),
            phase: Budget::Preflight,
            deadlines: budget::Deadlines::new(base, base + Budget::exchange_timeout()).unwrap(),
            base,
            elapsed: Duration::ZERO,
            schedule: None,
            reads: 0,
            stops: 0,
            starts: 0,
            released: false,
            fail_stop: false,
            fail_start: false,
            fail_final_post_persist: false,
            trace: Vec::new(),
        }
    }
    fn now(&self) -> Instant {
        self.base + self.elapsed
    }
    fn source(&self) -> bool {
        Arc::strong_count(&self.owner) == 1 && !self.pipe && !self.operations
    }
    fn durable(&self) -> Record {
        Record::decode(
            &fs::read(self.root.path().join(windows_mode_stop::RECORD)).unwrap(),
            &self.plan,
        )
        .unwrap()
    }
}
impl Session for Model {
    type Started = String;
    fn phase(&mut self, phase: Budget) -> Result<()> {
        let now = self.now();
        self.deadlines.enter(phase, now)?;
        self.phase = phase;
        self.reads = 0;
        self.trace.push(format!("{phase:?}"));
        Ok(())
    }
    fn check(&mut self) -> Result<()> {
        self.deadlines.check(self.now())?;
        if self.reads >= 5 && !self.source() {
            anyhow::bail!("original modeled resource still held");
        }
        if self.schedule.is_none() && self.reads >= 5 && self.stops != 0 && !self.exited {
            anyhow::bail!("original modeled physical exit unconfirmed");
        }
        Ok(())
    }
    fn save(&mut self, record: &Record) -> Result<()> {
        let root = self.root.path().to_owned();
        let bytes = record.encode(&self.plan)?;
        let final_save = record.pending.is_none()
            && record
                .refreshes
                .last()
                .is_some_and(|r| r.phase == ChoicePhase::Settled);
        let stopped_save = record
            .pending
            .as_ref()
            .is_some_and(|p| p.phase == Phase::Stopped);
        let mut calls = 0;
        windows_mode_stop::publish(
            &root,
            &mut || {
                calls += 1;
                if stopped_save {
                    self.elapsed +=
                        Duration::from_millis(self.schedule.unwrap_or_default().stopped_fence_ms);
                }
                self.check()?;
                ensure!(
                    !(final_save && self.fail_final_post_persist && calls == 3),
                    "modeled final post-persist fence failed"
                );
                Ok(())
            },
            || Ok(bytes),
        )
    }
    fn hold(&mut self) -> Result<()> {
        self.check()
    }
    fn drained(&mut self) -> Result<bool> {
        self.reads += 1;
        Ok(self.source())
    }
    fn pause(&mut self, _request: &str) {
        self.elapsed += Duration::from_millis(self.schedule.unwrap_or_default().pause_ms);
    }
    fn offer_stop(&mut self, record: &mut Record) -> Result<()> {
        self.elapsed += Duration::from_millis(self.schedule.unwrap_or_default().offer_ms);
        let current = self.current;
        let root = self.root.path().to_owned();
        let plan = self.plan.clone();
        windows_mode_stop::offer(
            record,
            &current,
            &mut || self.check(),
            |current, check| {
                check()?;
                ensure!(*current, "original modeled selection changed");
                Ok(())
            },
            |record, check| windows_mode_stop::publish(&root, check, || record.encode(&plan)),
        )
    }
    fn stop(&mut self) -> Result<()> {
        self.check()?;
        self.stops += 1;
        self.elapsed += Duration::from_millis(self.schedule.unwrap_or_default().stop_ms);
        ensure!(!self.fail_stop, "modeled original Stop reply lost");
        self.check()
    }
    fn stopped(&mut self) -> Result<bool> {
        self.reads += 1;
        if let Some(schedule) = self.schedule {
            self.elapsed += Duration::from_millis(100);
            self.exited = self.reads >= schedule.exit_polls;
            self.check()?;
        }
        Ok(self.exited)
    }
    fn start(&mut self) -> Result<Self::Started> {
        self.starts += 1;
        ensure!(!self.fail_start, "modeled original RunEx reply lost");
        Ok(self.started.clone())
    }
    fn guid(started: &Self::Started) -> &str {
        started
    }
    fn ready(&mut self, started: &mut Self::Started) -> Result<Option<Ready>> {
        ensure!(*started == self.started, "original modeled start changed");
        self.next.guid = started.clone();
        Ok(Some(Ready {
            instance: self.next.clone(),
            generation: 9,
        }))
    }
    fn release(&mut self) -> Result<()> {
        self.check()?;
        self.released = true;
        Ok(())
    }
}
fn reserved(model: &mut Model) -> (Record, Reserved) {
    let mut record = Record::empty(&model.plan).unwrap();
    let token = record
        .reserve_refresh(
            &super::super::tests::id(),
            super::super::tests::instance(23),
            super::super::tests::scope(),
            super::super::tests::consumer(),
        )
        .unwrap();
    model.save(&record).unwrap();
    (record, token)
}

#[test]
fn the_actual_engine_keeps_each_original_resource_ahead_of_every_stop() {
    for resource in 0..3 {
        let mut model = Model::new();
        let (mut record, token) = reserved(&mut model);
        match resource {
            0 => model.loan = Some(model.owner.clone()),
            1 => model.pipe = true,
            _ => model.operations = true,
        }
        let error = run(&mut model, &mut record, token).unwrap_err();
        assert!(
            error.to_string().contains("resource still held"),
            "{error:#}"
        );
        assert_eq!((model.stops, model.starts, model.released), (0, 0, false));
        assert_eq!(model.durable().pending.unwrap().phase, Phase::Prepared);
        assert!(model.loan.is_none() || Arc::ptr_eq(&model.owner, model.loan.as_ref().unwrap()));
    }
}
#[test]
fn no_start_or_release_precedes_the_original_physical_exit_observation() {
    let mut model = Model::new();
    let (mut record, token) = reserved(&mut model);
    model.exited = false;
    let error = run(&mut model, &mut record, token).unwrap_err();
    assert!(
        error.to_string().contains("physical exit unconfirmed"),
        "{error:#}"
    );
    assert_eq!((model.stops, model.starts, model.released), (1, 0, false));
    assert_eq!(model.durable().pending.unwrap().phase, Phase::StopOffered);
    assert_eq!(model.durable().refreshes[0].phase, ChoicePhase::Unknown);
}
#[test]
fn selection_refusal_is_before_publication_and_deadline_expiry_never_restarts_the_budget() {
    let mut model = Model::new();
    let (mut record, token) = reserved(&mut model);
    model.current = false;
    let error = run(&mut model, &mut record, token).unwrap_err();
    assert!(error.to_string().contains("selection changed"), "{error:#}");
    assert_eq!((model.stops, model.starts, model.released), (0, 0, false));
    assert_eq!(model.durable().pending.unwrap().phase, Phase::Held);
    let mut model = Model::new();
    let (mut record, token) = reserved(&mut model);
    model.elapsed += Duration::from_secs(2);
    let error = run(&mut model, &mut record, token).unwrap_err();
    assert!(error.to_string().contains("deadline expired"), "{error:#}");
    assert_eq!((model.stops, model.starts, model.released), (0, 0, false));
    assert_eq!(model.durable().pending.unwrap().phase, Phase::Prepared);
}
#[test]
fn one_retained_start_settles_without_any_update_or_mode_history_rewrite() {
    let mut model = Model::new();
    let (mut record, token) = reserved(&mut model);
    let old_completed = record.completed.clone();
    let old_choices = record.choices.clone();
    run(&mut model, &mut record, token).unwrap();
    assert_eq!((model.stops, model.starts, model.released), (1, 1, true));
    assert_eq!(
        model.trace,
        ["Hold", "Stop", "Start", "Readiness", "Release"]
    );
    assert_eq!(record.completed, old_completed);
    assert_eq!(record.choices, old_choices);
    let settled = model.durable();
    assert_eq!(settled.refreshes[0].phase, ChoicePhase::Settled);
    assert_eq!(
        settled.refreshes[0].ready.as_ref().unwrap().instance.guid,
        model.started
    );
    assert!(settled.pending.is_none());
    assert!(record
        .reserve_refresh(
            &settled.refreshes[0].request,
            super::super::tests::instance(23),
            super::super::tests::scope(),
            super::super::tests::consumer()
        )
        .is_err());
}

#[test]
fn uncertain_stop_and_start_stay_unknown_and_cannot_create_another_reservation() {
    for start in [false, true] {
        let mut model = Model::new();
        let (mut record, token) = reserved(&mut model);
        let request = record.pending.as_ref().unwrap().request_id.clone();
        model.fail_stop = !start;
        model.fail_start = start;
        let error = run(&mut model, &mut record, token).unwrap_err();
        assert!(error.to_string().contains("reply lost"), "{error:#}");
        assert_eq!(
            (model.stops, model.starts, model.released),
            (1, usize::from(start), false)
        );
        let mut original = model.durable();
        assert_eq!(
            original.pending.as_ref().unwrap().phase,
            if start {
                Phase::StartOffered
            } else {
                Phase::StopOffered
            }
        );
        let bytes = original.encode(&model.plan).unwrap();
        assert!(original.choice(None).is_err());
        assert!(original
            .reserve_refresh(
                &request,
                super::super::tests::instance(23),
                super::super::tests::scope(),
                super::super::tests::consumer()
            )
            .is_err());
        assert_eq!(original.encode(&model.plan).unwrap(), bytes);
        assert_eq!(original.refreshes[0].phase, ChoicePhase::Unknown);
    }
}
#[test]
fn the_local_reservation_is_bound_to_the_entire_original_record_and_configuration() {
    let mut model = Model::new();
    let (mut record, token) = reserved(&mut model);
    record.configuration = "f".repeat(64);
    let error = run(&mut model, &mut record, token).unwrap_err();
    assert!(
        error.to_string().contains("reservation changed"),
        "{error:#}"
    );
    assert_eq!((model.stops, model.starts, model.released), (0, 0, false));
    assert!(model.trace.is_empty());
}

#[test]
fn a_final_post_persist_failure_reads_the_original_settled_result_never_a_refusal() {
    let mut model = Model::new();
    let (mut record, token) = reserved(&mut model);
    let id = record.pending.as_ref().unwrap().request_id.clone();
    model.fail_final_post_persist = true;
    let error = run(&mut model, &mut record, token).unwrap_err();
    assert!(
        error.to_string().contains("final post-persist"),
        "{error:#}"
    );
    assert_eq!((model.stops, model.starts, model.released), (1, 1, true));
    let durable = model.durable();
    assert!(durable.pending.is_none());
    let receipt = durable.refreshes[0].clone();
    assert_eq!(receipt.phase, ChoicePhase::Settled);
    let observed = observation::read(Some(&durable), &durable.configuration, &id, None);
    let recovered = observation::after_error(observed, &error.to_string());
    assert_eq!(
        recovered["receipt"],
        serde_json::to_value(&receipt).unwrap()
    );
    assert_eq!(recovered["request_id"], id);
    assert_eq!(recovered["outcome"], "settled");
    assert_eq!(recovered["read_only"], true);
    assert_eq!(recovered["ok"], true);
    assert!(recovered["changed"].is_null() && recovered["manager_changed"].is_null());
    for (record, loss) in [(Some(&durable), Some(id.as_str())), (None, None)] {
        let observed = observation::read(record, &durable.configuration, &id, loss);
        let recovered = observation::after_error(observed, &error.to_string());
        assert_eq!(recovered["outcome"], "unknown");
        assert_eq!(recovered["request_id"], id);
        assert_eq!(recovered["ok"], false);
        assert!(recovered["receipt"].is_null());
        assert!(recovered["changed"].is_null() && recovered["manager_changed"].is_null());
    }
    assert_eq!(
        (model.stops, model.starts, model.released),
        (1, 1, true),
        "observation re-executed an effect"
    );
}

#[test]
fn the_same_stop_deadline_covers_pause_checks_exit_polls_and_stopped_publication() {
    // Deterministic virtual elapsed time drives the SAME deadline policy and
    // actual engine/file publisher. Process/COM costs remain modeled only.
    let mut model = Model::new();
    let (mut record, token) = reserved(&mut model);
    model.schedule = Some(Schedule {
        offer_ms: 800,
        stop_ms: 70,
        exit_polls: 12,
        stopped_fence_ms: 100,
        ..Default::default()
    });
    run(&mut model, &mut record, token).unwrap();
    assert_eq!(model.elapsed, Duration::from_millis(2370));
    assert_eq!((model.stops, model.starts, model.released), (1, 1, true));
    assert_eq!(
        model.trace,
        ["Hold", "Stop", "Start", "Readiness", "Release"]
    );
    assert_eq!(model.durable().refreshes[0].phase, ChoicePhase::Settled);

    for (schedule, phase, stops, exited) in [
        (
            Schedule {
                pause_ms: 4000,
                ..Default::default()
            },
            Phase::Held,
            0,
            true,
        ),
        (
            Schedule {
                offer_ms: 800,
                stop_ms: 70,
                exit_polls: 50,
                ..Default::default()
            },
            Phase::StopOffered,
            1,
            false,
        ),
        (
            Schedule {
                offer_ms: 800,
                stop_ms: 70,
                exit_polls: 12,
                stopped_fence_ms: 2000,
                ..Default::default()
            },
            Phase::StopOffered,
            1,
            true,
        ),
        (
            Schedule {
                offer_ms: 800,
                stop_ms: 70,
                exit_polls: 12,
                stopped_fence_ms: 700,
                ..Default::default()
            },
            Phase::Stopped,
            1,
            true,
        ),
    ] {
        let mut model = Model::new();
        let (mut record, token) = reserved(&mut model);
        let id = record.pending.as_ref().unwrap().request_id.clone();
        model.schedule = Some(schedule);
        let error = run(&mut model, &mut record, token).unwrap_err();
        assert!(error.to_string().contains("deadline expired"), "{error:#}");
        assert_eq!(
            (model.stops, model.starts, model.released),
            (stops, 0, false)
        );
        assert_eq!(model.exited, exited);
        let mut durable = model.durable();
        assert_eq!(durable.pending.as_ref().unwrap().phase, phase);
        assert_eq!(durable.pending.as_ref().unwrap().request_id, id);
        assert_eq!(durable.refreshes[0].phase, ChoicePhase::Unknown);
        assert_eq!(
            model.trace,
            ["Hold", "Stop"],
            "an expired phase was renewed"
        );
        assert!(durable.choice(None).is_err());
        assert!(durable
            .reserve_refresh(
                &id,
                super::super::tests::instance(23),
                super::super::tests::scope(),
                super::super::tests::consumer()
            )
            .is_err());
    }
}
