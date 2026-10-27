//! `hgs [@host] watch --json`: one long-lived NDJSON event stream per computer
//! (ABI `watch-v1`, frames in `crates/watch-protocol`). The catalog topic
//! carries the rows of `hgs ls --json --local --no-archives`; Activity carries
//! what `hgs inspect` returns for subscribed sessions. Change detection is a
//! 250 ms pulse over cheap stamps and the journal's data version, on
//! `std::thread` only. See "Event stream" in docs/reference.md.
mod activity;
mod catalog;
mod stamps;
mod writer;

use crate::cli::{Error, Result};
use crate::config::Config;
use crate::state::watch_source as source;
use activity::Subscription;
use catalog::{Digest32, Sent};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use writer::{Item, Writer};
use zerus_watch_protocol::{self as protocol, reason, topic, Body, Control};

/// How often sources are checked.
const PULSE: Duration = Duration::from_millis(250);
/// A listing waits this long after a change, so a burst lists once.
const DEBOUNCE: Duration = Duration::from_millis(150);
/// Listings start at least this far apart, and at least four times the
/// previous listing's duration apart.
const LISTING_GAP: Duration = Duration::from_millis(500);
/// Listing without any observed change: process exit, tmux kill or rename,
/// ended background jobs and terminal choosers report no file change.
const SAFETY: Duration = Duration::from_secs(5);
/// `catalog.meta` with only fresh metrics or presence goes out this rarely.
const META_INTERVAL: Duration = Duration::from_secs(10);
/// A changed session's summary is re-inspected at most this often.
const STATE_GAP: Duration = Duration::from_millis(500);
/// Unchanged summaries are re-inspected after this long: provider messages
/// between tools exist only in native transcripts.
const BUSY_REFRESH: Duration = Duration::from_secs(2);
const IDLE_REFRESH: Duration = Duration::from_secs(15);
/// DeepSeek and OpenCode answer from their own hosts.
const NATIVE_REFRESH: Duration = Duration::from_secs(2);
/// With more than this queued, catalog changes wait and coalesce.
const DEFER_BYTES: usize = protocol::QUEUE_BYTES / 4;
/// A repeated source failure is reported at most this often.
const ERROR_REPEAT: Duration = Duration::from_secs(60);
/// The largest page of catalog items.
const PAGE_LIMIT: usize = protocol::PAGE_BYTES - 4096;

enum Input {
    Line(String),
    TooLong,
    /// Control lines arrived faster than they were handled; some were dropped.
    Dropped,
    End,
}

/// Control lines read but not yet handled; beyond it lines are dropped, so a
/// client that only writes cannot grow this process.
const PENDING_CONTROLS: usize = 256;

pub(crate) fn run(config: &Config, args: &[String], dry: bool) -> Result<i32> {
    if dry || args != ["--json"] {
        return Err(Error::new(1, "usage: hgs [@host] watch --json   (NDJSON frames on stdout, control operations on stdin)"));
    }
    let (inputs, received) = mpsc::channel();
    let closed = inputs.clone();
    let writer = Writer::spawn(
        std::io::stdout(),
        protocol::QUEUE_BYTES,
        Duration::from_millis(protocol::HEARTBEAT_MS),
        move || {
            let _ = closed.send(Input::End);
        },
    );
    let pending = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&pending);
    let halt = writer.clone();
    std::thread::Builder::new()
        .name("hgs-watch-control".into())
        // EOF stops the writer at once, so a wait for queue room ends too.
        .spawn(move || read_controls(std::io::stdin().lock(), inputs, &counted, move || halt.halt()))
        .map_err(|error| Error::new(1, format!("cannot read control operations: {error}")))?;
    let mut watch = Watch::new(config, writer);
    watch.hello();
    watch.list(Instant::now());
    watch.emit_catalog();
    let mut next_pulse = Instant::now() + PULSE;
    loop {
        let now = Instant::now();
        let deadline = watch.listing_due.map_or(next_pulse, |due| due.min(next_pulse));
        let input = received.recv_timeout(deadline.saturating_duration_since(now));
        if matches!(input, Ok(Input::Line(_) | Input::TooLong)) {
            pending.fetch_sub(1, Ordering::SeqCst);
        }
        if watch.out.writer.stopped() {
            return Ok(0);
        }
        match input {
            Ok(Input::Line(text)) => watch.control(&text),
            Ok(Input::TooLong) => watch.out.error(
                "bad_request",
                &format!("control operations are at most {} bytes", protocol::CONTROL_LINE_BYTES),
                None,
                None,
            ),
            Ok(Input::Dropped) => watch.out.error(
                "bad_request",
                &format!("more than {PENDING_CONTROLS} control operations were waiting; later ones were dropped"),
                None,
                None,
            ),
            Ok(Input::End) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(0),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if watch.out.writer.stopped() {
            return Ok(0);
        }
        let now = Instant::now();
        if now >= next_pulse {
            next_pulse = now + PULSE;
            watch.pulse(now);
        }
        if watch.listing_due.is_some_and(|due| now >= due) {
            watch.list(now);
        }
        watch.emit_catalog();
        watch.resync_activity();
    }
}

