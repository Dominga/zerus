//! WhiteWh fork: a task's timeline as typed spans (U38), computed on request
//! from its attempts, its U36 stage runs and a bounded window of the team's
//! journal, the way U37's `timing` reads them. The spans of one lane never
//! overlap: a question inside the work cuts the work, a stage after the report
//! covers the lead's wait. Builds lie on top of the work in `overlay`. A gap
//! nothing recorded stays `unknown`, never guessed; a past phase gets its
//! spans the same way from what its attempts kept.
use super::model::{AttemptState, Task, TaskStatus, Team};
use super::timing::{opened, ready, Moment};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The journal entries a timeline reads; the rest is in the team file.
pub(crate) const EVENT_KINDS: &[&str] = &[
    "question_forwarded", "permission_answered", "stop_told", "held", "released",
    // U63 (owner, 2026-10-31): a phase's pause, and the owner's acceptance a
    // later phase waits for.
    "phase_paused", "phase_unpaused", "phase_returned_to_draft",
    "phase_awaiting_acceptance", "phase_owner_accepted", "phase_acceptance_sent_back", "phase_acceptance_changed",
];
/// One read of the journal holds at most this many entries; past it the
/// timelines say the events were cut.
pub(crate) const EVENT_LIMIT: i64 = 5000;
/// A page of a phase's or the plan's timelines.
pub(crate) const TASK_PAGE: usize = 50;

/// One journal entry as `Store::span_events` reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Journal {
    pub at: f64,
    pub kind: String,
    pub detail: Value,
}

/// A stage run that held a member's folder (a test, a command or land stage).
#[derive(Clone, Debug)]
pub(crate) struct Holder {
    task: u32,
    folder: String,
    from: f64,
    to: f64,
}

// Which candidate wins where two cover the same moment.
const BLOCK: u8 = 8;
const HOLD: u8 = 7;
const FOLDER: u8 = 6;
const STOP: u8 = 6;
const STAGE: u8 = 5;
const LEAD: u8 = 4;
const BACKGROUND: u8 = 4;
const NO_REPORT: u8 = 3;
const WORK: u8 = 2;
const WAIT: u8 = 1;

struct Candidate {
    kind: &'static str,
    from: f64,
    to: f64,
    rank: u8,
    /// Still going at `now`: its `to` is null.
    running: bool,
    extra: Map<String, Value>,
}

impl Candidate {
    fn new(kind: &'static str, from: f64, to: f64, rank: u8, running: bool) -> Self {
        Candidate { kind, from, to, rank, running, extra: Map::new() }
    }
    fn with(mut self, key: &str, value: Value) -> Self {
        if !value.is_null() && value != json!("") {
            self.extra.insert(key.to_owned(), value);
        }
        self
    }
}

fn seconds(value: f64) -> i64 {
    value.max(0.0).round() as i64
}

/// Every stage run that held a folder, once per request.
pub(crate) fn holders(team: &Team, now: f64) -> Vec<Holder> {
    let mut list = Vec::new();
    for task in &team.tasks {
        let Some(pipeline) = &task.pipeline_run else { continue };
        for run in &pipeline.runs {
            let command = pipeline.stages.get(run.stage).is_some_and(|stage| stage.runs_command());
            if !(run.holds || run.awaiting_run || command) || run.folder.is_empty() {
                continue;
            }
            let from = run.delivered_at.unwrap_or(run.at);
            let to = run.ended_at.unwrap_or(if run.state.open() { now } else { from });
            if to > from {
                list.push(Holder { task: task.id, folder: run.folder.clone(), from, to });
            }
        }
    }
    list
}

/// The journal window the chosen tasks need: from the earliest moment one of
/// them recorded to now.
pub(crate) fn window(team: &Team, tasks: &[&Task], now: f64) -> Option<(f64, f64)> {
    let from = tasks
        .iter()
        .filter_map(|task| match opened(team, task) {
            Moment::At(at) => Some(at),
            _ => task.attempts.first().map(|attempt| attempt.sent_at),
        })
        .reduce(f64::min)?;
    Some((from, now))
}

