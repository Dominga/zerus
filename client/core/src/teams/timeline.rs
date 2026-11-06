//! Task 92: the profile's Time view (Qt's `TeamProfile::Model` and its
//! `Cache`, tray/src/TeamProfile.cpp). One scope (a phase, or the plan) is
//! read on request: the team's status, the scope's profile, its task
//! timelines in at most [`MAX_SPAN_PAGES`] pages of [`SPANS_PAGE`] tasks, and
//! at most [`TEST_WINDOW_PAGE`] test runs of its window. [`build`] projects
//! them into the lanes the webview draws; the webview never parses a reply.
//!
//! Bounded: the pages above, at most [`MAX_SPANS`] spans in all (the rest is
//! named by `truncated`), at most [`MAX_LANES`] lanes and titles of at most
//! [`TITLE_CHARS`] characters for the tasks drawn.
use super::read::{read as team_read, TeamRead, SPANS_PAGE, TEST_WINDOW_PAGE};
use crate::ComputerId;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// Pages of task timelines one scope reads (Qt's Cache::MaxSpanPages).
pub const MAX_SPAN_PAGES: u64 = 20;
/// Spans one view keeps in all; the rest is named, never drawn.
pub const MAX_SPANS: usize = 20_000;
/// Lanes one view keeps.
pub const MAX_LANES: usize = 200;
/// The longest task title a view keeps.
pub const TITLE_CHARS: usize = 200;

/// Span kinds that are a member's own time (Qt's `Busy`); build and merge
/// run beside them, every other kind is a wait of the task.
const BUSY: [&str; 6] = [
    "work",
    "rework",
    "stage",
    "question",
    "permission",
    "background",
];

/// One span of a task's timeline: work, rework, a stage, a wait, a build, or
/// (kind `tests`) a test run, whose `member` is its suite and `stage_kind`
/// its status.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TimelineSpan {
    pub task: u64,
    pub attempt: u64,
    pub kind: String,
    pub stage_kind: String,
    pub member: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub record: String,
    pub from: f64,
    pub to: f64,
    /// Still open: `to` is the moment of the read.
    pub running: bool,
}

impl TimelineSpan {
    fn seconds(&self) -> f64 {
        self.to - self.from
    }
}

/// A member's lane: its own spans (build and merge among them), the waits of
/// the tasks it was given, and the moments it had no work while a task was
/// open.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TimelineLane {
    pub member: String,
    pub role: String,
    pub model: String,
    pub utilization: f64,
    pub busy_s: u64,
    pub idle_s: u64,
    pub spans: Vec<TimelineSpan>,
    pub waits: Vec<TimelineSpan>,
    pub idle: Vec<[f64; 2]>,
}

/// A task's estimate (minutes, null without one) against its working time
/// (work and rework) in the scope.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TimelineEstimate {
    pub task: u64,
    pub estimate_min: Option<f64>,
    pub worked_s: f64,
    pub running: bool,
}

/// What the Time view draws for one scope.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TimelineView {
    pub team: String,
    /// The phase, or null for the plan.
    pub phase: Option<u64>,
    pub from: f64,
    pub to: f64,
    pub elapsed_s: Option<u64>,
    pub worked_s: Option<u64>,
    pub lanes: Vec<TimelineLane>,
    /// The real critical path (each task from its first to its last worked
    /// moment) and the planned one (end to end, each task's first attempt).
    pub real: Vec<TimelineSpan>,
    pub planned: Vec<TimelineSpan>,
    pub real_s: u64,
    pub planned_s: u64,
    pub tests: Vec<TimelineSpan>,
    /// As many test runs as one read takes: there may be more.
    pub tests_more: bool,
    pub estimates: Vec<TimelineEstimate>,
    /// Task id to title, for the tasks drawn.
    pub titles: BTreeMap<u64, String>,
    pub tasks: u64,
    /// Spans past the page or span bound were not read or not kept.
    pub truncated: bool,
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Sorted, merged intervals.
fn unite(mut list: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    list.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut out: Vec<[f64; 2]> = Vec::new();
    for range in list {
        match out.last_mut() {
            Some(last) if range[0] <= last[1] => last[1] = last[1].max(range[1]),
            _ => out.push(range),
        }
    }
    out
}

