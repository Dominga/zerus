//! The Mobile connection page's state, kept for the window's life as Qt
//! keeps its panels: one workspace status read, one phones sweep and one
//! notification timing call at a time, and the last answer of each. A
//! refresh asked for while the phones are being read runs once after it,
//! as PhonePairing merges it (PhonePairing.cpp:323-326); a status read or a
//! timing call asked for while one runs is refused, as WorkspaceSettings and
//! MobileDeliverySettings refuse it.

use serde::Serialize;
use serde_json::Value;

use super::{Phones, Workspace, DELIVERY};

/// A stale sync service is named after this long (WorkerNoticeMs).
pub const WORKER_NOTICE_MS: i64 = 30_000;
/// How long a confirmed save outranks an older snapshot's value.
const CONFIRMED_MS: i64 = 15_000;

/// What the page shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct View {
    pub workspace: Option<Workspace>,
    /// Why the last status read failed.
    pub workspace_error: Option<String>,
    pub workspace_busy: bool,
    pub phones: Option<Phones>,
    pub phones_busy: bool,
    /// The shared timing; None until read.
    pub delivery: Option<String>,
    pub delivery_busy: bool,
    pub delivery_status: String,
    /// Computers this one syncs with directly that are too old for the
    /// timing (from the 5-second swarm read).
    pub delivery_outdated: Option<String>,
    /// This computer's sync service is outdated.
    pub delivery_worker: Option<String>,
    /// The timing could not be read: nothing to choose until it is.
    pub delivery_failed: bool,
    /// Grows with every change: a view older than one shown is stale.
    pub version: u64,
}

/// A notification timing call to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Delivery {
    Get,
    Set(String),
}

#[derive(Debug, Default)]
pub struct Page {
    workspace: Option<Workspace>,
    workspace_error: Option<String>,
    workspace_busy: bool,
    /// The last connection revision read, kept across failed reads, as Qt
    /// keeps its snapshot and clears only `known`.
    revision: Option<String>,
    phones: Option<Phones>,
    phones_busy: bool,
    phones_queued: bool,
    delivery: Option<String>,
    delivery_call: Option<Delivery>,
    delivery_status: String,
    load_failed: bool,
    /// A confirmed save and when it was confirmed.
    confirmed: Option<(String, i64)>,
    outdated: Option<String>,
    worker_since: Option<i64>,
    /// A snapshot with a worker was seen.
    worker_seen: bool,
    worker: bool,
    version: u64,
}

fn restart() -> &'static str {
    if cfg!(target_os = "macos") {
        "launchctl kickstart -k gui/$(id -u)/com.hgdev.hgs-swarm"
    } else {
        "systemctl --user restart hgs-swarm.service"
    }
}

impl Page {
    pub fn new() -> Page {
        Page::default()
    }

    fn changed(&mut self) {
        self.version += 1;
    }

    /// Refresh status, and opening the section: false while one runs.
    pub fn read_workspace(&mut self) -> bool {
        if self.workspace_busy {
            return false;
        }
        self.workspace_busy = true;
        self.changed();
        true
    }

    /// The status read answered. True when the connection's revision
    /// changed from a known one, which reads the phones again (Qt's
    /// connectionChanged); the first read is no change.
    pub fn workspace_answered(&mut self, answer: Result<Workspace, String>) -> bool {
        self.workspace_busy = false;
        self.changed();
        match answer {
            Ok(workspace) => {
                let changed = self
                    .revision
                    .as_ref()
                    .is_some_and(|known| *known != workspace.config_revision);
                self.revision = Some(workspace.config_revision.clone());
                self.workspace = Some(workspace);
                self.workspace_error = None;
                changed
            }
            Err(error) => {
                self.workspace = None;
                self.workspace_error = Some(error);
                false
            }
        }
    }

    /// Refresh, and showing the section: true when a sweep starts now;
    /// while one runs it runs once more after it.
    pub fn read_phones(&mut self) -> bool {
        if self.phones_busy {
            self.phones_queued = true;
            return false;
        }
        self.phones_busy = true;
        self.changed();
        true
    }

