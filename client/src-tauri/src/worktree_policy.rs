//! Settings > Worktrees (WorktreeSettings.h): a computer's cleanup rules,
//! read and changed through its own `hgs` on a thread of its own. The page
//! lives here for the window's life, as Qt's widget does: closing the
//! section loses neither the chosen computer nor a request in flight, and
//! "worktree-policy-changed" tells the page its answer came. Nothing here
//! removes a checkout or starts a maintenance pass. A replay never drives
//! the CLI.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Emitter, Manager, State};
use zerus_client_core::default_hgs;
use zerus_client_core::worktree_policy::page::{Call, Page, View};
use zerus_client_core::worktree_policy::{self, Policy, Rules};

use crate::commands::AppState;

/// What a replay says instead of reading or changing anything.
const REPLAY: &str = "Cleanup settings are not read in a replay.";

type Run = Box<dyn Fn(&Call) -> Result<Policy, String> + Send + Sync>;
type Notify = Box<dyn Fn() + Send + Sync>;

pub struct WorktreePolicy(Arc<Inner>);

struct Inner {
    page: Mutex<Page>,
    /// A replay without an explicit CLI: nothing is read or changed.
    blocked: bool,
    /// Runs a call on the route's hgs; blocks.
    run: Run,
    /// Tells the page an answer came.
    notify: Notify,
}

pub fn start(app: &AppHandle) -> WorktreePolicy {
    let handle = app.clone();
    WorktreePolicy(Arc::new(Inner::new(
        crate::actions::blocked(),
        Box::new(|call| worktree_policy::call(&default_hgs(), &call.route, call.change.as_ref())),
        Box::new(move || {
            let _ = handle.emit("worktree-policy-changed", ());
        }),
    )))
}

impl Inner {
    fn new(blocked: bool, run: Run, notify: Notify) -> Inner {
        Inner {
            page: Mutex::new(Page::new()),
            blocked,
            run,
            notify,
        }
    }

    fn page(&self) -> MutexGuard<'_, Page> {
        self.page.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The page after following the peers this computer lists now, when
    /// known (Qt's setPeers follows every fleet update).
    fn view(&self, peers: Option<BTreeSet<String>>) -> View {
        let mut page = self.page();
        if let Some(peers) = peers {
            page.set_peers(peers);
        }
        page.view()
    }

    /// Follows the peers, then starts what `step` asks of the page; in a
    /// replay the status says why nothing runs.
    fn begin(
        self: &Arc<Self>,
        peers: Option<BTreeSet<String>>,
        step: impl FnOnce(&mut Page) -> Option<Call>,
    ) -> View {
        let call = {
            let mut page = self.page();
            if let Some(peers) = peers {
                page.set_peers(peers);
            }
            if self.blocked {
                page.refuse(REPLAY);
                return page.view();
            }
            step(&mut page)
        };
        if let Some(call) = call {
            self.spawn(call);
        }
        self.page().view()
    }

    /// Runs a call on a thread of its own; its answer lands in the page
    /// whether or not the section is still open.
    fn spawn(self: &Arc<Self>, call: Call) {
        let (id, changed) = (call.id, call.change.is_some());
        let inner = Arc::clone(self);
        let started = std::thread::Builder::new()
            .name("worktree-policy".into())
            .spawn(move || {
                let result = (inner.run)(&call);
                inner.page().answered(call.id, changed, result);
                (inner.notify)();
            });
        if started.is_err() {
            let error = if changed {
                "Save was not confirmed. Refresh before changing settings again."
            } else {
                "Could not read cleanup settings."
            };
            self.page().answered(id, changed, Err(error.into()));
        }
    }
}

fn peers(app: &AppHandle) -> Option<BTreeSet<String>> {
    app.state::<AppState>().peers()
}

/// The page as it stands: the chosen computer, a request still running or
/// its outcome.
#[tauri::command(async)]
pub fn worktree_policy_view(app: AppHandle, state: State<'_, WorktreePolicy>) -> View {
    state.0.view(peers(&app))
}

/// Refresh, and opening the section: reads the chosen computer's policy
/// unless a request still runs, whose answer is then still to come.
#[tauri::command(async)]
pub fn worktree_policy_read(app: AppHandle, state: State<'_, WorktreePolicy>) -> View {
    state.0.begin(peers(&app), Page::read)
}

/// Choosing a computer: its policy is forgotten, then read.
#[tauri::command(async)]
pub fn worktree_policy_choose(
    app: AppHandle,
    state: State<'_, WorktreePolicy>,
    route: String,
) -> View {
    state.0.begin(peers(&app), |page| page.choose(&route))
}