/// `open` without `busy`; both united.
fn minus(open: &[[f64; 2]], busy: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for range in open {
        let mut at = range[0];
        for cut in busy {
            if cut[1] <= at || cut[0] >= range[1] {
                continue;
            }
            if cut[0] > at {
                out.push([at, cut[0]]);
            }
            at = at.max(cut[1]);
        }
        if at < range[1] {
            out.push([at, range[1]]);
        }
    }
    out
}

/// Projects one scope's reads as Qt's `Model::build` does: `status` is
/// `team status`, `profile` the scope's `team profile`, `timelines` the
/// `tasks` of its `team spans` pages and `tests` the window's test runs.
#[allow(clippy::too_many_arguments)]
pub fn build(
    team: &str,
    phase: Option<u64>,
    status: &Value,
    profile: &Value,
    timelines: &[Value],
    tests: &[Value],
    truncated: bool,
    now: f64,
) -> TimelineView {
    let summary = profile.get("summary").unwrap_or(&Value::Null);
    let mut view = TimelineView {
        team: team.to_owned(),
        phase,
        from: number(summary, "from").unwrap_or(0.0),
        to: number(summary, "to").unwrap_or(0.0),
        elapsed_s: summary
            .get("totals")
            .and_then(|totals| totals.get("elapsed_s"))
            .and_then(Value::as_u64),
        worked_s: summary
            .get("totals")
            .and_then(|totals| totals.get("worked_s"))
            .and_then(Value::as_u64),
        truncated,
        ..TimelineView::default()
    };
    let mut titles: HashMap<u64, String> = HashMap::new();
    let mut assignees: HashMap<u64, String> = HashMap::new();
    for task in items(status, "tasks") {
        if let Some(id) = task.get("id").and_then(Value::as_u64) {
            titles.insert(id, text(task, "title").chars().take(TITLE_CHARS).collect());
            assignees.insert(id, text(task, "assignee"));
        }
    }
    let role_of = |member: &str| {
        items(status, "members")
            .iter()
            .find(|row| text(row, "id") == member)
            .map(|row| text(row, "role"))
            .unwrap_or_default()
    };
    let mut busy: HashMap<String, Vec<[f64; 2]>> = HashMap::new();
    let mut spans: HashMap<String, Vec<TimelineSpan>> = HashMap::new();
    let mut waits: HashMap<String, Vec<TimelineSpan>> = HashMap::new();
    let mut worked: HashMap<u64, [f64; 2]> = HashMap::new();
    let mut busy_seconds: HashMap<u64, f64> = HashMap::new();
    let mut open: Vec<[f64; 2]> = Vec::new();
    let mut drawn: BTreeSet<u64> = BTreeSet::new();
    let mut kept = 0usize;
    let (mut low, mut high) = (
        if view.from > 0.0 { view.from } else { f64::MAX },
        if view.to > 0.0 { view.to } else { 0.0 },
    );
    for timeline in timelines {
        let Some(task) = timeline.get("task").and_then(Value::as_u64) else {
            continue;
        };
        view.tasks += 1;
        let (mut first, mut last) = (f64::MAX, 0.0f64);
        let mut running = false;
        let raws = items(timeline, "spans")
            .iter()
            .map(|raw| (raw, None))
            .chain(
                items(timeline, "overlay")
                    .iter()
                    .map(|raw| (raw, Some("build"))),
            );
        for (raw, forced) in raws {
            let Some(from) = number(raw, "from") else {
                continue;
            };
            let open_span = raw.get("to").is_none_or(Value::is_null);
            let to = if open_span {
                now
            } else {
                number(raw, "to").unwrap_or(from)
            };
            if to <= from {
                continue;
            }
            if kept >= MAX_SPANS {
                view.truncated = true;
                continue;
            }
            kept += 1;
            let span = TimelineSpan {
                task,
                attempt: raw.get("attempt").and_then(Value::as_u64).unwrap_or(0),
                kind: forced.map_or_else(|| text(raw, "kind"), str::to_owned),
                stage_kind: text(raw, "stage_kind"),
                member: text(raw, "member"),
                record: String::new(),
                from,
                to,
                running: open_span,
            };
            first = first.min(from);
            last = last.max(to);
            let is_busy = BUSY.contains(&span.kind.as_str());
            if is_busy {
                // Task #746: the work itself, as the estimate counts it.
                if span.kind == "work" || span.kind == "rework" {
                    *busy_seconds.entry(task).or_default() += span.seconds();
                    running |= span.running;
                }
                let at = worked.entry(task).or_insert([from, to]);
                *at = [at[0].min(from), at[1].max(to)];
            }
            drawn.insert(task);
            if is_busy || span.kind == "build" {
                let who = if span.member.is_empty() {
                    assignees.get(&task).cloned().unwrap_or_default()
                } else {
                    span.member.clone()
                };
                if is_busy {
                    busy.entry(who.clone()).or_default().push([from, to]);
                }
                spans.entry(who).or_default().push(span);
            } else {
                // A wait shows under the lane of the member the task was given.
                waits
                    .entry(assignees.get(&task).cloned().unwrap_or_default())
                    .or_default()
                    .push(span);
            }
        }
        if last > 0.0 {
            open.push([first, last]);
            low = low.min(first);
            high = high.max(last);
        }
        if timeline.get("estimate_source").is_some() {
            let estimate = number(timeline, "estimate_minutes");
            let worked_s = busy_seconds.get(&task).copied().unwrap_or(0.0);
            if estimate.is_some_and(|minutes| minutes > 0.0) || worked_s > 0.0 {
                view.estimates.push(TimelineEstimate {
                    task,
                    estimate_min: estimate.filter(|minutes| *minutes > 0.0),
                    worked_s,
                    running,
                });
            }
        }
    }
    if high > 0.0 {
        view.from = low;
        view.to = high;
    }
    let open = unite(open);
    // Lanes: the members the profile lists, in its order, then the lead.
    let mut members: Vec<Value> = items(profile, "members").to_vec();
    let lead = text(status, "lead");
    let listed: BTreeSet<String> = members.iter().map(|row| text(row, "member")).collect();
    if !lead.is_empty()
        && !listed.contains(&lead)
        && (spans.contains_key(&lead) || waits.contains_key(&lead))
    {
        members.push(serde_json::json!({"member": lead, "role": role_of(&lead)}));
    }
    for row in members.iter().take(MAX_LANES) {
        let member = text(row, "member");
        let role = row
            .get("role")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| role_of(&member));
        let mut lane_spans = spans.remove(&member).unwrap_or_default();
        lane_spans.sort_by(|a, b| a.from.total_cmp(&b.from));
        let idle = minus(&open, &unite(busy.remove(&member).unwrap_or_default()));
        let idle_s = match row.get("idle_s").and_then(Value::as_f64) {
            Some(seconds) => seconds.max(0.0) as u64,
            None => idle.iter().map(|range| range[1] - range[0]).sum::<f64>() as u64,
        };
        view.lanes.push(TimelineLane {
            role,
            model: text(row, "model"),
            utilization: number(row, "utilization").unwrap_or(0.0),
            busy_s: number(row, "busy_s").unwrap_or(0.0).max(0.0) as u64,
            idle_s,
            spans: lane_spans,
            waits: waits.remove(&member).unwrap_or_default(),
            idle,
            member,
        });
    }
    // The critical paths.
    let critical = profile.get("critical_path").unwrap_or(&Value::Null);
    let real = critical.get("real").unwrap_or(&Value::Null);
    for id in items(real, "tasks").iter().filter_map(Value::as_u64) {
        let Some(range) = worked.get(&id) else {
            continue;
        };
        view.real.push(TimelineSpan {
            task: id,
            kind: "work".into(),
            from: range[0],
            to: range[1],
            ..TimelineSpan::default()
        });
    }
    view.real.sort_by(|a, b| a.from.total_cmp(&b.from));
    view.real_s = number(real, "s").unwrap_or(0.0).max(0.0) as u64;
    let planned = critical.get("planned").unwrap_or(&Value::Null);
    let mut at = view.from;
    for id in items(planned, "tasks").iter().filter_map(Value::as_u64) {
        // Its first attempt as it went, returned or not.
        let own = || {
            view.lanes
                .iter()
                .flat_map(|lane| lane.spans.iter())
                .filter(move |span| span.task == id && span.kind != "build")
        };
        let Some(lowest) = own().map(|span| span.attempt).min() else {
            continue;
        };
        let first: f64 = own()
            .filter(|span| span.attempt == lowest)
            .map(TimelineSpan::seconds)
            .sum();
        if first <= 0.0 {
            continue;
        }
        view.planned.push(TimelineSpan {
            task: id,
            kind: "work".into(),
            from: at,
            to: at + first,
            ..TimelineSpan::default()
        });
        at += first;
        view.planned_s += first as u64;
        drawn.insert(id);
    }
    drawn.extend(view.real.iter().map(|span| span.task));
    // Task #719: the window's test runs, oldest first.
    for record in tests.iter().take(TEST_WINDOW_PAGE as usize) {
        let Some(from) = number(record, "at") else {
            continue;
        };
        let to = from + (number(record, "duration_ms").unwrap_or(0.0) / 1000.0).max(1.0);
        if view.to > view.from && (to < view.from || from > view.to) {
            continue;
        }
        let task = record.get("task").and_then(Value::as_u64).unwrap_or(0);
        if task > 0 {
            drawn.insert(task);
        }
        view.tests.push(TimelineSpan {
            task,
            kind: "tests".into(),
            stage_kind: text(record, "status"),
            member: text(record, "suite"),
            record: text(record, "id"),
            from,
            to,
            ..TimelineSpan::default()
        });
    }
    view.tests.sort_by(|a, b| a.from.total_cmp(&b.from));
    view.tests_more = tests.len() >= TEST_WINDOW_PAGE as usize;
    drawn.extend(view.estimates.iter().map(|row| row.task));
    view.titles = drawn
        .into_iter()
        .filter_map(|task| titles.get(&task).map(|title| (task, title.clone())))
        .collect();
    view
}