    /// A sweep finished: true when the merged refresh runs now.
    pub fn phones_answered(&mut self, phones: Phones) -> bool {
        self.phones = Some(phones);
        self.changed();
        if std::mem::take(&mut self.phones_queued) {
            return true;
        }
        self.phones_busy = false;
        false
    }

    /// Opening Settings reads the timing unless a call runs.
    pub fn read_delivery(&mut self) -> Option<Delivery> {
        if self.delivery_call.is_some() {
            return None;
        }
        self.delivery_call = Some(Delivery::Get);
        self.changed();
        Some(Delivery::Get)
    }

    /// The person chose `value`: nothing while a call runs or for the value
    /// shown, and only Qt's values.
    pub fn set_delivery(&mut self, value: &str) -> Option<Delivery> {
        if self.delivery_call.is_some()
            || self.delivery.as_deref() == Some(value)
            || !DELIVERY.contains(&value)
        {
            return None;
        }
        let call = Delivery::Set(value.to_owned());
        self.delivery_call = Some(call.clone());
        self.delivery_status = "Saving…".into();
        self.changed();
        Some(call)
    }

    /// A timing call answered. A failed save reads the value again, as Qt's
    /// does: Some(Get) is that read.
    pub fn delivery_answered(
        &mut self,
        answer: Result<String, String>,
        now_ms: i64,
    ) -> Option<Delivery> {
        let call = self.delivery_call.take();
        self.changed();
        let error = |error: String| {
            if error.is_empty() {
                "unexpected response from hgs".to_owned()
            } else {
                error
            }
        };
        match (call, answer) {
            (Some(Delivery::Set(_)), Ok(value)) => {
                self.confirmed = Some((value.clone(), now_ms));
                self.delivery = Some(value);
                self.delivery_status =
                    "Saved. Your other computers and the phone receive it with the next sync."
                        .into();
                None
            }
            (Some(Delivery::Set(_)), Err(failed)) => {
                self.delivery_status = format!("Could not save: {}", error(failed));
                self.read_delivery()
            }
            (_, Ok(value)) => {
                if std::mem::take(&mut self.load_failed) {
                    self.delivery_status.clear();
                }
                self.delivery = Some(value);
                None
            }
            (_, Err(failed)) => {
                self.load_failed = true;
                self.delivery_status =
                    format!("Could not read the shared setting: {}", error(failed));
                None
            }
        }
    }

    /// A timing call that could not start: it fails, and so does the read a
    /// failed save asks for, so no call stays held.
    pub fn delivery_unstarted(&mut self, now_ms: i64) {
        while self
            .delivery_answered(Err("hgs did not respond".into()), now_ms)
            .is_some()
        {}
    }

    /// A sweep that could not start ends, with a refresh merged meanwhile.
    pub fn phones_unstarted(&mut self, failed: Phones) {
        while self.phones_answered(failed.clone()) {}
    }

