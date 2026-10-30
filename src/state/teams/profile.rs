//! WhiteWh fork: U38's profile of a task, a phase or the plan, computed on
//! request from the tasks' timelines (spans.rs) and the tokens their members'
//! sessions spent inside them (usage.rs). It ranks the losses by time and by
//! tokens, sets the planned critical path against the real one and gives each
//! member, role and model its utilization and tokens per accepted task. Every
//! list is a bounded page in a stable order: a cursor names the last item, so
//! a record that arrives between two reads never repeats or skips one.
use super::model::{AttemptState, Task, TaskStatus, Team};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// A page of losses, at most `LOSS_PAGE_MAX`.
pub(crate) const LOSS_PAGE: usize = 20;
pub(crate) const LOSS_PAGE_MAX: usize = 100;
/// A page of the per-task rows.
pub(crate) const TASK_PAGE: usize = 50;
/// The most token reads one profile makes; the rest is unknown.
pub(crate) const TOKEN_READS: usize = 200;
/// A test stage or a test build at least this long is a loss.
pub(crate) const LONG_TEST_S: i64 = 300;

/// The span kinds in which a member works, for its utilization.
const BUSY: &[&str] = &["work", "rework", "stage", "question", "permission", "background"];
/// The span kinds of an attempt whose session spends tokens.
const SPENDING: &[&str] = &["work", "rework", "question", "permission", "background"];

/// What one part of a task (an attempt, or one stage of one round) spent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Tokens {
    pub total: u64,
    pub cost_usd: f64,
}

/// The tokens of each (task, part); None is unknown, never zero.
pub(crate) type TokenMap = BTreeMap<(u32, String), Option<Tokens>>;

/// One window whose tokens the caller reads from the member's session.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ask {
    pub task: u32,
    pub part: String,
    pub member: String,
    pub from: f64,
    pub to: f64,
}

/// What the caller asks for: the page sizes and the cursors of each list.
#[derive(Clone, Debug, Default)]
pub(crate) struct Page {
    pub limit: usize,
    pub after_time: Option<Value>,
    pub after_tokens: Option<Value>,
    pub after_task: u32,
}

fn seconds(value: f64) -> i64 {
    value.max(0.0).round() as i64
}

fn to_of(span: &Value, now: f64) -> f64 {
    span["to"].as_f64().unwrap_or(now)
}

/// The part a span belongs to: "a{n}" for an attempt, "s{stage}r{round}a{n}"
/// for a stage round; None for a wait no session spends in.
fn part_of(span: &Value) -> Option<String> {
    let attempt = span["attempt"].as_u64()?;
    match span["stage"].as_u64() {
        Some(stage) => Some(format!("s{stage}r{}a{attempt}", span["round"].as_u64().unwrap_or(0))),
        None => Some(format!("a{attempt}")),
    }
}

/// The token windows of the timelines: each attempt from its first to its last
/// spending span, each stage round over its stage span, with the member whose
/// session spent them. In task and part order.
pub(crate) fn asks(timelines: &[Value], now: f64) -> Vec<Ask> {
    let mut windows: BTreeMap<(u32, String), (String, f64, f64)> = BTreeMap::new();
    for timeline in timelines {
        let Some(task) = timeline["task"].as_u64() else { continue };
        for span in timeline["spans"].as_array().into_iter().flatten() {
            let kind = span["kind"].as_str().unwrap_or_default();
            let staged = span.get("stage").is_some();
            if !(if staged { kind == "stage" } else { SPENDING.contains(&kind) }) {
                continue;
            }
            let (Some(part), Some(from), Some(member)) = (part_of(span), span["from"].as_f64(), span["member"].as_str()) else { continue };
            if member.is_empty() {
                continue;
            }
            let to = to_of(span, now);
            let entry = windows.entry((task as u32, part)).or_insert_with(|| (member.to_owned(), from, to));
            entry.1 = entry.1.min(from);
            entry.2 = entry.2.max(to);
        }
    }
    windows.into_iter().map(|((task, part), (member, from, to))| Ask { task, part, member, from, to }).collect()
}

/// The sum of the known tokens of `parts`, and how many were unknown.
fn sum(tokens: &TokenMap, task: u32, parts: impl Iterator<Item = String>) -> (Option<Tokens>, usize) {
    let mut total = Tokens::default();
    let (mut known, mut unknown) = (0, 0);
    for part in parts {
        match tokens.get(&(task, part)).copied().flatten() {
            Some(found) => {
                total.total += found.total;
                total.cost_usd += found.cost_usd;
                known += 1;
            }
            None => unknown += 1,
        }
    }
    ((known > 0).then_some(total), unknown)
}

