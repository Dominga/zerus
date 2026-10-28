//! The Settings page's state (AnalyticsSettings.cpp): the routes a refresh
//! reads one at a time, each route's last reads, the epochs seen, this
//! computer's collection controls, and what the page shows, drawn when Qt
//! draws it (a refresh's start and end, a failed purge, the minute timer)
//! so no total comes from a read half done.
//!
//! Drawing a fleet at the limits (32 computers of 1080 daily buckets) takes
//! a while, so it happens outside the page's lock: [`Page::draw_due`] takes
//! what a drawing needs (reads are shared, not copied), [`Draw::render`]
//! draws it on the caller's thread, and [`Page::drawn`] keeps the newest.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use serde::Serialize;

use super::{
    histogram_totals, speech_totals, totals, unique_computers, valid_range, Bucket, Day,
    EpochGuard, Histogram, Observation, Speech, SpeechTotals, Summary, Waits, MAX_COMPUTERS,
    PAGE_SIZE, PROVIDERS,
};

/// How the page names this computer.
pub const THIS_COMPUTER: &str = "This computer";
/// The selection that reads this computer and every peer.
pub const ALL: &str = "*";
/// Why a click on Apply locally or Purge does nothing (Qt's buttons are
/// disabled then, but this view can be up to a minute old).
pub const READING: &str = "A read is running. Try again when it finishes.";
pub const CHANGING: &str = "A local analytics change is running. Try again when it finishes.";
pub const NOT_CURRENT_APPLY: &str =
    "Local analytics epoch is stale or unknown. Refresh before applying.";
pub const NOT_CURRENT_PURGE: &str =
    "Local analytics epoch is stale or unknown. Refresh before purging.";

/// Whether a summary was generated within the last five minutes (a minute
/// ahead tolerated).
pub fn fresh(summary: &Summary, now_ms: i64) -> bool {
    let age = now_ms as f64 / 1000.0 - summary.generated_at;
    (-60.0..=300.0).contains(&age)
}

fn display(route: &str) -> &str {
    if route.is_empty() {
        THIS_COMPUTER
    } else {
        route
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProviderLine {
    pub provider: String,
    /// None when the computer reported no coverage for it.
    pub partial: Option<bool>,
    pub unsupported: Option<Vec<String>>,
}

/// One computer's coverage paragraph.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComputerReport {
    pub route: String,
    pub current: bool,
    pub generated_at: f64,
    pub revision: f64,
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub partial: bool,
    pub gaps: Vec<String>,
    pub unsupported: Vec<String>,
    pub collecting: bool,
    pub error: String,
    pub waits: Option<Waits>,
    pub providers: Vec<ProviderLine>,
}

/// A route without a summary: its message.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Missing {
    pub route: String,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VoiceRead {
    pub current: bool,
    pub enabled: bool,
    pub totals: SpeechTotals,
    /// Whether the route's summary named its computer.
    pub identified: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VoiceReport {
    pub route: String,
    pub read: Option<VoiceRead>,
    pub error: String,
}

/// What AnalyticsSettings::render draws; the page words it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub from: String,
    pub through: String,
    pub routes: usize,
    /// Current unique computers.
    pub present: usize,
    /// Totals of the current buckets.
    pub totals: BTreeMap<String, f64>,
    /// The current buckets' merged work histogram; None when unavailable.
    pub work: Option<Histogram>,
    /// Peers past the 32 computers a view reads were left out.
    pub peer_limit: bool,
    pub missing: Vec<Missing>,
    pub computers: Vec<ComputerReport>,
    pub voice: Vec<VoiceReport>,
    /// The daily buckets drawn.
    pub buckets: usize,
}

/// One daily bucket of a computer.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub date: String,
    pub route: String,
    pub provider: String,
    pub producer: String,
    pub current: bool,
    pub counters: BTreeMap<String, f64>,
    pub histograms: BTreeMap<String, Histogram>,
}

/// One page of the daily buckets.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Rows {
    pub rows: Vec<Row>,
    pub offset: usize,
    pub total: usize,
}