    /// MobileDeliverySettings::setSwarmSnapshot: the 5-second swarm read
    /// keeps the timing current and names direct peers too old for it, or
    /// a local sync service that stays outdated.
    pub fn swarm_snapshot(&mut self, snapshot: &Value, now_ms: i64) {
        let before = (
            self.delivery.clone(),
            self.outdated.clone(),
            self.worker,
            self.delivery_status.clone(),
        );
        // An older hgs reports no worker and leaves the notice unchanged.
        // Qt follows the read from the start; this page sees it only while
        // open, so a service already stale when first seen is named at once.
        if let Some(state) = snapshot["worker"].as_object() {
            let stale = state.get("running").and_then(Value::as_bool) == Some(true)
                && state.get("current").and_then(Value::as_bool) != Some(true);
            let first = !std::mem::replace(&mut self.worker_seen, true);
            if !stale {
                self.worker_since = None;
            } else if self.worker_since.is_none() {
                self.worker_since = Some(if first {
                    now_ms - WORKER_NOTICE_MS
                } else {
                    now_ms
                });
            }
            self.worker = self
                .worker_since
                .is_some_and(|since| now_ms - since >= WORKER_NOTICE_MS);
        }
        let value = snapshot["preferences"]["mobile_delivery"]
            .as_str()
            .unwrap_or("");
        // A snapshot taken before a confirmed save may arrive after it: a
        // differing value is ignored until one agrees or 15 s pass.
        let stale = self
            .confirmed
            .as_ref()
            .is_some_and(|(confirmed, at)| value != confirmed && now_ms - at < CONFIRMED_MS);
        if !stale {
            self.confirmed = None;
        }
        let setting = matches!(self.delivery_call, Some(Delivery::Set(_)));
        if !setting
            && !stale
            && DELIVERY.contains(&value)
            && (self.delivery.as_deref() != Some(value) || self.load_failed)
        {
            self.delivery = Some(value.to_owned());
            if std::mem::take(&mut self.load_failed) {
                self.delivery_status.clear();
            }
        }
        // Commands without peers (an older hgs) leave the list unchanged.
        if let Some(peers) = snapshot["peers"].as_object() {
            let names: Vec<&str> = peers
                .iter()
                .filter(|(_, peer)| peer["catalog_outdated"] == Value::Bool(true))
                .map(|(name, _)| name.as_str())
                .collect();
            self.outdated = match names.as_slice() {
                [] => None,
                [one] => Some(format!(
                    "Update Zerus on {one}, a computer this one synchronizes with directly. Until then it keeps syncing projects but does not receive this choice."
                )),
                many => Some(format!(
                    "Update Zerus on these computers this one synchronizes with directly: {}. Until then they keep syncing projects but do not receive this choice.",
                    many.join(", ")
                )),
            };
        }
        let after = (
            self.delivery.clone(),
            self.outdated.clone(),
            self.worker,
            self.delivery_status.clone(),
        );
        if before != after {
            self.changed();
        }
    }

    /// Says why nothing is read (a replay).
    pub fn refuse(&mut self, reason: &str) {
        self.workspace_error = Some(reason.into());
        self.delivery_status = reason.into();
        self.phones = Some(Phones {
            lists: Vec::new(),
            lines: vec![reason.into()],
        });
        self.changed();
    }