fn tokens_value(tokens: Option<Tokens>, unknown: usize) -> Value {
    match tokens {
        Some(found) => json!({"total":found.total,"cost_usd":(found.cost_usd * 10000.0).round() / 10000.0,"partial":unknown > 0}),
        None if unknown > 0 => json!({"unknown":true}),
        None => Value::Null,
    }
}

/// Merges intervals into a sorted, disjoint list.
fn union(mut list: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    list.retain(|(from, to)| to > from);
    list.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (from, to) in list {
        match out.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => out.push((from, to)),
        }
    }
    out
}

fn length(list: &[(f64, f64)]) -> f64 {
    list.iter().map(|(from, to)| to - from).sum()
}

/// The part of `open` that `busy` (both disjoint and sorted) does not cover.
fn minus(open: &[(f64, f64)], busy: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for &(from, to) in open {
        let mut at = from;
        for &(low, high) in busy.iter().filter(|(low, high)| *high > from && *low < to) {
            if low > at {
                out.push((at, low.min(to)));
            }
            at = at.max(high);
        }
        if at < to {
            out.push((at, to));
        }
    }
    out
}

/// The moments a timeline covers, the null-start unknown left out.
fn extent(timeline: &Value, now: f64) -> Option<(f64, f64)> {
    let spans = timeline["spans"].as_array()?;
    let from = spans.iter().filter_map(|span| span["from"].as_f64()).reduce(f64::min)?;
    let to = spans.iter().filter(|span| !span["from"].is_null()).map(|span| to_of(span, now)).fold(from, f64::max);
    Some((from, to))
}

/// The member credited with an accepted task: its last reported attempt's.
fn accepted_by(task: &Task) -> Option<&str> {
    if task.status != TaskStatus::Accepted {
        return None;
    }
    task.attempts.iter().rev().find(|attempt| attempt.state == AttemptState::Reported).map(|attempt| attempt.member.as_str())
}

/// The model a member ran on: its own, or its agent's default.
fn model_of(team: &Team, member: &str) -> String {
    match team.member(member) {
        Some(found) if !found.model.is_empty() => found.model.clone(),
        Some(found) => format!("{} default", found.agent),
        None => "unknown".into(),
    }
}

/// A role's or a model's sums over its members.
#[derive(Default)]
struct Group {
    busy: f64,
    idle: f64,
    accepted: usize,
    tokens: u64,
    unknown: usize,
}

/// One ranked loss, its stable id first.
struct Loss {
    id: String,
    s: i64,
    tokens: Option<u64>,
    value: Value,
}

fn loss(kind: &str, key: &str, s: i64, tokens: Option<Tokens>, mut value: Value) -> Loss {
    let id = format!("{kind}:{key}");
    value["id"] = json!(id);
    value["kind"] = json!(kind);
    value["s"] = json!(s);
    if let Some(found) = tokens {
        value["tokens"] = json!(found.total);
        value["cost_usd"] = json!((found.cost_usd * 10000.0).round() / 10000.0);
    }
    Loss { id, s, tokens: tokens.map(|found| found.total), value }
}

