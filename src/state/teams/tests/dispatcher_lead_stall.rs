//! Task 95 (owner, 2026-11-06): a stalled lead never stalls the team
//! silently. A fake lead session (idle, asking, refused, exited, busy) sits
//! before decisions that wait past `lead_stall_minutes`.
use super::support::*;
use super::*;

const MINUTE: f64 = 60.0;

/// The team with dev's report on #1 submitted now (wall clock).
fn reported() -> (Team, f64) {
    let mut t = team();
    t.status = TeamStatus::Running;
    delivered(&mut t);
    let attempt = t.task(1).unwrap().attempts[0].id.clone();
    let mut events = Vec::new();
    ops::apply(&mut t, "report", &json!({"task":1,"attempt":attempt,"text":"Done"}), &ops::Actor::Member("dev".into()), &BTreeSet::new(), &mut events)
        .unwrap();
    assert_eq!(t.task(1).unwrap().status, TaskStatus::Submitted);
    let at = t.task(1).unwrap().attempts[0].report.as_ref().unwrap().at;
    (t, at)
}

fn lead_idle() -> Seen {
    session("claude/x/lead", "r0", true)
}

fn stall_of(found: &[Step]) -> Option<Option<LeadStall>> {
    found.iter().find_map(|step| match step {
        Step::LeadStall { stall } => Some(stall.clone()),
        _ => None,
    })
}

#[test]
fn an_idle_lead_with_a_report_waiting_past_the_threshold_is_raised_once() {
    let (t, at) = reported();
    assert_eq!(t.settings.lead_stall_minutes, 20);
    // Within the threshold nothing is raised.
    assert_eq!(stall_of(&steps(&t, &[lead_idle()], at + 19.0 * MINUTE)), None);
    let now = at + 21.0 * MINUTE;
    let stall = stall_of(&steps(&t, &[lead_idle()], now)).flatten().expect("the stalled lead is raised");
    assert_eq!((stall.reason.as_str(), stall.items, stall.members, stall.round), ("idle", 1, 1, 1));
    assert_eq!((stall.since, stall.notified_at, stall.oldest), (now, now, at));
    assert!(stall.title(now).contains("it is idle") && stall.title(now).contains("1 decision waits"), "{}", stall.title(now));
    // Raised, the same tick later changes nothing: no second notification.
    let mut raised = t.clone();
    raised.lead_stall = Some(stall.clone());
    assert_eq!(stall_of(&steps(&raised, &[lead_idle()], now + 30.0 * MINUTE)), None);
    // An hour after the last notification the next round starts.
    let next = stall_of(&steps(&raised, &[lead_idle()], now + 61.0 * MINUTE)).flatten().unwrap();
    assert_eq!((next.round, next.since, next.notified_at), (2, now, now + 61.0 * MINUTE));
}

#[test]
fn the_stall_clears_when_the_lead_moves_or_nothing_waits() {
    let (mut t, at) = reported();
    let now = at + 30.0 * MINUTE;
    t.lead_stall = stall_of(&steps(&t, &[lead_idle()], now)).flatten();
    assert!(t.lead_stall.is_some());
    // The lead works again: a turn, a Zerus build or its own jobs.
    for moved in [
        Seen { activity: "busy".into(), free: false, idle: false, ..lead_idle() },
        Seen { building: true, ..lead_idle() },
        Seen { background: true, ..lead_idle() },
    ] {
        assert_eq!(stall_of(&steps(&t, std::slice::from_ref(&moved), now)), Some(None), "{moved:?}");
    }
    // The report was decided: nothing waits.
    let mut decided = t.clone();
    decided.task_mut(1).unwrap().status = TaskStatus::Accepted;
    assert_eq!(stall_of(&steps(&decided, &[lead_idle()], now)), Some(None));
    // The owner turned it off, or the team is paused.
    let mut off = t.clone();
    off.settings.lead_stall_minutes = 0;
    assert_eq!(stall_of(&steps(&off, &[lead_idle()], now)), Some(None));
    let mut paused = t.clone();
    paused.status = TeamStatus::Paused;
    assert_eq!(stall_of(&steps(&paused, &[lead_idle()], now)), Some(None));
}

#[test]
fn the_stall_names_what_the_lead_waits_on() {
    let (t, at) = reported();
    let now = at + 25.0 * MINUTE;
    let reason = |team: &Team, lead: &[Seen]| stall_of(&steps(team, lead, now)).flatten().map(|stall| (stall.reason, stall.detail));
    // A refused tool call ended its turn (the listing's `interrupted`).
    let refused = Seen { phase: "interrupted".into(), activity: "unknown".into(), free: false, idle: false, ..lead_idle() };
    assert_eq!(reason(&t, &[refused]), Some(("rejected_tool".into(), String::new())));
    // Its agent exited, or its session left the listing of a computer that answered.
    assert_eq!(reason(&t, &[stopped("claude/x/lead", "r0", "stopped")]), Some(("exited".into(), String::new())));
    assert_eq!(reason(&t, &[]), Some(("exited".into(), String::new())));
    // N7: a computer that did not answer decides nothing.
    let none = BTreeSet::from(["other".to_owned()]);
    assert_eq!(stall_of(&steps_with(&t, &[], now, Some(&none))), None);
    // Its own question, as Zerus read it, is named.
    let mut asking = t.clone();
    let card = json!({"questions":[{"id":"q","header":"Install","question":"Install the shared build cache?","options":[]}]});
    asking.members.iter_mut().find(|member| member.id == "lead").unwrap().pending =
        Some(PendingQuestion { question_id: "q1".into(), question_hash: "h1".into(), approval: false, at, card });
    let lead = Seen { asking: true, phase: "question".into(), free: false, ..lead_idle() };
    let (kind, detail) = reason(&asking, &[lead]).unwrap();
    assert_eq!(kind, "question");
    assert_eq!(detail, "Install: Install the shared build cache?");
    let stall = stall_of(&steps(&asking, &[lead_idle()], now)).flatten().unwrap();
    assert!(stall.title(now).contains("its own question: Install"), "{}", stall.title(now));
}

