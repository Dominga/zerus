//! One historical creation-group job on the existing report admission. It
//! never waits on the legacy journal, gates Grant, or starts an effect worker.
use super::*;
use zerus_client_core::migration::{
    creation_report::{
        Journal as CreationJournal, Phase, Record as CreationRecord, Stored as CreationStored,
        SCOPE as CREATION_SCOPE,
    },
    preload::Prepared,
};
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
impl Default for CreationState {
    fn default() -> Self {
        Self {
            stored: None,
            running: false,
            pending: true,
            explicit: false,
            not_applicable: false,
            code: "creations_import_waiting",
            #[cfg(test)]
            before_record: None,
            #[cfg(test)]
            before_fence: None,
        }
    }
}
impl Report {
    fn creation_state(&self) -> std::sync::MutexGuard<'_, CreationState> {
        self.creations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
    pub(super) fn creation_status(&self) -> Value {
        let s = self.creation_state();
        json!({"scope":CREATION_SCOPE,"required":!s.not_applicable,"complete":s.not_applicable||s.stored.as_ref().is_some_and(|r|r.record.phase==Phase::Published),
            "running":s.running,"code":s.code,"import_id":s.stored.as_ref().map(|r|&r.record.import_id),
            "phase":s.stored.as_ref().map(|r|r.record.phase),"import_record":s.stored.as_ref().and_then(|r|r.record.expected.as_ref()).map(|r|&r.import_record)})
    }
    pub(super) fn retry_creations(&self) {
        let mut s = self.creation_state();
        s.pending = true;
        s.explicit |= !s.running;
    }
    /// Every save in one run compares against that run's own last durable
    /// record, never the report's shared view, so a stale run (or its late
    /// callback) cannot overwrite what a newer run recorded.
    fn advance(
        &self,
        journal: &CreationJournal,
        current: &mut CreationStored,
        change: impl FnOnce(&mut CreationRecord) -> Result<(), SourceError>,
    ) -> Result<(), SourceError> {
        let mut record = current.record.clone();
        change(&mut record)?;
        let stored = journal.save(record, Some(&current.sha256))?;
        *current = stored.clone();
        self.creation_state().stored = Some(stored);
        Ok(())
    }
    fn run_creation(
        self: &Arc<Self>,
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
            .inputs
            .creations
            .data_path()
            .map_err(|_| "creations_import_capture_unavailable")?;
        let journal = Arc::new(
            CreationJournal::new(&data, &configuration)
                .map_err(|_| "creations_import_configuration")?,
        );
        let existing = journal
            .load()
            .map_err(|_| "creations_import_record_unavailable")?;
        self.creation_state().stored = existing.clone();
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
                    .map_err(|_| "creations_import_cli_unavailable")?;
                if publisher.configuration() != configuration {
                    return Err("creations_import_configuration");
                }
                // Always inspect the original ID first, including before a
                // preparation recovery: the import ID is shared by attempts.
                let status = publisher
                    .status(&record.import_id)
                    .map_err(|_| "creations_import_result_unavailable")?;
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
                        self.advance(&journal, &mut current, |r| {
                            r.phase = Phase::Published;
                            Ok(())
                        })
                        .map_err(|_| "creations_import_record_unavailable")?;
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
                    _ => return Err("creations_import_original_result_unconfirmed"),
                }
            }
            if record.phase == Phase::Quarantined {
                return Err("creations_import_source_kept_for_review");
            }
            if record.phase == Phase::NotCommitted && !explicit {
                return Err("creations_import_handoff_busy");
            }
            if record.phase == Phase::BeforeRecord && !explicit {
                return Err("creations_import_before_record");
            }
        }
        let snapshot = match self
            .inputs
            .creations
            .retained()
            .map_err(|_| "creations_import_capture_unavailable")?
        {
            Prepared::Missing { .. } => {
                if existing.is_some() {
                    return Err("creations_import_source_changed");
                }
                self.creation_state().not_applicable = true;
                return Ok(());
            }
            Prepared::Snapshot(snapshot) => snapshot,
        };
        let reference = snapshot
            .reference()
            .map_err(|_| "creations_import_source_changed")?;
        let mut current = match existing {
            Some(stored) => {
                let mut current = stored;
                if current.record.source != reference {
                    // Also fences a recoverable attempt's pending callback.
                    self.advance(&journal, &mut current, |r| {
                        r.phase = Phase::Quarantined;
                        Ok(())
                    })
                    .map_err(|_| "creations_import_record_unavailable")?;
                    return Err("creations_import_source_changed");
                }
                current
            }
            None => {
                let created_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| "creations_import_clock_unavailable")?
                    .as_secs();
                let record =
                    CreationRecord::new_v2(configuration.clone(), reference.clone(), created_at)
                        .map_err(|_| "creations_import_configuration")?;
                let stored = journal
                    .save(record, None)
                    .map_err(|_| "creations_import_record_unavailable")?;
                self.creation_state().stored = Some(stored.clone());
                stored
            }
        };
        let sources = match snapshot.clone().report_sources() {
            Ok(sources) => sources,
            Err(_) => {
                let _ = self.advance(&journal, &mut current, |r| {
                    r.phase = Phase::Quarantined;
                    Ok(())
                });
                return Err("creations_import_source_kept_for_review");
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
        .map_err(|_| "creations_import_source_kept_for_review")?;
        let resources: Arc<dyn Send + Sync> =
            Arc::new((snapshot.clone(), self.clone(), work, relay.clone()));
        let path = relay
            .executable()
            .map_err(|_| "paired_desktop_cli_unavailable")?;
        let publisher = Publisher::probe_retained(path, &[], resources, stop.clone())
            .map_err(|_| "creations_import_cli_unavailable")?;
        if publisher.configuration() != configuration {
            return Err("creations_import_configuration");
        }
        if current.record.attempt == MAX_ATTEMPTS {
            return Err("creations_import_record_unavailable");
        }
        if recover {
            #[cfg(test)]
            if let Some(hook) = self.creation_state().before_fence.take() {
                hook();
            }
            // The same snapshot and configuration were just revalidated. This
            // compare-and-swap moves the exact Attempted record to BeforeRecord
            // at the same attempt; its new SHA fences the original callback,
            // which can no longer record a receipt or reach its final frame.
            // The shared Gate and the original resources stay as they are.
            current = journal
                .fence_preparation(&current)
                .map_err(|_| "creations_import_record_unavailable")?;
            self.creation_state().stored = Some(current.clone());
        }
        self.advance(&journal, &mut current, |r| {
            r.attempt += 1;
            r.phase = Phase::Attempted;
            Ok(())
        })
        .map_err(|_| "creations_import_record_unavailable")?;
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
                let hook = report.creation_state().before_record.take();
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
                report.creation_state().stored = Some(stored.clone());
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
                    .map_err(|_| "creations_import_record_unavailable")?
                    .ok_or("creations_import_record_unavailable")?;
                let gated = attempted
                    .record
                    .with_expected(&expected)
                    .map_err(|_| "creations_import_record_unavailable")?;
                // A failed write/readback may have persisted the gate. Reconcile
                // only this SAME attempted metadata, never an unknown, different
                // attempt, identity or payload.
                if loaded.record != attempted.record && loaded.record != gated {
                    return Err("creations_import_record_unavailable");
                }
                current = loaded;
                self.creation_state().stored = Some(current.clone());
                (Phase::BeforeRecord, Some("creations_import_before_record"))
            }
            Err(e) if e.reason == PublishError::HandoffBusy => {
                (Phase::NotCommitted, Some("creations_import_handoff_busy"))
            }
            Err(e) if matches!(e.reason, PublishError::Source(_)) => (
                Phase::Quarantined,
                Some("creations_import_source_kept_for_review"),
            ),
            Err(_) => (
                Phase::Unknown,
                Some("creations_import_original_result_unconfirmed"),
            ),
        };
        self.advance(&journal, &mut current, |r| {
            r.phase = phase;
            Ok(())
        })
        .map_err(|_| "creations_import_record_unavailable")?;
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
        let mut state = report.creation_state();
        state.pending = true;
        state.explicit |= explicit && !state.running;
    }
    schedule(shared, relay, configuration, stop);
}
pub(in crate::desktop_service) fn schedule_current(shared: &Arc<Shared>) {
    let Some(report) = &shared.retained_import else {
        return;
    };
    {
        let state = report.creation_state();
        if !state.pending || state.running {
            return;
        }
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
    let capture = report.inputs.creations.status();
    // Waiting/unavailable capture is visible already; do not spin helpers or
    // let this reporter initialize/recover the mutable source.
    if !matches!(
        capture.code,
        "creation_source_captured"
            | "creation_source_not_applicable"
            | "creation_source_unsupported"
            | "creation_source_too_large"
    ) {
        return;
    }
    let (charge, explicit) = {
        let mut state = report.creation_state();
        if !state.pending || state.running {
            return;
        }
        let Ok(charge) = report.work.admit(Role::Operations) else {
            state.code = "creations_import_source_resources_pending";
            return;
        };
        state.pending = false;
        state.running = true;
        state.code = "creations_import_running";
        let explicit = std::mem::take(&mut state.explicit);
        (charge, explicit)
    };
    shared.publish_retained_import();
    let weak = Arc::downgrade(shared);
    tauri::async_runtime::spawn_blocking(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            report.run_creation(relay, configuration, stop, explicit, Arc::new(charge))
        }))
        .unwrap_or(Err("creations_import_worker_unavailable"));
        {
            let mut state = report.creation_state();
            state.running = false;
            state.code = result.err().unwrap_or(if state.not_applicable {
                "creations_import_absent"
            } else {
                "creations_import_staged"
            });
        }
        if let Some(shared) = weak.upgrade() {
            shared.publish_retained_import();
        }
    });
}

#[cfg(all(test, unix))]
mod tests;