/// Reads control lines until EOF, then runs `ended`. A line longer than the
/// limit is skipped and reported, never buffered whole; so are lines beyond
/// `PENDING_CONTROLS` not yet handled.
fn read_controls<R: BufRead>(mut input: R, inputs: mpsc::Sender<Input>, pending: &AtomicUsize, ended: impl FnOnce()) {
    let mut line = Vec::new();
    let mut too_long = false;
    let mut dropping = false;
    loop {
        let buffer = match input.fill_buf() {
            Ok(buffer) => buffer,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        if buffer.is_empty() {
            break;
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let chunk = &buffer[..newline.unwrap_or(buffer.len())];
        if !too_long && line.len() + chunk.len() > protocol::CONTROL_LINE_BYTES {
            too_long = true;
            line.clear();
        }
        if !too_long {
            line.extend_from_slice(chunk);
        }
        let used = chunk.len() + usize::from(newline.is_some());
        input.consume(used);
        if newline.is_some() {
            let message = if too_long {
                Input::TooLong
            } else {
                Input::Line(String::from_utf8_lossy(&line).into_owned())
            };
            line.clear();
            too_long = false;
            if pending.load(Ordering::SeqCst) >= PENDING_CONTROLS {
                if !dropping && inputs.send(Input::Dropped).is_err() {
                    break;
                }
                dropping = true;
                continue;
            }
            dropping = false;
            pending.fetch_add(1, Ordering::SeqCst);
            if inputs.send(message).is_err() {
                break;
            }
        }
    }
    if !line.is_empty() && !too_long && pending.load(Ordering::SeqCst) < PENDING_CONTROLS {
        pending.fetch_add(1, Ordering::SeqCst);
        let _ = inputs.send(Input::Line(String::from_utf8_lossy(&line).into_owned()));
    }
    ended();
    let _ = inputs.send(Input::End);
}

/// Frames out, with repeated source failures limited.
struct Out {
    writer: Writer,
    reported: BTreeMap<String, Instant>,
}

impl Out {
    fn encode(body: &Body) -> Option<String> {
        protocol::encode_body(body).ok()
    }

    /// Queues a frame unless the client fell behind (false).
    fn offer(&self, body: &Body, coalesce: Option<String>) -> bool {
        match Self::encode(body) {
            Some(body) => self.writer.offer(Item { coalesce, body }),
            None => false,
        }
    }

    /// Queues a frame, waiting for room.
    fn put(&self, body: &Body) -> bool {
        match Self::encode(body) {
            Some(body) => self.writer.put(Item { coalesce: None, body }),
            None => false,
        }
    }

    fn error(&mut self, code: &str, message: &str, topic: Option<&str>, key: Option<&str>) {
        let body = Body::Error(protocol::ErrorFrame {
            code: code.into(),
            message: message.chars().take(2048).collect(),
            topic: topic.map(str::to_owned),
            key: key.map(str::to_owned),
            fatal: false,
        });
        self.put(&body);
    }

    /// An error of a source that is retried: once per minute per message.
    fn failure(&mut self, code: &str, message: &str, topic: Option<&str>, key: Option<&str>) {
        let id = format!("{code}\n{}\n{message}", key.unwrap_or(""));
        let now = Instant::now();
        if self.reported.get(&id).is_some_and(|at| now.duration_since(*at) < ERROR_REPEAT) {
            return;
        }
        self.reported.retain(|_, at| now.duration_since(*at) < ERROR_REPEAT);
        self.reported.insert(id, now);
        self.error(code, message, topic, key);
    }
}

struct Watch<'a> {
    config: &'a Config,
    out: Out,
    root: PathBuf,
    // The latest listing, and what the client holds of it.
    rows: Vec<(String, Value)>,
    digests: Vec<Digest32>,
    meta: Value,
    listed: bool,
    pending: bool,
    sent: BTreeMap<String, Sent>,
    revision: i64,
    snapshot_sent: bool,
    catalog_resync: bool,
    meta_sent: Option<(Digest32, Digest32, Instant)>,
    // Listing schedule.
    stamp: stamps::Stamp,
    listing_due: Option<Instant>,
    listed_at: Option<Instant>,
    listing_cost: Duration,
    // Activity.
    journal: Option<rusqlite::Connection>,
    data_version: Option<i64>,
    journal_max: i64,
    subs: Vec<Subscription>,
}

impl<'a> Watch<'a> {
    fn new(config: &'a Config, writer: Writer) -> Self {
        Self {
            config,
            out: Out { writer, reported: BTreeMap::new() },
            root: source::state_root(),
            rows: Vec::new(),
            digests: Vec::new(),
            meta: json!({}),
            listed: false,
            pending: false,
            sent: BTreeMap::new(),
            revision: 0,
            snapshot_sent: false,
            catalog_resync: false,
            meta_sent: None,
            stamp: stamps::Stamp::default(),
            listing_due: None,
            listed_at: None,
            listing_cost: Duration::ZERO,
            journal: None,
            data_version: None,
            journal_max: 0,
            subs: Vec::new(),
        }
    }

    fn hello(&mut self) {
        let hello = protocol::Hello {
            machine_id: crate::swarm::local_node_id(self.config).ok(),
            host: self.config.host().into(),
            hgs_version: env!("CARGO_PKG_VERSION").into(),
            stream: uuid::Uuid::new_v4().to_string(),
            heartbeat_ms: protocol::HEARTBEAT_MS,
            max_frame_bytes: protocol::MAX_FRAME_BYTES,
            topics: [topic::CATALOG, topic::CATALOG_META, topic::ACTIVITY].map(str::to_owned).to_vec(),
            limits: protocol::Limits::default(),
        };
        self.out.put(&Body::Hello(hello));
        self.open_journal();
    }

    // ---- Catalog -----------------------------------------------------------

    fn schedule(&mut self, at: Instant) {
        let gap = LISTING_GAP.max(self.listing_cost * 4);
        let due = self.listed_at.map_or(at, |started| at.max(started + gap));
        self.listing_due = Some(self.listing_due.map_or(due, |current| current.min(due)));
    }

    /// Re-runs the `ls --json --local --no-archives` builder.
    fn list(&mut self, now: Instant) {
        // The stamp is taken first: a change during the listing lists again.
        self.stamp = stamps::scan(&self.root);
        self.listing_due = None;
        self.listed_at = Some(now);
        match crate::cli::local_listing(self.config) {
            Ok(listing) => {
                let (rows, meta) = catalog::split(listing);
                self.digests = rows.iter().map(|(_, row)| catalog::row_digest(row)).collect();
                self.rows = rows;
                self.meta = meta;
                self.listed = true;
                self.pending = true;
            }
            Err(error) => self.out.failure("listing_failed", &error.message, Some(topic::CATALOG), None),
        }
        self.listing_cost = now.elapsed();
    }

    /// Sends what changed since the client's copy, unless the client is
    /// behind: then the latest listing waits, so changes coalesce per key.
    fn emit_catalog(&mut self) {
        if !self.listed || !(self.pending || self.catalog_resync) || self.out.writer.backlog() > DEFER_BYTES {
            return;
        }
        let mut removed = BTreeMap::new();
        if self.catalog_resync || !self.snapshot_sent {
            if self.catalog_resync {
                self.out.put(&Body::Gap(protocol::Gap { topic: topic::CATALOG.into(), key: None, reason: reason::OVERFLOW.into() }));
            }
            self.catalog_resync = false;
            self.snapshot();
            self.snapshot_sent = true;
        } else {
            let change = catalog::diff(&self.sent, &self.rows, &self.digests);
            if !change.is_empty() {
                removed = self.delta(change);
            }
            self.send_meta(false);
        }
        self.pending = false;
        self.follow_catalog(&removed);
    }

    fn item(&self, index: usize) -> Value {
        let (key, row) = &self.rows[index];
        json!({"key": key, "row": row})
    }

    /// Rows too large for any frame are left out with an error: read them
    /// with `hgs ls --json --local`.
    fn oversized(&mut self, index: usize, size: usize) -> bool {
        if size <= activity::PAYLOAD_LIMIT {
            return false;
        }
        let key = self.rows[index].0.clone();
        self.out.failure(
            "row_too_large",
            &format!("catalog row exceeds the frame limit ({size} bytes); read it with hgs ls --json --local"),
            Some(topic::CATALOG),
            Some(&key),
        );
        true
    }

    fn snapshot(&mut self) {
        let mut items = Vec::new();
        for index in 0..self.rows.len() {
            let item = self.item(index);
            let size = item.to_string().len();
            if !self.oversized(index, size) {
                items.push((item, size));
            }
        }
        self.revision += 1;
        let pages = catalog::pages(items, |(_, size)| *size, PAGE_LIMIT);
        let count = pages.len();
        for (page, items) in pages.into_iter().enumerate() {
            let body = Body::Snapshot(protocol::Snapshot {
                topic: topic::CATALOG.into(),
                key: None,
                items: Some(items.into_iter().map(|(item, _)| item).collect()),
                value: None,
                page: page as u32,
                last: page + 1 == count,
                cursor: self.revision,
            });
            if !self.out.put(&body) {
                return;
            }
        }
        self.sent = self
            .rows
            .iter()
            .zip(&self.digests)
            .map(|((key, row), digest)| (key.clone(), Sent::of(row, *digest)))
            .collect();
        self.send_meta(true);
    }

    /// Sends a delta; returns the removed keys with their reasons.
    fn delta(&mut self, change: catalog::Change) -> BTreeMap<String, &'static str> {
        /// One entry of a delta: a removal with its reason, or a row.
        enum Entry {
            Remove(String, &'static str),
            Upsert(usize),
        }
        let listed: BTreeSet<&str> = self.rows.iter().filter_map(|(_, row)| row["name"].as_str()).collect();
        let mut entries = Vec::new();
        let mut removed = BTreeMap::new();
        for key in change.remove {
            let Some(old) = self.sent.get(&key) else { continue };
            let why = catalog::removal_reason(old, &listed, source::archived, source::saved);
            removed.insert(key.clone(), why);
            entries.push((Entry::Remove(key.clone(), why), key.len() + 48));
        }
        for index in change.upsert {
            let size = self.item(index).to_string().len();
            if self.oversized(index, size) {
                let (key, row) = &self.rows[index];
                self.sent.insert(key.clone(), Sent::of(row, self.digests[index]));
            } else {
                entries.push((Entry::Upsert(index), size));
            }
        }
        for page in catalog::pages(entries, |(_, size)| *size, PAGE_LIMIT) {
            if page.is_empty() {
                continue;
            }
            let mut upsert = Vec::new();
            let mut remove = Vec::new();
            for (entry, _) in &page {
                match entry {
                    Entry::Remove(key, why) => remove.push(protocol::Removal { key: key.clone(), reason: Some((*why).into()) }),
                    Entry::Upsert(index) => upsert.push(protocol::CatalogItem {
                        key: self.rows[*index].0.clone(),
                        row: self.rows[*index].1.clone(),
                    }),
                }
            }
            self.revision += 1;
            let body = Body::Delta(protocol::Delta { topic: topic::CATALOG.into(), upsert, remove, cursor: self.revision });
            if !self.out.offer(&body, None) {
                self.catalog_resync = true;
                return removed;
            }
            for (entry, _) in page {
                match entry {
                    Entry::Remove(key, _) => {
                        self.sent.remove(&key);
                    }
                    Entry::Upsert(index) => {
                        let (key, row) = &self.rows[index];
                        self.sent.insert(key.clone(), Sent::of(row, self.digests[index]));
                    }
                }
            }
        }
        removed
    }

    /// `catalog.meta`: at once when a lasting field changed, at most every
    /// 10 s when only metrics or presence did.
    fn send_meta(&mut self, force: bool) {
        let lasting = catalog::digest(&catalog::masked(&self.meta, &catalog::VOLATILE_META));
        let full = catalog::digest(&self.meta);
        let due = match self.meta_sent {
            None => true,
            Some((sent_lasting, sent_full, at)) => {
                sent_lasting != lasting || (sent_full != full && at.elapsed() >= META_INTERVAL)
            }
        };
        if !(force || due) {
            return;
        }
        let size = self.meta.to_string().len();
        if size > activity::PAYLOAD_LIMIT {
            // Never expected; a resync would only meet the same limit again.
            self.out.failure(
                "meta_too_large",
                &format!("catalog.meta exceeds the frame limit ({size} bytes); read it with hgs ls --json --local"),
                Some(topic::CATALOG_META),
                None,
            );
            self.meta_sent = Some((lasting, full, Instant::now()));
            return;
        }
        let body = Body::Snapshot(protocol::Snapshot {
            topic: topic::CATALOG_META.into(),
            key: None,
            items: None,
            value: Some(self.meta.clone()),
            page: 0,
            last: true,
            cursor: self.revision,
        });
        if self.out.offer(&body, Some(format!("snapshot|{}", topic::CATALOG_META))) {
            self.meta_sent = Some((lasting, full, Instant::now()));
        } else {
            self.catalog_resync = true;
        }
    }

    /// Activity subscriptions follow their catalog rows: a rename changes the
    /// name, a changed row refreshes the summary, a removed row ends them.
    fn follow_catalog(&mut self, removed: &BTreeMap<String, &'static str>) {
        let rows: BTreeMap<&str, usize> = self.rows.iter().enumerate().map(|(index, (key, _))| (key.as_str(), index)).collect();
        let mut ended = Vec::new();
        for (position, sub) in self.subs.iter_mut().enumerate() {
            match rows.get(sub.key.as_str()) {
                Some(index) => {
                    let row = &self.rows[*index].1;
                    sub.listed = true;
                    if let Some(name) = row["name"].as_str().filter(|name| *name != sub.name) {
                        sub.name = name.to_owned();
                        sub.dirty = true;
                    }
                    if sub.row != Some(self.digests[*index]) {
                        sub.row = Some(self.digests[*index]);
                        sub.dirty = true;
                    }
                    sub.busy = activity::busy(row);
                }
                None if sub.listed => ended.push((position, removed.get(&sub.key).copied().unwrap_or(reason::ENDED))),
                None => {}
            }
        }
        for (position, why) in ended.into_iter().rev() {
            let sub = self.subs.remove(position);
            self.out.error(
                "subscription_ended",
                &format!("{} left the catalog ({why})", sub.name),
                Some(topic::ACTIVITY),
                Some(&sub.key),
            );
        }
    }

    // ---- Pulse -------------------------------------------------------------

    fn pulse(&mut self, now: Instant) {
        let journal_changed = self.poll_journal();
        let stamp = stamps::scan(&self.root);
        if stamp != self.stamp || journal_changed {
            self.schedule(now + DEBOUNCE);
        }
        if self.listed_at.is_none_or(|at| now >= at + SAFETY) {
            self.schedule(now);
        }
        self.activity(now);
    }

    fn open_journal(&mut self) {
        if self.journal.is_some() {
            return;
        }
        self.journal = source::journal();
        if let Some(db) = &self.journal {
            self.data_version = source::data_version(db).ok();
            self.journal_max = source::bounds(db).map_or(0, |(_, max)| max);
            // Rows written before this connection opened: read after each cursor.
            for sub in self.subs.iter_mut().filter(|sub| !sub.native) {
                sub.more = true;
            }
        }
    }

    /// Whether the journal changed. Subscriptions with new rows read them.
    fn poll_journal(&mut self) -> bool {
        self.open_journal();
        let Some(db) = &self.journal else { return false };
        let version = match source::data_version(db) {
            Ok(version) => version,
            Err(_) => {
                self.journal = None;
                return false;
            }
        };
        if Some(version) == self.data_version {
            return false;
        }
        self.data_version = Some(version);
        let Ok((_, max)) = source::bounds(db) else { return true };
        if max < self.journal_max {
            // A replaced journal: every cursor is meaningless now.
            for sub in self.subs.iter_mut().filter(|sub| !sub.native) {
                sub.resync = Some(reason::RESET);
            }
            self.journal_max = max;
            return true;
        }
        if max == self.journal_max {
            return false;
        }
        const SCAN: usize = 10_000;
        let names: Option<BTreeSet<String>> = if max - self.journal_max > SCAN as i64 {
            None
        } else {
            source::changed_names(db, self.journal_max, SCAN).ok().map(|names| names.into_iter().collect())
        };
        for sub in self.subs.iter_mut().filter(|sub| !sub.native) {
            let touched = match (&names, &sub.journal) {
                (Some(names), Some(journal)) => names.contains(journal),
                _ => true,
            };
            if touched {
                sub.more = true;
            }
        }
        self.journal_max = max;
        true
    }

    fn activity(&mut self, now: Instant) {
        for index in 0..self.subs.len() {
            if self.subs[index].resync.is_some() {
                continue;
            }
            if self.subs[index].more {
                self.read_events(index);
            }
            let sub = &self.subs[index];
            let interval = if sub.native {
                NATIVE_REFRESH
            } else if sub.busy {
                BUSY_REFRESH
            } else {
                IDLE_REFRESH
            };
            let since = now.saturating_duration_since(sub.refreshed);
            if (sub.dirty && since >= STATE_GAP) || since >= interval {
                self.refresh(index);
            }
        }
    }

    /// Journal rows after the subscription's cursor, as `inspect` shapes them.
    fn read_events(&mut self, index: usize) {
        let Some(db) = &self.journal else { return };
        let sub = &mut self.subs[index];
        sub.more = false;
        let timeline = match source::timeline(&sub.name) {
            Ok(Some(timeline)) => timeline,
            Ok(None) => return,
            Err(error) => {
                self.out.failure("inspect_failed", &error, Some(topic::ACTIVITY), Some(&sub.key));
                return;
            }
        };
        if sub.run_id.as_deref().is_some_and(|run| run != timeline.run_id()) {
            // The name now holds another run; the catalog removes or renames ours.
            sub.dirty = true;
            return;
        }
        sub.journal = Some(timeline.journal().to_owned());
        let events = match timeline.events(db, sub.cursor, protocol::ACTIVITY_READ, false) {
            Ok(events) => events,
            Err(error) => {
                self.out.failure("journal_failed", &error, Some(topic::ACTIVITY), Some(&sub.key));
                return;
            }
        };
        sub.more = events.len() == protocol::ACTIVITY_READ;
        if events.is_empty() {
            return;
        }
        sub.dirty = true;
        for page in activity::event_pages(events, activity::PAYLOAD_LIMIT) {
            let cursor = page.last().and_then(|event| event["seq"].as_i64()).unwrap_or(sub.cursor);
            let body = Body::Events(protocol::Events {
                topic: topic::ACTIVITY.into(),
                key: sub.key.clone(),
                events: page,
                cursor,
            });
            if !self.out.offer(&body, None) {
                sub.resync = Some(reason::OVERFLOW);
                sub.more = false;
                return;
            }
            sub.cursor = cursor;
        }
    }

    /// Re-inspects one session; sends `state` when its summary changed, and
    /// for DeepSeek and OpenCode a replacement of their event window.
    fn refresh(&mut self, index: usize) {
        let after = if self.subs[index].native { 0 } else { self.journal_max.max(self.subs[index].cursor) };
        let sub = &mut self.subs[index];
        sub.refreshed = Instant::now();
        sub.dirty = false;
        let inspection = match source::inspect(&sub.name, after) {
            Ok(inspection) => inspection,
            Err(error) => {
                self.out.failure("inspect_failed", &error, Some(topic::ACTIVITY), Some(&sub.key));
                return;
            }
        };
        if inspection["run_id"].as_str().zip(sub.run_id.as_deref()).is_some_and(|(seen, wanted)| seen != wanted) {
            return;
        }
        if !sub.native {
            if let Ok(Some(timeline)) = source::timeline(&sub.name) {
                sub.journal = Some(timeline.journal().to_owned());
            }
        } else {
            let events = inspection["events"].as_array().cloned().unwrap_or_default();
            let digest = catalog::digest(&Value::Array(events.clone()));
            if sub.events != Some(digest) && send_window(&self.out, sub, events, 0) {
                sub.events = Some(digest);
            }
        }
        send_state(&self.out, sub, &inspection);
    }

    /// Topics the client fell behind on: `gap`, then a tail snapshot.
    fn resync_activity(&mut self) {
        for index in 0..self.subs.len() {
            let Some(why) = self.subs[index].resync else { continue };
            if self.out.writer.backlog() > DEFER_BYTES {
                return;
            }
            self.out.put(&Body::Gap(protocol::Gap {
                topic: topic::ACTIVITY.into(),
                key: Some(self.subs[index].key.clone()),
                reason: why.into(),
            }));
            self.subs[index].resync = None;
            self.subs[index].state = None;
            self.subs[index].events = None;
            self.start(index, 0, true);
        }
    }

    // ---- Control -----------------------------------------------------------

    fn control(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let operation = match protocol::decode_control(text) {
            Ok(operation) => operation,
            Err(error) => {
                self.out.error("bad_request", &format!("invalid control operation: {error}"), None, None);
                return;
            }
        };
        match operation {
            Control::Ping(ping) => {
                self.out.put(&Body::Pong(protocol::Pong { id: ping.id }));
            }
            Control::Subscribe(subscription) => self.subscribe(subscription),
            Control::Unsubscribe(subscription) => {
                if let Some(session) = subscription.session.filter(|_| subscription.topic == topic::ACTIVITY) {
                    let key = session.key();
                    self.subs.retain(|sub| sub.key != key);
                }
            }
        }
    }

    fn subscribe(&mut self, request: protocol::Subscription) {
        match request.topic.as_str() {
            topic::CATALOG | topic::CATALOG_META => return,
            topic::ACTIVITY => {}
            other => {
                self.out.error(
                    "unsupported_topic",
                    &format!("watch-v1 does not stream {other}; keep reading it with its command"),
                    Some(other),
                    None,
                );
                return;
            }
        }
        let Some(session) = request
            .session
            .filter(|session| !session.name.is_empty() && session.name.len() <= 1024 && !session.name.chars().any(char::is_control))
        else {
            self.out.error("bad_request", "an Activity subscription names its session", Some(topic::ACTIVITY), None);
            return;
        };
        let after = request.after.unwrap_or(0);
        if after < 0 {
            self.out.error("bad_request", "after must be nonnegative", Some(topic::ACTIVITY), None);
            return;
        }
        let key = session.key();
        self.subs.retain(|sub| sub.key != key);
        if self.subs.len() >= protocol::ACTIVITY_SUBSCRIPTIONS {
            self.out.error(
                "too_many_subscriptions",
                &format!("a stream follows at most {} sessions", protocol::ACTIVITY_SUBSCRIPTIONS),
                Some(topic::ACTIVITY),
                Some(&key),
            );
            return;
        }
        // A renamed session keeps its run key; follow its current name.
        let row = self.rows.iter().position(|(row_key, _)| *row_key == key);
        let name = row
            .and_then(|index| self.rows[index].1["name"].as_str())
            .unwrap_or(&session.name)
            .to_owned();
        let tail = request.tail.map_or(protocol::ACTIVITY_TAIL, |tail| (tail as usize).min(protocol::ACTIVITY_TAIL));
        let mut sub = Subscription::new(key, name.clone(), session.run_id.filter(|run| !run.is_empty()), source::native(&name), tail);
        if let Some(index) = row {
            sub.listed = true;
            sub.row = Some(self.digests[index]);
            sub.busy = activity::busy(&self.rows[index].1);
        }
        self.subs.push(sub);
        let index = self.subs.len() - 1;
        self.open_journal();
        if !self.start(index, after, false) {
            self.subs.pop();
        }
    }

    /// The first frames of a subscription, or its resumption after a gap.
    /// False when it cannot follow the requested session.
    fn start(&mut self, index: usize, after: i64, resumed: bool) -> bool {
        let (minimum, maximum) = self.journal.as_ref().and_then(|db| source::bounds(db).ok()).unwrap_or((0, 0));
        let sub = &mut self.subs[index];
        let gap = if resumed || after == 0 || sub.native {
            None
        } else if after > maximum {
            Some(reason::RESET)
        } else if after < minimum - 1 {
            Some(reason::RETENTION)
        } else {
            None
        };
        let inspection = match source::inspect(&sub.name, if gap.is_some() { 0 } else { after }) {
            Ok(inspection) => inspection,
            Err(error) => {
                self.out.error("inspect_failed", &error, Some(topic::ACTIVITY), Some(&sub.key));
                return false;
            }
        };
        if let (Some(seen), Some(wanted)) = (inspection["run_id"].as_str(), sub.run_id.as_deref()) {
            if seen != wanted {
                self.out.error(
                    "session_changed",
                    &format!("{} now runs {seen}, not {wanted}", sub.name),
                    Some(topic::ACTIVITY),
                    Some(&sub.key),
                );
                return false;
            }
        }
        sub.refreshed = Instant::now();
        let mut events = inspection["events"].as_array().cloned().unwrap_or_default();
        if !sub.native {
            if let Ok(Some(timeline)) = source::timeline(&sub.name) {
                sub.journal = Some(timeline.journal().to_owned());
            }
        }
        if sub.native {
            sub.events = Some(catalog::digest(&Value::Array(events.clone())));
            send_window(&self.out, sub, events, 0);
        } else {
            let cursor = inspection["cursor"].as_i64().unwrap_or(after);
            if gap.is_some() || resumed || after == 0 {
                events.drain(..events.len().saturating_sub(sub.tail));
            }
            if let Some(why) = gap {
                self.out.put(&Body::Gap(protocol::Gap { topic: topic::ACTIVITY.into(), key: Some(sub.key.clone()), reason: why.into() }));
            }
            sub.more = after > 0 && gap.is_none() && events.len() >= protocol::ACTIVITY_READ;
            if gap.is_some() || resumed {
                send_window(&self.out, sub, events, cursor);
            } else {
                for page in activity::event_pages(events, activity::PAYLOAD_LIMIT) {
                    let page_cursor = page.last().and_then(|event| event["seq"].as_i64()).unwrap_or(cursor);
                    self.out.put(&Body::Events(protocol::Events {
                        topic: topic::ACTIVITY.into(),
                        key: sub.key.clone(),
                        events: page,
                        cursor: page_cursor,
                    }));
                }
            }
            sub.cursor = cursor;
        }
        send_state(&self.out, sub, &inspection);
        true
    }
}

/// An Activity event window as a snapshot: a DeepSeek or OpenCode window
/// (cursor 0), or a journal tail after a gap. Pages share the window's cursor.
fn send_window(out: &Out, sub: &mut Subscription, events: Vec<Value>, cursor: i64) -> bool {
    let pages = activity::event_pages(events, activity::PAYLOAD_LIMIT);
    let count = pages.len();
    for (page, items) in pages.into_iter().enumerate() {
        let body = Body::Snapshot(protocol::Snapshot {
            topic: topic::ACTIVITY.into(),
            key: Some(sub.key.clone()),
            items: Some(items),
            value: None,
            page: page as u32,
            last: page + 1 == count,
            cursor,
        });
        if !out.put(&body) {
            return false;
        }
    }
    true
}

/// `state` when the summary changed apart from observation times.
fn send_state(out: &Out, sub: &mut Subscription, inspection: &Value) {
    let mut value = activity::state_value(inspection);
    sub.busy = activity::busy(&value);
    let digest = activity::state_digest(&value);
    if sub.state == Some(digest) {
        return;
    }
    activity::shrink_state(&mut value, activity::PAYLOAD_LIMIT);
    sub.rev += 1;
    let body = Body::State(protocol::State { topic: topic::ACTIVITY.into(), key: sub.key.clone(), value, rev: sub.rev });
    if out.offer(&body, Some(format!("state|{}|{}", topic::ACTIVITY, sub.key))) {
        sub.state = Some(digest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(input: &[u8], pending: usize) -> Vec<String> {
        let (sender, receiver) = mpsc::channel();
        let ended = std::cell::Cell::new(false);
        read_controls(input, sender, &AtomicUsize::new(pending), || ended.set(true));
        assert!(ended.get());
        receiver
            .into_iter()
            .map(|input| match input {
                Input::Line(text) => text,
                Input::TooLong => "<too long>".into(),
                Input::Dropped => "<dropped>".into(),
                Input::End => "<end>".into(),
            })
            .collect()
    }

    fn lines(input: &[u8]) -> Vec<String> {
        read(input, 0)
    }

    #[test]
    fn control_lines_are_framed_bounded_and_end_at_eof() {
        assert_eq!(lines(b"{\"op\":\"ping\"}\n\n{\"op\":\"ping\",\"id\":2}"), ["{\"op\":\"ping\"}", "", "{\"op\":\"ping\",\"id\":2}", "<end>"]);
        let mut long = vec![b'x'; protocol::CONTROL_LINE_BYTES + 10];
        long.extend_from_slice(b"\nnext\n");
        assert_eq!(lines(&long), ["<too long>", "next", "<end>"]);
        assert_eq!(lines(b""), ["<end>"]);
        // A client that writes faster than lines are handled loses the excess.
        assert_eq!(read(b"a\nb\nc", PENDING_CONTROLS), ["<dropped>", "<end>"]);
    }
}
