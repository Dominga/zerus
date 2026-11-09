//! Task #289 (owner, 2026-11-08): the lead's inbox separates decisions from
//! notices. A notice (a decided proposal, a cancelled dependency, an unread
//! count, a digest) can be marked seen and returns only on change; the idle
//! nag names and counts decisions only; a cancelled dependency the task does
//! not need is kept and clears.
use super::*;
use super::support::*;

fn lead() -> Actor {
    Actor::Member("lead".into())
}

fn run(team: &mut Team, op: &str, input: Value, actor: &Actor) -> anyhow::Result<Value> {
    ops::apply(team, op, &input, actor, &BTreeSet::new(), &mut Vec::new())
}

fn item(team: &Team, kind: &str) -> Option<crate::state::teams::inbox::Item> {
    crate::state::teams::inbox::items(team).into_iter().find(|item| item.kind == kind)
}

/// The team of the screenshot: dev's report (a decision), a pending task whose
/// dependencies #204, #230 and #237 were cancelled, the owner's decision on
/// proposal p77, and a member that does not read its messages.
fn screenshot_team() -> Team {
    let mut team = running_team();
    {
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: "00000000-0000-4000-8000-000000000002".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            sent_at: 20.0,
            delivered_at: Some(21.0),
            report: Some(Report { text: "done".into(), at: 30.0, ..Default::default() }),
            ..Default::default()
        });
        task.status = TaskStatus::Submitted;
    }
    for (id, title) in [(204u32, "Old"), (230, "Older"), (237, "Oldest")] {
        team.tasks.push(Task {
            id,
            title: title.into(),
            status: TaskStatus::Cancelled,
            assignee: "dev".into(),
            attempts: vec![Attempt { n: 1, id: format!("A{id}"), member: "dev".into(), state: AttemptState::Reported, decided_at: Some(100.0), ..Default::default() }],
            ..Default::default()
        });
    }
    let next = Task { id: 300, title: "Later work".into(), status: TaskStatus::Pending, assignee: "dev".into(), ..Default::default() };
    team.tasks.push(next);
    let later = team.task_mut(300).unwrap();
    later.deps = [204u32, 230, 237].into_iter().map(|task| crate::state::teams::model::Dependency { task, kind: crate::state::teams::model::DependencyKind::Hard }).collect();
    team.proposals.push(json!({"id":"p77","kind":"scratch-rule","state":"accepted","decided_at":200.0,"note":"allowed"}));
    let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    member.unread = Some(crate::state::teams::model::Unread {
        count: 3,
        urgent: 0,
        oldest: "w-message-1".into(),
        oldest_at: crate::state::now() - 4000.0,
        rings: 2,
        rung_at: Some(crate::state::now() - 4000.0),
        lead_told: true,
        ..Default::default()
    });
    team
}

#[test]
fn the_screenshot_items_are_notices_and_the_nag_names_decisions_only() {
    let team = screenshot_team();
    for kind in ["dependency_cancelled", "proposal_decided", "unread_messages"] {
        let found = item(&team, kind).unwrap_or_else(|| panic!("no {kind} item"));
        assert!(crate::state::teams::inbox::notice(kind), "{kind} is a notice");
        let value = found.value();
        assert_eq!(value["notice"], json!(true), "{value}");
        assert!(value["seen"].as_str().unwrap_or_default().contains("hgs team seen --json"), "{value}");
    }
    // The cancelled dependencies are one item naming all three tasks.
    let cancelled = item(&team, "dependency_cancelled").unwrap();
    assert_eq!(cancelled.task, Some(300));
    assert!(cancelled.summary.contains("#204") && cancelled.summary.contains("#230") && cancelled.summary.contains("#237"), "{}", cancelled.summary);
    assert!(cancelled.command.contains("hgs team keep --json"), "{}", cancelled.command);
    assert!(cancelled.detail.contains("hgs team hold --json") && cancelled.detail.contains("hgs team cancel --json"), "{}", cancelled.detail);
    // The report is a decision.
    assert_eq!(item(&team, "report").unwrap().value()["notice"], json!(false));
    // The idle nag names the report only and counts the one decision.
    let stall = crate::state::teams::model::LeadStall { since: 1.0, reason: "idle".into(), items: 1, members: 1, oldest: 30.0, round: 1, notified_at: 0.0, ..Default::default() };
    let text = crate::state::teams::look::lead_idle_text(&team, &stall, crate::state::now());
    assert!(text.contains("1 decision(s) wait"), "{text}");
    assert!(text.contains("done"), "the report is named: {text}");
    for notice in ["p77", "no longer waits on cancelled", "does not read"] {
        assert!(!text.contains(notice), "a notice was named: {text}");
    }
}