/// This computer's collection as its current summary reports it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Local {
    pub enabled: bool,
    pub retention_days: i64,
    /// The epoch a change pins, taken when the person starts it.
    pub epoch: String,
    /// Counts the reads that set it, so the page resets its controls to
    /// each one as Qt does.
    pub reads: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct View {
    pub status: String,
    pub busy: bool,
    pub mutating: bool,
    pub can_refresh: bool,
    pub can_export: bool,
    pub can_change_local: bool,
    /// The peers the computer list offers, at most 31.
    pub peers: Vec<String>,
    pub peer_limit: bool,
    /// The last refresh's computers and days, which the page starts from.
    pub selection: String,
    pub from: Option<String>,
    pub through: Option<String>,
    pub local: Option<Local>,
    /// None before the first refresh.
    pub report: Option<Report>,
    /// A purge is waiting for the person's confirmation.
    pub purge_pending: bool,
    /// Apply locally shortens the retention to these days and waits for
    /// the person's confirmation.
    pub shorten_pending: Option<i64>,
}

/// A local change as the person clicked it: the epoch their view's read
/// came from and the controls' values (Qt captures all three before its
/// question).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configure {
    pub epoch: String,
    pub enabled: bool,
    pub retention_days: i64,
}

/// A computer the drawing counted, with its whole summary.
#[derive(Clone, Debug)]
struct Drawn {
    route: String,
    current: bool,
    agent: Arc<Summary>,
}

/// What a drawing needs, taken under the page's lock without copying a read.
#[derive(Debug)]
pub struct Draw {
    id: u64,
    from: Day,
    through: Day,
    routes: usize,
    peer_limit: bool,
    observations: Vec<Observation>,
}

/// A finished drawing: the report and the daily buckets in Qt's order, as
/// places in the computers' summaries.
#[derive(Debug, Default)]
pub struct Drawing {
    id: u64,
    report: Option<Report>,
    computers: Vec<Drawn>,
    rows: Vec<(usize, usize)>,
}

impl Draw {
    /// AnalyticsSettings::render, off the page's lock.
    pub fn render(self) -> Drawing {
        let computers = unique_computers(&self.observations);
        let mut current: Vec<&Bucket> = Vec::new();
        let mut rows = Vec::new();
        let mut reports = Vec::new();
        let mut drawn = Vec::new();
        for (index, observation) in computers.iter().enumerate() {
            let agent = observation
                .agent
                .as_ref()
                .expect("a unique computer has a summary");
            let coverage = &agent.coverage;
            reports.push(ComputerReport {
                route: observation.route.clone(),
                current: observation.current,
                generated_at: agent.generated_at,
                revision: agent.revision,
                start: coverage.start,
                end: coverage.end,
                partial: coverage.partial,
                gaps: coverage.gaps.clone(),
                unsupported: coverage.unsupported.clone(),
                collecting: agent.enabled,
                error: observation.error.clone(),
                waits: agent.unresolved_waits,
                providers: PROVIDERS
                    .iter()
                    .map(|provider| {
                        let reported = coverage.providers.get(*provider);
                        ProviderLine {
                            provider: (*provider).to_owned(),
                            partial: reported.map(|c| c.partial),
                            unsupported: reported.map(|c| c.unsupported.clone()),
                        }
                    })
                    .collect(),
            });
            rows.extend((0..agent.buckets.len()).map(|bucket| (index, bucket)));
            if observation.current {
                current.extend(agent.buckets.iter());
            }
            drawn.push(Drawn {
                route: observation.route.clone(),
                current: observation.current,
                agent: Arc::clone(agent),
            });
        }
        let missing: Vec<&Observation> = self
            .observations
            .iter()
            .filter(|o| o.agent.is_none())
            .collect();
        let voice = computers
            .iter()
            .chain(missing.iter().copied())
            .map(|observation| VoiceReport {
                route: observation.route.clone(),
                read: observation.voice.as_ref().map(|voice| VoiceRead {
                    current: observation.voice_current,
                    enabled: voice.enabled,
                    totals: speech_totals(&voice.days),
                    identified: observation.agent.is_some(),
                }),
                error: observation.voice_error.clone(),
            })
            .collect();
        let key = |&(computer, bucket): &(usize, usize)| {
            let b = &drawn[computer].agent.buckets[bucket];
            (
                b.date.as_str(),
                display(&drawn[computer].route),
                b.provider.as_str(),
                b.producer.as_str(),
            )
        };
        rows.sort_by(|a, b| key(a).cmp(&key(b)));
        let report = Report {
            from: self.from.iso(),
            through: self.through.iso(),
            routes: self.routes,
            present: computers.iter().filter(|o| o.current).count(),
            totals: totals(current.iter().copied(), None),
            work: histogram_totals(current.iter().copied(), None).remove("work_ms"),
            peer_limit: self.peer_limit,
            missing: missing
                .iter()
                .map(|o| Missing {
                    route: o.route.clone(),
                    error: o.error.clone(),
                })
                .collect(),
            computers: reports,
            voice,
            buckets: rows.len(),
        };
        Drawing {
            id: self.id,
            report: Some(report),
            computers: drawn,
            rows,
        }
    }
}