/// The member's native questions and permission requests between `from` and
/// `to`, paired from the journal: asked (`question_forwarded`) to answered
/// (`permission_answered`, with who answered). The journal names a member by
/// its role; when two members share it the block is not attributed (None).
fn native_blocks(team: &Team, member: &str, journal: &[Journal], from: f64, to: f64, now: f64) -> Option<Vec<Candidate>> {
    let label = team.role_of(member);
    let mine = |entry: &Journal| entry.detail["member"].as_str() == Some(label.as_str());
    if !journal.iter().any(|entry| entry.kind == "question_forwarded" && mine(entry) && entry.at >= from && entry.at <= to) {
        return Some(Vec::new());
    }
    if team.members.iter().filter(|other| team.role_of(&other.id) == label).count() > 1 {
        return None;
    }
    let mut blocks = Vec::new();
    let mut asked: Option<(f64, bool)> = None;
    for entry in journal.iter().filter(|entry| mine(entry)) {
        match entry.kind.as_str() {
            "question_forwarded" => asked = Some((entry.at, entry.detail["approval"] == true)),
            "permission_answered" => {
                if let Some((at, approval)) = asked.take() {
                    blocks.push(
                        Candidate::new(if approval { "permission" } else { "question" }, at, entry.at, BLOCK, false)
                            .with("by", entry.detail["by"].clone())
                            .with("decision", entry.detail["decision"].clone()),
                    );
                }
            }
            _ => {}
        }
    }
    if let Some((at, approval)) = asked {
        blocks.push(Candidate::new(if approval { "permission" } else { "question" }, at, to, BLOCK, to >= now));
    }
    blocks.retain(|block| block.to > from && block.from < to);
    Some(blocks)
}

/// The task's lead holds (A5) from the journal; a hold the journal lost but
/// the task still has counts from its own moment.
fn holds(task: &Task, journal: &[Journal], live: bool, now: f64) -> Vec<Candidate> {
    let mut list = Vec::new();
    let mut open: Option<(f64, Value)> = None;
    let this = |entry: &Journal| entry.detail["task"].as_u64() == Some(u64::from(task.id));
    for entry in journal.iter().filter(|entry| this(entry)) {
        match entry.kind.as_str() {
            "held" => {
                if open.is_none() {
                    open = Some((entry.at, entry.detail["by"].clone()));
                }
            }
            "released" => {
                if let Some((at, by)) = open.take() {
                    list.push(Candidate::new("hold", at, entry.at, HOLD, false).with("by", by));
                }
            }
            _ => {}
        }
    }
    match (open, &task.hold) {
        (Some((at, by)), Some(_)) if live => list.push(Candidate::new("hold", at, now, HOLD, true).with("by", by)),
        (None, Some(hold)) if live => list.push(Candidate::new("hold", hold.at, now, HOLD, true).with("by", json!(hold.by))),
        _ => {}
    }
    list
}

/// U63 (owner, 2026-10-31): the task's waits that are the owner's or the
/// lead's call on a phase: its own phase paused ("phase_pause"), or a phase
/// before it waiting for the owner's acceptance ("owner_acceptance").
fn phase_waits(team: &Team, task: &Task, journal: &[Journal], live: bool, now: f64) -> Vec<Candidate> {
    let mut list = Vec::new();
    let order_of = |n: u32| team.phases.iter().find(|phase| phase.n == n).map_or(n, |phase| phase.order());
    let mine = order_of(task.phase);
    let mut paused: Option<(f64, Value)> = None;
    let mut awaiting: BTreeMap<u64, f64> = BTreeMap::new();
    for entry in journal {
        let Some(phase) = entry.detail["phase"].as_u64() else { continue };
        let own = phase == u64::from(task.phase);
        let before = u32::try_from(phase).is_ok_and(|n| n != task.phase && order_of(n) < mine);
        match entry.kind.as_str() {
            "phase_paused" if own && paused.is_none() => paused = Some((entry.at, entry.detail["by"].clone())),
            "phase_unpaused" | "phase_returned_to_draft" if own => {
                if let Some((at, by)) = paused.take() {
                    list.push(Candidate::new("phase_pause", at, entry.at, HOLD, false).with("by", by));
                }
            }
            "phase_awaiting_acceptance" if before => {
                awaiting.entry(phase).or_insert(entry.at);
            }
            "phase_owner_accepted" | "phase_acceptance_sent_back" | "phase_acceptance_changed" if before => {
                if let Some(at) = awaiting.remove(&phase) {
                    list.push(Candidate::new("owner_acceptance", at, entry.at, HOLD, false).with("phase", json!(phase)));
                }
            }
            _ => {}
        }
    }
    if live {
        if let Some((at, by)) = paused.filter(|_| super::plan::phase_pause(team, task.phase).is_some()) {
            list.push(Candidate::new("phase_pause", at, now, HOLD, true).with("by", by));
        }
        if let Some(held) = super::plan::acceptance_block(team, task.phase) {
            if let Some(at) = awaiting.get(&u64::from(held)) {
                list.push(Candidate::new("owner_acceptance", *at, now, HOLD, true).with("phase", json!(held)));
            }
        }
    }
    list
}