/// The losses one task's timeline shows: rework, waits on the lead and on
/// permissions, a blocked chain, a wait for a free member, long tests, merges
/// and conflicts.
fn task_losses(task: &Task, timeline: &Value, tokens: &TokenMap, now: f64, out: &mut Vec<Loss>) {
    let id = task.id;
    let spans: Vec<&Value> = timeline["spans"].as_array().into_iter().flatten().filter(|span| !span["from"].is_null()).collect();
    let total = |kinds: &[&str]| -> i64 {
        spans.iter().filter(|span| kinds.contains(&span["kind"].as_str().unwrap_or_default())).map(|span| span["s"].as_i64().unwrap_or(0)).sum()
    };
    let all_parts: BTreeSet<String> = spans.iter().filter(|span| span["member"].is_string()).filter_map(|span| part_of(span)).collect();
    let (task_tokens, _) = sum(tokens, id, all_parts.iter().cloned());
    // Rework: each rejected or returned attempt, with its share of the task's tokens.
    let mut rework: BTreeMap<u64, (i64, f64, f64, String)> = BTreeMap::new();
    for span in spans.iter().filter(|span| span["kind"] == "rework") {
        let n = span["attempt"].as_u64().unwrap_or(0);
        let entry = rework.entry(n).or_insert((0, f64::MAX, 0.0, span["member"].as_str().unwrap_or_default().to_owned()));
        entry.0 += span["s"].as_i64().unwrap_or(0);
        entry.1 = entry.1.min(span["from"].as_f64().unwrap_or(0.0));
        entry.2 = entry.2.max(to_of(span, now));
    }
    for (n, (s, from, to, member)) in rework {
        let (spent, unknown) = sum(tokens, id, std::iter::once(format!("a{n}")));
        let returned = task.attempts.iter().any(|attempt| u64::from(attempt.n) == n && attempt.pipeline_return);
        let mut value = json!({"task":id,"attempt":n,"member":member,"from":from,"to":to,"cause":if returned {"returned"} else {"rejected"}});
        if let (Some(spent), Some(whole)) = (spent, task_tokens.filter(|whole| whole.total > 0)) {
            value["share"] = json!(((spent.total as f64 / whole.total as f64) * 1000.0).round() / 1000.0);
        }
        if unknown > 0 {
            value["tokens_unknown"] = json!(true);
        }
        out.push(loss("rework", &format!("{id}:a{n}"), s, spent, value));
    }
    // The lead's decisions and answers.
    let (lead, questions) = (total(&["lead"]), total(&["question"]));
    if lead + questions > 0 {
        out.push(loss("lead_wait", &id.to_string(), lead + questions, None, json!({"task":id,"lead_s":lead,"question_s":questions})));
    }
    let permission = total(&["permission"]);
    if permission > 0 {
        let by: BTreeSet<&str> = spans.iter().filter(|span| span["kind"] == "permission").filter_map(|span| span["by"].as_str()).collect();
        out.push(loss("permission_wait", &id.to_string(), permission, None, json!({"task":id,"answered_by":by})));
    }
    // A chain: the dependencies the task waited for, or a folder another task's
    // stage held.
    let (deps, held) = (total(&["deps"]), total(&["folder_held"]));
    if deps + held > 0 {
        let on: Vec<u32> = task.deps.iter().map(|dep| dep.task).collect();
        let by: BTreeSet<u64> = spans.iter().filter(|span| span["kind"] == "folder_held").filter_map(|span| span["held_by"].as_u64()).collect();
        out.push(loss("blocked_chain", &id.to_string(), deps + held, None, json!({"task":id,"deps_s":deps,"folder_held_s":held,"on":on,"held_by":by})));
    }
    let member = total(&["member"]);
    if member > 0 {
        out.push(loss("member_wait", &id.to_string(), member, None, json!({"task":id,"member":task.assignee})));
    }
    // Long test stages and test builds; merges, and the ones that conflicted.
    for span in spans.iter().filter(|span| span["kind"] == "stage") {
        let (kind, s) = (span["stage_kind"].as_str().unwrap_or_default(), span["s"].as_i64().unwrap_or(0));
        let part = part_of(span).unwrap_or_default();
        let detail = json!({"task":id,"member":span["member"],"stage":span["stage"],"round":span["round"],"from":span["from"],"to":span["to"]});
        if kind == "test" && s >= LONG_TEST_S {
            out.push(loss("long_test", &format!("{id}:{part}"), s, sum(tokens, id, std::iter::once(part.clone())).0, detail));
        } else if kind == "land" {
            // A land stage that returned the task did not merge: a conflict.
            let conflicted = span["state"] == "returned";
            out.push(loss(if conflicted { "conflict" } else { "merge" }, &format!("{id}:{part}"), s, None, detail));
        }
    }
    for (index, build) in timeline["overlay"].as_array().into_iter().flatten().enumerate() {
        let template = build["template"].as_str().unwrap_or_default();
        let s = build["s"].as_i64().unwrap_or(0);
        let detail = json!({"task":id,"member":build["member"],"attempt":build["attempt"],"template":template,"exit":build["exit"],
            "from":build["from"],"to":build["to"]});
        let key = format!("{id}:b{index}:{}", build["to"].as_f64().map_or(0, |to| to.round() as i64));
        if template == "git-merge-lead" {
            let conflicted = build["exit"].as_i64() != Some(0);
            out.push(loss(if conflicted { "conflict" } else { "merge" }, &key, s, None, detail));
        } else if template.contains("test") && s >= LONG_TEST_S {
            out.push(loss("long_test", &key, s, None, detail));
        }
    }
}

