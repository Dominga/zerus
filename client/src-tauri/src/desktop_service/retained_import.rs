//! One best-effort native Qt/Settings report before the first service Grant.
//! Its result never gates Grant or the read connection. Managed paths and
//! original source handles come from setup, never a webview request.
use super::*;
pub(super) mod creations;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use zerus_client_core::{
    migration::{
        self,
        journal::{Journal, Phase, Record, Stored, MAX_ATTEMPTS, SCOPE},
        transport::{ImportStatus, PublishError, Publisher},
        SourceError,
    },
    settings::{CapturedQt, CapturedSettings, QtStore, Settings},
};

#[derive(Clone)]
pub(crate) struct Inputs {
    data: PathBuf,
    settings: Settings,
    qt: Option<QtStore>,
    pub(crate) creations: Arc<zerus_client_core::migration::preload::Capture>,
    pub(crate) local_reads: Arc<zerus_client_core::migration::preload::Capture>,
    pub(crate) worktree_cache: Arc<zerus_client_core::migration::preload::Capture>,
    /// The G3 drafts capture (zerus-5wmh.66.5.2.1.2): the Next draft store
    /// runs it as its first job; its status and Retry go through this report.
    pub(crate) drafts: Arc<zerus_client_core::migration::draft_preload::Capture>,
    /// The configured download folder, whose G3 evidence
    /// (zerus-5wmh.66.5.2.1.3) the report shows.
    pub(crate) downloads: Option<Downloads>,
}
/// Resolves the configured download folder when the report is shown.
pub(crate) type Downloads = Arc<dyn Fn() -> PathBuf + Send + Sync>;
/// How long a shown download-folder status is reused: it is a few bounded
/// stats, but the report is published on every service state change.
const DOWNLOADS_FRESH: Duration = Duration::from_secs(5);
impl Inputs {
    pub(crate) fn native(
        identifier: &str,
        replay: bool,
        data: PathBuf,
        settings: Settings,
        downloads: Downloads,
    ) -> Option<Self> {
        (identifier == crate::identity::SWITCH_ID && !replay).then(|| Self {
            creations: zerus_client_core::migration::preload::Capture::new(data.clone()),
            local_reads: zerus_client_core::migration::preload::Capture::local_reads(data.clone()),
            worktree_cache: zerus_client_core::migration::preload::Capture::worktree_cache(
                data.clone(),
            ),
            drafts: Arc::new(zerus_client_core::migration::draft_preload::Capture::native(&data)),
            downloads: Some(downloads),
            data,
            settings,
            qt: QtStore::here(),
        })
    }
}
struct Captured {
    qt: CapturedQt,
    settings: Arc<CapturedSettings>,
    resources: Arc<dyn Send + Sync>,
}
impl Captured {
    fn qt_hash(&self, cancel: &AtomicBool) -> Result<String, SourceError> {
        self.qt.recheck(self.resources.clone(), cancel)
    }
    fn sources(
        self: &Arc<Self>,
        stop: Arc<AtomicBool>,
    ) -> Result<Vec<migration::SourceStream>, SourceError> {
        let groups = migration::families::retained_qt(
            self.qt.snapshot(),
            self.settings.bytes(),
            cfg!(target_os = "linux"),
        )?;
        let mut sources = Vec::new();
        for source in groups.qt {
            let captured = self.clone();
            let stop = stop.clone();
            sources.push(source.recheck(move || captured.qt_hash(&stop)));
        }
        for source in groups.settings {
            let captured = self.clone();
            sources.push(source.recheck(move || {
                captured
                    .settings
                    .recheck(Instant::now() + Duration::from_secs(5))
            }));
        }
        Ok(sources)
    }
}
#[derive(Default)]
struct State {
    stored: Option<Stored>,
    captured: Option<Arc<Captured>>,
    running: bool,
    not_applicable: bool,
    code: &'static str,
    retry_requested: bool,
}
pub(super) struct Report {
    inputs: Inputs,
    /// The last download-folder status shown: for which folder, and when.
    downloads: Mutex<Option<(PathBuf, Instant, Value)>>,
    state: Mutex<State>,
    creations: Mutex<creations::CreationState>,
    local_reads: Mutex<creations::CreationState>,
    capture: Gate,
    work: Gate,
}
/// The download folder's G3 evidence by bounded stats only: evidence, never
/// download, removal or cleanup consent.
fn downloads_value(root: &std::path::Path) -> Value {
    use zerus_client_core::download_cache::evidence::{status, Evidence};
    let (code, capture_id) = match status(root) {
        Evidence::None => ("download_capture_none", None),
        Evidence::Captured(receipt) => ("download_capture_captured", Some(receipt.capture_id)),
        Evidence::Unsupported => ("download_capture_unsupported", None),
        Evidence::Unreadable => ("download_capture_unreadable", None),
    };
    json!({"code": code, "capture_id": capture_id})
}
impl Report {
    pub fn new(inputs: Inputs) -> Arc<Self> {
        Arc::new(Self {
            inputs,
            downloads: Mutex::new(None),
            state: Mutex::new(State::default()),
            creations: Mutex::new(creations::CreationState::new(
                migration::creation_report::Group::Creations,
            )),
            local_reads: Mutex::new(creations::CreationState::new(
                migration::creation_report::Group::LocalReads,
            )),
            capture: Gate::new(1).unwrap(),
            work: Gate::new(1).unwrap(),
        })
    }
    pub fn close(&self) -> Vec<zerus_client_core::desktop::drain::Closed> {
        let closed = vec![self.work.close(), self.capture.close()];
        // Ending this client does not clear the source or its durable unknown
        // receipt. Original child/pipe clones retain the actual capture charge.
        self.state().captured = None;
        closed
    }
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    pub fn hook_creations(&self, notify: Arc<dyn Fn() + Send + Sync>) {
        self.inputs.creations.hook(notify.clone());
        self.inputs.local_reads.hook(notify.clone());
        self.inputs.worktree_cache.hook(notify);
    }
    pub fn hook_drafts(&self, notify: Arc<dyn Fn() + Send + Sync>) {
        self.inputs.drafts.hook(notify);
    }
    pub fn request_retry(&self) {
        self.retry_creations();
        self.inputs.creations.request_retry();
        self.inputs.local_reads.request_retry();
        self.inputs.worktree_cache.request_retry();
        self.inputs.drafts.request_retry();
        self.state().retry_requested = true;
    }
    pub fn take_retry(&self) -> bool {
        std::mem::take(&mut self.state().retry_requested)
    }
    /// The configured download folder's evidence, reused for
    /// [`DOWNLOADS_FRESH`].
    fn downloads_status(&self) -> Value {
        let Some(folder) = &self.inputs.downloads else {
            return Value::Null;
        };
        // The folder as configured now: a cached value is reused only for
        // that same folder, never across a settings change.
        let folder = folder();
        let mut shown = self
            .downloads
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some((cached, at, value)) = shown.as_ref() {
            if *cached == folder && at.elapsed() < DOWNLOADS_FRESH {
                return value.clone();
            }
        }
        let value = downloads_value(&folder);
        *shown = Some((folder, Instant::now(), value.clone()));
        value
    }
    pub fn status(&self) -> Value {
        let downloads = self.downloads_status();
        let state = self.state();
        json!({"creations_report":self.group_status(migration::creation_report::Group::Creations),"local_reads_report":self.group_status(migration::creation_report::Group::LocalReads),"creations_capture":self.inputs.creations.status(),"local_read_capture":self.inputs.local_reads.status(),"drafts_capture":self.inputs.drafts.status(),"downloads_capture":downloads,"worktree_cache_capture":self.inputs.worktree_cache.status(),"scope":SCOPE,"running":state.running,"code":state.code,
            "required":!state.not_applicable,
            "complete":state.stored.as_ref().is_some_and(|s| s.record.phase == Phase::Published),
            "import_id":state.stored.as_ref().map(|s|&s.record.import_id),
            "phase":state.stored.as_ref().map(|s|s.record.phase),
            "import_record":state.stored.as_ref().and_then(|s|s.record.expected.as_ref()).map(|r|&r.import_record)})
    }
    fn save(
        &self,
        journal: &Journal,
        change: impl FnOnce(&mut Record) -> Result<(), SourceError>,
    ) -> Result<Record, SourceError> {
        let mut state = self.state();
        let old = state.stored.as_ref().ok_or(SourceError::InvalidMetadata)?;
        let mut record = old.record.clone();
        change(&mut record)?;
        let stored = journal.save(record, Some(&old.sha256))?;
        let record = stored.record.clone();
        state.stored = Some(stored);
        Ok(record)
    }
    fn run(
        self: &Arc<Self>,
        relay: Arc<zerus_client_core::desktop::relay::Connection>,
        configuration: String,
        stop: Arc<AtomicBool>,
        explicit: bool,
        work: Arc<dyn Send + Sync>,
    ) -> Result<(), &'static str> {
        let result = self.run_inner(relay, configuration, stop, explicit, work);
        // Do not leave ordinary preferences frozen by a report outcome. Every
        // original child/pipe still owns its own capture clone until it really
        // finishes. The journal keeps the original identity and receipt; an
        // explicit known-negative retry takes a fresh same-source capture.
        self.state().captured = None;
        result
    }
    fn run_inner(
        self: &Arc<Self>,
        relay: Arc<zerus_client_core::desktop::relay::Connection>,
        configuration: String,
        stop: Arc<AtomicBool>,
        explicit: bool,
        work: Arc<dyn Send + Sync>,
    ) -> Result<(), &'static str> {
        let journal = Arc::new(
            Journal::new(&self.inputs.data, &configuration)
                .map_err(|_| "retained_import_configuration")?,
        );
        {
            let mut state = self.state();
            if state.stored.is_none() {
                state.stored = journal
                    .load()
                    .map_err(|_| "retained_import_record_unavailable")?;
            }
            if state
                .stored
                .as_ref()
                .is_some_and(|s| s.record.configuration != configuration)
            {
                return Err("retained_import_configuration");
            }
        }
        let existing = self.state().stored.as_ref().map(|s| s.record.clone());
        // Already handled reports are inspected by exact identity. They never
        // re-seed or recapture changed Qt values after rollback/upgrade.
        if let Some(record) = existing.as_ref().filter(|r| {
            matches!(
                r.phase,
                Phase::Published | Phase::Unknown | Phase::Attempted
            )
        }) {
            let hold: Arc<dyn Send + Sync> = Arc::new((relay.clone(), self.clone(), work));
            let path = relay
                .executable()
                .map_err(|_| "paired_desktop_cli_unavailable")?;
            let publisher = Publisher::probe_retained(path, &[], hold, stop)
                .map_err(|_| "retained_import_cli_unavailable")?;
            if publisher.configuration() != configuration {
                return Err("retained_import_configuration");
            }
            let status = publisher
                .status(&record.import_id)
                .map_err(|_| "retained_import_result_unavailable")?;
            if let (
                Some(expected),
                ImportStatus::Present {
                    evidence,
                    pages,
                    import_record,
                },
            ) = (&record.expected, status)
            {
                if evidence == expected.evidence
                    && pages == expected.pages
                    && import_record == expected.import_record
                {
                    self.save(&journal, |r| {
                        r.phase = Phase::Published;
                        Ok(())
                    })
                    .map_err(|_| "retained_import_record_unavailable")?;
                    self.state().captured = None;
                    return Ok(());
                }
            }
            return Err("retained_import_original_result_unconfirmed");
        }
        if existing
            .as_ref()
            .is_some_and(|r| r.phase == Phase::Quarantined)
        {
            return Err("retained_import_source_kept_for_review");
        }
        if existing
            .as_ref()
            .is_some_and(|r| r.phase == Phase::NotCommitted)
            && !explicit
        {
            return Err("retained_import_handoff_busy");
        }
        let retained = { self.state().captured.clone() };
        let captured = if let Some(captured) = retained {
            captured
        } else {
            let charge = self
                .capture
                .admit(Role::Operations)
                .map_err(|_| "retained_import_source_resources_pending")?;
            let qt_resources: Arc<dyn Send + Sync> = Arc::new((charge, relay.clone()));
            let store = self
                .inputs
                .qt
                .clone()
                .ok_or("retained_import_qt_unavailable")?;
            let Some(qt) = store
                .capture_for_migration(qt_resources.clone(), &stop)
                .map_err(|_| "retained_import_qt_unavailable")?
            else {
                self.state().not_applicable = true;
                return Ok(());
            };
            let settings = Arc::new(
                self.inputs
                    .settings
                    .capture_for_migration(Instant::now() + Duration::from_secs(30))
                    .map_err(|_| "retained_import_settings_unavailable")?,
            );
            let resources: Arc<dyn Send + Sync> = Arc::new((qt_resources, settings.clone()));
            let captured = Arc::new(Captured {
                qt,
                settings,
                resources,
            });
            self.state().captured = Some(captured.clone());
            captured
        };
        if stop.load(Ordering::Acquire) {
            return Err("stopped");
        }
        if let Some(record) = &existing {
            if record.qt_sha256 != captured.qt.snapshot().sha256
                || record.settings_sha256 != captured.settings.sha256()
            {
                self.save(&journal, |record| {
                    record.phase = Phase::Quarantined;
                    Ok(())
                })
                .map_err(|_| "retained_import_record_unavailable")?;
                return Err("retained_import_source_changed");
            }
        } else {
            let record = Record {
                version: 1,
                scope: SCOPE.into(),
                configuration: configuration.clone(),
                import_id: uuid::Uuid::new_v4().to_string(),
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|_| "retained_import_clock_unavailable")?
                    .as_secs(),
                qt_sha256: captured.qt.snapshot().sha256.clone(),
                settings_sha256: captured.settings.sha256().into(),
                attempt: 0,
                phase: Phase::Reserved,
                expected: None,
            };
            let stored = journal
                .save(record, None)
                .map_err(|_| "retained_import_record_unavailable")?;
            self.state().stored = Some(stored);
        }
        let record = self.state().stored.as_ref().unwrap().record.clone();
        let sources = captured.sources(stop.clone()).map_err(|_| {
            let _ = self.save(&journal, |r| {
                r.phase = Phase::Quarantined;
                Ok(())
            });
            "retained_import_source_kept_for_review"
        })?;
        let bundle = migration::Bundle::new(
            crate::identity::SWITCH_ID,
            false,
            configuration.clone(),
            record.import_id.clone(),
            record.created_at,
            sources,
            64 << 20,
        )
        .map_err(|_| "retained_import_source_kept_for_review")?;
        let hold: Arc<dyn Send + Sync> = Arc::new((captured.clone(), work));
        let path = relay
            .executable()
            .map_err(|_| "paired_desktop_cli_unavailable")?;
        let publisher = Publisher::probe_retained(path, &[], hold, stop.clone())
            .map_err(|_| "retained_import_cli_unavailable")?;
        if publisher.configuration() != configuration {
            return Err("retained_import_configuration");
        }
        let attempt = self
            .save(&journal, |r| {
                if r.attempt == MAX_ATTEMPTS {
                    return Err(SourceError::TooLarge);
                }
                r.attempt += 1;
                r.phase = Phase::Attempted;
                Ok(())
            })
            .map_err(|_| "retained_import_record_unavailable")?
            .attempt;
        let report = self.clone();
        let save = journal.clone();
        let result = publisher.publish_with_receipt(bundle, move |expected| {
            // Recheck the complete original capture again after every family
            // has been consumed, immediately before the final record. These
            // are endpoint checks, never a claim that Qt's writer drained.
            captured.qt_hash(&stop)?;
            captured
                .settings
                .recheck(Instant::now() + Duration::from_secs(5))?;
            report
                .save(&save, |r| {
                    if r.attempt != attempt {
                        return Err(SourceError::SourceChanged);
                    }
                    r.expected = Some(expected.clone());
                    Ok(())
                })
                .map(|_| ())
        });
        let (phase, code) = match result {
            Ok(_) => (Phase::Published, None),
            Err(error) if error.reason == PublishError::HandoffBusy => {
                (Phase::NotCommitted, Some("retained_import_handoff_busy"))
            }
            Err(error) if matches!(error.reason, PublishError::Source(_)) => (
                Phase::Quarantined,
                Some("retained_import_source_kept_for_review"),
            ),
            Err(_) => (
                Phase::Unknown,
                Some("retained_import_original_result_unconfirmed"),
            ),
        };
        self.save(&journal, |r| {
            r.phase = phase;
            Ok(())
        })
        .map_err(|_| "retained_import_record_unavailable")?;
        if let Some(code) = code {
            return Err(code);
        }
        self.state().captured = None;
        Ok(())
    }
}