#[derive(Debug, Default)]
pub struct Page {
    peers: Vec<String>,
    peer_limit: bool,
    snapshots: BTreeMap<String, Observation>,
    epochs: EpochGuard,
    selection: String,
    routes: Vec<String>,
    queue: VecDeque<String>,
    local: Option<Arc<Summary>>,
    local_reads: u64,
    applied: Option<(Day, Day)>,
    busy: bool,
    mutating: bool,
    purge_epoch: Option<String>,
    /// Apply locally waits for the person to confirm a shorter retention.
    configure: Option<Configure>,
    status: String,
    /// A drawing is due (Qt would render now).
    due: bool,
    /// The last drawing asked for, and the drawing shown.
    draws: u64,
    drawing: Drawing,
}

impl Page {
    pub fn new() -> Page {
        Page::default()
    }

    /// AnalyticsSettings::setPeers: names past 256 UTF-16 units, `*` and
    /// repeats are dropped, and at most 31 peers are kept (32 computers with
    /// this one). A peer that leaves takes its last reads with it.
    pub fn set_peers(&mut self, peers: impl IntoIterator<Item = String>) {
        let mut kept: Vec<String> = Vec::new();
        self.peer_limit = false;
        for peer in peers {
            if peer.is_empty()
                || peer == ALL
                || peer.encode_utf16().count() > 256
                || kept.contains(&peer)
            {
                continue;
            }
            if kept.len() == MAX_COMPUTERS - 1 {
                self.peer_limit = true;
                break;
            }
            kept.push(peer);
        }
        if kept == self.peers {
            return;
        }
        self.peers = kept;
        let peers = &self.peers;
        self.snapshots
            .retain(|route, _| route.is_empty() || peers.contains(route));
    }

    fn idle(&self) -> bool {
        !self.busy && !self.mutating
    }

    fn local_fresh(&self, now_ms: i64) -> Option<&Summary> {
        self.local.as_deref().filter(|local| fresh(local, now_ms))
    }

    /// Apply and refresh (and opening the section): reads `selection` (this
    /// computer, `*` for all, or a peer; anything else is this computer, as
    /// Qt's list falls back) for the inclusive UTC days `from`..`through`.
    /// Returns whether a read began; the caller then reads each
    /// [`Page::next_route`] route.
    pub fn refresh(&mut self, selection: &str, from: &str, through: &str) -> bool {
        if !self.idle() {
            return false;
        }
        let range = Day::parse(from)
            .zip(Day::parse(through))
            .filter(|(from, through)| valid_range(*from, *through));
        let Some(range) = range else {
            self.status = "Choose an inclusive UTC range of at most 90 days.".into();
            return false;
        };
        let selection = if selection == ALL || self.peers.iter().any(|peer| peer == selection) {
            selection
        } else {
            ""
        };
        if self.applied != Some(range) {
            self.snapshots.clear();
        }
        self.applied = Some(range);
        self.selection = selection.to_owned();
        self.routes = if selection == ALL {
            std::iter::once(String::new())
                .chain(self.peers.iter().cloned())
                .collect()
        } else {
            vec![selection.to_owned()]
        };
        self.queue = self.routes.iter().cloned().collect();
        self.busy = true;
        self.local = None;
        for route in &self.routes {
            let observation = self.snapshots.entry(route.clone()).or_default();
            observation.route = route.clone();
            observation.current = false;
            observation.voice_current = false;
            observation.error = "Reading…".into();
        }
        self.due = true;
        self.status = format!("Reading {} configured route(s)…", self.routes.len());
        true
    }