/// A page of `items` in their order after `cursor` ({value, id}).
fn page_of(items: &[&Loss], value: impl Fn(&Loss) -> i64, field: &str, cursor: Option<&Value>, limit: usize) -> Value {
    let after = cursor.and_then(|cursor| Some((cursor[field].as_i64()?, cursor["id"].as_str()?.to_owned())));
    let rest: Vec<&&Loss> = items
        .iter()
        .filter(|item| after.as_ref().is_none_or(|(at, id)| value(item) < *at || (value(item) == *at && item.id.as_str() > id.as_str())))
        .collect();
    let shown: Vec<Value> = rest.iter().take(limit).map(|item| item.value.clone()).collect();
    let next = (rest.len() > limit).then(|| rest[limit - 1]).map(|item| json!({field:value(item),"id":item.id}));
    json!({"items":shown,"next_cursor":next,"total":items.len()})
}

/// The longest planned dependency chain among the scope's tasks, by task
/// count; ties go to the lower ids.
fn planned_path(tasks: &[&Task]) -> Vec<u32> {
    let ids: BTreeSet<u32> = tasks.iter().map(|task| task.id).collect();
    let by_id: BTreeMap<u32, &Task> = tasks.iter().map(|task| (task.id, *task)).collect();
    let mut memo: BTreeMap<u32, usize> = BTreeMap::new();
    // In id order a task's deps usually come first; a cycle counts once.
    fn depth(id: u32, by_id: &BTreeMap<u32, &Task>, ids: &BTreeSet<u32>, memo: &mut BTreeMap<u32, usize>, seen: &mut BTreeSet<u32>) -> usize {
        if let Some(found) = memo.get(&id) {
            return *found;
        }
        if !seen.insert(id) {
            return 0;
        }
        let deps = by_id.get(&id).map(|task| task.deps.iter().map(|dep| dep.task).filter(|dep| ids.contains(dep)).collect::<Vec<_>>()).unwrap_or_default();
        let found = 1 + deps.into_iter().map(|dep| depth(dep, by_id, ids, memo, seen)).max().unwrap_or(0);
        memo.insert(id, found);
        found
    }
    for id in &ids {
        depth(*id, &by_id, &ids, &mut memo, &mut BTreeSet::new());
    }
    let Some(mut at) = ids.iter().copied().max_by_key(|id| (memo[id], std::cmp::Reverse(*id))) else { return Vec::new() };
    let mut path = vec![at];
    while let Some(next) = by_id[&at]
        .deps
        .iter()
        .map(|dep| dep.task)
        .filter(|dep| ids.contains(dep) && !path.contains(dep))
        .max_by_key(|dep| (memo.get(dep).copied().unwrap_or(0), std::cmp::Reverse(*dep)))
    {
        path.push(next);
        at = next;
    }
    path.reverse();
    path
}

/// The chain that really set the end: from the task that ended last, back
/// through the dependency that ended last each time.
fn real_path(tasks: &[&Task], extents: &BTreeMap<u32, (f64, f64)>) -> (Vec<u32>, i64) {
    let end_of = |id: &u32| extents.get(id).map(|extent| extent.1);
    let Some(mut at) = tasks.iter().map(|task| task.id).filter(|id| extents.contains_key(id)).max_by(|a, b| {
        end_of(a).unwrap_or(0.0).total_cmp(&end_of(b).unwrap_or(0.0)).then(b.cmp(a))
    }) else {
        return (Vec::new(), 0);
    };
    let by_id: BTreeMap<u32, &Task> = tasks.iter().map(|task| (task.id, *task)).collect();
    let mut path = vec![at];
    while let Some(next) = by_id[&at]
        .deps
        .iter()
        .map(|dep| dep.task)
        .filter(|dep| extents.contains_key(dep) && !path.contains(dep))
        .max_by(|a, b| end_of(a).unwrap_or(0.0).total_cmp(&end_of(b).unwrap_or(0.0)).then(b.cmp(a)))
    {
        path.push(next);
        at = next;
    }
    path.reverse();
    let s = match (path.first().and_then(|id| extents.get(id)), path.last().and_then(|id| extents.get(id))) {
        (Some(first), Some(last)) => seconds(last.1 - first.0),
        _ => 0,
    };
    (path, s)
}

