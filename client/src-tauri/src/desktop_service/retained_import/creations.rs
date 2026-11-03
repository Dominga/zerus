//! Closed historical preload groups on the SAME existing report admission. This
//! never waits on the legacy journal, gates Grant, or starts an effect worker.
use super::*;
use zerus_client_core::migration::{
    creation_report::{
        Group, Journal as CreationJournal, Phase, Record as CreationRecord,
        Stored as CreationStored,
    },
    preload::Prepared,
};
/// Both historical groups use the same bounded executor and receipt rules.
macro_rules! codes {
    ($($variant:ident => $suffix:literal),* $(,)?) => {
        enum Code { $($variant),* }
        impl Code {
            fn for_group(self, group: Group) -> &'static str {
                match group {
                    Group::Creations => match self { $(Self::$variant => concat!("creations_import_", $suffix)),* },
                    Group::LocalReads => match self { $(Self::$variant => concat!("local_reads_import_", $suffix)),* },
                }
            }
        }
    };
}
codes! {
    Absent => "absent",
    BeforeRecord => "before_record",
    CaptureUnavailable => "capture_unavailable",
    CliUnavailable => "cli_unavailable",
    ClockUnavailable => "clock_unavailable",
    Configuration => "configuration",
    HandoffBusy => "handoff_busy",
    OriginalResultUnconfirmed => "original_result_unconfirmed",
    RecordUnavailable => "record_unavailable",
    ResultUnavailable => "result_unavailable",
    Running => "running",
    SourceChanged => "source_changed",
    SourceKeptForReview => "source_kept_for_review",
    SourceResourcesPending => "source_resources_pending",
    Staged => "staged",
    Waiting => "waiting",
    WorkerUnavailable => "worker_unavailable",
}
const GROUPS: [Group; 2] = [Group::Creations, Group::LocalReads];
#[cfg(test)]
type BeforeRecordHook =
    Box<dyn FnOnce(&migration::transport::StagedReceipt) -> Box<dyn Send> + Send>;
#[cfg(test)]
type BeforeFenceHook = Box<dyn FnOnce() + Send>;