    /// The next route to read, one at a time so a large catalog never runs
    /// more than one child; None once the read is finished.
    pub fn next_route(&mut self) -> Option<(String, Day, Day)> {
        let (from, through) = self.applied?;
        while let Some(route) = self.queue.pop_front() {
            if route.is_empty() || self.peers.contains(&route) {
                return Some((route, from, through));
            }
        }
        if self.busy {
            self.busy = false;
            self.status =
                "Read finished. Last-known or missing data is excluded from current totals.".into();
            self.due = true;
        }
        None
    }

    /// A route's summary answered.
    pub fn summary_answered(&mut self, route: &str, reply: Result<Summary, String>, now_ms: i64) {
        let accepted = match &reply {
            Ok(summary) => self.epochs.accept(summary),
            Err(_) => false,
        };
        let observation = self.snapshots.entry(route.to_owned()).or_default();
        observation.route = route.to_owned();
        observation.current = false;
        match reply {
            Err(error) => observation.error = error,
            Ok(_) if !accepted => {
                observation.error =
                    "Retired, older or ambiguous analytics source. Excluded from current totals."
                        .into();
            }
            Ok(reply) => {
                observation.error.clear();
                let reply = Arc::new(reply);
                // A lower revision of the same epoch never replaces a newer
                // one kept; a new epoch always replaces it (a purge, a reset).
                let replace = observation.agent.as_ref().is_none_or(|kept| {
                    kept.source_epoch != reply.source_epoch || reply.revision >= kept.revision
                });
                if replace {
                    observation.agent = Some(Arc::clone(&reply));
                }
                let kept = observation.agent.as_ref().expect("kept");
                observation.current =
                    fresh(kept, now_ms) && (Arc::ptr_eq(kept, &reply) || **kept == *reply);
                if !observation.current {
                    observation.error = "Summary is stale or its clock is unconfirmed.".into();
                }
                if route.is_empty() && observation.current {
                    self.local = observation.agent.clone();
                    self.local_reads += 1;
                }
            }
        }
    }

    /// A route's speech statistics answered.
    pub fn speech_answered(&mut self, route: &str, reply: Result<Speech, String>, now_ms: i64) {
        let observation = self.snapshots.entry(route.to_owned()).or_default();
        observation.route = route.to_owned();
        match reply {
            Ok(voice) => {
                observation.voice_current = true;
                observation.voice_error.clear();
                observation.voice = Some(Arc::new(voice));
                observation.voice_read_at = now_ms;
            }
            Err(error) => {
                observation.voice_current = false;
                observation.voice_error = error;
            }
        }
    }

    /// Each selected route's reads as they stand now: a summary or a voice
    /// read past its age is no longer current. Reads are shared, not copied.
    pub fn observations(&self, now_ms: i64) -> Vec<Observation> {
        self.routes
            .iter()
            .map(|route| {
                let mut observation = self.snapshots.get(route).cloned().unwrap_or_default();
                observation.route = route.clone();
                if observation.agent.is_none() && observation.error.is_empty() {
                    observation.error = "No current read available.".into();
                }
                observation.current = observation.current
                    && observation
                        .agent
                        .as_ref()
                        .is_some_and(|agent| fresh(agent, now_ms));
                observation.voice_current =
                    observation.voice_current && now_ms - observation.voice_read_at <= 300_000;
                observation
            })
            .collect()
    }

    /// What a due drawing needs, if one is due; quick, for under the lock.
    pub fn draw_due(&mut self, now_ms: i64) -> Option<Draw> {
        if !self.due {
            return None;
        }
        let (from, through) = self.applied?;
        self.due = false;
        self.draws += 1;
        Some(Draw {
            id: self.draws,
            from,
            through,
            routes: self.routes.len(),
            peer_limit: self.peer_limit,
            observations: self.observations(now_ms),
        })
    }

