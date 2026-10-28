//! The open session's part of the composer, ported from `SessionsWindow`:
//! the block reason (`messageBlockReason`, S1), the cache chip and its
//! details (S2), and one inspect when no Activity state arrived. Task 4b adds
//! the delivery to this module.
//!
//! The ticker looks at the open composer once a second, as Qt's cache chip
//! refreshes, and sleeps while none is open. It wakes the pump only when the
//! session part of the view changed: the chip's minute, or a write, row or
//! computer status that no batch announced.

use std::sync::{Arc, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::Value;
use zerus_client_core::cache::{self, Chip};
use zerus_client_core::send::{block_reason, BlockFacts};
use zerus_client_core::{Target, ViewRow};

use super::{Composer, Composers, Hub, SessionRef};
use crate::actions::WriteKind;

/// No Activity state this long after opening: inspect once.
const DETAILS_WAIT: f64 = 3.0;
/// The ticker's period: Qt's cache chip refreshes once a second.
const TICK: Duration = Duration::from_secs(1);

/// The session's state for the composer view.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SessionView {
    pub(crate) available: bool,
    pub(crate) unavailable_reason: String,
    pub(crate) block_reason: Option<String>,
    pub(crate) attachments_available: bool,
    pub(crate) attachments_reason: String,
    pub(crate) cache: Chip,
    pub(crate) cache_warning: String,
    pub(crate) cache_details: String,
}

/// The agent command the send rules read: the row says `deepseek` for `dsh`.
pub(crate) fn cmd(row: &ViewRow) -> String {
    if row.agent == "deepseek" {
        "dsh".into()
    } else {
        row.agent.clone()
    }
}

/// Local time as the cache details show it (Qt's `d MMM HH:mm:ss`, English
/// month names); formatted in Rust, never in the webview.
fn local_time(seconds: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(seconds, 0)
        .single()
        .map(|time| time.format("%-d %b %H:%M:%S").to_string())
        .unwrap_or_default()
}

/// The cache expiry of `details` in local time, as `cache::details` names
/// it: the seconds and their text. Callers format it before they take the
/// hub lock, since local time may read the time zone files.
pub(crate) fn expiry(details: &Value) -> Option<(i64, String)> {
    let at = details["session_usage"]["prompt_cache"]["expires_at"]
        .as_f64()
        .filter(|at| *at > 0.0)? as i64;
    Some((at, local_time(at)))
}

/// The row's run, conversation and state an inspect read: a change of any
/// makes the inspected details stale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Basis {
    run_id: Option<String>,
    conversation_id: Option<String>,
    state: String,
}

impl Basis {
    pub(crate) fn of(row: &ViewRow) -> Self {
        Basis {
            run_id: row.run_id.clone(),
            conversation_id: row.conversation_id.clone(),
            state: row.state.clone(),
        }
    }
}

/// The open session's row, online state, details and facts.
pub(crate) struct Context {
    pub(crate) session: SessionRef,
    pub(crate) key: String,
    pub(crate) row: ViewRow,
    pub(crate) online: bool,
    pub(crate) details: Option<Arc<Value>>,
    pub(crate) facts: BlockFacts,
}

impl Context {
    pub(crate) fn reason(&self) -> Option<String> {
        block_reason(&self.facts, self.details.as_deref())
    }

    /// Qt's `recorded`: session data that is not live (`SessionsWindow.cpp:3783`).
    pub(crate) fn recorded(&self) -> bool {
        !self.online
            || matches!(self.row.state.as_str(), "archived" | "paused")
            || self.row.process_state == "exited"
            || self.facts.inspect_error
    }
}

impl Composers {
    pub(crate) fn now(&self) -> f64 {
        (self.0.deps.clock)()
    }

    /// The open session as the catalog and the followed Activity show it now.
    pub(crate) fn context(&self, hub: &Hub) -> Option<Context> {
        let session = hub.open.clone()?;
        let key = hub.open_key.clone()?;
        let (row, online) = (self.0.deps.rows)(&session.computer, &session.key, &session.name)?;
        let followed = hub.followed.computer == session.computer && hub.followed.key == session.key;
        let details = if followed {
            hub.followed.details.clone()
        } else {
            None
        };
        let write = (self.0.deps.facts)(&session.computer, &session.name);
        let facts = BlockFacts {
            start_send_waiting: false,
            compacting: false,
            clearing: false,
            terminating: write == Some(WriteKind::Terminate),
            settings_changing: false,
            online,
            state: row.state.clone(),
            updating: write == Some(WriteKind::Update),
            cmd: cmd(&row),
            inspect_error: followed && hub.followed.inspect_error,
        };
        Some(Context {
            session,
            key,
            row,
            online,
            details,
            facts,
        })
    }