/// Paints the candidates onto [start, end]: where two cover a moment the
/// higher rank wins (the later one on a tie); where none does, `unknown`.
fn paint(lane: &[Candidate], start: f64, end: f64, now: f64) -> Vec<Value> {
    let mut points = vec![start, end];
    for candidate in lane {
        points.push(candidate.from.clamp(start, end));
        points.push(candidate.to.clamp(start, end));
    }
    points.sort_by(f64::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let mut pieces: Vec<(Option<usize>, f64, f64)> = Vec::new();
    for pair in points.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        if to - from <= 1e-9 {
            continue;
        }
        let middle = (from + to) / 2.0;
        let best = lane
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.from <= middle && middle < candidate.to)
            .max_by_key(|(index, candidate)| (candidate.rank, *index))
            .map(|(index, _)| index);
        match pieces.last_mut() {
            Some(last) if last.0 == best && (last.2 - from).abs() < 1e-9 => last.2 = to,
            _ => pieces.push((best, from, to)),
        }
    }
    pieces
        .into_iter()
        .map(|(index, from, to)| match index {
            Some(index) => {
                let candidate = &lane[index];
                let running = candidate.running && to >= now - 1e-9;
                let mut value = json!({"kind":candidate.kind,"from":from,"to":if running { None } else { Some(to) },"s":seconds(to - from)});
                value.as_object_mut().unwrap().extend(candidate.extra.clone());
                value
            }
            None => json!({"kind":"unknown","from":from,"to":to,"s":seconds(to - from)}),
        })
        .collect()
}

