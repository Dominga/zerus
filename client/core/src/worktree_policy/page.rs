//! The Worktrees page's state, kept for the window's life as Qt keeps its
//! widget (WorktreeSettings.h): the chosen computer, the one request in
//! flight, the policy read and the status. The section can close and open
//! again while a read or a change runs; its answer still lands here.

use serde::Serialize;
use serde_json::{Map, Value};

use super::{change, Policy, Rules};

/// The computers the list offers besides this one.
pub const MAX_PEERS: usize = 32;
/// Before the first read.
pub const NOT_READ: &str = "Refresh to read this computer's cleanup rules.";
/// While a read or a change runs (Qt shows it for both).
pub const READING: &str = "Reading cleanup settings…";
pub const LOADED: &str = "Cleanup settings loaded. Uncertain removals stay for manual review.";
pub const SAVED: &str = "Cleanup settings saved on the selected computer.";

/// What the page shows.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct View {
    /// The computers besides this one, at most 32.
    pub peers: Vec<String>,
    /// The chosen computer; empty for this one.
    pub route: String,
    pub busy: bool,
    /// The rules of the policy read; None when nothing can be applied.
    pub rules: Option<Rules>,
    /// Counts the answers that set `rules`: the controls take them again
    /// whenever it changes, as Qt sets its controls from each reply.
    pub loaded: u64,
    pub status: String,
    /// The status says why nothing can be applied.
    pub failed: bool,
    /// Grows with every change: a view older than one shown is stale.
    pub version: u64,
}

/// A read (`change` None) or a change to run on `route`'s hgs; its answer
/// goes to [`Page::answered`] with `id`.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub id: u64,
    pub route: String,
    pub change: Option<Map<String, Value>>,
}

#[derive(Debug)]
pub struct Page {
    peers: Vec<String>,
    route: String,
    /// The request in flight; nothing else starts until it answers.
    running: Option<u64>,
    /// The last request's number.
    requests: u64,
    /// The policy read from `route` (Qt's m_policy); a change is made on it.
    policy: Option<Policy>,
    loaded: u64,
    status: String,
    failed: bool,
    version: u64,
}

impl Default for Page {
    fn default() -> Page {
        Page {
            peers: Vec::new(),
            route: String::new(),
            running: None,
            requests: 0,
            policy: None,
            loaded: 0,
            status: NOT_READ.into(),
            failed: false,
            version: 0,
        }
    }
}