    /// One `inspect` of the open session, off the UI thread; its value
    /// stands in for the stream's state until the next state frame. As Qt's
    /// `inspect()` (`SessionsWindow.cpp:3467-3492`), it reads nothing while
    /// the stream keeps the details or the computer is offline, and one
    /// inspect runs at a time. Tasks 4b and later call it after their
    /// actions, where Qt calls `inspect()`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn refresh_details(&self) {
        let hub = self.lock();
        self.inspect_with(hub);
    }

    fn inspect_with(&self, mut hub: MutexGuard<'_, Hub>) {
        if hub.closed || hub.followed.inspecting.is_some() || hub.followed.streamed {
            return;
        }
        let Some(ctx) = self.context(&hub) else {
            return;
        };
        let followed =
            hub.followed.computer == ctx.session.computer && hub.followed.key == ctx.session.key;
        if !followed || !ctx.online {
            return;
        }
        hub.next_inspect += 1;
        let number = hub.next_inspect;
        hub.followed.inspecting = Some(number);
        hub.followed.inspected = Some(Basis::of(&ctx.row));
        drop(hub);
        let target = Target {
            computer: ctx.session.computer.clone(),
            name: ctx.session.name.clone(),
            run_id: ctx.row.run_id.clone().unwrap_or_default(),
            conversation_id: ctx.row.conversation_id.clone().unwrap_or_default(),
        };
        let archive = ctx.session.archive_id;
        let me = self.clone();
        (self.0.deps.spawn)(Box::new(move || {
            let read = (me.0.deps.inspect)(&target, archive.as_deref()).map(|mut value| {
                // The batches bring the events; the composer holds none of them.
                if let Some(fields) = value.as_object_mut() {
                    fields.remove("events");
                }
                let expiry = expiry(&value);
                (Arc::new(value), expiry)
            });
            let mut hub = me.lock();
            // Another session was opened meanwhile: this answer is not its.
            if hub.followed.inspecting != Some(number) {
                return;
            }
            hub.followed.inspecting = None;
            // A state frame that arrived meanwhile is newer.
            if !hub.followed.streamed {
                match read {
                    Some((details, expiry)) => {
                        hub.followed.inspect_error = false;
                        hub.followed.details = Some(details);
                        hub.followed.expiry = expiry;
                    }
                    None => hub.followed.inspect_error = true,
                }
            }
            drop(hub);
            me.after_observe();
        }));
    }

    /// Whether the tick inspects: once when no Activity state arrived within
    /// 3 s of opening (or after the follow ended), then again only when the
    /// row's run, conversation or state changed.
    fn wants_details(&self, hub: &Hub, now: f64) -> bool {
        let followed = &hub.followed;
        if hub.open.is_none() || followed.inspecting.is_some() || followed.streamed {
            return false;
        }
        match &followed.inspected {
            None => {
                followed.details.is_none()
                    && !followed.inspect_error
                    && now - followed.opened_at >= DETAILS_WAIT
            }
            Some(basis) => self
                .context(hub)
                .is_some_and(|ctx| Basis::of(&ctx.row) != *basis),
        }
    }

    /// Once a second while a composer is open: the session part of its view
    /// (the cache chip's minute, and a write, row or status no batch
    /// announced), and the inspect fallback.
    pub fn tick(&self) {
        let now = self.now();
        let mut hub = self.lock();
        let seen = hub
            .open_key
            .as_ref()
            .and_then(|key| hub.composers.get(key))
            .map(|composer| session_view(self, &hub, composer));
        let changed = hub.seen != seen;
        hub.seen = seen;
        if self.wants_details(&hub, now) {
            self.inspect_with(hub);
        } else {
            drop(hub);
        }
        if changed {
            self.changed();
        }
    }

    /// Ticks once a second while a composer is open, until the app quits.
    pub fn start_ticker(&self) {
        self.start_ticker_every(TICK);
    }

    fn start_ticker_every(&self, every: Duration) -> JoinHandle<()> {
        let me = self.clone();
        std::thread::Builder::new()
            .name("zerus-composer".into())
            .spawn(move || {
                while me.wait_open() {
                    std::thread::sleep(every);
                    me.tick();
                }
            })
            .expect("the composer ticker starts")
    }

    /// Sleeps while no composer is open, so a closed composer never wakes
    /// the app; false once the app quits.
    fn wait_open(&self) -> bool {
        let hub = self
            .0
            .opened
            .wait_while(self.lock(), |hub| hub.open.is_none() && !hub.closed)
            .unwrap_or_else(PoisonError::into_inner);
        !hub.closed
    }
}