/// Apply to this computer: the policy read, with these rules; the CLI
/// refuses it when the policy changed meanwhile.
#[tauri::command(async)]
pub fn worktree_policy_apply(
    app: AppHandle,
    state: State<'_, WorktreePolicy>,
    rules: Rules,
) -> View {
    state.0.begin(peers(&app), |page| page.apply(&rules))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn policy(grace_hours: i64) -> Policy {
        let mut raw = serde_json::Map::new();
        raw.insert("version".into(), 1.into());
        raw.insert("revision".into(), 4.into());
        Policy {
            rules: Rules {
                enabled: true,
                after_session: true,
                unused_integrated: true,
                missing: false,
                grace_hours,
            },
            raw,
        }
    }

    /// A service whose CLI answers through `answers`; `runs` counts calls
    /// and `told` the answers announced.
    struct Fixture {
        inner: Arc<Inner>,
        runs: Arc<Mutex<Vec<Call>>>,
        answers: mpsc::Sender<Result<Policy, String>>,
        told: mpsc::Receiver<()>,
    }

    fn service(blocked: bool) -> Fixture {
        let runs = Arc::new(Mutex::new(Vec::new()));
        let (answers, replies) = mpsc::channel::<Result<Policy, String>>();
        let replies = Mutex::new(replies);
        let (tell, told) = mpsc::channel();
        let seen = Arc::clone(&runs);
        let inner = Arc::new(Inner::new(
            blocked,
            Box::new(move |call| {
                seen.lock().unwrap().push(call.clone());
                replies
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap_or_else(|_| Err("no answer".into()))
            }),
            Box::new(move || {
                let _ = tell.send(());
            }),
        ));
        Fixture {
            inner,
            runs,
            answers,
            told,
        }
    }

    fn names(names: &[&str]) -> Option<BTreeSet<String>> {
        Some(names.iter().map(|name| (*name).to_owned()).collect())
    }

    #[test]
    fn a_replay_runs_nothing_and_says_so() {
        let f = service(true);
        let views = [
            f.inner.begin(None, Page::read),
            f.inner.begin(None, |page| page.choose("")),
            f.inner.begin(None, |page| page.apply(&policy(24).rules)),
        ];
        for view in views {
            assert_eq!(view.status, REPLAY);
            assert!(view.failed && !view.busy && view.rules.is_none());
        }
        assert!(f.runs.lock().unwrap().is_empty());
    }

    #[test]
    fn an_answer_lands_after_the_section_closed_and_nothing_runs_meanwhile() {
        let f = service(false);
        let view = f.inner.begin(names(&["mac"]), |page| page.choose("mac"));
        assert!(view.busy);
        // Opening the section again while the read runs starts nothing.
        let view = f.inner.begin(names(&["mac"]), Page::read);
        assert!(view.busy && view.route == "mac");
        f.answers.send(Ok(policy(24))).unwrap();
        f.told.recv_timeout(Duration::from_secs(10)).unwrap();
        let view = f.inner.view(None);
        assert!(!view.busy && view.rules.is_some());
        assert_eq!(view.route, "mac");
        let runs = f.runs.lock().unwrap().clone();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].route, "mac");
    }

    #[test]
    fn peers_are_followed_bounded_and_named() {
        let f = service(false);
        let mut many: Vec<String> = (0..40).map(|n| format!("peer-{n}")).collect();
        many.push(String::new());
        let view = f.inner.view(Some(many.into_iter().collect()));
        assert_eq!(view.peers.len(), 32);
        assert!(view.peers.iter().all(|peer| !peer.is_empty()));
        // A peer that leaves before Apply: the page is back on this
        // computer with nothing to apply, and nothing runs.
        f.inner.begin(names(&["mac"]), |page| page.choose("mac"));
        f.answers.send(Ok(policy(24))).unwrap();
        f.told.recv_timeout(Duration::from_secs(10)).unwrap();
        let view = f
            .inner
            .begin(names(&[]), |page| page.apply(&policy(24).rules));
        assert_eq!(view.route, "");
        assert!(view.rules.is_none() && !view.busy);
        assert_eq!(f.runs.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_grace_out_of_range_never_runs_the_cli() {
        let f = service(false);
        f.inner.begin(None, Page::read);
        f.answers.send(Ok(policy(24))).unwrap();
        f.told.recv_timeout(Duration::from_secs(10)).unwrap();
        let calls = f.runs.lock().unwrap().len();
        for grace in [0, 8761] {
            let view = f.inner.begin(None, |page| page.apply(&policy(grace).rules));
            assert_eq!(
                view.status,
                "Choose a minimum inactivity of 1 to 8760 hours."
            );
            assert!(!view.busy);
        }
        assert_eq!(f.runs.lock().unwrap().len(), calls);
    }
}