#[test]
fn a_member_question_and_a_permission_count_with_the_members_they_block() {
    let (mut t, at) = reported();
    let now = at + 40.0 * MINUTE;
    let card = json!({"questions":[{"id":"q","question":"Run cargo test?","options":[]}]});
    for member in t.members.iter_mut().filter(|member| member.id == "qa") {
        member.state = MemberState::Active;
        member.session = Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r3".into(), ..Default::default() });
        member.pending = Some(PendingQuestion { question_id: "p1".into(), question_hash: "ph".into(), approval: true, at: at - MINUTE, card: card.clone() });
    }
    let stall = stall_of(&steps(&t, &[lead_idle()], now)).flatten().unwrap();
    assert_eq!((stall.items, stall.members, stall.oldest), (2, 2, at - MINUTE));
    assert!(stall.title(now).contains("2 decisions wait") && stall.title(now).contains("2 members blocked"), "{}", stall.title(now));
}

#[test]
fn the_owner_sees_one_item_per_round_and_its_end_in_the_feed() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let (t, at) = reported();
    stored(&mut store, &t);
    let tick = |store: &mut Store, lead: &[Seen], now: f64| {
        let team = store.load(&t.id).unwrap().unwrap();
        for step in steps(&team, lead, now).iter().filter(|step| matches!(step, Step::LeadStall { .. })) {
            run_step(store, &team, step, "").unwrap();
        }
        store.load(&t.id).unwrap().unwrap()
    };
    // The lead stopped: the stall covers the stopped-lead item, with its resume.
    let gone = [stopped("claude/x/lead", "r0", "stopped")];
    let mut now = at + 21.0 * MINUTE;
    let mut team = tick(&mut store, &gone, now);
    team.members.iter_mut().find(|member| member.id == "lead").unwrap().seen = Some(MemberSeen { at, live: false });
    let attention = super::super::inbox::attention(&team, now);
    let found = attention.iter().find(|item| item["kind"] == "lead_stalled").expect("the owner sees the stalled lead");
    assert_eq!((found["reason"].clone(), found["round"].clone(), found["items"].clone()), (json!("exited"), json!(1), json!(1)));
    assert!(!attention.iter().any(|item| item["kind"] == "lead_stopped"), "{attention:?}");
    let waiting = super::super::owner_waiting::items(std::slice::from_ref(&team), now);
    let item = waiting.iter().find(|item| item["kind"] == "lead_stalled").unwrap();
    assert_eq!(item["id"], format!("{}/lead_stalled/Lead-1", team.id));
    assert_eq!(item["action"], "resume_lead");
    assert!(item["title"].as_str().unwrap().contains("its agent exited"), "{item}");
    // Within the hour: the same item, no new event.
    now += 30.0 * MINUTE;
    let team = tick(&mut store, &gone, now);
    assert_eq!(team.lead_stall.as_ref().unwrap().round, 1);
    // An hour on: round 2 is a new waiting id, so the desktops notify again.
    now += 31.0 * MINUTE;
    let team = tick(&mut store, &[lead_idle()], now);
    let waiting = super::super::owner_waiting::items(std::slice::from_ref(&team), now);
    let item = waiting.iter().find(|item| item["kind"] == "lead_stalled").unwrap();
    assert_eq!(item["id"], format!("{}/lead_stalled/Lead-2", team.id));
    assert_eq!(item["action"], "open_lead");
    // The lead moves: the item clears.
    let team = tick(&mut store, &[Seen { activity: "busy".into(), free: false, idle: false, ..lead_idle() }], now + MINUTE);
    assert!(team.lead_stall.is_none());
    assert!(!super::super::owner_waiting::items(std::slice::from_ref(&team), now).iter().any(|item| item["kind"] == "lead_stalled"));
    let kinds = event_kinds(&store, &t.id);
    assert_eq!(kinds.iter().filter(|kind| *kind == "lead_stalled").count(), 2, "{kinds:?}");
    assert_eq!(kinds.iter().filter(|kind| *kind == "lead_stall_cleared").count(), 1, "{kinds:?}");
}

#[test]
fn only_the_owner_sets_the_stall_threshold() {
    let (mut t, _) = reported();
    let mut events = Vec::new();
    let lead = ops::Actor::Member("lead".into());
    let refused = ops::apply(&mut t, "settings", &json!({"settings":{"lead_stall_minutes":0}}), &lead, &BTreeSet::new(), &mut events);
    assert!(refused.unwrap_err().to_string().contains("lead_stall_minutes"));
    ops::apply(&mut t, "settings", &json!({"settings":{"lead_stall_minutes":45}}), &ops::Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(t.settings.lead_stall_minutes, 45);
}
