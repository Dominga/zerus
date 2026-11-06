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
        offer: Box<programs::ForceOffer>,
    },
}
impl ProgramJob {
    fn current(&self) -> bool {
        match self {
            Self::Stopped(stopped) => stopped.current(),
            Self::Force { offer, .. } => offer.current(),
        }
    }
    fn close(&self) -> Result<(), String> {
        match self {
            Self::Stopped(stopped) => stopped.close(),
            Self::Force { offer, .. } => offer.close(),
        }
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
                source: Some(capture.target.clone()),
                restore_supported: true,
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
    let mut m = lock(&inner.model);
    let Some(job) = m.jobs.iter_mut().find(|j| j.id == id) else {
        return;
    };
    job.busy = false;
    job.phase = "checking";
    let (detail, live) = match stopped {
        Ok(stopped) => {
            let detail = match stopped.report() {
                Ok(report) if report.state == StopState::Complete => zerus_client_core::source!("Programs have stopped. Checking the working folder…"),
                Ok(report) if report.state == StopState::Incomplete => zerus_client_core::source!("Some programs are still running. Review them before choosing Force stop."),
                Ok(report) if report.state == StopState::Refused => "The program list changed or could not be checked. Review the working folder again.",
                _ => zerus_client_core::source!("The program stop is unconfirmed. Check the original result before continuing."),
            };
            job.programs = Some(ProgramJob::Stopped(Box::new(stopped)));
            (detail, true)
        }
        Err(_) => (
            zerus_client_core::source!(
                "The program stop is unconfirmed. Check the original result before continuing."
            ),
            false,
        ),
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
        let mut m = lock(&inner.model);
        let Some(job) = m.jobs.iter_mut().find(|j| j.id == id) else {
            return;
        };
        let Some(program) = &job.programs else {
            return;
        };
        if program.current() {
            continue;
        }
        let program = job.programs.take();
        changed(&app, &mut m);
        drop(m);
        if let Some(program) = program {
            let _ = program.close();
        }
        return;
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
            job.programs = Some(ProgramJob::Force {
                id: confirmation.clone(),
                offer: Box::new(offer),
            });
            Ok(super::ForceView {
                confirmation,
                programs,
            })
        }
        Err(error) => Err(error),
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