/// Reads one scope (Qt's `Cache::load`, `spans` and `tests`): the status,
/// the profile, at most [`MAX_SPAN_PAGES`] pages of timelines, then the
/// window's test runs. A refused test read (an older hgs) leaves the Tests
/// row out; the timeline still shows.
pub fn read(
    hgs: &Path,
    computer: &ComputerId,
    team: &str,
    phase: Option<u64>,
    now: f64,
) -> Result<TimelineView, String> {
    let phase = phase.filter(|phase| *phase > 0);
    let get = |read: &TeamRead| team_read(hgs, computer, read).map_err(|error| error.to_string());
    let status = get(&TeamRead::Status {
        team: team.to_owned(),
    })?;
    let profile = get(&TeamRead::ScopedProfile {
        team: team.to_owned(),
        phase,
    })?;
    let mut timelines: Vec<Value> = Vec::new();
    let mut after = 0;
    let mut truncated = false;
    for page in 0..MAX_SPAN_PAGES {
        let reply = get(&TeamRead::ScopedSpans {
            team: team.to_owned(),
            phase,
            after,
        })?;
        timelines.extend(
            items(&reply, "tasks")
                .iter()
                .take(SPANS_PAGE as usize)
                .cloned(),
        );
        match reply.get("next_cursor").and_then(Value::as_u64) {
            Some(next) if next > after => {
                if page + 1 == MAX_SPAN_PAGES {
                    truncated = true;
                }
                after = next;
            }
            _ => break,
        }
    }
    let summary = profile.get("summary").unwrap_or(&Value::Null);
    let tests = get(&TeamRead::TestWindow {
        team: team.to_owned(),
        from: number(summary, "from"),
        before: number(summary, "to").map(|to| to + 1.0),
    })
    .map(|reply| items(&reply, "items").to_vec())
    .unwrap_or_default();
    Ok(build(
        team, phase, &status, &profile, &timelines, &tests, truncated, now,
    ))
}