    /// Keeps a finished drawing unless a newer one is already shown, and
    /// hands back the one not kept: a fleet's drawing is large, so the
    /// caller frees it after letting the page go.
    #[must_use = "free the drawing not kept outside the page's lock"]
    pub fn drawn(&mut self, drawing: Drawing) -> Drawing {
        if drawing.id > self.drawing.id {
            std::mem::replace(&mut self.drawing, drawing)
        } else {
            drawing
        }
    }

    /// Draws now, on this thread, under whatever lock holds the page.
    pub fn draw(&mut self, now_ms: i64) {
        if let Some(draw) = self.draw_due(now_ms) {
            let drawing = draw.render();
            drop(self.drawn(drawing));
        }
    }

    /// The minute timer: ages the view unless a read runs.
    pub fn age(&mut self) {
        if !self.busy {
            self.due = true;
        }
    }

    /// A page of the drawn buckets for one provider (empty for all); an
    /// offset past the end shows the last page.
    pub fn rows(&self, provider: &str, offset: usize) -> Rows {
        let drawing = &self.drawing;
        let bucket = |&(computer, bucket): &(usize, usize)| {
            &drawing.computers[computer].agent.buckets[bucket]
        };
        let filtered: Vec<&(usize, usize)> = drawing
            .rows
            .iter()
            .filter(|place| provider.is_empty() || bucket(place).provider == provider)
            .collect();
        let total = filtered.len();
        let offset = if offset >= total {
            (total.saturating_sub(1) / PAGE_SIZE) * PAGE_SIZE
        } else {
            offset
        };
        Rows {
            rows: filtered
                .into_iter()
                .skip(offset)
                .take(PAGE_SIZE)
                .map(|place| {
                    let computer = &drawing.computers[place.0];
                    let b = bucket(place);
                    Row {
                        date: b.date.clone(),
                        route: computer.route.clone(),
                        provider: b.provider.clone(),
                        producer: b.producer.clone(),
                        current: computer.current,
                        counters: b.counters.clone(),
                        histograms: b.histograms.clone(),
                    }
                })
                .collect(),
            offset,
            total,
        }
    }

    pub fn view(&self, now_ms: i64) -> View {
        let idle = self.idle();
        View {
            status: self.status.clone(),
            busy: self.busy,
            mutating: self.mutating,
            can_refresh: idle,
            can_export: idle && !self.routes.is_empty(),
            can_change_local: idle && self.local_fresh(now_ms).is_some(),
            peers: self.peers.clone(),
            peer_limit: self.peer_limit,
            selection: self.selection.clone(),
            from: self.applied.map(|(from, _)| from.iso()),
            through: self.applied.map(|(_, through)| through.iso()),
            local: self.local.as_ref().map(|local| Local {
                enabled: local.enabled,
                retention_days: local.retention_days,
                epoch: local.source_epoch.clone(),
                reads: self.local_reads,
            }),
            report: self.drawing.report.clone(),
            purge_pending: self.purge_epoch.is_some(),
            shorten_pending: self.configure.as_ref().map(|change| change.retention_days),
        }
    }

    /// Why a click that needs this computer's current read does nothing.
    fn refused_click(&mut self, now_ms: i64, not_current: &str) -> Option<String> {
        let reason = if self.busy {
            READING
        } else if self.mutating {
            CHANGING
        } else if self.local_fresh(now_ms).is_none() {
            not_current
        } else {
            return None;
        };
        self.status = reason.into();
        Some(reason.into())
    }