pub(super) struct CreationState {
    stored: Option<CreationStored>,
    running: bool,
    pending: bool,
    explicit: bool,
    not_applicable: bool,
    code: &'static str,
    #[cfg(test)]
    before_record: Option<BeforeRecordHook>,
    #[cfg(test)]
    before_fence: Option<BeforeFenceHook>,
}
impl CreationState {
    pub(super) fn new(group: Group) -> Self {
        Self {
            stored: None,
            running: false,
            pending: true,
            explicit: false,
            not_applicable: false,
            code: Code::Waiting.for_group(group),
            #[cfg(test)]
            before_record: None,
            #[cfg(test)]
            before_fence: None,
        }
    }
}
impl Report {
    fn group_state(&self, group: Group) -> std::sync::MutexGuard<'_, CreationState> {
        match group {
            Group::Creations => &self.creations,
            Group::LocalReads => &self.local_reads,
        }
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
    }
    pub(super) fn group_status(&self, group: Group) -> Value {
        let s = self.group_state(group);
        json!({"scope":group.scope(),"required":!s.not_applicable,"complete":s.not_applicable||s.stored.as_ref().is_some_and(|r|r.record.phase==Phase::Published),
            "running":s.running,"code":s.code,"import_id":s.stored.as_ref().map(|r|&r.record.import_id),
            "phase":s.stored.as_ref().map(|r|r.record.phase),"import_record":s.stored.as_ref().and_then(|r|r.record.expected.as_ref()).map(|r|&r.import_record)})
    }
    pub(super) fn retry_creations(&self) {
        for group in GROUPS {
            let mut s = self.group_state(group);
            s.pending = true;
            s.explicit |= !s.running;
        }
    }
    fn capture_for(&self, group: Group) -> &Arc<migration::preload::Capture> {
        match group {
            Group::Creations => &self.inputs.creations,
            Group::LocalReads => &self.inputs.local_reads,
        }
    }
    #[cfg(all(test, unix))]
    fn creation_state(&self) -> std::sync::MutexGuard<'_, CreationState> {
        self.group_state(Group::Creations)
    }
    #[cfg(all(test, unix))]
    fn creation_status(&self) -> Value {
        self.group_status(Group::Creations)
    }
    #[cfg(all(test, unix))]
    fn run_creation(
        self: &Arc<Self>,
        relay: Arc<zerus_client_core::desktop::relay::Connection>,
        configuration: String,
        stop: Arc<AtomicBool>,
        explicit: bool,
        work: Arc<dyn Send + Sync>,
    ) -> Result<(), &'static str> {
        self.run_group(Group::Creations, relay, configuration, stop, explicit, work)
    }
    /// Every save in one run compares against that run's own last durable
    /// record, never the report's shared view, so a stale run (or its late
    /// callback) cannot overwrite what a newer run recorded.
    fn advance(
        &self,
        group: Group,
        journal: &CreationJournal,
        current: &mut CreationStored,
        change: impl FnOnce(&mut CreationRecord) -> Result<(), SourceError>,
    ) -> Result<(), SourceError> {
        let mut record = current.record.clone();
        change(&mut record)?;
        let stored = journal.save(record, Some(&current.sha256))?;
        *current = stored.clone();
        self.group_state(group).stored = Some(stored);
        Ok(())
    }
    fn run_group(
        self: &Arc<Self>,
        group: Group,
        relay: Arc<zerus_client_core::desktop::relay::Connection>,
        configuration: String,
        stop: Arc<AtomicBool>,
        explicit: bool,
        work: Arc<dyn Send + Sync>,
    ) -> Result<(), &'static str> {
        if stop.load(Ordering::Acquire) {
            return Err("stopped");
        }
        // The original worker resolves this before publishing capture status.
        // This reporter never creates a capture or reads today's live journal.
        let data = self
            .capture_for(group)
            .data_path()
            .map_err(|_| Code::CaptureUnavailable.for_group(group))?;
        let journal = Arc::new(
            CreationJournal::for_group(group, &data, &configuration)
                .map_err(|_| Code::Configuration.for_group(group))?,
        );
        let existing = journal
            .load()
            .map_err(|_| Code::RecordUnavailable.for_group(group))?;
        self.group_state(group).stored = existing.clone();
        let mut recover = false;
        if let Some(stored) = &existing {
            let record = &stored.record;
            if matches!(
                record.phase,
                Phase::Published | Phase::Unknown | Phase::Attempted
            ) {
                let resources: Arc<dyn Send + Sync> =
                    Arc::new((self.clone(), work.clone(), relay.clone()));
                let path = relay
                    .executable()
                    .map_err(|_| "paired_desktop_cli_unavailable")?;
                let publisher = Publisher::probe_retained(path, &[], resources, stop.clone())
                    .map_err(|_| Code::CliUnavailable.for_group(group))?;
                if publisher.configuration() != configuration {
                    return Err(Code::Configuration.for_group(group));
                }
                // Always inspect the original ID first, including before a
                // preparation recovery: the import ID is shared by attempts.
                let status = publisher
                    .status(&record.import_id)
                    .map_err(|_| Code::ResultUnavailable.for_group(group))?;
                match (&record.expected, status) {
                    (
                        Some(expected),
                        ImportStatus::Present {
                            evidence,
                            pages,
                            import_record,
                        },
                    ) if evidence == expected.evidence
                        && pages == expected.pages
                        && import_record == expected.import_record =>
                    {
                        let mut current = stored.clone();
                        self.advance(group, &journal, &mut current, |r| {
                            r.phase = Phase::Published;
                            Ok(())
                        })
                        .map_err(|_| Code::RecordUnavailable.for_group(group))?;
                        return Ok(());
                    }
                    // Only an explicit Retry, a definite absence of the
                    // original ID and durable counters proving this Attempted
                    // record never passed its commit gate may revalidate it.
                    // Unknown, a recorded gate and every uncertain answer stay
                    // lookup-only.
                    (_, ImportStatus::Absent) if explicit && record.preparation_recoverable() => {
                        recover = true;
                    }
                    _ => return Err(Code::OriginalResultUnconfirmed.for_group(group)),
                }
            }
            if record.phase == Phase::Quarantined {
                return Err(Code::SourceKeptForReview.for_group(group));
            }
            if record.phase == Phase::NotCommitted && !explicit {
                return Err(Code::HandoffBusy.for_group(group));
            }
            if record.phase == Phase::BeforeRecord && !explicit {
                return Err(Code::BeforeRecord.for_group(group));
            }
        }
        let snapshot = match self
            .capture_for(group)
            .retained()
            .map_err(|_| Code::CaptureUnavailable.for_group(group))?
        {
            Prepared::Missing { .. } => {
                if existing.is_some() {
                    return Err(Code::SourceChanged.for_group(group));
                }
                self.group_state(group).not_applicable = true;
                return Ok(());
            }
            Prepared::Snapshot(snapshot) => snapshot,
        };
        let reference = snapshot
            .reference()
            .map_err(|_| Code::SourceChanged.for_group(group))?;
        let mut current = match existing {
            Some(stored) => {
                let mut current = stored;
                if current.record.source != reference {
                    // Also fences a recoverable attempt's pending callback.
                    self.advance(group, &journal, &mut current, |r| {
                        r.phase = Phase::Quarantined;
                        Ok(())
                    })
                    .map_err(|_| Code::RecordUnavailable.for_group(group))?;
                    return Err(Code::SourceChanged.for_group(group));
                }
                current
            }
            None => {
                let created_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| Code::ClockUnavailable.for_group(group))?
                    .as_secs();
                let record = CreationRecord::new_group(
                    group,
                    configuration.clone(),
                    reference.clone(),
                    created_at,
                )
                .map_err(|_| Code::Configuration.for_group(group))?;
                let stored = journal
                    .save(record, None)
                    .map_err(|_| Code::RecordUnavailable.for_group(group))?;
                self.group_state(group).stored = Some(stored.clone());
                stored
            }
        };
        let sources = match snapshot.clone().report_sources() {
            Ok(sources) => sources,
            Err(_) => {
                let _ = self.advance(group, &journal, &mut current, |r| {
                    r.phase = Phase::Quarantined;
                    Ok(())
                });
                return Err(Code::SourceKeptForReview.for_group(group));
            }
        };
        let bundle = migration::Bundle::new(
            crate::identity::SWITCH_ID,
            false,
            configuration.clone(),
            current.record.import_id.clone(),
            current.record.created_at,
            sources,
            64 << 20,
        )
        .map_err(|_| Code::SourceKeptForReview.for_group(group))?;
        let resources: Arc<dyn Send + Sync> =
            Arc::new((snapshot.clone(), self.clone(), work, relay.clone()));
        let path = relay
            .executable()
            .map_err(|_| "paired_desktop_cli_unavailable")?;
        let publisher = Publisher::probe_retained(path, &[], resources, stop.clone())
            .map_err(|_| Code::CliUnavailable.for_group(group))?;
        if publisher.configuration() != configuration {
            return Err(Code::Configuration.for_group(group));
        }
        if current.record.attempt == MAX_ATTEMPTS {
            return Err(Code::RecordUnavailable.for_group(group));
        }
        if recover {
            #[cfg(test)]
            if let Some(hook) = self.group_state(group).before_fence.take() {
                hook();
            }
            // The same snapshot and configuration were just revalidated. This
            // compare-and-swap moves the exact Attempted record to BeforeRecord
            // at the same attempt; its new SHA fences the original callback,
            // which can no longer record a receipt or reach its final frame.
            // The shared Gate and the original resources stay as they are.
            current = journal
                .fence_preparation(&current)
                .map_err(|_| Code::RecordUnavailable.for_group(group))?;
            self.group_state(group).stored = Some(current.clone());
        }
        self.advance(group, &journal, &mut current, |r| {
            r.attempt += 1;
            r.phase = Phase::Attempted;
            Ok(())
        })
        .map_err(|_| Code::RecordUnavailable.for_group(group))?;
        let attempted = current.clone();
        let gate = attempted.clone();
        let report = self.clone();
        let save = journal.clone();
        let recorded = Arc::new(Mutex::new(None::<CreationStored>));
        let gated = recorded.clone();
        let failed_preparation = Arc::new(Mutex::new(None));
        let failed = failed_preparation.clone();
        let result = publisher.publish_with_receipt(bundle, move |expected| {
            let prepared = (|| {
                #[cfg(test)]
                let hook = report.group_state(group).before_record.take();
                #[cfg(test)]
                let _held = hook.map(|hook| hook(expected));
                if stop.load(Ordering::Acquire) {
                    return Err(SourceError::SourceUnreadable);
                }
                if snapshot.reference()? != reference {
                    return Err(SourceError::SourceChanged);
                }
                // The commit gate: durably record this attempt's receipt (and,
                // in version 2, recorded_attempt) against the exact Stored SHA
                // this attempt reserved, before the final frame. Any change to
                // that record, including a recovery fence, fails this swap.
                let stored = save.save(gate.record.with_expected(expected)?, Some(&gate.sha256))?;
                report.group_state(group).stored = Some(stored.clone());
                *gated.lock().unwrap_or_else(PoisonError::into_inner) = Some(stored);
                Ok(())
            })();
            if prepared == Err(SourceError::SourceUnreadable) {
                *failed.lock().unwrap_or_else(PoisonError::into_inner) = Some(expected.clone());
            }
            prepared
        });
        if let Some(stored) = recorded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            current = stored;
        }
        let (phase, code) = match result {
            Ok(_) => (Phase::Published, None),
            Err(e)
                if e.reason == PublishError::Source(SourceError::SourceUnreadable)
                    && failed_preparation
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .is_some() =>
            {
                // encode returned this exact callback error BEFORE its final
                // frame. This is not inferred from an absent/lost child reply.
                let expected = failed_preparation
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
                    .unwrap();
                let loaded = journal
                    .load()
                    .map_err(|_| Code::RecordUnavailable.for_group(group))?
                    .ok_or(Code::RecordUnavailable.for_group(group))?;
                let gated = attempted
                    .record
                    .with_expected(&expected)
                    .map_err(|_| Code::RecordUnavailable.for_group(group))?;
                // A failed write/readback may have persisted the gate. Reconcile
                // only this SAME attempted metadata, never an unknown, different
                // attempt, identity or payload.
                if loaded.record != attempted.record && loaded.record != gated {
                    return Err(Code::RecordUnavailable.for_group(group));
                }
                current = loaded;
                self.group_state(group).stored = Some(current.clone());
                (
                    Phase::BeforeRecord,
                    Some(Code::BeforeRecord.for_group(group)),
                )
            }
            Err(e) if e.reason == PublishError::HandoffBusy => (
                Phase::NotCommitted,
                Some(Code::HandoffBusy.for_group(group)),
            ),
            Err(e) if matches!(e.reason, PublishError::Source(_)) => (
                Phase::Quarantined,
                Some(Code::SourceKeptForReview.for_group(group)),
            ),
            Err(_) => (
                Phase::Unknown,
                Some(Code::OriginalResultUnconfirmed.for_group(group)),
            ),
        };
        self.advance(group, &journal, &mut current, |r| {
            r.phase = phase;
            Ok(())
        })
        .map_err(|_| Code::RecordUnavailable.for_group(group))?;
        code.map_or(Ok(()), Err)
    }
}

