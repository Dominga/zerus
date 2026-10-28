//! The Settings page's state (AnalyticsSettings.cpp): the routes a refresh
//! reads one at a time, each route's last reads, the epochs seen, this
//! computer's collection controls, and what the page shows, rendered when
//! Qt renders it (a refresh's start and end, a failed change, the minute
//! timer) so no total is drawn from a read half done.

use std::collections::{BTreeMap, VecDeque};

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
    /// The daily buckets rendered.
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
    pub local: Option<Local>,
    /// None before the first refresh.
    pub report: Option<Report>,
    /// A purge is waiting for the person's confirmation.
    pub purge_pending: bool,
}

#[derive(Debug, Default)]
pub struct Page {
    peers: Vec<String>,
    peer_limit: bool,
    snapshots: BTreeMap<String, Observation>,
    epochs: EpochGuard,
    routes: Vec<String>,
    queue: VecDeque<String>,
    local: Option<Summary>,
    local_reads: u64,
    applied: Option<(Day, Day)>,
    busy: bool,
    mutating: bool,
    purge_epoch: Option<String>,
    status: String,
    report: Option<Report>,
    rows: Vec<Row>,
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
        self.local.as_ref().filter(|local| fresh(local, now_ms))
    }

    /// Apply and refresh (and opening the section): reads `selection` (this
    /// computer, `*` for all, or a peer) for the inclusive UTC days
    /// `from`..`through`. Returns whether a read began; the caller then
    /// reads each [`Page::next`] route.
    pub fn refresh(&mut self, selection: &str, from: &str, through: &str, now_ms: i64) -> bool {
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
        if self.applied != Some(range) {
            self.snapshots.clear();
        }
        self.applied = Some(range);
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
        self.render(now_ms);
        self.status = format!("Reading {} configured route(s)…", self.routes.len());
        true
    }

    /// The next route to read, one at a time so a large catalog never runs
    /// more than one child; None once the read is finished.
    pub fn next(&mut self, now_ms: i64) -> Option<(String, Day, Day)> {
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
            self.render(now_ms);
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
                // A lower revision of the same epoch never replaces a newer
                // one kept; a new epoch always replaces it (a purge, a reset).
                let replace = observation.agent.as_ref().is_none_or(|kept| {
                    kept.source_epoch != reply.source_epoch || reply.revision >= kept.revision
                });
                if replace {
                    observation.agent = Some(reply.clone());
                }
                let kept = observation.agent.as_ref().expect("kept");
                observation.current = fresh(kept, now_ms) && *kept == reply;
                if !observation.current {
                    observation.error = "Summary is stale or its clock is unconfirmed.".into();
                }
                if route.is_empty() && observation.current {
                    self.local = Some(reply);
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
                observation.voice = Some(voice);
                observation.voice_read_at = now_ms;
            }
            Err(error) => {
                observation.voice_current = false;
                observation.voice_error = error;
            }
        }
    }

    /// Each selected route's reads as they stand now: a summary or a voice
    /// read past its age is no longer current.
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

    /// AnalyticsSettings::render.
    fn render(&mut self, now_ms: i64) {
        let Some((from, through)) = self.applied else {
            return;
        };
        let all = self.observations(now_ms);
        let computers = unique_computers(&all);
        let mut current: Vec<&Bucket> = Vec::new();
        let mut rows = Vec::new();
        let mut reports = Vec::new();
        for observation in &computers {
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
            for bucket in &agent.buckets {
                rows.push(Row {
                    date: bucket.date.clone(),
                    route: observation.route.clone(),
                    provider: bucket.provider.clone(),
                    producer: bucket.producer.clone(),
                    current: observation.current,
                    counters: bucket.counters.clone(),
                    histograms: bucket.histograms.clone(),
                });
                if observation.current {
                    current.push(bucket);
                }
            }
        }
        let missing: Vec<&Observation> = all.iter().filter(|o| o.agent.is_none()).collect();
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
        rows.sort_by(|a, b| {
            (&a.date, display(&a.route), &a.provider, &a.producer).cmp(&(
                &b.date,
                display(&b.route),
                &b.provider,
                &b.producer,
            ))
        });
        self.report = Some(Report {
            from: from.iso(),
            through: through.iso(),
            routes: self.routes.len(),
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
        });
        self.rows = rows;
    }

    /// The minute timer: ages the view unless a read runs.
    pub fn age(&mut self, now_ms: i64) {
        if !self.busy {
            self.render(now_ms);
        }
    }

    /// A page of the rendered buckets for one provider (empty for all); an
    /// offset past the end shows the last page.
    pub fn rows(&self, provider: &str, offset: usize) -> Rows {
        let filtered: Vec<&Row> = self
            .rows
            .iter()
            .filter(|row| provider.is_empty() || row.provider == provider)
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
                .cloned()
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
            local: self.local.as_ref().map(|local| Local {
                enabled: local.enabled,
                retention_days: local.retention_days,
                reads: self.local_reads,
            }),
            report: self.report.clone(),
            purge_pending: self.purge_epoch.is_some(),
        }
    }

    /// Apply locally: the epoch to pin, once the person has confirmed a
    /// shorter retention; None when the controls are not usable now.
    pub fn begin_configure(&mut self, now_ms: i64) -> Option<String> {
        if !self.idle() {
            return None;
        }
        let epoch = self.local_fresh(now_ms)?.source_epoch.clone();
        self.mutating = true;
        self.status = "Applying local collection settings…".into();
        Some(epoch)
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
    /// for, before the person sees what a purge removes.
    pub fn begin_purge(&mut self, now_ms: i64) -> Option<(Day, Day)> {
        if !self.idle() || self.local_fresh(now_ms).is_none() {
            return None;
        }
        self.mutating = true;
        self.applied
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
    pub fn purge_answered(&mut self, reply: Result<(), String>, now_ms: i64) -> bool {
        self.mutating = false;
        self.local = None;
        self.snapshots.clear();
        match reply {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                self.render(now_ms);
                false
            }
        }
    }

    /// Review aggregate diagnostics: the file's text, or None while a read
    /// or a change runs.
    pub fn diagnostic(&self, now_ms: i64) -> Option<String> {
        let (from, through) = self.applied?;
        self.idle().then(|| {
            super::diagnostic_text(&super::diagnostic(
                &self.observations(now_ms),
                from,
                through,
            ))
        })
    }

    /// The line under the page, set when a saved file's outcome is known.
    pub fn set_status(&mut self, status: &str) {
        self.status = status.to_owned();
    }
}
