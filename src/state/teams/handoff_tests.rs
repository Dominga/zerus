//! Task 135: the handoff brief on a swap and on a fresh session, built from the
//! team's own record, bounded, kept with the team, with the control questions of
//! a task as the acceptance check.
use super::super::ops::{self, Actor};
use super::*;

fn team_with(members: Value, tasks: Value) -> Team {
    let (mut team, _) = ops::create(
        "t",
        &json!({"name":"Handoff","goal":"Ship the v2 items API","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0","conversation_id":"c"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    ops::apply(&mut team, "plan", &json!({"members":members,"tasks":tasks}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    team
}

fn member(id: &str, role: &str, agent: &str, model: &str) -> Value {
    json!({"id":id,"role":role,"agent":agent,"model":model,
        "session":{"name":format!("{agent}/x/{id}"),"run_id":format!("run-{id}"),"conversation_id":"c"}})
}

fn on(name: &str, model: &str) -> Seen {
    Seen {
        name: name.into(),
        run_id: format!("run-{}", name.rsplit('/').next().unwrap()),
        conversation_id: "c".into(),
        free: true,
        idle: true,
        live: true,
        model: model.into(),
        ..Default::default()
    }
}

fn lead_seen() -> Seen {
    on("claude/x/lead", "")
}

fn stored(team: &Team) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111135", &json!({}), team.clone(), vec![]).unwrap();
    (dir, store)
}

fn kinds(store: &Store, team: &str) -> Vec<String> {
    let page = store.events(team, None, 200).unwrap();
    let mut out: Vec<String> = page["items"].as_array().unwrap().iter().map(|item| item["kind"].as_str().unwrap().to_owned()).collect();
    out.reverse();
    out
}

/// The calls a fake `hgs` recorded: each one's arguments and its input.
type Calls = std::rc::Rc<std::cell::RefCell<Vec<(String, Value)>>>;

/// A fake `hgs` that records each call: its arguments and its input.
fn fake() -> Calls {
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let record = calls.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
        record.borrow_mut().push((args.join(" "), input.cloned().unwrap_or_default()));
        Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
    calls
}

/// The texts of the hand-out messages stored for `member`, oldest first (task
/// 134: the text lives in the working tier; the terminal gets a doorbell, or,
/// for an agent without a turn hook, the doorbell and the text).
fn stored_hand_outs(store: &Store, team: &str, member: &str) -> Vec<String> {
    crate::state::teams::working::unread(store.connection(), team, member, crate::state::now())
        .unwrap()
        .into_iter()
        .filter(|message| message.kind == "hand_out")
        .map(|message| message.text)
        .collect()
}

/// The text of the `n`th send to `session`.
fn sent(calls: &Calls, session: &str, n: usize) -> String {
    let wanted = format!("send {session} --json");
    calls.borrow().iter().filter(|(args, _)| *args == wanted).nth(n).map(|(_, input)| input["text"].as_str().unwrap_or_default().to_owned()).unwrap_or_default()
}

const MARK: &str = "[Zerus team \u{b7} from: Zerus \u{b7} handoff brief]";

/// Dev (claude) holds #2, whose control questions the brief must answer; #1 is
/// accepted with a reason, #3 waits for qa behind #2.
fn fixture() -> Team {
    team_with(
        json!([member("dev", "Backend", "claude", "opus"), member("qa", "Tester", "kimi", "k2")]),
        json!([
            {"id":1,"title":"Schema","assignee":"dev","pipeline":"none"},
            {"id":2,"title":"Items API","assignee":"dev","pipeline":"none","description":"Serve GET /v2/items from the new schema",
             "criteria":["the endpoint answers 200 with a JSON list","old /v1 stays untouched"],"output":["src/api/items.rs"],
             "deps":[{"task":1}],
             "control":[{"q":"Which file holds the endpoint?","keys":["src/api/items.rs"]},
                        {"q":"What must the endpoint answer?","keys":["200"]},
                        {"q":"What must stay untouched?","keys":["/v1"]},
                        {"q":"What did task 1 settle?","keys":["schema v2 kept"]}]},
            {"id":3,"title":"Items tests","assignee":"qa","pipeline":"none","deps":[{"task":2}]},
            {"id":4,"title":"Docs","assignee":"dev","pipeline":"none"}
        ]),
    )
}

/// Task 1 handed out, reported and accepted; task 2 handed out to dev's claude
/// session; returns the store with the fake hgs set.
fn running() -> (tempfile::TempDir, Store, Team, Calls) {
    let t = fixture();
    let (dir, mut store) = stored(&t);
    let calls = fake();
    run_step(&mut store, &t, &Step::HandOut { task: 1, member: "dev".into() }, "").unwrap();
    let attempt = store.load(&t.id).unwrap().unwrap().task(1).unwrap().attempts[0].id.clone();
    store.mutate(None, &t.id, None, |team, events| {
        ops::apply(team, "report", &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &BTreeSet::new(), events)
    })
    .unwrap();
    store.mutate(None, &t.id, None, |team, events| {
        ops::apply(team, "accept", &json!({"task":1,"reason":"schema v2 kept, v1 columns stay"}), &Actor::Member("lead".into()), &BTreeSet::new(), events)
    })
    .unwrap();
    let now = store.load(&t.id).unwrap().unwrap();
    run_step(&mut store, &now, &Step::HandOut { task: 2, member: "dev".into() }, "").unwrap();
    let team = store.load(&t.id).unwrap().unwrap();
    (dir, store, team, calls)
}

fn swap_to_kimi(store: &mut Store, team: &str, session: Option<Value>) {
    let mut input = json!({"member":"dev","agent":"kimi","model":"k2"});
    if let Some(session) = session {
        input["session"] = session;
    }
    store.mutate(None, team, None, |team, events| ops::apply(team, "swap-member", &input, &Actor::Human, &BTreeSet::new(), events)).unwrap();
}

#[test]
fn a_claude_to_kimi_swap_gives_the_new_session_a_brief_with_its_first_message() {
    let (dir, mut store, team, calls) = running();
    // The working session's own hand-out of task 2 carried no brief.
    let before = stored_hand_outs(&store, &team.id, "dev");
    assert!(before.iter().any(|text| text.contains("Task #2:")) && before.iter().all(|text| !text.contains(MARK)), "the default path is unchanged");
    swap_to_kimi(&mut store, &team.id, Some(json!({"name":"kimi/x/dev","run_id":"run-dev","conversation_id":"c"})));
    let swapped = store.load(&team.id).unwrap().unwrap();
    let need = swapped.member("dev").unwrap().handoff.clone().expect("a swap leaves a brief to give");
    assert_eq!(need.why, "swap");
    assert_eq!(need.from, "claude/x/dev");
    assert_eq!(need.changes, vec!["agent claude -> kimi".to_owned(), "model opus -> k2".to_owned()]);
    // The task is back in the queue for the new session.
    let seen = [lead_seen(), on("kimi/x/dev", "k2"), on("kimi/x/qa", "k2")];
    let found = steps(&swapped, &seen, 100.0);
    assert!(found.contains(&Step::HandOut { task: 2, member: "dev".into() }), "{found:?}");
    run_step(&mut store, &swapped, &Step::HandOut { task: 2, member: "dev".into() }, "").unwrap();
    set_fake_hgs(None);
    // What the new session is told: its header line keeps the hand-out's own
    // fields (the doorbell is made from it), and the brief is under it, ahead of the task text.
    let text = stored_hand_outs(&store, &team.id, "dev").into_iter().find(|text| text.contains(MARK)).expect("the brief is in the stored hand-out");
    assert!(text.starts_with("[Zerus team \u{b7} from: Lead \u{b7} task: #2"), "{text}");
    let at = text.find(MARK).unwrap();
    let body = text.find("Task #2:").expect("the hand-out's task text follows the brief");
    assert!(at < body, "{text}");
    let brief = &text[at..body];
    let pasted = sent(&calls, "kimi/x/dev", 0);
    assert!(pasted.starts_with("[Zerus team \u{b7} from: Lead \u{b7} task: #2"), "the doorbell keeps the hand-out's header: {pasted}");
    for wanted in [
        "You are Backend in the team Handoff",
        "swapped away from claude/x/dev",
        "agent claude -> kimi",
        "model opus -> k2",
        "Goal: Ship the v2 items API",
        "Your work: #2 Items API",
        "Serve GET /v2/items from the new schema",
        "the endpoint answers 200 with a JSON list",
        "src/api/items.rs",
        "Needs: #1 (accepted)",
        "- #1 Schema: schema v2 kept, v1 columns stay",
        "- #3 Items tests",
        "Last pulses:",
        "handed out #2",
    ] {
        assert!(brief.contains(wanted), "{wanted:?} missing in\n{brief}");
    }
    // It is kept with the team, once the session had it, and the seat is told.
    let after = store.load(&team.id).unwrap().unwrap();
    assert!(after.member("dev").unwrap().handoff.is_none());
    assert_eq!(after.handoffs.len(), 1);
    let kept = handoff::find(&after, 2, None).unwrap();
    assert_eq!((kept.task, kept.version, kept.why.as_str()), (2, 1, "swap"));
    assert!(text.contains(&kept.text), "the kept brief is the one in the message");
    assert!(kept.misses.is_empty(), "{:?}", kept.misses);
    let kinds = kinds(&store, &team.id);
    assert!(kinds.contains(&"handoff_sent".to_owned()) && !kinds.contains(&"handoff_gap".to_owned()), "{kinds:?}");
    // The restart of the home computer: a new store handle on the same file
    // still has the brief (it is part of the team's record).
    let path = dir.path().join("teams/teams.sqlite3");
    drop(store);
    let reopened = Store::open_at(&path, false).unwrap();
    let again = reopened.load(&team.id).unwrap().unwrap();
    assert_eq!(handoff::find(&again, 2, None).unwrap().text, kept.text);
}

#[test]
fn a_fresh_session_is_briefed_once_and_the_next_hand_out_carries_none() {
    let (_dir, mut store, team, calls) = running();
    // A swap with no session adopted: the dispatcher launches a new one; here
    // the launch is simulated by the session it would record.
    swap_to_kimi(&mut store, &team.id, None);
    store
        .mutate(None, &team.id, None, |team, _| {
            let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            assert!(dev.launch_after_swap && dev.session.is_none());
            dev.session = Some(SessionRef { name: "kimi/x/dev".into(), run_id: "run-dev".into(), conversation_id: "c".into(), ..Default::default() });
            dev.state = MemberState::Active;
            dev.launch_after_swap = false;
            Ok(json!({}))
        })
        .unwrap();
    let launched = store.load(&team.id).unwrap().unwrap();
    assert!(launched.member("dev").unwrap().handoff.is_some(), "a launch alone does not clear it");
    run_step(&mut store, &launched, &Step::HandOut { task: 2, member: "dev".into() }, "").unwrap();
    let first = stored_hand_outs(&store, &team.id, "dev").into_iter().find(|text| text.contains(MARK)).expect("the fresh session's hand-out carries the brief");
    assert!(first.contains("swapped away from claude/x/dev"), "{first}");
    // The next hand-out to the same session has no brief: it is no longer fresh.
    let mut after = store.load(&team.id).unwrap().unwrap();
    let attempt = after.task(2).unwrap().attempts.last().unwrap().id.clone();
    store.mutate(None, &team.id, None, |team, events| {
        ops::apply(team, "report", &json!({"task":2,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &BTreeSet::new(), events)
    })
    .unwrap();
    store.mutate(None, &team.id, None, |team, events| ops::apply(team, "accept", &json!({"task":2}), &Actor::Member("lead".into()), &BTreeSet::new(), events)).unwrap();
    after = store.load(&team.id).unwrap().unwrap();
    run_step(&mut store, &after, &Step::HandOut { task: 4, member: "dev".into() }, "").unwrap();
    let all = stored_hand_outs(&store, &team.id, "dev");
    let docs = all.iter().find(|text| text.contains("Task #4:")).expect("the docs task was handed out");
    assert!(!docs.contains(MARK), "only the fresh session's first message carries it: {docs}");
    assert_eq!(all.iter().filter(|text| text.contains(MARK)).count(), 1);
    assert!(sent(&calls, "kimi/x/dev", 1).starts_with("[Zerus team \u{b7} from: Lead \u{b7} task: #4"));
    // The task closed: its brief is still the latest, and it is the only version.
    let closed = store.load(&team.id).unwrap().unwrap();
    assert_eq!(closed.handoffs.iter().filter(|handoff| handoff.task == 2).count(), 1);
    set_fake_hgs(None);
}

#[test]
fn a_refused_send_keeps_the_brief_pending_for_the_next_try() {
    let (_dir, mut store, team, _) = running();
    swap_to_kimi(&mut store, &team.id, Some(json!({"name":"kimi/x/dev","run_id":"run-dev","conversation_id":"c"})));
    let swapped = store.load(&team.id).unwrap().unwrap();
    set_fake_hgs(Some(Box::new(|_, args: &[&str], _| match args.first().copied() {
        Some("send") => Ok(fake_output(1, "", "the session is busy")),
        _ => Ok(fake_output(0, "{}", "")),
    })));
    let _ = run_step(&mut store, &swapped, &Step::HandOut { task: 2, member: "dev".into() }, "");
    set_fake_hgs(None);
    let after = store.load(&team.id).unwrap().unwrap();
    assert!(after.member("dev").unwrap().handoff.is_some(), "a refused send is not a delivery");
    assert!(after.handoffs.is_empty());
}

#[test]
fn a_swapped_lead_gets_the_brief_ahead_of_the_lead_brief() {
    let t = fixture();
    let (_dir, mut store) = stored(&t);
    let calls = fake();
    store
        .mutate(None, &t.id, None, |team, events| {
            ops::apply(
                team,
                "swap-member",
                &json!({"member":"lead","agent":"kimi","model":"k2","session":{"name":"kimi/x/lead","run_id":"run-lead","conversation_id":"c"}}),
                &Actor::Human,
                &BTreeSet::new(),
                events,
            )
        })
        .unwrap();
    let swapped = store.load(&t.id).unwrap().unwrap();
    assert_eq!(swapped.brief, Notice::Pending);
    assert!(swapped.member("lead").unwrap().handoff.is_some());
    send_brief(&mut store, &t.id).unwrap();
    set_fake_hgs(None);
    let text = sent(&calls, "kimi/x/lead", 0);
    assert!(text.starts_with(MARK), "{text}");
    assert!(text.contains("You are Lead in the team Handoff"), "{text}");
    assert!(text.find(MARK).unwrap() < text.find("You are now the lead of the team").expect("the lead brief follows"), "{text}");
    let after = store.load(&t.id).unwrap().unwrap();
    assert!(after.member("lead").unwrap().handoff.is_none());
    assert_eq!(after.brief, Notice::Sent);
    assert!(handoff::find(&after, 0, None).is_some(), "a seat with no task keeps its brief under task 0");
}

#[test]
fn a_brief_that_cannot_answer_a_control_question_is_a_gap_the_lead_sees() {
    let mut t = fixture();
    t.task_mut(2).unwrap().control.push(Control { q: "Who owns the staging key?".into(), keys: vec!["staging-key-owner".into()] });
    t.task_mut(2).unwrap().deps.clear();
    // Task 1 is not accepted in this variant, so its question has no answer either way.
    t.task_mut(2).unwrap().control.retain(|control| !control.q.contains("task 1"));
    let (_dir, mut store) = stored(&t);
    let _calls = fake();
    swap_to_kimi(&mut store, &t.id, Some(json!({"name":"kimi/x/dev","run_id":"run-dev","conversation_id":"c"})));
    let swapped = store.load(&t.id).unwrap().unwrap();
    run_step(&mut store, &swapped, &Step::HandOut { task: 2, member: "dev".into() }, "").unwrap();
    set_fake_hgs(None);
    assert!(stored_hand_outs(&store, &t.id, "dev").iter().any(|text| text.contains(MARK)));
    let after = store.load(&t.id).unwrap().unwrap();
    let kept = handoff::find(&after, 2, None).unwrap();
    assert_eq!(kept.misses, vec!["Who owns the staging key?".to_owned()]);
    let page = store.events(&t.id, None, 200).unwrap();
    let gap = page["items"].as_array().unwrap().iter().find(|item| item["kind"] == "handoff_gap").expect("a handoff_gap event");
    assert_eq!(gap["detail"]["questions"], json!(["Who owns the staging key?"]));
}

#[test]
fn the_brief_answers_the_tasks_control_questions_from_the_brief_alone() {
    // The acceptance of task 135 in its mechanical form: a fresh session given
    // only the brief answers a control question when the brief holds one of the
    // question's strings, as the retrieval bench judges an answer.
    let (_dir, mut store, team, _) = running();
    swap_to_kimi(&mut store, &team.id, Some(json!({"name":"kimi/x/dev","run_id":"run-dev","conversation_id":"c"})));
    let swapped = store.load(&team.id).unwrap().unwrap();
    let built = handoff::build(&swapped, "dev", handoff::Why::Swap(swapped.member("dev").unwrap().handoff.as_ref().unwrap()), &[], handoff::BRIEF_UNITS);
    assert_eq!(built.task, 2);
    let questions = &swapped.task(2).unwrap().control;
    assert_eq!(questions.len(), 4);
    for control in questions {
        let lower = built.text.to_lowercase();
        assert!(control.keys.iter().any(|key| lower.contains(&key.to_lowercase())), "{} is not answered by\n{}", control.q, built.text);
    }
    assert!(built.misses.is_empty(), "{:?}", built.misses);
}

#[test]
fn a_brief_is_bounded_whatever_the_team_holds() {
    let mut t = fixture();
    // A team with a hundred tasks of long text, an owner word and proposals.
    for n in 10..110u32 {
        let mut task = t.task(3).unwrap().clone();
        task.id = n;
        task.title = format!("Task {n} {}", "title ".repeat(40));
        task.description = "description ".repeat(300);
        task.criteria = (0..20).map(|i| format!("criterion {i} {}", "word ".repeat(80))).collect();
        task.output = (0..30).map(|i| format!("src/very/long/path/{n}/{i}/file.rs")).collect();
        task.deps.clear();
        task.status = if n % 2 == 0 { TaskStatus::Accepted } else { TaskStatus::Pending };
        task.decision_reason = "a reason ".repeat(100);
        t.tasks.push(task);
    }
    t.goal = "goal ".repeat(500);
    for n in 0..60 {
        t.proposals.push(json!({"id":format!("p{n}"),"kind":"swap-member","state":"allowed","reason":"because ".repeat(100)}));
    }
    let need = HandoffNeed { why: "swap".into(), at: 1.0, from: "claude/x/dev".into(), changes: vec!["agent claude -> kimi".into(); 30] };
    let pulses: Vec<String> = (0..40).map(|n| format!("{n} min ago {}", "pulse ".repeat(60))).collect();
    let units = |text: &str| text.encode_utf16().count();
    let brief = handoff::build(&t, "dev", handoff::Why::Swap(&need), &pulses, handoff::BRIEF_UNITS);
    assert!(units(&brief.text) <= handoff::BRIEF_UNITS, "{} units", units(&brief.text));
    assert!(brief.text.starts_with(MARK) && brief.text.contains("Goal: goal"), "{}", brief.text);
    assert!(brief.text.contains("Your work: #1 Schema"), "the seat's own task survives the bound: {}", brief.text);
    let recall = handoff::build(&t, "dev", handoff::Why::Compaction, &pulses, handoff::RECALL_UNITS);
    assert!(units(&recall.text) <= handoff::RECALL_UNITS, "{} units", units(&recall.text));
    assert!(recall.text.contains("your context was compacted or cleared") && recall.text.contains("Your work: #1 Schema"), "{}", recall.text);
    // Multi-byte text is cut on a character and counted in UTF-16 units.
    let wide = handoff::clip(&"\u{1f600}".repeat(100), 51);
    assert_eq!(units(&wide), 51);
    assert!(wide.ends_with('\u{2026}'));
    assert_eq!(handoff::clip("short", 51), "short");
}

#[test]
fn the_briefs_a_team_keeps_are_bounded_and_the_latest_of_a_task_stays() {
    let mut t = fixture();
    let built = |task: u32, text: &str| handoff::Built { text: text.into(), task, misses: Vec::new() };
    for n in 0..25 {
        handoff::record(&mut t, &built(2, &format!("version {n}")), "dev", "swap", f64::from(n));
    }
    let versions: Vec<u32> = t.handoffs.iter().filter(|h| h.task == 2).map(|h| h.version).collect();
    assert_eq!(versions.len(), 20, "at most 20 versions of a task");
    assert_eq!((versions.first(), versions.last()), (Some(&6), Some(&25)), "the oldest go, the latest stays");
    handoff::record(&mut t, &built(3, "for qa"), "qa", "swap", 99.0);
    // The task closes: its older versions go, the latest stays; the open one keeps its versions.
    t.task_mut(2).unwrap().status = TaskStatus::Accepted;
    handoff::prune(&mut t);
    assert_eq!(t.handoffs.iter().filter(|h| h.task == 2).count(), 1);
    assert_eq!(handoff::find(&t, 2, None).unwrap().text, "version 24");
    assert_eq!(handoff::find(&t, 3, None).unwrap().text, "for qa");
    assert!(handoff::find(&t, 2, Some(7)).is_none());
    // The team ends: every task's older versions go.
    handoff::record(&mut t, &built(3, "for qa again"), "qa", "swap", 100.0);
    t.status = TeamStatus::Done;
    handoff::prune(&mut t);
    assert_eq!(t.handoffs.iter().filter(|h| h.task == 3).count(), 1);
    // The total is bounded at 200 by dropping older versions, never a latest.
    let mut big = fixture();
    for task in 1..=150u32 {
        for version in 0..2 {
            handoff::record(&mut big, &built(task, &format!("{task}.{version}")), "dev", "swap", f64::from(version));
        }
    }
    assert!(big.handoffs.len() <= 200, "{}", big.handoffs.len());
    assert!((1..=150u32).all(|task| handoff::find(&big, task, None).is_some_and(|h| h.text == format!("{task}.1"))), "a task's latest is never dropped by a bound");
}

#[test]
fn the_pulse_lines_are_the_seats_own_recent_events_newest_first() {
    let t = fixture();
    let items = vec![
        json!({"seq":9,"at":10_000.0,"kind":"reported","detail":{"task":2,"attempt":1}}),
        json!({"seq":8,"at":9_700.0,"kind":"handed_out","detail":{"by":"Lead","task":2,"to":"Backend","attempt":1}}),
        json!({"seq":7,"at":9_600.0,"kind":"handed_out","detail":{"by":"Lead","task":3,"to":"Tester","attempt":1}}),
        json!({"seq":6,"at":5_000.0,"kind":"member_swapped","detail":{"member":"Backend","from":"a","to":"b"}}),
    ];
    let lines = handoff::pulse_lines(&t, "dev", &items, 10_060.0);
    assert_eq!(lines, vec!["1 min ago reported #2".to_owned(), "6 min ago handed out #2".to_owned(), "84 min ago member swapped".to_owned()]);
    assert!(handoff::pulse_lines(&t, "dev", &[], 1.0).is_empty());
}

#[test]
fn control_questions_are_bounded_when_a_plan_is_made() {
    let planned = |control: Value| {
        let (mut team, _) = ops::create("t", &json!({"name":"Plan","lead":{"id":"lead","agent":"claude"}})).unwrap();
        ops::apply(
            &mut team,
            "plan",
            &json!({"members":[{"id":"dev","agent":"claude"}],"tasks":[{"id":1,"title":"X","assignee":"dev","control":control}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut Vec::new(),
        )
        .map(|_| team)
    };
    let too_many: Vec<Value> = (0..13).map(|n| json!({"q":format!("question {n}"),"keys":["k"]})).collect();
    assert!(format!("{:#}", planned(json!(too_many)).unwrap_err()).contains("at most 12 control questions"));
    assert!(format!("{:#}", planned(json!([{"q":"no key","keys":[]}])).unwrap_err()).contains("names 1 to 6 keys"));
    assert!(format!("{:#}", planned(json!([{"q":"seven","keys":["1","2","3","4","5","6","7"]}])).unwrap_err()).contains("names 1 to 6 keys"));
    let team = planned(json!([{"q":"which file?","keys":["src/api.rs"]}])).unwrap();
    assert_eq!(team.task(1).unwrap().control, vec![Control { q: "which file?".into(), keys: vec!["src/api.rs".into()] }]);
    // A task without them stores nothing new.
    let plain = planned(json!([])).unwrap();
    assert!(serde_json::to_value(plain.task(1).unwrap()).unwrap().get("control").is_none());
}