/// The profile of `tasks` (the scope, in id order) from their `timelines` in
/// the same order and the tokens the caller read for `asks`.
pub(crate) fn profile(team: &Team, tasks: &[&Task], timelines: &[Value], tokens: &TokenMap, page: &Page, now: f64) -> Value {
    let limit = page.limit.clamp(1, LOSS_PAGE_MAX);
    let mut losses: Vec<Loss> = Vec::new();
    let mut extents: BTreeMap<u32, (f64, f64)> = BTreeMap::new();
    let mut totals: BTreeMap<String, i64> = BTreeMap::new();
    let mut rows: Vec<Value> = Vec::new();
    // Each member's busy moments and the parts its session spent in.
    let mut busy: BTreeMap<String, Vec<(f64, f64)>> = BTreeMap::new();
    let mut parts: BTreeMap<String, Vec<(u32, String)>> = BTreeMap::new();
    for (task, timeline) in tasks.iter().zip(timelines) {
        task_losses(task, timeline, tokens, now, &mut losses);
        if let Some(extent) = extent(timeline, now) {
            extents.insert(task.id, extent);
        }
        for (kind, s) in timeline["totals"].as_object().into_iter().flatten() {
            *totals.entry(kind.clone()).or_default() += s.as_i64().unwrap_or(0);
        }
        let mut own: BTreeSet<String> = BTreeSet::new();
        for span in timeline["spans"].as_array().into_iter().flatten() {
            let (Some(member), Some(from)) = (span["member"].as_str(), span["from"].as_f64()) else { continue };
            if member.is_empty() {
                continue;
            }
            if BUSY.contains(&span["kind"].as_str().unwrap_or_default()) {
                busy.entry(member.to_owned()).or_default().push((from, to_of(span, now)));
            }
            if let Some(part) = part_of(span) {
                if own.insert(part.clone()) {
                    parts.entry(member.to_owned()).or_default().push((task.id, part));
                }
            }
        }
        let (spent, unknown) = sum(tokens, task.id, own.iter().cloned());
        let rework_parts = timeline["spans"].as_array().into_iter().flatten().filter(|span| span["kind"] == "rework").filter_map(part_of).collect::<BTreeSet<_>>();
        let (rework, _) = sum(tokens, task.id, rework_parts.into_iter());
        let elapsed = extents.get(&task.id).map_or(0, |(from, to)| seconds(to - from));
        let kind_s = |kinds: &[&str]| kinds.iter().map(|kind| timeline["totals"][*kind].as_i64().unwrap_or(0)).sum::<i64>();
        let mut row = json!({"task":task.id,"title":task.title,"status":task.status,"phase":task.phase,"elapsed_s":elapsed,
            "worked_s":kind_s(&["work","rework","stage"]),"rework_s":kind_s(&["rework"]),"tokens":tokens_value(spent, unknown)});
        if let (Some(rework), Some(spent)) = (rework, spent.filter(|spent| spent.total > 0)) {
            row["rework_tokens"] = json!(rework.total);
            row["rework_share"] = json!(((rework.total as f64 / spent.total as f64) * 1000.0).round() / 1000.0);
        }
        rows.push(row);
    }
    let window = extents.values().fold(None, |acc: Option<(f64, f64)>, (from, to)| Some(acc.map_or((*from, *to), |(low, high)| (low.min(*from), high.max(*to)))));
    // The moments any task of the scope was open: a member without work then
    // was idle.
    let open = union(extents.values().copied().collect());
    let mut members: Vec<Value> = Vec::new();
    let mut groups: BTreeMap<(&str, String), Group> = BTreeMap::new();
    let assignees: BTreeSet<&str> = tasks.iter().map(|task| task.assignee.as_str()).collect();
    for member in team.members.iter().filter(|member| member.id != team.lead) {
        let mine = union(busy.remove(&member.id).unwrap_or_default());
        let listed = parts.remove(&member.id).unwrap_or_default();
        if mine.is_empty() && listed.is_empty() && !assignees.contains(member.id.as_str()) {
            continue;
        }
        // Before it joined a member was not idle.
        let joined = member.added.as_ref().and_then(|added| added["at"].as_f64()).unwrap_or(f64::MIN);
        let available: Vec<(f64, f64)> = open.iter().filter(|(_, to)| *to > joined).map(|(from, to)| (from.max(joined), *to)).collect();
        let idle = minus(&available, &mine);
        let (busy_s, idle_s) = (length(&mine), length(&idle));
        if idle_s >= 1.0 {
            let detail = json!({"member":member.id,"role":member.role,"open_s":seconds(length(&available)),
                "from":idle.first().map(|span| span.0),"to":idle.last().map(|span| span.1)});
            losses.push(loss("idle_member", &member.id, seconds(idle_s), None, detail));
        }
        let accepted = tasks.iter().filter(|task| accepted_by(task) == Some(member.id.as_str())).count();
        let mut spent = Tokens::default();
        let (mut known, mut unknown) = (0usize, 0usize);
        for (task, part) in &listed {
            match tokens.get(&(*task, part.clone())).copied().flatten() {
                Some(found) => {
                    spent.total += found.total;
                    spent.cost_usd += found.cost_usd;
                    known += 1;
                }
                None => unknown += 1,
            }
        }
        let spent = (known > 0).then_some(spent);
        let model = model_of(team, &member.id);
        let mut value = json!({"member":member.id,"role":member.role,"model":model,"busy_s":seconds(busy_s),"idle_s":seconds(idle_s),
            "utilization":if busy_s + idle_s > 0.0 { ((busy_s / (busy_s + idle_s)) * 1000.0).round() / 1000.0 } else { 0.0 },
            "accepted":accepted,"tokens":tokens_value(spent, unknown)});
        if let (Some(spent), true) = (spent, accepted > 0) {
            value["tokens_per_accepted"] = json!(spent.total / accepted as u64);
        }
        members.push(value);
        for key in [("role", member.role.clone()), ("model", model)] {
            let entry = groups.entry(key).or_default();
            entry.busy += busy_s;
            entry.idle += idle_s;
            entry.accepted += accepted;
            entry.tokens += spent.map_or(0, |spent| spent.total);
            entry.unknown += unknown;
        }
    }
    let group = |by: &str| -> Vec<Value> {
        groups
            .iter()
            .filter(|((kind, _), _)| *kind == by)
            .map(|((_, name), sums)| {
                let (busy, idle) = (sums.busy, sums.idle);
                let mut value = json!({by:name,"busy_s":seconds(busy),"idle_s":seconds(idle),
                    "utilization":if busy + idle > 0.0 { ((busy / (busy + idle)) * 1000.0).round() / 1000.0 } else { 0.0 },
                    "accepted":sums.accepted,"tokens":sums.tokens,"tokens_unknown":sums.unknown});
                if sums.accepted > 0 && sums.tokens > 0 {
                    value["tokens_per_accepted"] = json!(sums.tokens / sums.accepted as u64);
                }
                value
            })
            .collect()
    };
    // Stable orders: the most seconds (or tokens) first, then the id.
    let mut by_time: Vec<&Loss> = losses.iter().collect();
    by_time.sort_by(|a, b| b.s.cmp(&a.s).then_with(|| a.id.cmp(&b.id)));
    let mut by_tokens: Vec<&Loss> = losses.iter().filter(|loss| loss.tokens.is_some_and(|tokens| tokens > 0)).collect();
    by_tokens.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.id.cmp(&b.id)));
    let planned = planned_path(tasks);
    let (real, real_s) = real_path(tasks, &extents);
    let off_plan: Vec<u32> = real.iter().copied().filter(|id| !planned.contains(id)).collect();
    let (spent, unknown) = {
        let mut whole = Tokens::default();
        let (mut known, mut unknown) = (0, 0);
        for found in tokens.iter().filter(|((task, _), _)| tasks.iter().any(|scope| scope.id == *task)).map(|(_, found)| found) {
            match found {
                Some(found) => {
                    whole.total += found.total;
                    whole.cost_usd += found.cost_usd;
                    known += 1;
                }
                None => unknown += 1,
            }
        }
        ((known > 0).then_some(whole), unknown)
    };
    let task_rows: Vec<&Value> = rows.iter().filter(|row| row["task"].as_u64().unwrap_or(0) > u64::from(page.after_task)).collect();
    let task_limit = page.limit.clamp(1, TASK_PAGE);
    let worked: i64 = ["work", "rework", "stage"].iter().map(|kind| totals.get(*kind).copied().unwrap_or(0)).sum();
    let task_next = (task_rows.len() > task_limit).then(|| task_rows[task_limit - 1]["task"].clone());
    json!({
        "summary":{
            "tasks":tasks.len(),
            "accepted":tasks.iter().filter(|task| task.status == TaskStatus::Accepted).count(),
            "from":window.map(|window| window.0),
            "to":window.map(|window| window.1),
            "elapsed_s":window.map_or(0, |(from, to)| seconds(to - from)),
            "worked_s":worked,
            "totals":totals,
            "tokens":tokens_value(spent, unknown),
        },
        "losses":{
            "by_time":page_of(&by_time, |loss| loss.s, "s", page.after_time.as_ref(), limit),
            "by_tokens":page_of(&by_tokens, |loss| loss.tokens.unwrap_or(0) as i64, "tokens", page.after_tokens.as_ref(), limit),
        },
        "critical_path":{
            "planned":{"tasks":planned,"length":planned.len()},
            "real":{"tasks":real,"s":real_s},
            "off_plan":off_plan,
            "matches":off_plan.is_empty() && real.len() == planned.len(),
        },
        "members":members,
        "roles":group("role"),
        "models":group("model"),
        "tasks":{"items":task_rows.iter().take(task_limit).collect::<Vec<_>>(),"next_cursor":task_next},
    })
}