impl Page {
    pub fn new() -> Page {
        Page::default()
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// WorktreeSettings::setPeers: ignored while a request runs; empty and
    /// repeated names are dropped and at most 32 kept. When the chosen
    /// computer is gone the page switches to this one and forgets its
    /// policy, so nothing is applied to it.
    pub fn set_peers(&mut self, peers: impl IntoIterator<Item = String>) {
        if self.busy() {
            return;
        }
        let mut kept: Vec<String> = Vec::new();
        for peer in peers {
            if kept.len() == MAX_PEERS {
                break;
            }
            if !peer.is_empty() && !kept.contains(&peer) {
                kept.push(peer);
            }
        }
        if !self.route.is_empty() && !kept.contains(&self.route) {
            self.route.clear();
            self.policy = None;
            self.version += 1;
        }
        if kept != self.peers {
            self.peers = kept;
            self.version += 1;
        }
    }

    fn begin(&mut self, change: Option<Map<String, Value>>) -> Call {
        self.requests += 1;
        self.version += 1;
        self.running = Some(self.requests);
        self.status = READING.into();
        self.failed = false;
        Call {
            id: self.requests,
            route: self.route.clone(),
            change,
        }
    }

    /// Refresh, and opening the section: reads the chosen computer's
    /// policy. None while a request runs; its answer is still to come.
    pub fn read(&mut self) -> Option<Call> {
        if self.busy() {
            return None;
        }
        Some(self.begin(None))
    }

    /// Choosing a computer (a name the list does not offer is this one):
    /// its policy is forgotten first, then read. None while a request
    /// runs, when Qt's list is disabled.
    pub fn choose(&mut self, route: &str) -> Option<Call> {
        if self.busy() {
            return None;
        }
        self.route = if self.peers.iter().any(|peer| peer == route) {
            route.to_owned()
        } else {
            String::new()
        };
        self.policy = None;
        Some(self.begin(None))
    }

    /// Apply to this computer: the policy read, with these rules. None
    /// while a request runs or with no policy read (Qt's Apply is disabled
    /// then), and for a grace the page does not offer (the status says so).
    pub fn apply(&mut self, rules: &Rules) -> Option<Call> {
        if self.busy() {
            return None;
        }
        let read = &self.policy.as_ref()?.raw;
        match change(read, rules) {
            Ok(change) => Some(self.begin(Some(change))),
            Err(error) => {
                self.status = error;
                self.failed = true;
                self.version += 1;
                None
            }
        }
    }

    /// A request answered (`changed` for a change), with Qt's status; one
    /// that is not the request in flight is dropped. A failed one leaves
    /// nothing to apply, as Qt's fail() does.
    pub fn answered(&mut self, id: u64, changed: bool, result: Result<Policy, String>) -> bool {
        if self.running != Some(id) {
            return false;
        }
        self.running = None;
        self.version += 1;
        match result {
            Ok(policy) => {
                self.policy = Some(policy);
                self.loaded += 1;
                self.status = if changed { SAVED } else { LOADED }.into();
                self.failed = false;
            }
            Err(error) => {
                self.policy = None;
                self.status = error;
                self.failed = true;
            }
        }
        true
    }

    /// Says why nothing runs (a replay).
    pub fn refuse(&mut self, reason: &str) {
        self.status = reason.into();
        self.failed = true;
        self.version += 1;
    }

    pub fn view(&self) -> View {
        View {
            peers: self.peers.clone(),
            route: self.route.clone(),
            busy: self.busy(),
            rules: self.policy.as_ref().map(|policy| policy.rules.clone()),
            loaded: self.loaded,
            status: self.status.clone(),
            failed: self.failed,
            version: self.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::answer;
    use super::*;
    use crate::actions::Exit;
    use serde_json::json;

    fn policy(enabled: bool, revision: i64) -> Policy {
        let reply = json!({"schema": 1, "policy": {"version": 1, "revision": revision, "enabled": enabled,
            "after_session": true, "unused_integrated": true, "missing": false, "grace_hours": 24}});
        answer(
            false,
            Some(Exit::Code {
                code: 0,
                stdout: reply.to_string().into_bytes(),
                stderr: String::new(),
            }),
        )
        .unwrap()
    }

    /// A page that has read this computer's policy.
    fn loaded() -> Page {
        let mut page = Page::new();
        let read = page.read().unwrap();
        assert!(page.answered(read.id, false, Ok(policy(true, 4))));
        page
    }

    #[test]
    fn one_request_runs_at_a_time_and_its_answer_outlives_the_section() {
        let mut page = loaded();
        let view = page.view();
        assert_eq!((view.status.as_str(), view.loaded), (LOADED, 1));
        let rules = Rules {
            enabled: false,
            ..view.rules.unwrap()
        };
        let set = page.apply(&rules).unwrap();
        assert_eq!(set.change.as_ref().unwrap()["enabled"], json!(false));
        assert_eq!(set.change.as_ref().unwrap()["revision"], json!(4));
        // A second click, and opening the section again, start nothing:
        // the page shows the change still running.
        assert_eq!(page.apply(&rules), None);
        assert_eq!(page.read(), None);
        assert_eq!(page.choose(""), None);
        let view = page.view();
        assert!(view.busy && !view.failed);
        assert_eq!(view.status, READING);
        // The answer shows whenever the section is next drawn.
        assert!(page.answered(set.id, true, Ok(policy(false, 5))));
        let view = page.view();
        assert_eq!(view.status, SAVED);
        assert_eq!((view.busy, view.loaded), (false, 2));
        assert!(!view.rules.unwrap().enabled);
        // Opening it once idle reads again, as SettingsPage does.
        assert!(page.read().is_some());
        // Every change makes a newer view.
        assert!(view.version < page.view().version);
    }

    #[test]
    fn a_failed_read_leaves_nothing_to_apply() {
        let mut page = loaded();
        let read = page.read().unwrap();
        assert!(page.answered(
            read.id,
            false,
            Err("Could not read cleanup settings.".into())
        ));
        let view = page.view();
        assert!(view.failed && view.rules.is_none() && !view.busy);
        assert_eq!(view.status, "Could not read cleanup settings.");
        assert_eq!(view.loaded, 1);
        let rules = policy(true, 4).rules;
        assert_eq!(page.apply(&rules), None);
        // A failed change too: Refresh before changing settings again.
        let mut page = loaded();
        let set = page.apply(&rules).unwrap();
        let unconfirmed = "Save was not confirmed. Refresh before changing settings again.";
        page.answered(set.id, true, Err(unconfirmed.into()));
        assert!(page.view().rules.is_none());
        assert_eq!(page.apply(&rules), None);
    }

    #[test]
    fn choosing_a_computer_forgets_the_policy_first_and_drops_late_answers() {
        let mut page = loaded();
        page.set_peers(["mac".to_owned()]);
        let read = page.choose("mac").unwrap();
        assert_eq!((read.route.as_str(), read.change.is_none()), ("mac", true));
        let view = page.view();
        assert!(view.rules.is_none() && view.busy);
        assert_eq!(view.route, "mac");
        // An answer for an earlier request is not this one's.
        assert!(!page.answered(read.id - 1, false, Ok(policy(true, 9))));
        assert!(page.view().rules.is_none());
        assert!(page.answered(read.id, false, Ok(policy(false, 2))));
        assert!(!page.answered(read.id, false, Ok(policy(true, 3))));
        assert!(!page.view().rules.unwrap().enabled);
        // A name the list does not offer is this computer.
        let read = page.choose("not-a-peer").unwrap();
        assert_eq!(read.route, "");
    }

    #[test]
    fn a_computer_that_leaves_is_never_applied_to() {
        let mut page = Page::new();
        page.set_peers(["mac".to_owned(), "linux".to_owned()]);
        let read = page.choose("mac").unwrap();
        // Ignored while a request runs, as Qt's setPeers is.
        page.set_peers(Vec::new());
        assert_eq!(page.view().peers, ["mac", "linux"]);
        page.answered(read.id, false, Ok(policy(true, 4)));
        page.set_peers(["linux".to_owned()]);
        let view = page.view();
        assert_eq!((view.route.as_str(), view.peers.len()), ("", 1));
        assert!(view.rules.is_none());
        assert_eq!(page.apply(&policy(true, 4).rules), None);
        // A peer that stays keeps its policy.
        let read = page.choose("linux").unwrap();
        page.answered(read.id, false, Ok(policy(true, 4)));
        page.set_peers(["linux".to_owned(), "mac".to_owned()]);
        assert!(page.view().rules.is_some());
    }

    #[test]
    fn peers_are_bounded_named_and_unique() {
        let mut page = Page::new();
        let mut peers = vec![String::new(), "a".to_owned(), "a".to_owned()];
        peers.extend((0..40).map(|n| format!("peer-{n}")));
        page.set_peers(peers);
        let kept = page.view().peers;
        assert_eq!(kept.len(), MAX_PEERS);
        assert_eq!(kept[0], "a");
        assert_eq!(kept[31], "peer-30");
    }

    #[test]
    fn a_grace_the_page_does_not_offer_is_never_sent() {
        let mut page = loaded();
        let rules = Rules {
            grace_hours: 8761,
            ..policy(true, 4).rules
        };
        assert_eq!(page.apply(&rules), None);
        let view = page.view();
        assert_eq!(
            view.status,
            "Choose a minimum inactivity of 1 to 8760 hours."
        );
        assert!(view.failed && !view.busy && view.rules.is_some());
        // Corrected, it applies.
        assert!(page.apply(&policy(true, 4).rules).is_some());
    }

    #[test]
    fn a_replay_says_why() {
        let mut page = Page::new();
        page.refuse("Cleanup settings are not read in a replay.");
        let view = page.view();
        assert!(view.failed && !view.busy);
        assert_eq!(view.status, "Cleanup settings are not read in a replay.");
        assert_eq!(Page::new().view().status, NOT_READ);
    }
}