/// Reconnect/Rearm may inspect an original result; only explicit user Retry
/// enables another exact known-negative attempt. Frames merely wake an initial
/// job that was waiting for the original capture or Qt helper charge to drain.
pub(super) fn arm(
    shared: &Arc<Shared>,
    relay: Arc<zerus_client_core::desktop::relay::Connection>,
    configuration: String,
    stop: Arc<AtomicBool>,
    explicit: bool,
) {
    if let Some(report) = &shared.retained_import {
        for group in GROUPS {
            let mut state = report.group_state(group);
            state.pending = true;
            state.explicit |= explicit && !state.running;
        }
    }
    schedule(shared, relay, configuration, stop);
}
pub(in crate::desktop_service) fn schedule_current(shared: &Arc<Shared>) {
    let Some(report) = &shared.retained_import else {
        return;
    };
    if !GROUPS.into_iter().any(|group| {
        let state = report.group_state(group);
        state.pending && !state.running
    }) {
        return;
    }
    let relay = shared
        .relay
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let configuration = shared
        .configuration
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let stop = shared
        .cancellation
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if let (Some(relay), Some(configuration), Some(stop)) = (relay, configuration, stop) {
        schedule(shared, relay, configuration, stop);
    }
}
fn schedule(
    shared: &Arc<Shared>,
    relay: Arc<zerus_client_core::desktop::relay::Connection>,
    configuration: String,
    stop: Arc<AtomicBool>,
) {
    if shared.stopped.load(Ordering::Acquire) || stop.load(Ordering::Acquire) {
        return;
    }
    let Some(report) = shared.retained_import.clone() else {
        return;
    };
    {
        let qt = report.state();
        if qt.code.is_empty() || qt.running {
            return;
        }
    }
    for group in GROUPS {
        let capture = report.capture_for(group).status();
        // Waiting/unavailable capture is visible already; do not spin helpers or
        // let this reporter initialize/recover the mutable source.
        if !matches!(
            capture.code,
            "creation_source_captured"
                | "creation_source_not_applicable"
                | "creation_source_unsupported"
                | "creation_source_too_large"
                | "local_read_source_captured"
                | "local_read_source_not_applicable"
                | "local_read_source_unsupported"
                | "local_read_source_too_large"
        ) {
            continue;
        }
        let (charge, explicit) = {
            let mut state = report.group_state(group);
            if !state.pending || state.running {
                continue;
            }
            let Ok(charge) = report.work.admit(Role::Operations) else {
                state.code = Code::SourceResourcesPending.for_group(group);
                return;
            };
            state.pending = false;
            state.running = true;
            state.code = Code::Running.for_group(group);
            let explicit = std::mem::take(&mut state.explicit);
            (charge, explicit)
        };
        shared.publish_retained_import();
        let weak = Arc::downgrade(shared);
        tauri::async_runtime::spawn_blocking(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                report.run_group(
                    group,
                    relay,
                    configuration,
                    stop,
                    explicit,
                    Arc::new(charge),
                )
            }))
            .unwrap_or(Err(Code::WorkerUnavailable.for_group(group)));
            {
                let mut state = report.group_state(group);
                state.running = false;
                state.code = result.err().unwrap_or(if state.not_applicable {
                    Code::Absent.for_group(group)
                } else {
                    Code::Staged.for_group(group)
                });
            }
            if let Some(shared) = weak.upgrade() {
                shared.publish_retained_import();
                schedule_current(&shared);
            }
        });
        return;
    }
}

#[cfg(all(test, unix))]
mod local_reads_tests;
#[cfg(all(test, unix))]
mod tests;