    pub fn view(&self) -> View {
        View {
            workspace: self.workspace.clone(),
            workspace_error: self.workspace_error.clone(),
            workspace_busy: self.workspace_busy,
            phones: self.phones.clone(),
            phones_busy: self.phones_busy,
            delivery: self.delivery.clone(),
            delivery_busy: self.delivery_call.is_some(),
            delivery_status: self.delivery_status.clone(),
            delivery_outdated: self.outdated.clone(),
            delivery_failed: self.load_failed,
            delivery_worker: self.worker.then(|| {
                format!(
                    "The sync service on this computer is outdated and must be restarted ({}, or sign out and back in). Until then this computer may stop syncing projects and this choice.",
                    restart()
                )
            }),
            version: self.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{workspace, Phones};
    use super::*;
    use serde_json::json;

    fn read(revision: &str) -> Workspace {
        let value =
            json!({"state": "unconfigured", "encrypted": false, "config_revision": revision});
        workspace(value.as_object().unwrap())
    }

    #[test]
    fn one_status_read_at_a_time_and_a_new_connection_reads_the_phones() {
        let mut page = Page::new();
        assert!(page.read_workspace());
        assert!(!page.read_workspace());
        assert!(page.view().workspace_busy);
        // The first answer is no change.
        assert!(!page.workspace_answered(Ok(read("a"))));
        assert!(page.read_workspace());
        assert!(!page.workspace_answered(Ok(read("a"))));
        page.read_workspace();
        assert!(page.workspace_answered(Ok(read("b"))));
        page.read_workspace();
        assert!(!page.workspace_answered(Err("Workspace operation was not confirmed.".into())));
        let view = page.view();
        assert!(view.workspace.is_none() && !view.workspace_busy);
        assert_eq!(
            view.workspace_error.as_deref(),
            Some("Workspace operation was not confirmed.")
        );
        // The revision survives a failed read: a change across it still
        // reads the phones again, and the same one does not.
        page.read_workspace();
        assert!(page.workspace_answered(Ok(read("c"))));
        page.read_workspace();
        page.workspace_answered(Err("timed out".into()));
        page.read_workspace();
        assert!(!page.workspace_answered(Ok(read("c"))));
        page.read_workspace();
        page.workspace_answered(Err("timed out".into()));
        page.read_workspace();
        assert!(page.workspace_answered(Ok(read("d"))));
    }

    #[test]
    fn calls_that_cannot_start_leave_nothing_held() {
        let mut page = Page::new();
        page.read_delivery();
        page.delivery_answered(Ok("away".into()), 0);
        assert!(page.set_delivery("away_5").is_some());
        page.delivery_unstarted(0);
        assert!(!page.view().delivery_busy);
        assert!(page.read_delivery().is_some());
        // A sweep with a refresh merged into it.
        assert!(page.read_phones());
        page.read_phones();
        page.phones_unstarted(Phones::default());
        assert!(!page.view().phones_busy);
        assert!(page.read_phones());
    }

    #[test]
    fn refreshes_during_a_sweep_run_it_once_more() {
        let mut page = Page::new();
        assert!(page.read_phones());
        // Showing the section again, Refresh, and a new connection.
        assert!(!page.read_phones());
        assert!(!page.read_phones());
        assert!(!page.read_phones());
        let phones = Phones {
            lists: Vec::new(),
            lines: vec!["1 phone".into()],
        };
        assert!(page.phones_answered(phones.clone()));
        assert!(page.view().phones_busy);
        assert!(!page.phones_answered(phones.clone()));
        let view = page.view();
        assert!(!view.phones_busy);
        assert_eq!(view.phones, Some(phones));
        assert!(page.read_phones());
    }

    #[test]
    fn one_timing_call_at_a_time_and_a_failed_save_reads_again() {
        let mut page = Page::new();
        assert_eq!(page.read_delivery(), Some(Delivery::Get));
        assert_eq!(page.read_delivery(), None);
        assert_eq!(page.set_delivery("away"), None);
        assert_eq!(page.delivery_answered(Ok("away_5".into()), 0), None);
        assert_eq!(page.view().delivery.as_deref(), Some("away_5"));
        // The value shown, or one Qt does not offer, sends nothing.
        assert_eq!(page.set_delivery("away_5"), None);
        assert_eq!(page.set_delivery("sometimes"), None);
        assert_eq!(
            page.set_delivery("away_10"),
            Some(Delivery::Set("away_10".into()))
        );
        assert_eq!(page.view().delivery_status, "Saving…");
        assert_eq!(page.read_delivery(), None);
        // A save that timed out may have landed: the value is read again.
        assert_eq!(
            page.delivery_answered(Err("timed out".into()), 0),
            Some(Delivery::Get)
        );
        let view = page.view();
        assert!(view.delivery_busy);
        assert_eq!(view.delivery_status, "Could not save: timed out");
        assert_eq!(page.delivery_answered(Ok("away_10".into()), 0), None);
        assert_eq!(page.view().delivery.as_deref(), Some("away_10"));
        assert_eq!(
            page.set_delivery("away"),
            Some(Delivery::Set("away".into()))
        );
        assert_eq!(page.delivery_answered(Ok("away".into()), 1_000), None);
        assert_eq!(
            page.view().delivery_status,
            "Saved. Your other computers and the phone receive it with the next sync."
        );
        // A failed read says so, with nothing to choose, until a value
        // comes in; the value shown before stays shown.
        let mut failed = Page::new();
        failed.read_delivery();
        failed.delivery_answered(Ok("away_5".into()), 0);
        failed.read_delivery();
        failed.delivery_answered(Err(String::new()), 0);
        let view = failed.view();
        assert_eq!(
            view.delivery_status,
            "Could not read the shared setting: unexpected response from hgs"
        );
        assert!(view.delivery_failed);
        assert_eq!(view.delivery.as_deref(), Some("away_5"));
        failed.swarm_snapshot(&json!({"preferences": {"mobile_delivery": "away"}}), 0);
        let view = failed.view();
        assert_eq!(
            (view.delivery.as_deref(), view.delivery_status.as_str()),
            (Some("away"), "")
        );
    }

    #[test]
    fn the_swarm_read_keeps_the_timing_current_and_names_what_is_outdated() {
        let mut page = Page::new();
        page.read_delivery();
        page.delivery_answered(Ok("away_5".into()), 0);
        page.set_delivery("away_10");
        page.delivery_answered(Ok("away_10".into()), 1_000);
        // A snapshot taken before the save does not put the old value back
        // for 15 s, unless one agrees.
        let old = json!({"preferences": {"mobile_delivery": "away_5"}});
        page.swarm_snapshot(&old, 5_000);
        assert_eq!(page.view().delivery.as_deref(), Some("away_10"));
        page.swarm_snapshot(&old, 16_001);
        assert_eq!(page.view().delivery.as_deref(), Some("away_5"));
        // Another computer's change while nothing is being saved.
        page.swarm_snapshot(
            &json!({"preferences": {"mobile_delivery": "immediate"}}),
            17_000,
        );
        assert_eq!(page.view().delivery.as_deref(), Some("immediate"));
        // Direct peers too old for the timing.
        page.swarm_snapshot(
            &json!({"peers": {"mac": {"catalog_outdated": true}, "lab": {}}}),
            18_000,
        );
        assert_eq!(
            page.view().delivery_outdated.as_deref(),
            Some("Update Zerus on mac, a computer this one synchronizes with directly. Until then it keeps syncing projects but does not receive this choice.")
        );
        page.swarm_snapshot(
            &json!({"peers": {"mac": {"catalog_outdated": true}, "lab": {"catalog_outdated": true}}}),
            18_500,
        );
        assert_eq!(
            page.view().delivery_outdated.as_deref(),
            Some("Update Zerus on these computers this one synchronizes with directly: lab, mac. Until then they keep syncing projects but do not receive this choice.")
        );
        // Without peers (an older hgs) the list stays.
        page.swarm_snapshot(&json!({}), 19_000);
        assert!(page.view().delivery_outdated.is_some());
        page.swarm_snapshot(&json!({"peers": {}}), 19_500);
        assert!(page.view().delivery_outdated.is_none());
        // A service that turns stale while the page watches is named after
        // 30 s, and forgotten once current.
        page.swarm_snapshot(
            &json!({"worker": {"running": true, "current": true}}),
            19_800,
        );
        let stale = json!({"worker": {"running": true, "current": false}});
        page.swarm_snapshot(&stale, 20_000);
        assert!(page.view().delivery_worker.is_none());
        page.swarm_snapshot(&stale, 49_999);
        assert!(page.view().delivery_worker.is_none());
        let version = page.view().version;
        page.swarm_snapshot(&stale, 50_000);
        let named = page.view();
        assert!(named
            .delivery_worker
            .unwrap()
            .starts_with("The sync service on this computer is outdated and must be restarted ("));
        assert!(named.version > version);
        page.swarm_snapshot(&json!({}), 60_000);
        assert!(page.view().delivery_worker.is_some());
        page.swarm_snapshot(
            &json!({"worker": {"running": true, "current": true}}),
            61_000,
        );
        assert!(page.view().delivery_worker.is_none());
        // One already stale when the page first sees it is named at once.
        let mut opened = Page::new();
        opened.swarm_snapshot(&stale, 5);
        assert!(opened.view().delivery_worker.is_some());
        // An unchanged snapshot makes no newer view.
        let version = page.view().version;
        page.swarm_snapshot(
            &json!({"worker": {"running": true, "current": true}}),
            62_000,
        );
        assert_eq!(page.view().version, version);
    }
}