/// The task's timeline at `now`: {task, status, spans: [{kind, from, to, s,
/// ...}], overlay: [builds], totals: {kind: s}, unknown?}. `to` is null on a
/// span still going; a leading span with a null `from` is a wait whose start
/// was never recorded.
pub(crate) fn spans(team: &Team, task: &Task, journal: &[Journal], holders: &[Holder], now: f64) -> Value {
    let live = !task.status.finished() && task.status != TaskStatus::Draft;
    let mut lane: Vec<Candidate> = Vec::new();
    let mut overlay: Vec<Value> = Vec::new();
    let mut unknown: BTreeSet<&str> = BTreeSet::new();
    let opened = opened(team, task);
    let ready = ready(team, task, opened);
    let pending = live && task.status == TaskStatus::Pending && task.open_attempt().is_none();
    if !task.deps.is_empty() {
        match (opened, ready) {
            (Moment::At(open), Moment::At(ready)) => lane.push(Candidate::new("deps", open, ready, WAIT, false)),
            (Moment::At(open), Moment::NotYet) if live => lane.push(Candidate::new("deps", open, now, WAIT, true)),
            _ => {}
        }
    }
    // Waits for a free member: (from, to, still going, the member it waited for).
    let mut waits: Vec<(f64, f64, bool, String)> = Vec::new();
    if let Moment::At(ready) = ready {
        match task.attempts.first() {
            Some(first) => waits.push((ready, first.sent_at, false, first.member.clone())),
            None if pending => waits.push((ready, now, true, task.assignee.clone())),
            None => {}
        }
    }
    for (index, attempt) in task.attempts.iter().enumerate() {
        let next = task.attempts.get(index + 1);
        let open = attempt.state.open();
        let tag = |candidate: Candidate| candidate.with("attempt", json!(attempt.n)).with("member", json!(attempt.member));
        match attempt.delivered_at {
            Some(delivered) => lane.push(tag(Candidate::new("delivery", attempt.sent_at, delivered, WORK, false))),
            None if open => lane.push(tag(Candidate::new("delivery", attempt.sent_at, now, WORK, true))),
            None => {}
        }
        if let (Some(delivered), Some(started)) = (attempt.delivered_at, attempt.started_at) {
            lane.push(tag(Candidate::new("start_delay", delivered, started, WORK, false)));
        }
        let begin = attempt.started_at.or(attempt.delivered_at);
        let stopped = journal
            .iter()
            .filter(|entry| entry.kind == "stop_told")
            .filter(|entry| entry.detail["task"].as_u64() == Some(u64::from(task.id)) && entry.detail["attempt"].as_u64() == Some(u64::from(attempt.n)))
            .map(|entry| entry.at)
            .reduce(f64::min);
        // As U37 counts it: to the report or the turn end, whichever came
        // first; a revoked attempt without either ends at the next hand-out.
        let ends = [attempt.report.as_ref().map(|report| report.at), attempt.turn_ended_at, stopped];
        let mut end = ends.into_iter().flatten().reduce(f64::min);
        let running = begin.is_some() && end.is_none() && open;
        if begin.is_some() && end.is_none() && !open {
            end = next.map(|next| next.sent_at);
        }
        if let Some(begin) = begin {
            let kind = if attempt.state == AttemptState::Rejected || attempt.pipeline_return { "rework" } else { "work" };
            match (running, end) {
                (true, _) => lane.push(tag(Candidate::new(kind, begin, now, WORK, true))),
                (false, Some(end)) => lane.push(tag(Candidate::new(kind, begin, end, WORK, false))),
                (false, None) => {
                    unknown.insert("work");
                }
            }
        }
        // Where this attempt's own part ends: its report, the next hand-out,
        // or now while it is open.
        let reach = attempt
            .report
            .as_ref()
            .map(|report| report.at)
            .or(next.map(|next| next.sent_at))
            .or(if open { Some(now) } else { end });
        if let (Some(turn), Some(reach)) = (attempt.turn_ended_at, reach) {
            let going = open && attempt.report.is_none() && reach >= now;
            if let Some(blocked) = attempt.blocked_since.filter(|blocked| *blocked < reach) {
                let to = attempt.report_ready_at.map_or(reach, |ready| ready.min(reach));
                lane.push(tag(Candidate::new("background", blocked.max(turn), to, BACKGROUND, going && attempt.report_ready_at.is_none())));
            }
            // The turn ended without the member's report: Zerus waited, then
            // took its last reply (A2), or the member never reported.
            let captured = attempt.report.as_ref().is_some_and(|report| report.captured);
            if (attempt.no_report || captured) && reach > turn {
                lane.push(tag(Candidate::new("no_report", turn, reach, NO_REPORT, going)));
            }
        }
        for question in &attempt.questions {
            let to = question.answered_at.or(reach).unwrap_or(now);
            lane.push(tag(Candidate::new("question", question.at, to, BLOCK, question.open() && open)
                .with("by", json!(question.by))
                .with("question", json!(question.id))));
        }
        if let (Some(from), Some(to)) = (begin, reach) {
            match native_blocks(team, &attempt.member, journal, from, to, now) {
                Some(blocks) => lane.extend(blocks.into_iter().map(&tag)),
                None => {
                    unknown.insert("permission");
                }
            }
        }
        for run in &attempt.runs {
            overlay.push(json!({"kind":"build","from":run.at - run.ms as f64 / 1000.0,"to":run.at,"s":seconds(run.ms as f64 / 1000.0),
                "template":run.template,"exit":run.exit,"attempt":attempt.n,"member":attempt.member}));
        }
        // U36: each stage of each round that checked this attempt.
        let mut stage_end: Option<f64> = None;
        for run in task.pipeline_run.iter().flat_map(|pipeline| pipeline.runs.iter()).filter(|run| run.attempt == attempt.n) {
            let going = run.ended_at.is_none() && run.state.open() && live;
            let close = run.ended_at.or(if going { Some(now) } else { None });
            let stage = |candidate: Candidate| {
                candidate
                    .with("stage", json!(run.stage))
                    .with("stage_kind", json!(run.kind))
                    .with("round", json!(run.round))
                    .with("state", json!(run.state))
                    .with("member", json!(run.member))
                    .with("attempt", json!(attempt.n))
            };
            match (run.delivered_at, close) {
                (Some(delivered), Some(close)) => {
                    lane.push(stage(Candidate::new("stage_wait", run.at, delivered, STAGE, false)));
                    lane.push(stage(Candidate::new("stage", delivered, close, STAGE, going)));
                }
                (None, Some(close)) => lane.push(stage(Candidate::new("stage_wait", run.at, close, STAGE, going))),
                (_, None) => {
                    unknown.insert("stage");
                }
            }
            for question in &run.questions {
                let to = question.answered_at.or(close).unwrap_or(now);
                lane.push(stage(Candidate::new("question", question.at, to, BLOCK, question.open() && going)
                    .with("by", json!(question.by))
                    .with("question", json!(question.id))));
            }
            for record in &run.runs {
                overlay.push(json!({"kind":"build","from":record.at - record.ms as f64 / 1000.0,"to":record.at,"s":seconds(record.ms as f64 / 1000.0),
                    "template":record.template,"exit":record.exit,"attempt":attempt.n,"stage":run.stage,"round":run.round,"member":run.member}));
            }
            stage_end = match (stage_end, close) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
        }
        // The lead's decision on the report; a stage covers its part of it.
        if let Some(report) = &attempt.report {
            let undecided = attempt.state == AttemptState::Reported && task.status.reported() && next.is_none();
            match attempt.decided_at {
                Some(decided) => lane.push(tag(Candidate::new("lead", report.at, decided, LEAD, false))),
                None if undecided && live => lane.push(tag(Candidate::new("lead", report.at, now, LEAD, true))),
                None => {}
            }
        }
        // From the decision, the revoked attempt's end or a stage's return to
        // the next hand-out: a wait for a member.
        let after = attempt
            .decided_at
            .or(if attempt.state == AttemptState::Revoked { end } else { None })
            .or(if attempt.pipeline_return { stage_end } else { None });
        if let Some(after) = after {
            match next {
                Some(next) => waits.push((after, next.sent_at, false, next.member.clone())),
                None if pending => waits.push((after, now, true, task.assignee.clone())),
                None => {}
            }
        }
        if let Some(stop) = stopped {
            match next {
                Some(next) => lane.push(tag(Candidate::new("stop", stop, next.sent_at, STOP, false))),
                None if pending || !live => lane.push(tag(Candidate::new("stop", stop, if live { now } else { reach.unwrap_or(stop) }, STOP, live))),
                None => {}
            }
        }
    }
    // A wait for a member while another task's stage held that member's folder.
    for (from, to, going, member) in waits {
        lane.push(Candidate::new("member", from, to, WAIT, going));
        let folder = team.member(&member).and_then(|found| super::dispatcher::member_folder_path(team, found).ok()).unwrap_or_default();
        for holder in holders.iter().filter(|holder| holder.task != task.id && super::stages::same_folder(&holder.folder, &folder)) {
            let (low, high) = (from.max(holder.from), to.min(holder.to));
            if high > low {
                lane.push(Candidate::new("folder_held", low, high, FOLDER, going && high >= now).with("held_by", json!(holder.task)));
            }
        }
    }
    lane.extend(holds(task, journal, live, now));
    lane.extend(phase_waits(team, task, journal, live, now));
    lane.retain(|candidate| candidate.to > candidate.from);
    let first_moment = lane.iter().map(|candidate| candidate.from).reduce(f64::min);
    let start = match opened {
        Moment::At(at) => Some(first_moment.map_or(at, |first| first.min(at))),
        _ => first_moment,
    };
    let mut spans = Vec::new();
    if start.is_none() && opened == Moment::Unknown && live {
        // Nothing recorded since the phase opened at a moment nobody knows.
        spans.push(json!({"kind":"unknown","from":null,"to":null,"s":null}));
    }
    if let Some(start) = start {
        // A finished task ends at the last moment its attempts recorded.
        let recorded = task.attempts.iter().flat_map(|attempt| [attempt.report.as_ref().map(|report| report.at), attempt.decided_at, attempt.turn_ended_at]);
        let end = if live { now } else { lane.iter().map(|candidate| candidate.to).chain(recorded.flatten()).fold(start, f64::max) };
        // The waits before the first hand-out when the phase's opening was
        // never recorded: they end at `start`, from a moment nobody knows.
        if opened == Moment::Unknown && !task.attempts.is_empty() {
            spans.push(json!({"kind":"unknown","from":null,"to":start,"s":null}));
        }
        spans.extend(paint(&lane, start, end, now));
    }
    let mut totals: BTreeMap<String, f64> = BTreeMap::new();
    for span in &spans {
        if let (Some(kind), Some(from)) = (span["kind"].as_str(), span["from"].as_f64()) {
            *totals.entry(kind.to_owned()).or_default() += span["to"].as_f64().unwrap_or(now) - from;
        }
    }
    let totals: BTreeMap<String, i64> = totals.into_iter().map(|(kind, value)| (kind, seconds(value))).collect();
    let mut value = json!({"task":task.id,"status":task.status,"spans":spans,"overlay":overlay,"totals":totals});
    if !unknown.is_empty() {
        value["unknown"] = json!(unknown);
    }
    value
}

#[cfg(test)]
#[path = "spans_tests.rs"]
mod tests;