/// The open session's part of the composer view (`renderDetails`,
/// `SessionsWindow.cpp:3783-3791, 3982-3989`).
pub(crate) fn session_view(composers: &Composers, hub: &Hub, composer: &Composer) -> SessionView {
    let Some(ctx) = composers.context(hub).filter(|ctx| ctx.key == composer.key) else {
        return SessionView {
            unavailable_reason: "Select a session.".into(),
            attachments_available: true,
            ..SessionView::default()
        };
    };
    let details = ctx.details.as_deref().cloned().unwrap_or_default();
    let now = composers.now();
    let reason = ctx.reason();
    let attachments_available = ctx.facts.cmd != "opencode"
        && details
            .get("attachments_supported")
            .and_then(Value::as_bool)
            .unwrap_or(true);
    let attachments_reason = details
        .get("attachments_reason")
        .and_then(Value::as_str)
        .unwrap_or(
            "This native adapter does not support file attachments. Your existing files remain saved; remove them to send text.",
        )
        .to_owned();
    let recorded = ctx.recorded();
    // The expiry was formatted when the details came; local time may read
    // files, so it is never formatted under the hub lock.
    let expiry = hub.followed.expiry.as_ref();
    let local = |seconds: i64| match expiry {
        Some((at, text)) if *at == seconds => text.clone(),
        _ => local_time(seconds),
    };
    SessionView {
        available: reason.is_none(),
        unavailable_reason: reason.clone().unwrap_or_default(),
        block_reason: reason,
        attachments_available,
        attachments_reason,
        cache: cache::chip(&details, now),
        cache_warning: cache::warning(&details, now),
        cache_details: cache::details(&details, recorded, local),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::composer::tests::{dir, drafts, session, until, Scratch};
    use crate::composer::{lock, ComposerView, Composers, Config, Deps, Job};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use zerus_client_core::drafts::DraftStore;
    use zerus_client_core::{ActivityOut, Batch, ComputerId, RowOut};

    /// The session, clock and inspect a test controls.
    pub(crate) struct Fake {
        pub(crate) row: Mutex<ViewRow>,
        pub(crate) now: Mutex<f64>,
        pub(crate) inspected: Mutex<Option<Value>>,
        pub(crate) inspects: AtomicUsize,
        pub(crate) online: Mutex<bool>,
        pub(crate) write: Mutex<Option<WriteKind>>,
    }

    pub(crate) fn row(name: &str) -> ViewRow {
        ViewRow {
            name: name.into(),
            agent: "claude".into(),
            state: "running".into(),
            tracked: true,
            run_id: Some("run-1".into()),
            conversation_id: Some("conv-1".into()),
            ..ViewRow::default()
        }
    }

    pub(crate) fn send_hub_on(store: &Arc<DraftStore>) -> (Composers, Arc<Fake>) {
        let fake = Arc::new(Fake {
            row: Mutex::new(row("claude/sample-project/review")),
            now: Mutex::new(1_793_000_100.0),
            inspected: Mutex::new(None),
            inspects: AtomicUsize::new(0),
            online: Mutex::new(true),
            write: Mutex::new(None),
        });
        let (f1, f2, f3, f4) = (fake.clone(), fake.clone(), fake.clone(), fake.clone());
        let composers = Composers::new(
            Arc::clone(store),
            Config {
                cache: store.root().with_file_name("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(|| {}),
                clock: Arc::new(move || *lock(&f1.now)),
                rows: Arc::new(move |_, _, _| Some((lock(&f2.row).clone(), *lock(&f2.online)))),
                facts: Arc::new(move |_, _| *lock(&f4.write)),
                inspect: Arc::new(move |_, _| {
                    f3.inspects.fetch_add(1, Ordering::SeqCst);
                    lock(&f3.inspected).clone()
                }),
            },
        );
        (composers, fake)
    }

    pub(crate) fn send_hub(root: &Scratch) -> (Composers, Arc<Fake>) {
        send_hub_on(&drafts(root))
    }

    pub(crate) fn ready() -> Value {
        json!({"tracked": true, "run_id": "run-1", "conversation_id": "conv-1", "runtime_state": "live", "process_state": "running",
               "phase": "idle", "activity": "idle", "compact_context_supported": true, "clear_context_supported": true})
    }

    pub(crate) fn state(details: Value) -> Batch {
        Batch {
            activity: vec![ActivityOut {
                computer: ComputerId::from("studio"),
                key: "run:run-1".into(),
                kind: "state".into(),
                payload: json!({"value": details, "rev": 1}),
            }],
            ..Batch::default()
        }
    }

    pub(crate) fn opened(c: &Composers, details: Value) -> ComposerView {
        c.open(Some(session()));
        c.observe(&state(details));
        until(c, |v| v.loaded)
    }

    #[test]
    fn a_ready_session_takes_messages() {
        let root = dir("ready");
        let (c, _) = send_hub(&root);
        let view = opened(&c, ready());
        assert!(
            view.available && view.block_reason.is_none() && view.unavailable_reason.is_empty()
        );
        let handle = view.handle;
        c.edit(handle, 1, view.text_epoch, "hello".into(), 5, 5);
        assert!(c.current().unwrap().can_send);
    }

    #[test]
    fn a_block_reason_is_shown() {
        let root = dir("blocked");
        let (c, _) = send_hub(&root);
        let mut details = ready();
        details["phase"] = json!("approval");
        let view = opened(&c, details);
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Respond to the agent in the Terminal tab.")
        );
        assert_eq!(
            view.unavailable_reason,
            "Respond to the agent in the Terminal tab."
        );
        assert!(!view.available);
        c.edit(view.handle, 1, view.text_epoch, "hi".into(), 2, 2);
        let view = c.current().unwrap();
        assert!(!view.can_send);
        assert_eq!(view.status, "Respond to the agent in the Terminal tab.");
    }

    #[test]
    fn the_cache_chip_counts_down_in_minutes() {
        let root = dir("chip");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["session_usage"] =
            json!({"prompt_cache": {"status": "warm", "expires_at": 1_793_000_100.0 + 540.0}});
        let view = opened(&c, details);
        assert_eq!(
            (view.cache.label.as_str(), view.cache.short_label.as_str()),
            ("Cache ~9m", "~9m")
        );
        assert!(view
            .cache_details
            .starts_with("Estimated expiry from the TTL reported in native usage: "));
        *lock(&fake.now) += 240.0;
        c.tick();
        assert_eq!(c.current().unwrap().cache.label, "Cache ~5m");
    }

    #[test]
    fn without_a_state_the_session_is_inspected_once() {
        let root = dir("inspect");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        c.open(Some(session()));
        let view = until(&c, |v| v.loaded);
        assert_eq!(view.block_reason.as_deref(), Some("Checking the session…"));
        *lock(&fake.now) += 3.5;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        c.tick();
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_fallback_waits_three_seconds_after_opening() {
        let root = dir("inspect-wait");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 2.9;
        c.tick();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 0);
        *lock(&fake.now) += 0.2;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_state_after_a_failed_inspect_clears_its_reason() {
        let root = dir("inspect-failed");
        let (c, fake) = send_hub(&root);
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 3.5;
        c.tick();
        until(&c, |v| {
            v.block_reason.as_deref() == Some("Refresh the session before sending.")
        });
        c.observe(&state(ready()));
        assert!(c.current().unwrap().block_reason.is_none());
    }

    /// Waits until `done` holds, within five seconds.
    fn eventually(done: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_chip_changes_when_its_minute_does_not_on_the_clocks() {
        let root = dir("chip-boundary");
        let (c, fake) = send_hub(&root);
        let start = *lock(&fake.now);
        // Ten minutes from a time that is not on a whole minute.
        let mut details = ready();
        details["session_usage"] =
            json!({"prompt_cache": {"status": "warm", "expires_at": start + 545.0}});
        assert_eq!(opened(&c, details).cache.label, "Cache ~10m");
        assert!(c.take_view().is_some());
        c.tick();
        assert!(c.take_view().is_none(), "the same minute wakes nothing");
        *lock(&fake.now) = start + 5.0;
        c.tick();
        assert_eq!(c.take_view().unwrap().cache.label, "Cache ~9m");
        *lock(&fake.now) = start + 544.0;
        c.tick();
        assert_eq!(c.take_view().unwrap().cache.label, "Cache ~1m");
        *lock(&fake.now) = start + 545.0;
        c.tick();
        let view = c.take_view().unwrap();
        assert_eq!(view.cache.label, "Cold cache");
        assert_eq!(
            view.cache_warning,
            "The estimated cache lifetime has ended. Continuing may rebuild the cached context."
        );
    }

    #[test]
    fn a_tick_without_an_open_composer_wakes_nothing() {
        let root = dir("tick-closed");
        let store = drafts(&root);
        let woken = Arc::new(AtomicUsize::new(0));
        let count = woken.clone();
        let now = Arc::new(Mutex::new(1_793_000_100.0));
        let clock = now.clone();
        let c = Composers::new(
            Arc::clone(&store),
            Config {
                cache: root.join("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(move || {
                    count.fetch_add(1, Ordering::SeqCst);
                }),
                clock: Arc::new(move || *lock(&clock)),
                rows: Arc::new(|_, _, _| Some((row("claude/sample-project/review"), true))),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(|_, _| panic!("nothing is open to inspect")),
            },
        );
        // The stored draft keys' first answer wakes the pump once.
        eventually(|| woken.load(Ordering::SeqCst) > 0);
        let before = woken.load(Ordering::SeqCst);
        for minute in 1..5 {
            *lock(&now) += 60.0 * f64::from(minute);
            c.tick();
        }
        assert_eq!(woken.load(Ordering::SeqCst), before);
    }

    #[test]
    fn the_ticker_sleeps_while_no_composer_is_open() {
        let root = dir("ticker");
        let store = drafts(&root);
        let clocks = Arc::new(AtomicUsize::new(0));
        let counted = clocks.clone();
        let c = Composers::new(
            Arc::clone(&store),
            Config {
                cache: root.join("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(|| {}),
                clock: Arc::new(move || {
                    counted.fetch_add(1, Ordering::SeqCst);
                    1_793_000_100.0
                }),
                rows: Arc::new(|_, _, _| None),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(|_, _| None),
            },
        );
        let ticker = c.start_ticker_every(Duration::from_millis(5));
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(clocks.load(Ordering::SeqCst), 0, "nothing open: no tick");
        c.open(Some(session()));
        let opened = clocks.load(Ordering::SeqCst);
        eventually(|| clocks.load(Ordering::SeqCst) > opened + 3);
        c.open(None);
        std::thread::sleep(Duration::from_millis(50));
        let closed = clocks.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            clocks.load(Ordering::SeqCst),
            closed,
            "closed again: no tick"
        );
        c.flush();
        let (done, ended) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            ticker.join().unwrap();
            let _ = done.send(());
        });
        ended
            .recv_timeout(Duration::from_secs(5))
            .expect("the ticker ends when the app quits");
    }

    #[test]
    fn nothing_is_inspected_while_the_computer_is_offline() {
        let root = dir("inspect-offline");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        *lock(&fake.online) = false;
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 10.0;
        c.tick();
        c.refresh_details();
        assert!(c.lock().followed.inspecting.is_none(), "none started");
        std::thread::sleep(Duration::from_millis(50));
        let view = c.current().unwrap();
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Machine unavailable. Your draft stays here.")
        );
        assert!(view.cache_details.ends_with("Last recorded session data."));
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 0);
        *lock(&fake.online) = true;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_inspect_is_read_again_only_when_the_run_conversation_or_state_changes() {
        let root = dir("inspect-again");
        let (c, fake) = send_hub(&root);
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&fake.now) += 3.5;
        c.tick();
        until(&c, |v| {
            v.block_reason.as_deref() == Some("Refresh the session before sending.")
        });
        for _ in 0..3 {
            c.tick();
        }
        assert_eq!(
            fake.inspects.load(Ordering::SeqCst),
            1,
            "a failure is not retried by itself"
        );
        *lock(&fake.inspected) = Some(ready());
        lock(&fake.row).run_id = Some("run-2".into());
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        // The n-th inspect has answered.
        let reads = |n| {
            eventually(|| {
                fake.inspects.load(Ordering::SeqCst) == n && c.lock().followed.inspecting.is_none()
            })
        };
        reads(2);
        lock(&fake.row).conversation_id = Some("conv-2".into());
        c.tick();
        reads(3);
        lock(&fake.row).state = "paused".into();
        c.tick();
        reads(4);
        for _ in 0..3 {
            c.tick();
        }
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 4);
    }

    #[test]
    fn the_stream_keeps_the_details_and_an_end_inspects() {
        let root = dir("inspect-streamed");
        let (c, fake) = send_hub(&root);
        *lock(&fake.inspected) = Some(ready());
        opened(&c, ready());
        *lock(&fake.now) += 10.0;
        lock(&fake.row).state = "paused".into();
        c.tick();
        c.refresh_details();
        assert!(c.lock().followed.inspecting.is_none(), "none started");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            fake.inspects.load(Ordering::SeqCst),
            0,
            "Qt reads nothing while streamed"
        );
        lock(&fake.row).state = "running".into();
        c.observe(&Batch {
            activity: vec![ActivityOut {
                computer: ComputerId::from("studio"),
                key: "run:run-1".into(),
                kind: "ended".into(),
                payload: Value::Null,
            }],
            ..Batch::default()
        });
        assert_eq!(
            c.current().unwrap().block_reason.as_deref(),
            Some("Checking the session…")
        );
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        assert_eq!(fake.inspects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_inspect_of_a_session_no_longer_open_is_dropped() {
        let root = dir("inspect-late");
        let store = drafts(&root);
        let now = Arc::new(Mutex::new(1_793_000_100.0));
        let clock = now.clone();
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let inspects = Arc::new(AtomicUsize::new(0));
        let counted = inspects.clone();
        let c = Composers::new(
            Arc::clone(&store),
            Config {
                cache: root.join("cache"),
            },
            Deps {
                spawn: Arc::new(|job: Job| {
                    std::thread::spawn(job);
                }),
                notify: Arc::new(|| {}),
                clock: Arc::new(move || *lock(&clock)),
                rows: Arc::new(|_, key, name| {
                    let mut row = row(name);
                    row.run_id = Some(key.trim_start_matches("run:").into());
                    Some((row, true))
                }),
                facts: Arc::new(|_, _| None),
                inspect: Arc::new(move |target, _| {
                    // The first inspect waits until the test lets it answer.
                    if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                        let _ = lock(&gate).recv_timeout(Duration::from_secs(5));
                    }
                    let mut details = ready();
                    details["run_id"] = json!(target.run_id);
                    Some(details)
                }),
            },
        );
        c.open(Some(session()));
        until(&c, |v| v.loaded);
        *lock(&now) += 3.5;
        c.tick();
        eventually(|| inspects.load(Ordering::SeqCst) == 1);
        let mut other = session();
        other.key = "run:run-2".into();
        other.name = "claude/sample-project/other".into();
        c.open(Some(other));
        until(&c, |v| v.loaded && v.key.ends_with("/other"));
        release.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let view = c.current().unwrap();
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Checking the session…"),
            "the first session's answer is not the second's"
        );
        *lock(&now) += 3.5;
        c.tick();
        until(&c, |v| v.block_reason.is_none());
        assert_eq!(
            c.lock().followed.details.as_ref().unwrap()["run_id"],
            "run-2"
        );
        assert_eq!(inspects.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_batch_with_the_open_row_or_its_status_changes_the_view_at_once() {
        let root = dir("row-batch");
        let (c, fake) = send_hub(&root);
        opened(&c, ready());
        assert!(c.take_view().unwrap().available);
        lock(&fake.row).state = "paused".into();
        assert!(c.take_view().is_none(), "nothing announced it yet");
        let mut paused = row("claude/sample-project/review");
        paused.state = "paused".into();
        // Another session's row changes nothing.
        let mut other = paused.clone();
        other.name = "claude/sample-project/other".into();
        let upsert = |key: &str, row: ViewRow| Batch {
            upserts: vec![RowOut {
                computer: ComputerId::from("studio"),
                key: key.into(),
                row,
            }],
            ..Batch::default()
        };
        c.observe(&upsert("run:run-2", other));
        assert!(c.take_view().is_none());
        c.observe(&upsert("run:run-1", paused));
        assert_eq!(
            c.take_view().unwrap().block_reason.as_deref(),
            Some("Resume the session to send a message.")
        );
        lock(&fake.row).state = "running".into();
        *lock(&fake.online) = false;
        c.observe(&Batch {
            computers: vec![zerus_client_core::ComputerOut {
                computer: ComputerId::from("studio"),
                status: "reconnecting".into(),
                attempt: 1,
            }],
            ..Batch::default()
        });
        assert_eq!(
            c.take_view().unwrap().block_reason.as_deref(),
            Some("Machine unavailable. Your draft stays here.")
        );
    }

    #[test]
    fn a_write_blocks_messages_and_the_tick_shows_it() {
        let root = dir("write");
        let (c, fake) = send_hub(&root);
        opened(&c, ready());
        assert!(c.take_view().is_some());
        c.tick();
        assert!(c.take_view().is_none(), "nothing changed");
        *lock(&fake.write) = Some(WriteKind::Update);
        c.tick();
        let view = c.take_view().unwrap();
        assert_eq!(
            view.block_reason.as_deref(),
            Some("Wait for the session update to finish.")
        );
        *lock(&fake.write) = Some(WriteKind::Terminate);
        c.tick();
        assert_eq!(
            c.take_view().unwrap().block_reason.as_deref(),
            Some("The session is terminating. Your draft stays here.")
        );
        *lock(&fake.write) = None;
        c.tick();
        assert!(c.take_view().unwrap().available);
    }

    #[test]
    fn files_wait_for_an_agent_that_takes_them() {
        let root = dir("attachments");
        let file = root.join("notes.txt");
        std::fs::write(&file, "notes").unwrap();
        let (c, fake) = send_hub(&root);
        let view = opened(&c, ready());
        c.attach_paths(
            view.handle,
            vec![file],
            zerus_client_core::attachments::Selection::default(),
        );
        let view = until(&c, |v| v.attachments.len() == 1 && !v.attaching);
        assert!(view.can_send && view.attachments_available);
        lock(&fake.row).agent = "opencode".into();
        let view = c.current().unwrap();
        assert!(!view.attachments_available && !view.can_send);
        assert_eq!(view.status, "This native adapter does not support file attachments. Your existing files remain saved; remove them to send text.");
        assert_eq!(view.attachments_reason, view.status);
        lock(&fake.row).agent = "claude".into();
        let mut details = ready();
        details["attachments_supported"] = json!(false);
        details["attachments_reason"] = json!("This agent takes no files here.");
        c.observe(&state(details));
        let view = c.current().unwrap();
        assert_eq!(view.status, "This agent takes no files here.");
        assert!(!view.can_send);
    }

    #[test]
    fn a_deepseek_row_is_dsh_to_the_rules() {
        let root = dir("dsh");
        let (c, fake) = send_hub(&root);
        lock(&fake.row).agent = "deepseek".into();
        let mut details = ready();
        details["auth_required"] = json!(true);
        assert_eq!(
            opened(&c, details).block_reason.as_deref(),
            Some("Sign in to DeepSeek above. Your draft stays here.")
        );
    }

    #[test]
    fn the_expiry_is_local_time_in_qts_form() {
        let text = local_time(1_793_000_640);
        let parts: Vec<&str> = text.split(' ').collect();
        assert_eq!(parts.len(), 3, "{text}");
        assert!(
            (1..=31).contains(&parts[0].parse::<u32>().unwrap()),
            "{text}"
        );
        assert!(
            ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
                .contains(&parts[1]),
            "{text}"
        );
        assert!(
            parts[2].len() == 8 && parts[2].split(':').all(|n| n.len() == 2),
            "{text}"
        );
        assert_eq!(
            expiry(&json!({"session_usage": {"prompt_cache": {"expires_at": 1_793_000_640.5}}})),
            Some((1_793_000_640, text))
        );
        assert_eq!(expiry(&ready()), None);
    }
}