/// U38: two phases side by side: each one's summary and top three losses by
/// time, and what changed from the first to the second.
pub(crate) fn compare(a: (u32, &Value), b: (u32, &Value)) -> Value {
    let side = |(n, profile): (u32, &Value)| {
        let top: Vec<Value> = profile["losses"]["by_time"]["items"].as_array().into_iter().flatten().take(3).cloned().collect();
        json!({"phase":n,"summary":profile["summary"],"top_losses":top})
    };
    let delta = |key: &str| b.1["summary"][key].as_i64().unwrap_or(0) - a.1["summary"][key].as_i64().unwrap_or(0);
    let mut kinds: BTreeMap<String, i64> = BTreeMap::new();
    for (sign, profile) in [(-1, a.1), (1, b.1)] {
        for (kind, s) in profile["summary"]["totals"].as_object().into_iter().flatten() {
            *kinds.entry(kind.clone()).or_default() += sign * s.as_i64().unwrap_or(0);
        }
    }
    let tokens = |profile: &Value| profile["summary"]["tokens"]["total"].as_u64();
    let token_delta = match (tokens(a.1), tokens(b.1)) {
        (Some(first), Some(second)) => json!(second as i64 - first as i64),
        _ => Value::Null,
    };
    json!({"a":side(a),"b":side(b),"delta":{"elapsed_s":delta("elapsed_s"),"worked_s":delta("worked_s"),"tasks":delta("tasks"),
        "accepted":delta("accepted"),"tokens":token_delta,"totals":kinds}})
}