    /// Apply locally clicked, with Qt's checks at the click: nothing runs,
    /// and the status says why, while a read or a change runs or without a
    /// fresh read of this computer. Otherwise the page is held, as Qt's is
    /// while it asks, and the change keeps the epoch of that read; the CLI
    /// compares it atomically. A shorter retention than the read's waits
    /// for [`Page::configure_confirmed`] (`shorten_pending`); `Ok(Some)` is
    /// the change to make now.
    pub fn begin_configure(
        &mut self,
        enabled: bool,
        retention_days: i64,
        now_ms: i64,
    ) -> Result<Option<Configure>, String> {
        if let Some(reason) = self.refused_click(now_ms, NOT_CURRENT_APPLY) {
            return Err(reason);
        }
        if !(1..=90).contains(&retention_days) {
            return Err("Choose a retention of 1 to 90 days.".into());
        }
        let Some(local) = self.local_fresh(now_ms) else {
            return Err(NOT_CURRENT_APPLY.into());
        };
        let shorter = retention_days < local.retention_days;
        let change = Configure {
            epoch: local.source_epoch.clone(),
            enabled,
            retention_days,
        };
        self.mutating = true;
        if shorter {
            self.configure = Some(change);
            return Ok(None);
        }
        self.status = "Applying local collection settings…".into();
        Ok(Some(change))
    }

    /// The person confirmed the shorter retention: the change to make.
    pub fn configure_confirmed(&mut self) -> Option<Configure> {
        let change = self.configure.take()?;
        self.status = "Applying local collection settings…".into();
        Some(change)
    }

    /// The person cancelled (or left the page) while asked.
    pub fn configure_cancelled(&mut self) {
        if self.configure.take().is_some() {
            self.mutating = false;
        }
    }

    /// The change answered; true when the page reads again (a failed or
    /// uncertain change is never retried).
    pub fn configure_answered(&mut self, reply: Result<(), String>) -> bool {
        self.mutating = false;
        self.local = None;
        match reply {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            }
        }
    }

    /// Purge local history: the range to read this computer's latest epoch
    /// for, before the person sees what a purge removes, or why nothing
    /// happens (as for Apply locally).
    pub fn begin_purge(&mut self, now_ms: i64) -> Result<(Day, Day), String> {
        if let Some(reason) = self.refused_click(now_ms, NOT_CURRENT_PURGE) {
            return Err(reason);
        }
        let range = self.applied.ok_or_else(|| NOT_CURRENT_PURGE.to_owned())?;
        self.mutating = true;
        Ok(range)
    }

    /// The latest summary before a purge: a fresh one from a known epoch
    /// waits for confirmation, anything else ends the purge.
    pub fn purge_checked(&mut self, reply: Result<Summary, String>, now_ms: i64) -> bool {
        let checked = reply.and_then(|summary| {
            if fresh(&summary, now_ms) && self.epochs.accept(&summary) {
                Ok(summary.source_epoch)
            } else {
                Err("Local analytics epoch is stale or ambiguous. Refresh before purging.".into())
            }
        });
        match checked {
            Ok(epoch) => {
                self.purge_epoch = Some(epoch);
                true
            }
            Err(error) => {
                self.mutating = false;
                self.status = error;
                false
            }
        }
    }

    /// The person confirmed: the epoch to purge.
    pub fn purge_confirmed(&mut self) -> Option<String> {
        self.purge_epoch.take()
    }

    /// The person cancelled (or left the page) while asked.
    pub fn purge_cancelled(&mut self) {
        if self.purge_epoch.take().is_some() {
            self.mutating = false;
        }
    }

    /// The purge answered; true when the page reads again.
    pub fn purge_answered(&mut self, reply: Result<(), String>) -> bool {
        self.mutating = false;
        self.local = None;
        self.snapshots.clear();
        match reply {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                self.due = true;
                false
            }
        }
    }

    /// What Review aggregate diagnostics… shows, to be written off the lock
    /// with [`super::diagnostic_text`]; None while a read or a change runs.
    pub fn diagnostic_input(&self, now_ms: i64) -> Option<(Vec<Observation>, Day, Day)> {
        let (from, through) = self.applied?;
        self.idle()
            .then(|| (self.observations(now_ms), from, through))
    }

    /// The diagnostic's text, written here.
    pub fn diagnostic(&self, now_ms: i64) -> Option<String> {
        self.diagnostic_input(now_ms)
            .map(|(observations, from, through)| {
                super::diagnostic_text(&super::diagnostic(&observations, from, through))
            })
    }

    /// The line under the page.
    pub fn set_status(&mut self, status: &str) {
        self.status = status.to_owned();
    }
}
