//! Windows: a process-lifetime one-shot that starts the connector the person
//! left running, on the same effects lane and with the same start call as
//! Start connector, using only the connector packaged beside this app. Busy,
//! unknown or unavailable means skip: nothing is queued, retried or watched,
//! and this never stops a connector. `mobile::autostart` holds the rules.
use super::*;
use connector_actions::Active;
use mobile::connector_actions as core;
use mobile::enrollment::Target;
use std::path::{Path, PathBuf};
use zerus_client_core::mobile::autostart::{FILE, MAX_RECORD};

type Packaged = Box<dyn Fn() -> Option<PathBuf> + Send + Sync>;
type Start = Box<dyn Fn(&Path, &Target) -> Result<core::Answer, String> + Send + Sync>;

/// What the attempt needs from the computer; each call blocks.
pub(super) struct Io {
    pub configured: Box<dyn Fn() -> bool + Send + Sync>,
    pub packaged: Packaged,
    pub status: CallWith<Path, Workspace>,
    pub save: CallWith<[u8], ()>,
    pub start: Start,
}

/// The actual installed Windows app, outside a replay, owning its instance.
pub(super) fn allowed(windows: bool, installed: bool, replay: bool, instance_owner: bool) -> bool {
    windows && installed && !replay && instance_owner
}

pub(super) fn record_path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// The saved record, at most one byte past what a real one may hold.
pub(super) fn read_record(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_RECORD as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

pub(super) fn io(record: PathBuf) -> Io {
    Io {
        configured: Box::new(|| mobile::config_path().is_file()),
        packaged: Box::new(|| mobile::packaged_connector(std::env::current_exe().ok()?.parent()?)),
        status: Box::new(|connector| mobile::read_status(connector, &mobile::config_path())),
        save: Box::new(move |bytes| {
            zerus_client_core::fsx::write_private_atomic(&record, bytes, true)
                .map_err(|error| error.to_string())
        }),
        start: Box::new(|connector, target| {
            core::apply(connector, &mobile::config_path(), target, core::Kind::Start)
                .map_err(|failure| failure.message())
        }),
    }
}

impl Inner {
    pub(super) fn enable_autostart(&mut self, stored: Option<&[u8]>, io: Io) {
        self.page
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .enable_autostart(stored);
        self.autostart = Some(io);
    }

    /// A status answer, with the choice it changed saved outside the page lock.
    pub(super) fn answered(&self, answer: Result<Workspace, String>) -> bool {
        let changed = self.page().workspace_answered(answer);
        self.save_autostart();
        changed
    }

    /// A record that could not be saved is surfaced and offered again by the next
    /// answer. Held from before the snapshot until the write ends, so an older
    /// record still on its way to the disk can never land after a newer one, and
    /// a Stop returns from here only once its disarm is the last write.
    pub(super) fn save_autostart(&self) {
        let Some(io) = &self.autostart else {
            return;
        };
        let _order = self
            .autostart_saves
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(bytes) = self.page().autostart_unsaved() else {
            return;
        };
        match (io.save)(&bytes) {
            Ok(()) => self.page().autostart_save_succeeded(),
            Err(error) => {
                eprintln!("zerus-client: cannot save the connector choice: {error}");
                self.page().autostart_save_failed();
            }
        }
    }

    /// A workspace change stops a running connector (`enrollment::enroll`), which
    /// is the person's Stop too: disarm it before the connector is told.
    pub(super) fn enrollment_disarm(&self, target: &Target) {
        if target.changing() && target.encrypted && target.running {
            self.page()
                .autostart_stop_chosen(&target.node_id, target.generation.as_deref());
            self.save_autostart();
        }
    }

    /// This GUI run's one attempt, consumed before anything can start.
    pub(super) fn autostart_once(self: &Arc<Self>) {
        if self.autostart_attempted.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(io) = self.autostart.as_ref() else {
            return;
        };
        if self.blocked || !(io.configured)() {
            return;
        }
        let Some(connector) = (io.packaged)() else {
            self.page().autostart_unavailable();
            (self.notify)();
            return;
        };
        if self.effects.swap(true, Ordering::AcqRel) {
            return;
        }
        let _active = Active(&self.effects);
        if !self.page().read_workspace() {
            return;
        }
        (self.notify)();
        let answer = (io.status)(&connector);
        // One lock: no Stop or read can come between the arm check and its plan.
        let (changed, target) = {
            let mut page = self.page();
            let changed = page.workspace_answered(answer);
            (changed, page.autostart_begin())
        };
        self.save_autostart();
        (self.notify)();
        if changed {
            self.phones();
        }
        let Some(target) = target else {
            return;
        };
        match (io.start)(&connector, &target) {
            Ok(answer) => {
                if self.answered(Ok(answer.workspace)) {
                    self.phones();
                }
                self.page().autostart_finished(None);
            }
            Err(reason) => {
                // Uncertain: the page's next read says what happened; this run
                // makes no further automatic call.
                let mut page = self.page();
                page.workspace_answered(Err(mobile::WORKSPACE_UNCONFIRMED.into()));
                page.autostart_finished(Some(reason));
            }
        }
        (self.notify)();
    }

    pub(super) fn spawn_autostart(self: &Arc<Self>) {
        let inner = Arc::clone(self);
        if !spawn("mobile-autostart", move || inner.autostart_once()) {
            eprintln!("zerus-client: cannot start the connector automatically");
        }
    }
}

#[cfg(test)]
mod tests;