/// Reads the token windows from the members' sessions on this computer, at
/// most `TOKEN_READS`; a member elsewhere, a missing record or a read past the
/// bound is unknown. The second value says the bound cut the reads.
pub(crate) fn read_tokens(team: &Team, asks: &[Ask]) -> (TokenMap, bool) {
    let mut out = TokenMap::new();
    let mut records: BTreeMap<String, Option<Value>> = BTreeMap::new();
    for (index, ask) in asks.iter().enumerate() {
        let key = (ask.task, ask.part.clone());
        if index >= TOKEN_READS {
            out.insert(key, None);
            continue;
        }
        let session = team.member(&ask.member).and_then(|member| member.session.as_ref()).filter(|session| session.host.is_empty());
        let record = session.and_then(|session| {
            records.entry(session.name.clone()).or_insert_with(|| super::super::storage::read(&session.name).ok()).clone()
        });
        out.insert(key, record.and_then(|record| known(&super::super::usage::span(&record, ask.from, ask.to))));
    }
    (out, asks.len() > TOKEN_READS)
}

/// The total tokens and cost of a known `usage::span` answer.
fn known(span: &Value) -> Option<Tokens> {
    if span["status"] != "ok" {
        return None;
    }
    let mut found = Tokens::default();
    for provider in span["providers"].as_array().into_iter().flatten() {
        found.cost_usd += provider["cost_usd"].as_f64().unwrap_or(0.0);
        for model in provider["models"].as_array().into_iter().flatten() {
            found.total += model["tokens"]["total"].as_u64().unwrap_or(0);
        }
    }
    Some(found)
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
