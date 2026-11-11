//! Source-flow controls with synthetic opaque programs and an offline model.
//! These are not native custody, installed GUI, or platform qualification.
#![allow(dead_code)]
use std::{
    ops::Deref,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
    },
    thread,
    time::{Duration, Instant},
};

const STATE_CHANGED: &str = "changed";
mod run {
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub enum Retirement {
        Retired,
        Pending,
        Failed,
    }
}
mod zerus_client_core {
    pub mod creation {
        pub fn canonical_uuid(s: &str) -> bool {
            s == "11111111-1111-4111-8111-111111111111"
        }
    }
}

#[derive(Clone, Debug, Default)]
struct AppHandle;
struct State<'a, T> {
    inner: &'a T,
}
impl<T> Deref for State<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.inner
    }
}
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap()
}
static WATCHES: OnceLock<Mutex<Vec<thread::JoinHandle<()>>>> = OnceLock::new();
static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
/// A failed control must not fail the next ones through a poisoned lock.
fn serial() -> MutexGuard<'static, ()> {
    let guard = SERIAL
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    WATCHES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    guard
}
mod tauri {
    pub mod async_runtime {
        pub fn spawn_blocking(f: impl FnOnce() + Send + 'static) {
            let h = std::thread::spawn(f);
            super::super::WATCHES
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(h);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Capture;
mod program_pipeline {
    use super::*;
    #[derive(Clone, Debug, PartialEq)]
    pub(super) struct ProgramReservation {
        pub revision: u64,
        pub request_id: String,
        pub binding: String,
        pub capture: Capture,
    }
    pub(super) struct ProgramJob(pub Arc<Control>);
    impl ProgramJob {
        pub(super) fn identity(&self) -> (String, String, Capture) {
            ("request".into(), "binding".into(), Capture)
        }
        pub(super) fn matches(&self, r: &ProgramReservation) -> bool {
            let (a, b, c) = self.identity();
            r.request_id == a && r.binding == b && r.capture == c
        }
        pub(super) fn current(&self) -> bool {
            self.0.current.load(Ordering::Acquire)
        }
        pub(super) fn holder_unavailable(&self) -> bool {
            false
        }
        pub(super) fn remaining_millis(&self) -> u64 {
            0
        }
        pub(super) fn normal_refusal(&self) -> bool {
            true
        }
        pub(super) fn reviewable_refusal(&self) -> bool {
            self.0.reviewable.load(Ordering::Acquire)
        }
        pub(super) fn physical_retirement(&self) -> run::Retirement {
            self.0.physical_calls.fetch_add(1, Ordering::AcqRel);
            if self.0.failed.load(Ordering::Acquire) {
                run::Retirement::Failed
            } else if self.0.retired.load(Ordering::Acquire) {
                run::Retirement::Retired
            } else {
                run::Retirement::Pending
            }
        }
        pub(super) fn retirement(&self) -> run::Retirement {
            self.0.retirement_calls.fetch_add(1, Ordering::AcqRel);
            if !self.0.entered_once.swap(true, Ordering::AcqRel) {
                self.0.current.store(false, Ordering::Release);
                if let Some(entered) = &self.0.entered {
                    entered.send(()).unwrap();
                }
                if let Some(release) = self.0.release.lock().unwrap().as_ref() {
                    release.recv().unwrap();
                }
            }
            if self.0.failed.load(Ordering::Acquire) {
                run::Retirement::Failed
            } else if self.0.retired.load(Ordering::Acquire) {
                run::Retirement::Retired
            } else {
                run::Retirement::Pending
            }
        }
    }
}
struct Control {
    current: AtomicBool,
    retired: AtomicBool,
    failed: AtomicBool,
    reviewable: AtomicBool,
    entered_once: AtomicBool,
    entered: Option<std::sync::mpsc::Sender<()>>,
    release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    retirement_calls: AtomicUsize,
    physical_calls: AtomicUsize,
}
impl Control {
    fn new(current: bool) -> Self {
        Self {
            current: AtomicBool::new(current),
            retired: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            reviewable: AtomicBool::new(true),
            entered_once: AtomicBool::new(false),
            entered: None,
            release: Mutex::new(None),
            retirement_calls: AtomicUsize::new(0),
            physical_calls: AtomicUsize::new(0),
        }
    }
}
struct Entry {
    machine: String,
}
struct Cleanup;
impl Cleanup {
    fn can_refresh(&self) -> bool {
        true
    }
    fn review_remaining(&mut self) {}
    fn step(&mut self) -> Step {
        Step
    }
}
#[derive(Clone, Copy)]
struct Step;
#[derive(Clone, Copy)]
enum ProgramRetirement {
    Expired,
    SourceUnavailable,
    HolderUnavailable,
}
struct Job {
    id: String,
    busy: bool,
    cleanup: Cleanup,
    programs: Option<program_pipeline::ProgramJob>,
    reservation: Option<program_pipeline::ProgramReservation>,
    manual_review: bool,
    program_review_pending: bool,
    program_retirement: Option<ProgramRetirement>,
    lookup_only: bool,
    physically_released: bool,
    phase: &'static str,
    dismissed: bool,
}
impl Job {
    fn has_programs(&self) -> bool {
        self.programs.is_some()
    }
    fn reviewable_refusal(&self) -> bool {
        self.reservation.is_none()
            && self
                .programs
                .as_ref()
                .is_some_and(program_pipeline::ProgramJob::reviewable_refusal)
    }
}
struct Model {
    jobs: Vec<Job>,
    revision: u64,
}
#[derive(Debug)]
struct View {
    can_check: bool,
    ready: bool,
    can_force: bool,
    busy: bool,
}
impl Model {
    fn unknown_folder(&self, _: &Job) -> bool {
        false
    }
    fn can_dismiss(&self, j: &Job) -> bool {
        !j.busy && !j.program_review_pending && !j.dismissed
    }
    fn view(&self) -> View {
        let job = &self.jobs[0];
        let entry = Entry {
            machine: "11111111-1111-4111-8111-111111111111".into(),
        };
        View {
            can_check: self.can_dismiss(job)
                && !self.unknown_folder(job)
                && !job.busy
                && !job.lookup_only
                && (!job.has_programs() || job.reviewable_refusal())
                && job.cleanup.can_refresh()
                && zerus_client_core::creation::canonical_uuid(&entry.machine),
            ready: self.can_dismiss(job)
                && !self.unknown_folder(job)
                && !job.busy
                && !job.lookup_only
                && !job.has_programs()
                && job.manual_review,
            can_force: false,
            busy: job.busy,
        }
    }
}
fn changed(_: &AppHandle, m: &mut Model) {
    m.revision = m.revision.wrapping_add(1)
}
trait PipelineApp: Clone + Send + 'static {
    fn changed(&self, model: &mut Model);
    fn drive(&self, inner: &Arc<Inner>, id: String, step: Step);
}
impl PipelineApp for AppHandle {
    fn changed(&self, model: &mut Model) {
        changed(self, model)
    }
    fn drive(&self, inner: &Arc<Inner>, id: String, step: Step) {
        inner.drive(self.clone(), id, step, false)
    }
}
struct Inner {
    model: Mutex<Model>,
    drives: AtomicUsize,
}
impl Inner {
    fn drive(self: &Arc<Self>, _: AppHandle, _: String, _: Step, _: bool) {
        self.drives.fetch_add(1, Ordering::AcqRel);
        lock(&self.model).jobs[0].busy = false;
    }
}
#[derive(Clone)]
struct TerminationCleanup(Arc<Inner>);
fn fixture(control: Arc<Control>) -> Arc<Inner> {
    Arc::new(Inner {
        model: Mutex::new(Model {
            jobs: vec![Job {
                id: "job".into(),
                busy: false,
                cleanup: Cleanup,
                programs: Some(program_pipeline::ProgramJob(control)),
                reservation: None,
                manual_review: false,
                program_review_pending: false,
                program_retirement: None,
                lookup_only: false,
                physically_released: false,
                phase: "checking",
                dismissed: false,
            }],
            revision: 10,
        }),
        drives: AtomicUsize::new(0),
    })
}

fn watch_job<A: PipelineApp>(inner: &Arc<Inner>, app: A, id: String) {
    // Expiry is one-way. Retaining a UI row never keeps an Actions claim
    // indefinitely or renews consent after source/authority loss.
    let weak = Arc::downgrade(inner);
    tauri::async_runtime::spawn_blocking(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        let Some(inner) = weak.upgrade() else {
            return;
        };
        match watch_once(&inner, &id, |m| app.changed(m)) {
            Watch::Continue => {}
            Watch::Stop => return,
            Watch::Review(step) => {
                app.drive(&inner, id, step);
                return;
            }
        }
    });
}
enum Watch {
    Continue,
    Stop,
    Review(Step),
}
/// One passive watcher pass. It runs without the app handle so the real
/// program controls can drive it; the result says what the loop does next.
fn watch_once(inner: &Inner, id: &str, emit: impl Fn(&mut Model)) -> Watch {
    let (reservation, program) = {
        let mut m = lock(&inner.model);
        let Some(job) = m.jobs.iter_mut().find(|j| j.id == id) else {
            return Watch::Stop;
        };
        let Some(program) = &job.programs else {
            if job.busy && job.reservation.is_some() && !job.dismissed {
                return Watch::Continue;
            }
            return Watch::Stop;
        };
        if !job.busy && job.program_review_pending && job.reservation.is_some() {
            return Watch::Stop;
        }
        if program.current() {
            // A finished control worker is observable failure data, never a
            // physical-drain proof. Keep the original resources/expiry exactly
            // as before; only disable presentation of a usable Force offer.
            if program.holder_unavailable() && job.program_retirement.is_none() {
                job.program_retirement = Some(ProgramRetirement::HolderUnavailable);
                emit(&mut m);
            }
            return Watch::Continue;
        }
        job.program_retirement
            .get_or_insert(if program.remaining_millis() == 0 {
                ProgramRetirement::Expired
            } else {
                ProgramRetirement::SourceUnavailable
            });
        job.busy = true;
        let reservation = job.reservation.clone();
        let program = job.programs.take().expect("retained program");
        (reservation, program)
    };
    // A reviewable refusal keeps its own retirement and review path. Any other
    // result only waits for its original call, mutation and source owner to
    // physically retire (zerus-1y897.5); nothing is closed, observed or renewed.
    let release = !program.reviewable_refusal();
    let retired = if release {
        program.physical_retirement()
    } else {
        program.retirement()
    };
    let mut m = lock(&inner.model);
    let Some(job) = m.jobs.iter_mut().find(|j| j.id == id) else {
        return Watch::Stop;
    };
    if !job.busy
        || job.programs.is_some()
        || job.reservation != reservation
        || reservation
            .as_ref()
            .is_some_and(|reservation| !program.matches(reservation))
    {
        return Watch::Stop;
    }
    match retired {
        run::Retirement::Pending => {
            job.programs = Some(program);
            job.program_review_pending = true;
            emit(&mut m);
            Watch::Continue
        }
        run::Retirement::Failed => {
            job.programs = Some(program);
            job.busy = false;
            job.program_review_pending = true;
            emit(&mut m);
            Watch::Stop
        }
        run::Retirement::Retired if release => {
            // The resources are released; the unrefused result stays a
            // lookup-only row. No Check again, Force, review or consent.
            job.lookup_only = true;
            job.physically_released = true;
            job.busy = false;
            job.program_review_pending = true;
            emit(&mut m);
            Watch::Stop
        }
        run::Retirement::Retired => {
            if program.normal_refusal() {
                job.program_review_pending = false;
            }
            let step = job.cleanup.step();
            let review = reservation.is_some();
            job.programs = None;
            job.reservation = None;
            job.busy = review;
            if review {
                job.manual_review = true;
            }
            emit(&mut m);
            drop(m);
            if review {
                Watch::Review(step)
            } else {
                Watch::Stop
            }
        }
    }
}

/// Check again's click (zerus-wfynn). The production command is async and runs
/// the guarded check_with_runtime copy below on a blocking task; its
/// off-main-thread behaviour is pinned by the real delayed-close control.
fn click(selected: &TerminationCleanup) -> Result<View, String> {
    check_with_runtime(
        &selected.0,
        "job".into(),
        |model| changed(&AppHandle, model),
        |inner, id, step| inner.drive(AppHandle, id, step, false),
    )
}
fn check_with_runtime(
    inner: &Arc<Inner>,
    id: String,
    emit: impl Fn(&mut Model) + Copy,
    run_review: impl Fn(Arc<Inner>, String, Step) + Copy,
) -> Result<View, String> {
    let mut m = lock(&inner.model);
    if m.jobs
        .iter()
        .find(|j| j.id == id)
        .is_some_and(|j| j.dismissed || j.lookup_only || m.unknown_folder(j))
    {
        return Err("An unknown result will not be retried.".into());
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let revision = m.revision.wrapping_add(1);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let (step, program, reservation) = {
        let j = m
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(STATE_CHANGED)?;
        if j.busy || !j.cleanup.can_refresh() {
            return Err("An unknown result will not be retried.".into());
        }
        let (program, reservation) = if j.has_programs() {
            if !j.reviewable_refusal() {
                return Err("An unknown result will not be retried.".into());
            }
            let reservation = match j.reservation.clone() {
                Some(reservation) if reservation.revision == revision => reservation,
                Some(_) => return Err(STATE_CHANGED.into()),
                None => {
                    let (request_id, binding, capture) =
                        j.programs.as_ref().expect("retained program").identity();
                    program_pipeline::ProgramReservation {
                        revision,
                        request_id,
                        binding,
                        capture,
                    }
                }
            };
            j.reservation = Some(reservation.clone());
            let program = j.programs.take();
            (program, Some(reservation))
        } else {
            (None, None)
        };
        j.manual_review = false;
        j.cleanup.review_remaining();
        let step = j.cleanup.step();
        j.busy = true;
        j.phase = "checking";
        (step, program, reservation)
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let (step, program, reservation): (_, Option<()>, Option<()>) = {
        let j = m
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or(STATE_CHANGED)?;
        if j.busy || !j.cleanup.can_refresh() {
            return Err("An unknown result will not be retried.".into());
        }
        j.manual_review = false;
        j.cleanup.review_remaining();
        let step = j.cleanup.step();
        j.busy = true;
        j.phase = "checking";
        (step, None, None)
    };
    emit(&mut m);
    drop(m);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(program) = program {
        let reservation = reservation.expect("program reservation");
        let retired = program.retirement();
        let (start_review, view) = {
            let mut m = lock(&inner.model);
            let j = m
                .jobs
                .iter_mut()
                .find(|j| j.id == id)
                .ok_or(STATE_CHANGED)?;
            if !program.matches(&reservation)
                || j.reservation.as_ref() != Some(&reservation)
                || j.programs.is_some()
            {
                if j.reservation.as_ref() == Some(&reservation) && j.programs.is_none() {
                    j.programs = Some(program);
                    j.busy = false;
                    j.program_review_pending = true;
                    emit(&mut m);
                }
                return Err(STATE_CHANGED.into());
            }
            let start_review = match retired {
                run::Retirement::Retired => {
                    j.programs = None;
                    j.reservation = None;
                    j.program_review_pending = false;
                    j.manual_review = true;
                    true
                }
                run::Retirement::Pending => {
                    j.programs = Some(program);
                    j.program_review_pending = true;
                    false
                }
                run::Retirement::Failed => {
                    j.programs = Some(program);
                    j.busy = false;
                    j.program_review_pending = true;
                    false
                }
            };
            emit(&mut m);
            let view = m.view();
            (start_review, view)
        };
        if start_review {
            run_review(inner.clone(), id, step);
        }
        return Ok(view);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = (program, reservation);
    let mut m = lock(&inner.model);
    let j = m
        .jobs
        .iter_mut()
        .find(|j| j.id == id)
        .ok_or(STATE_CHANGED)?;
    j.manual_review = true;
    let view = m.view();
    drop(m);
    run_review(inner.clone(), id, step);
    Ok(view)
}

fn pop_watcher() -> thread::JoinHandle<()> {
    WATCHES
        .get()
        .unwrap()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pop()
        .unwrap()
}
fn wait_drives(inner: &Arc<Inner>, expected: usize) {
    let until = Instant::now() + Duration::from_secs(2);
    while inner.drives.load(Ordering::Acquire) < expected && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_live_known_refusal_offers_check_again() {
    let _serial = serial();
    let inner = fixture(Arc::new(Control::new(true)));
    let view = lock(&inner.model).view();
    assert!(view.can_check);
    assert!(!view.ready);
    assert!(!view.can_force);
}

#[test]
fn invalid_refusal_never_enters_retirement() {
    let _serial = serial();
    let control = Arc::new(Control::new(true));
    control.reviewable.store(false, Ordering::Release);
    let inner = fixture(control.clone());
    assert!(!lock(&inner.model).view().can_check);
    let selected = TerminationCleanup(inner);
    assert!(click(&selected).is_err());
    assert_eq!(control.retirement_calls.load(Ordering::Acquire), 0);
}

#[test]
fn pending_retirement_continues_after_reserved_empty_slot() {
    let _serial = serial();
    let (tx, rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut control = Control::new(true);
    control.entered = Some(tx);
    control.release = Mutex::new(Some(release_rx));
    let control = Arc::new(control);
    let inner = fixture(control.clone());
    watch_job(&inner, AppHandle, "job".into());
    let watcher = pop_watcher();
    let selected = TerminationCleanup(inner.clone());
    let checking = thread::spawn(move || click(&selected));
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    thread::sleep(Duration::from_millis(250));
    assert!(
        !watcher.is_finished(),
        "reserved empty slot ended the observer"
    );
    release_tx.send(()).unwrap();
    checking.join().unwrap().unwrap();
    control.retired.store(true, Ordering::Release);
    wait_drives(&inner, 1);
    watcher.join().unwrap();
    assert_eq!(inner.drives.load(Ordering::Acquire), 1);
    assert!(control.retirement_calls.load(Ordering::Acquire) >= 2);
}

#[test]
fn watcher_before_check_keeps_exclusive_ownership() {
    let _serial = serial();
    let (tx, rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut control = Control::new(false);
    control.entered = Some(tx);
    control.release = Mutex::new(Some(release_rx));
    let control = Arc::new(control);
    let inner = fixture(control.clone());
    watch_job(&inner, AppHandle, "job".into());
    let watcher = pop_watcher();
    let selected = TerminationCleanup(inner.clone());
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(click(&selected).is_err());
    release_tx.send(()).unwrap();
    thread::sleep(Duration::from_millis(150));
    control.retired.store(true, Ordering::Release);
    watcher.join().unwrap();
    assert_eq!(inner.drives.load(Ordering::Acquire), 0);
    assert!(control.retirement_calls.load(Ordering::Acquire) >= 2);
}

#[test]
fn unrelated_global_revision_does_not_cancel_the_reservation() {
    let _serial = serial();
    let (tx, rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut control = Control::new(true);
    control.entered = Some(tx);
    control.release = Mutex::new(Some(release_rx));
    let control = Arc::new(control);
    let inner = fixture(control.clone());
    watch_job(&inner, AppHandle, "job".into());
    let watcher = pop_watcher();
    let selected = TerminationCleanup(inner.clone());
    let checking = thread::spawn(move || click(&selected));
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    lock(&inner.model).revision += 100;
    release_tx.send(()).unwrap();
    checking.join().unwrap().unwrap();
    control.retired.store(true, Ordering::Release);
    wait_drives(&inner, 1);
    watcher.join().unwrap();
    assert_eq!(inner.drives.load(Ordering::Acquire), 1);
}

#[test]
fn failed_reservation_stays_lookup_only() {
    let _serial = serial();
    let mut control = Control::new(false);
    control.failed = AtomicBool::new(true);
    let control = Arc::new(control);
    let inner = fixture(control);
    let selected = TerminationCleanup(inner.clone());
    let view = click(&selected).unwrap();
    assert!(!view.can_check);
    assert!(!view.busy);
    watch_job(&inner, AppHandle, "job".into());
    pop_watcher().join().unwrap();
    assert_eq!(inner.drives.load(Ordering::Acquire), 0);
}

#[test]
fn stale_reservation_never_clears_a_newer_operation() {
    let _serial = serial();
    let (tx, rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let mut control = Control::new(true);
    control.entered = Some(tx);
    control.release = Mutex::new(Some(release_rx));
    let control = Arc::new(control);
    let inner = fixture(control);
    let selected = TerminationCleanup(inner.clone());
    let checking = thread::spawn(move || click(&selected));
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    lock(&inner.model).jobs[0].reservation = Some(program_pipeline::ProgramReservation {
        revision: 999,
        request_id: "newer".into(),
        binding: "newer".into(),
        capture: Capture,
    });
    release_tx.send(()).unwrap();
    assert!(checking.join().unwrap().is_err());
    let model = lock(&inner.model);
    assert_eq!(model.jobs[0].reservation.as_ref().unwrap().revision, 999);
    assert!(model.jobs[0].busy);
    assert_eq!(inner.drives.load(Ordering::Acquire), 0);
}

/// The program path keeps a result that is not a reviewable refusal pending
/// review; its source has expired.
fn unrefused(control: Control) -> (Arc<Control>, Arc<Inner>) {
    control.reviewable.store(false, Ordering::Release);
    let control = Arc::new(control);
    let inner = fixture(control.clone());
    lock(&inner.model).jobs[0].program_review_pending = true;
    (control, inner)
}

/// zerus-1y897.5: an expired result that is not a reviewable refusal releases
/// its physically retired program and leaves a lookup-only row.
#[test]
fn an_expired_unrefused_result_is_released_lookup_only() {
    let _serial = serial();
    let (control, inner) = unrefused(Control::new(false));
    control.retired.store(true, Ordering::Release);
    watch_job(&inner, AppHandle, "job".into());
    pop_watcher().join().unwrap();
    assert_eq!(
        control.retirement_calls.load(Ordering::Acquire),
        0,
        "an unrefused result entered the refusal retirement"
    );
    assert_eq!(control.physical_calls.load(Ordering::Acquire), 1);
    assert_eq!(Arc::strong_count(&control), 1, "the program was kept");
    let selected = TerminationCleanup(inner.clone());
    assert!(
        click(&selected).is_err(),
        "a released unrefused result offered Check again"
    );
    {
        let m = lock(&inner.model);
        let job = &m.jobs[0];
        assert!(job.programs.is_none());
        assert!(job.lookup_only);
        assert!(!job.busy);
        assert!(job.program_review_pending);
        assert!(matches!(
            job.program_retirement,
            Some(ProgramRetirement::Expired)
        ));
        let view = m.view();
        assert!(!view.can_check && !view.ready && !view.can_force);
    }
    assert_eq!(inner.drives.load(Ordering::Acquire), 0);
    assert!(!lock(&inner.model).jobs[0].busy);
}

#[test]
fn pending_physical_retirement_keeps_the_program_and_keeps_watching() {
    let _serial = serial();
    let (control, inner) = unrefused(Control::new(false));
    watch_job(&inner, AppHandle, "job".into());
    let watcher = pop_watcher();
    let until = Instant::now() + Duration::from_secs(2);
    while control.physical_calls.load(Ordering::Acquire) < 3 && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        control.physical_calls.load(Ordering::Acquire) >= 3,
        "Pending stopped the watcher"
    );
    assert!(!watcher.is_finished());
    {
        let m = lock(&inner.model);
        // The job or the running pass holds the program; it is never dropped.
        assert_eq!(Arc::strong_count(&control), 2);
        assert!(m.jobs[0].busy);
        assert!(!m.jobs[0].lookup_only);
    }
    let selected = TerminationCleanup(inner.clone());
    assert!(click(&selected).is_err());
    control.retired.store(true, Ordering::Release);
    watcher.join().unwrap();
    assert_eq!(Arc::strong_count(&control), 1);
    assert!(
        click(&selected).is_err(),
        "a released unrefused result offered Check again"
    );
    assert!(lock(&inner.model).jobs[0].lookup_only);
    assert_eq!(control.retirement_calls.load(Ordering::Acquire), 0);
}

#[test]
fn failed_physical_retirement_keeps_the_program_and_stops() {
    let _serial = serial();
    let (control, inner) = unrefused(Control::new(false));
    control.failed.store(true, Ordering::Release);
    watch_job(&inner, AppHandle, "job".into());
    pop_watcher().join().unwrap();
    assert_eq!(
        Arc::strong_count(&control),
        2,
        "a failed retirement released the program"
    );
    {
        let m = lock(&inner.model);
        assert!(m.jobs[0].programs.is_some());
        assert!(!m.jobs[0].busy);
        assert!(!m.jobs[0].lookup_only);
    }
    let selected = TerminationCleanup(inner.clone());
    assert!(click(&selected).is_err());
    assert_eq!(control.retirement_calls.load(Ordering::Acquire), 0);
}

/// zerus-1y897.2: the controls above run copies of the production Check and
/// passive watcher in a synthetic model. Bind them to the production source:
/// a change there fails here until the copies and their controls are updated.
#[test]
fn the_controls_run_the_exact_production_check_and_watcher() {
    let production = [
        include_str!("../src/termination_cleanup.rs"),
        include_str!("../src/termination_cleanup/program_pipeline.rs"),
    ];
    let controls = include_str!("cleanup_review_controls.rs");
    for name in ["check_with_runtime", "watch_job", "watch_once"] {
        let original = production
            .iter()
            .find_map(|source| function(source, name))
            .unwrap_or_else(|| panic!("production {name} is missing"));
        assert_eq!(
            function(controls, name).as_deref(),
            Some(original.as_str()),
            "{name} differs from production"
        );
    }
}

/// The function's code from `fn name(` to its closing brace, with comment and
/// blank lines dropped and indentation trimmed.
fn function(source: &str, name: &str) -> Option<String> {
    let start = source
        .find(&format!("fn {name}("))
        .or_else(|| source.find(&format!("fn {name}<")))?;
    let body = start + source[start..].find('{')?;
    let mut depth = 0;
    for (offset, c) in source[body..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let code = &source[start..=body + offset];
                    return Some(
                        code.lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty() && !line.starts_with("//"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                }
            }
            _ => {}
        }
    }
    None
}