#[test]
fn a_seen_notice_stays_away_across_a_restart_and_returns_when_it_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let team = screenshot_team();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    let keys: Vec<String> = ["proposal:p77", "dependency_cancelled:300:#204,#230,#237"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let seen = store
        .mutate(None, &team.id, None, |stored, events| run(stored, "seen", json!({"items":keys}), &lead()))
        .unwrap();
    assert_eq!(seen["status"], "seen", "{seen}");
    assert_eq!(seen["seen"], json!(keys));
    assert_eq!(seen["unknown"], json!([]));
    // Across a restart (a fresh read of the store) they stay away.
    let reloaded = store.load(&team.id).unwrap().unwrap();
    assert!(item(&reloaded, "proposal_decided").is_none(), "the decided proposal stays seen");
    assert!(item(&reloaded, "dependency_cancelled").is_none(), "the cancelled dependency stays seen");
    assert!(crate::state::teams::inbox::all_items(&reloaded).iter().any(|item| item.key == "proposal:p77"), "the notice is still known to hgs team seen");
    // A change returns it: the owner's decision on p77 is different now.
    store
        .mutate(None, &team.id, None, |stored, _| {
            for proposal in &mut stored.proposals {
                if proposal["id"] == "p77" {
                    proposal["state"] = json!("rejected");
                    proposal["note"] = json!("not this time");
                }
            }
            Ok(Value::Null)
        })
        .unwrap();
    let changed = store.load(&team.id).unwrap().unwrap();
    let returned = item(&changed, "proposal_decided").expect("the changed notice returns");
    assert!(returned.summary.contains("rejected") && returned.summary.contains("not this time"), "{}", returned.summary);
    // A decision is never marked seen.
    let error = store
        .mutate(None, &team.id, None, |stored, _| run(stored, "seen", json!({"items":["report"]}), &lead()))
        .unwrap();
    assert_eq!(error["unknown"], json!(["report"]), "a decision is not a notice: {error}");
}

#[test]
fn an_accepted_landing_is_no_decision_and_is_never_a_report() {
    // Task #443 (owner, 2026-11-09): a report the lead accepted whose work
    // waits for its landing is decided: the queue's landing, or a hand one.
    let mut team = screenshot_team();
    team.task_mut(1).unwrap().merge = Some(MergeEntry {
        state: "queued".into(),
        author: "dev".into(),
        commit: "46de9c4d5a6345395d221dcaa3eaf48aed0a1d71".into(),
        at: 25.0,
        ..Default::default()
    });
    assert!(item(&team, "report").is_none(), "a queued landing is no decision: {:?}", crate::state::teams::inbox::items(&team));
    // The idle nag names no decision and the report's text.
    let stall = LeadStall { since: 1.0, reason: "idle".into(), items: 0, members: 0, oldest: 25.0, round: 1, notified_at: 0.0, ..Default::default() };
    let text = crate::state::teams::look::lead_idle_text(&team, &stall, crate::state::now());
    assert!(!text.contains("done") && !text.contains("#1"), "the waiting landing was named: {text}");
    assert_eq!(
        crate::state::teams::merge_queue::waiting_to_land(&team, 25.0 + 26.0 * 60.0).expect("the waiting line")["line"],
        json!("accepted, waiting to land: #1 (26 min)")
    );
    // The hand landing (an accept outside the queue) is none either.
    let mut hand = screenshot_team();
    hand.task_mut(1).unwrap().status = TaskStatus::Accepted;
    assert!(item(&hand, "report").is_none());
    // Once landed it leaves the line too.
    hand.task_mut(1).unwrap().landed = Some(Landed { commit: "46de9c4".into(), by: "Zerus".into(), at: 30.0 });
    assert!(crate::state::teams::merge_queue::waiting_to_land(&hand, 100.0).is_none());
    // A failed or conflicted landing stays the lead's decision.
    let mut failed = screenshot_team();
    failed.task_mut(1).unwrap().merge = Some(MergeEntry {
        state: "queued".into(),
        note: "the merge could not be rolled back".into(),
        commit: "46de9c4d5a6345395d221dcaa3eaf48aed0a1d71".into(),
        ..Default::default()
    });
    assert!(item(&failed, "report").is_some(), "a failed landing is decided by the lead");
}