pub(super) async fn before_grant(
    shared: &Arc<Shared>,
    relay: Arc<zerus_client_core::desktop::relay::Connection>,
    configuration: String,
    stop: Arc<AtomicBool>,
    explicit: bool,
) {
    let result = attempt(
        shared,
        relay.clone(),
        configuration.clone(),
        stop.clone(),
        explicit,
    )
    .await;
    creations::arm(shared, relay, configuration, stop, explicit);
    if let Err(code) = result {
        record_error(shared, code);
    } else {
        shared.publish_retained_import();
    }
}

pub(super) fn record_error(shared: &Shared, code: &'static str) {
    if let Some(report) = &shared.retained_import {
        report.state().code = code;
    }
    shared.publish_retained_import();
}

async fn attempt(
    shared: &Arc<Shared>,
    relay: Arc<zerus_client_core::desktop::relay::Connection>,
    configuration: String,
    stop: Arc<AtomicBool>,
    explicit: bool,
) -> Result<(), &'static str> {
    let Some(report) = shared.retained_import.clone() else {
        return Ok(());
    };
    let charge = report
        .work
        .admit(Role::Operations)
        .map_err(|_| "retained_import_source_resources_pending")?;
    {
        let mut state = report.state();
        if state.running {
            return Err("retained_import_running");
        }
        state.running = true;
        state.code = "retained_import_running";
    }
    shared.publish_retained_import();
    let worker = report.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        worker.run(relay, configuration, stop, explicit, Arc::new(charge))
    })
    .await
    .map_err(|_| "retained_import_worker_unavailable")
    .and_then(|r| r);
    if shared.stopped.load(Ordering::Acquire) {
        report.close();
    }
    let mut state = report.state();
    state.running = false;
    state.code = result
        .as_ref()
        .err()
        .copied()
        .unwrap_or(if state.not_applicable {
            "retained_import_qt_absent"
        } else {
            "retained_import_staged"
        });
    result
}

#[cfg(all(test, unix))]
mod tests;
