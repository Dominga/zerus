//! The native live Remove pipeline. Model entries are presentation holders,
//! never serialized permissions; loss/expiry cannot rebuild a continuation.
use super::*;
use crate::actions::Claim;
use zerus_client_core::{end_action::Captured, worktree_programs::stop::State as StopState};

pub(super) fn expire_offer(inner: &Arc<Inner>, id: &str) {
    let weak = Arc::downgrade(inner);
    let id = id.to_owned();
    tauri::async_runtime::spawn_blocking(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let mut m = lock(&inner.model);
        let Some(index) = m.offers.iter().position(|o| o.id == id) else {
            return;
        };
        let Some(programs) = m.offers[index].programs.as_ref() else {
            return;
        };
        if programs.current() {
            continue;
        }
        let offer = m.offers.remove(index);
        drop(m);
        if let Some(programs) = offer.and_then(|o| o.programs) {
            let _ = programs.close();
        }
        return;
    });
}

pub(super) enum ProgramJob {
    Stopped(Box<programs::Stopped>),
    Force {
        id: String,
        revision: u64,
        offer: Box<programs::ForceOffer>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ProgramReservation {
    pub revision: u64,
    pub request_id: String,
    pub binding: String,
    pub capture: Capture,
}

impl ProgramJob {
    pub(super) fn identity(&self) -> (String, String, Capture) {
        match self {
            Self::Stopped(stopped) => stopped.identity(),
            Self::Force { offer, .. } => offer.identity(),
        }
    }

    pub(super) fn matches(&self, reservation: &ProgramReservation) -> bool {
        let (request_id, binding, capture) = self.identity();
        reservation.request_id == request_id
            && reservation.binding == binding
            && reservation.capture == capture
    }

    pub(super) fn retirement(&self) -> run::Retirement {
        match self {
            Self::Stopped(stopped) => stopped.retirement(),
            Self::Force { .. } => run::Retirement::Failed,
        }
    }
    fn remaining_millis(&self) -> u64 {
        match self {
            Self::Stopped(stopped) => stopped.remaining_millis(),
            Self::Force { offer, .. } => offer.remaining_millis(),
        }
    }
    fn holder_unavailable(&self) -> bool {
        match self {
            Self::Stopped(stopped) => stopped.holder_unavailable(),
            Self::Force { offer, .. } => offer.holder_unavailable(),
        }
    }
    fn current(&self) -> bool {
        match self {
            Self::Stopped(stopped) => stopped.current(),
            Self::Force { offer, .. } => offer.current(),
        }
    }
    pub(super) fn normal_refusal(&self) -> bool {
        matches!(self, Self::Stopped(stopped) if stopped.normal_refusal())
    }
    pub(super) fn reviewable_refusal(&self) -> bool {
        matches!(self, Self::Stopped(stopped) if stopped.reviewable_refusal())
    }
    pub(super) fn can_force(&self) -> bool {
        matches!(self, Self::Stopped(stopped) if stopped.can_force())
    }
}
impl TerminationCleanup {
    pub(crate) fn program_claim(
        &self,
        id: &str,
        captures: &[Captured],
    ) -> Result<Option<Arc<Claim>>, String> {
        let m = lock(&self.0.model);
        let offer = m.offers.iter().find(|o| o.id == id).ok_or(STATE_CHANGED)?;
        let Some(programs) = &offer.programs else {
            return Ok(None);
        };
        if captures.len() != 1
            || captures[0].target != offer.capture.target
            || captures[0].action != zerus_client_core::Action::Terminate
            || !captures[0].valid()
            || !programs.current()
        {
            return Err(STATE_CHANGED.into());
        }
        Ok(Some(programs.claim()))
    }
}
struct Reservation(Arc<Inner>);
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut m = lock(&self.0.model);
        m.reserved = m.reserved.saturating_sub(1);
    }
}
pub(super) async fn confirm(
    inner: Arc<Inner>,
    app: AppHandle,
    actions: &crate::actions::Actions,
    id: String,
    offer: programs::Offer,
    delayed: Option<(Captured, Arc<Claim>)>,
) -> Result<Outcome, String> {
    let reservation = Reservation(inner.clone());
    let capture = offer.capture().clone();
    let confirmation = offer.consume(id.clone())?;
    let (outcome, continuation) =
        crate::actions::dispatch_program_termination(actions, confirmation, delayed).await?;
    if let (Outcome::Done { receipt, cleanup }, Some(continuation)) = (&outcome, continuation) {
        if let Some(entry) = capture.entry(cleanup.as_ref()) {
            let mut m = lock(&inner.model);
            m.jobs.push_back(Job {
                id: id.clone(),
                cleanup: Cleanup::after_termination(vec![entry]),
                busy: true,
                phase: "stopping",
                manual_review: false,
                dismissed: false,
                program_review_pending: true,
                program_retirement: None,
                source: Some(capture.target.clone()),
                restore_supported: true,
                reservation: None,
                archive: Some(ArchiveIdentity {
                    version: 1,
                    archive_id: receipt["result_target"]["archive_id"]
                        .as_str()
                        .unwrap_or("")
                        .into(),
                    name: capture.target.name.clone(),
                    run_id: capture.target.run_id.clone(),
                    conversation_id: capture.target.conversation_id.clone(),
                    owner_id: capture.owner_id.clone(),
                    launch_owned: !capture.owner_id.is_empty(),
                }),
                programs: None,
            });
            changed(&app, &mut m);
            drop(m);
            let worker_inner = inner.clone();
            tauri::async_runtime::spawn_blocking(move || {
                // A returned native receipt is not physical pipe settlement.
                while continuation.current() && !continuation.original_call_joined() {
                    std::thread::sleep(Duration::from_millis(10));
                }
                let stopped = continuation.normal().and_then(settle);
                publish(worker_inner, app, id, stopped);
            });
        }
    }
    drop(reservation);
    Ok(outcome)
}
fn settle(mut stopped: programs::Stopped) -> Result<programs::Stopped, String> {
    while stopped.current() {
        if stopped.try_retire()? {
            return Ok(stopped);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = stopped.close();
    Err(crate::desktop_service::termination_outcome::UNCONFIRMED.into())
}
fn publish(
    inner: Arc<Inner>,
    app: AppHandle,
    id: String,
    stopped: Result<programs::Stopped, String>,
) {
    let stopped = match stopped {
        Ok(stopped)
            if stopped
                .report()
                .is_ok_and(|report| report.state == StopState::Complete) =>
        {
            finish_cleanup(inner, app, id, stopped);
            return;
        }
        other => other,
    };
    let mut m = lock(&inner.model);
    let Some(job) = m.jobs.iter_mut().find(|j| j.id == id) else {
        return;
    };
    job.busy = false;
    job.phase = "checking";
    let (detail, live) = match stopped {
        Ok(stopped) => {
            if stopped.reviewable_refusal() {
                job.cleanup = Cleanup::retained(std::mem::take(&mut job.cleanup.entries));
                job.program_review_pending = false;
            }
            job.program_retirement = None;
            let detail = match stopped.report() {
                Ok(report) if report.state == StopState::Complete => zerus_client_core::source!("Programs have stopped. Checking the working folder…"),
                Ok(report) if report.state == StopState::Incomplete => zerus_client_core::source!("Some programs are still running. Review them before choosing Force stop."),
                Ok(report) if report.state == StopState::Refused => "The program list changed or could not be checked. Review the working folder again.",
                _ => zerus_client_core::source!("The program stop is unconfirmed. Check the original result before continuing."),
            };
            job.programs = Some(ProgramJob::Stopped(Box::new(stopped)));
            (detail, true)
        }
        Err(_) => {
            job.program_retirement = Some(ProgramRetirement::StopUnconfirmed);
            (
                zerus_client_core::source!(
                    "The program stop is unconfirmed. Check the original result before continuing."
                ),
                false,
            )
        }
    };
    for entry in &mut job.cleanup.entries {
        entry.status = Some(Status::Kept);
        entry.detail = detail.into();
    }
    changed(&app, &mut m);
    drop(m);
    if live {
        watch_job(&inner, app, id);
    }
}
fn watch_job(inner: &Arc<Inner>, app: AppHandle, id: String) {
    // Expiry is one-way. Retaining a UI row never keeps an Actions claim
    // indefinitely or renews consent after source/authority loss.
    let weak = Arc::downgrade(inner);
    tauri::async_runtime::spawn_blocking(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let (reservation, program) = {
            let mut m = lock(&inner.model);
            let Some(job) = m.jobs.iter_mut().find(|j| j.id == id) else {
                return;
            };
            let Some(program) = &job.programs else {
                if job.busy && job.reservation.is_some() && !job.dismissed {
                    continue;
                }
                return;
            };
            if !job.busy && job.program_review_pending && job.reservation.is_some() {
                return;
            }
            if program.current() {
                // A finished control worker is observable failure data, never a
                // physical-drain proof. Keep the original resources/expiry exactly
                // as before; only disable presentation of a usable Force offer.
                if program.holder_unavailable() && job.program_retirement.is_none() {
                    job.program_retirement = Some(ProgramRetirement::HolderUnavailable);
                    changed(&app, &mut m);
                }
                continue;
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
        let retired = program.retirement();
        let mut m = lock(&inner.model);
        let Some(job) = m.jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        if !job.busy
            || job.programs.is_some()
            || job.reservation != reservation
            || reservation
                .as_ref()
                .is_some_and(|reservation| !program.matches(reservation))
        {
            return;
        }
        match retired {
            run::Retirement::Pending => {
                job.programs = Some(program);
                job.program_review_pending = true;
                changed(&app, &mut m);
                continue;
            }
            run::Retirement::Failed => {
                job.programs = Some(program);
                job.busy = false;
                job.program_review_pending = true;
                changed(&app, &mut m);
                return;
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
                changed(&app, &mut m);
                drop(m);
                if review {
                    inner.drive(app, id, step, false);
                }
                return;
            }
        }
    });
}

pub(super) async fn force_offer(
    inner: Arc<Inner>,
    app: AppHandle,
    id: String,
) -> Result<super::ForceView, String> {
    let stopped = {
        let mut m = lock(&inner.model);
        let job = m
            .jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or(STATE_CHANGED)?;
        if job.busy || !job.programs.as_ref().is_some_and(ProgramJob::can_force) {
            return Err(STATE_CHANGED.into());
        }
        let Some(ProgramJob::Stopped(stopped)) = job.programs.take() else {
            return Err(STATE_CHANGED.into());
        };
        job.busy = true;
        changed(&app, &mut m);
        stopped
    };
    let offered = tauri::async_runtime::spawn_blocking(move || stopped.offer_force())
        .await
        .map_err(|error| error.to_string())?;
    let mut m = lock(&inner.model);
    let revision = m.revision.wrapping_add(1);
    let job = m
        .jobs
        .iter_mut()
        .find(|job| job.id == id)
        .ok_or(STATE_CHANGED)?;
    job.busy = false;
    let result = match offered {
        Ok(offer) => {
            let confirmation = uuid::Uuid::new_v4().to_string();
            let programs = offer.programs();
            let remaining_millis = offer.remaining_millis();
            job.programs = Some(ProgramJob::Force {
                id: confirmation.clone(),
                revision,
                offer: Box::new(offer),
            });
            Ok(super::ForceView {
                confirmation,
                programs,
                remaining_millis,
                revision,
            })
        }
        Err(error) => {
            job.program_retirement = Some(ProgramRetirement::OfferUnavailable);
            Err(error)
        }
    };
    changed(&app, &mut m);
    drop(m);
    if result.is_ok() {
        watch_job(&inner, app, id);
    }
    result
}
pub(super) async fn force_confirm(
    inner: Arc<Inner>,
    app: AppHandle,
    id: String,
    confirmation: String,
) -> Result<View, String> {
    let offer = {
        let mut m = lock(&inner.model);
        let job = m
            .jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or(STATE_CHANGED)?;
        if job.busy
            || !matches!(&job.programs, Some(ProgramJob::Force { id, .. }) if id == &confirmation)
        {
            return Err(STATE_CHANGED.into());
        }
        let Some(ProgramJob::Force { offer, .. }) = job.programs.take() else {
            return Err(STATE_CHANGED.into());
        };
        job.busy = true;
        job.phase = "stopping";
        changed(&app, &mut m);
        offer
    };
    let copy = inner.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = (*offer).confirm().and_then(settle);
        publish(copy, app, id, result);
    });
    Ok(lock(&inner.model).view())
}
pub(super) fn force_cancel(
    inner: &Arc<Inner>,
    app: &AppHandle,
    id: &str,
    confirmation: &str,
) -> Result<View, String> {
    let mut m = lock(&inner.model);
    let job = m
        .jobs
        .iter_mut()
        .find(|job| job.id == id)
        .ok_or(STATE_CHANGED)?;
    if job.busy
        || !matches!(&job.programs, Some(ProgramJob::Force { id, .. }) if id == confirmation)
    {
        return Err(STATE_CHANGED.into());
    }
    let Some(ProgramJob::Force { offer, .. }) = job.programs.take() else {
        return Err(STATE_CHANGED.into());
    };
    job.programs = Some(ProgramJob::Stopped(Box::new((*offer).cancel())));
    changed(app, &mut m);
    Ok(m.view())
}

fn finish_cleanup(inner: Arc<Inner>, app: AppHandle, id: String, stopped: programs::Stopped) {
    {
        let mut model = lock(&inner.model);
        let Some(job) = model.jobs.iter_mut().find(|job| job.id == id) else {
            return;
        };
        job.busy = true;
        job.phase = "removing";
        changed(&app, &mut model);
    }
    // No model lock across native preparation, child I/O, deletion or join.
    // Taking Stopped consumes the only live Remove continuation. Every result,
    // including a lost reply, ends this pipeline; there is no automatic retry.
    let mut physically_closed = false;
    let result = stopped.cleanup().and_then(|mut removal| {
        while removal.current() {
            if removal.try_retire()? {
                let report = removal.report().cloned().map_err(str::to_owned);
                // The read-only holder closes only AFTER the mutating helper,
                // descendants, all readers and its loan physically retire.
                removal.close()?;
                physically_closed = true;
                return report;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = removal.close();
        Err(crate::desktop_service::termination_outcome::UNCONFIRMED.into())
    });
    let mut model = lock(&inner.model);
    let Some(job) = model.jobs.iter_mut().find(|job| job.id == id) else {
        return;
    };
    let removed = record_cleanup_result(job, &result, physically_closed);
    if removed {
        let labels: Vec<_> = job
            .cleanup
            .entries
            .iter()
            .map(|entry| leaf(entry.row["path"].as_str().unwrap_or("")))
            .collect();
        model.jobs.retain(|job| job.id != id);
        for label in labels {
            let _ = app.emit("termination-folder-removed", label);
        }
    }
    changed(&app, &mut model);
}

/// Called only after the known-complete stop's cleanup and holder close return.
/// This is result retention, not an execution or custody admission.
pub(super) fn record_cleanup_result(
    job: &mut Job,
    result: &Result<zerus_client_core::worktree_programs::cleanup::Report, String>,
    physically_closed: bool,
) -> bool {
    job.busy = false;
    job.phase = "checking";
    job.programs = None;
    job.program_retirement = Some(
        if physically_closed
            && result.as_ref().is_ok_and(|report| {
                matches!(
                    report.state,
                    zerus_client_core::worktree_programs::cleanup::State::Removed
                        | zerus_client_core::worktree_programs::cleanup::State::Kept
                )
            })
        {
            ProgramRetirement::CleanupSettled
        } else {
            ProgramRetirement::CleanupUnconfirmed
        },
    );
    use zerus_client_core::worktree_programs::cleanup::State;
    let removed = result
        .as_ref()
        .is_ok_and(|report| report.state == State::Removed);
    // A parsed reply alone is not physical retirement. This observation is
    // set only after try_retire AND holder close above, never worker_finished.
    // A settled missing/invalid reply is Unknown and may be dismissed singly.
    job.program_review_pending = !physically_closed;
    for entry in &mut job.cleanup.entries {
        entry.status = Some(match result {
            Ok(report) if report.state == State::Removed => Status::Removed,
            Ok(report) if report.state == State::Kept => Status::Kept,
            _ => Status::Unconfirmed,
        });
        entry.detail = match result {
            Ok(report) if report.state == State::Removed => String::new(),
            Ok(report) if report.state == State::Kept => zerus_client_core::source!(
                "The working folder changed or is still in use. Review it before removing it."
            )
            .into(),
            _ => zerus_client_core::source!(
                "Removal could not be confirmed. Check the original result before continuing."
            )
            .into(),
        };
    }
    removed
}