#[test]
fn the_lead_keeps_a_task_after_its_dependency_was_cancelled() {
    let mut team = screenshot_team();
    assert!(item(&team, "dependency_cancelled").is_some());
    let kept = run(&mut team, "keep", json!({"task":300}), &lead()).unwrap();
    assert_eq!((kept["status"].as_str(), kept["deps"].clone()), (Some("kept"), json!([204, 230, 237])), "{kept}");
    assert!(item(&team, "dependency_cancelled").is_none(), "the notice clears");
    assert!(plan::cancelled_deps(&team, team.task(300).unwrap()).is_empty());
    assert_eq!(team.task(300).unwrap().deps.len(), 3, "the dependencies stay on the plan");
    assert_eq!(team.task(300).unwrap().kept_cancelled, vec![204, 230, 237]);
    // Keeping one named dependency leaves the others.
    let mut other = screenshot_team();
    let kept = run(&mut other, "keep", json!({"task":300,"deps":[204]}), &lead()).unwrap();
    assert_eq!(kept["deps"], json!([204]));
    let cancelled = item(&other, "dependency_cancelled").expect("the other two remain");
    assert!(cancelled.summary.contains("#230") && cancelled.summary.contains("#237") && !cancelled.summary.contains("#204"), "{}", cancelled.summary);
    // A task without a cancelled dependency has nothing to keep.
    let error = run(&mut other, "keep", json!({"task":1}), &lead()).unwrap_err().to_string();
    assert!(error.contains("no cancelled dependency"), "{error}");
}

/// Task #460 (owner, 2026-11-09): a refused queued call is information, one
/// item per failure keyed by that failure's own request id. A failure ageing
/// out of the 24 h window closes its item and pages nothing again; a new
/// failure is the only thing told, once; the item never makes the digest
/// urgent, however urgent the member's own message was.
#[test]
fn a_refused_queued_call_is_told_once_and_never_urgent() {
    use crate::state::teams::inbox::{informational, items, mark_told, urgency, Item};
    use crate::state::teams::working::Urgency;
    let now = crate::state::now();
    let failure = |id: &str, member: &str, minutes: f64| {
        json!({"member":member,"op":"ask","request_id":id,"error":"attempt does not run for this member","urgent":true,"at":now - minutes * 60.0})
    };
    let first = "11111111-1111-4111-8111-111111111111";
    let second = "22222222-2222-4222-8222-222222222222";
    let mut team = running_team();
    team.outbox_failures = vec![failure(first, "qa", 1400.0), failure(second, "core2", 15.0)];
    let listed: Vec<Item> = items(&team).into_iter().filter(|item| item.kind == "refused_queued_call").collect();
    assert_eq!(listed.len(), 2, "one item per failure: {listed:?}");
    assert_eq!(listed[0].key, format!("refused_queued_call:{first}"));
    assert_eq!(listed[1].key, format!("refused_queued_call:{second}"));
    // Each item names its own failure, and its command asks nothing of the lead.
    assert!(listed[0].summary.contains("qa") && !listed[0].summary.contains("core2"), "{}", listed[0].summary);
    assert!(listed[1].summary.contains("core2") && !listed[1].summary.contains("qa"), "{}", listed[1].summary);
    assert!(listed.iter().all(informational), "{:?}", listed.iter().map(|item| &item.command).collect::<Vec<_>>());
    // The record's `urgent` was the member's message: the lead's digest is normal.
    assert_eq!(urgency(listed.iter()), Urgency::Normal);
    // Told once: the digest's mark covers both items and a second pass names none.
    let mut told: Vec<String> = Vec::new();
    let open: BTreeSet<String> = items(&team).into_iter().map(|item| item.key).collect();
    mark_told(&mut told, &listed, &open);
    assert!(items(&team).iter().all(|item| told.contains(&item.key)), "nothing is told twice");
    // The oldest ages out of the 24 h window: its item closes, no key changes.
    let mut aged = team.clone();
    aged.outbox_failures[0]["at"] = json!(now - 25.0 * 3600.0);
    let left: Vec<Item> = items(&aged).into_iter().filter(|item| item.kind == "refused_queued_call").collect();
    assert_eq!(left.len(), 1, "the aged failure closes its item: {left:?}");
    assert_eq!(left[0].key, format!("refused_queued_call:{second}"));
    assert!(left.iter().all(|item| told.contains(&item.key)), "ageing out is no new notice");
    // A new failure is the only thing told again, and only once.
    let third = "33333333-3333-4333-8333-333333333333";
    let mut fresh = aged.clone();
    fresh.outbox_failures.push(failure(third, "hgs", 1.0));
    let arrived: Vec<Item> = items(&fresh).into_iter().filter(|item| !told.contains(&item.key)).collect();
    assert_eq!(arrived.len(), 1, "one notice for the new failure: {arrived:?}");
    assert_eq!(arrived[0].key, format!("refused_queued_call:{third}"));
    let open: BTreeSet<String> = items(&fresh).into_iter().map(|item| item.key).collect();
    mark_told(&mut told, &arrived, &open);
    assert!(items(&fresh).iter().all(|item| told.contains(&item.key)), "the new failure is told once");
}

