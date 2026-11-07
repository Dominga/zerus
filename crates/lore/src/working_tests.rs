use super::*;
use std::cell::RefCell;

const T0: i64 = 1_800_000_000;

fn open(settings: Settings) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_at(&dir.path().join(FILE_NAME), &settings, &[]).unwrap();
    (dir, store)
}

fn pulse(by: &str, task: &str, done: &str) -> Pulse {
    Pulse { team: "t".into(), by: by.into(), task: task.into(), done: done.into(), ..Pulse::default() }
}

fn message(to: &str, text: &str, urgent: bool) -> Message {
    Message { team: "t".into(), by: "dev".into(), to: to.into(), text: text.into(), urgent, recipients: Vec::new() }
}

struct Closed(Option<i64>);
impl TaskClock for Closed {
    fn closed_at(&self, _team: &str, _task: &str) -> Option<i64> {
        self.0
    }
}

/// 11.15 test 4: every rule of 11.3, exactly, with a fake clock.
#[test]
fn expiry_follows_each_rule_exactly() {
    let (_dir, store) = open(Settings::default());
    let day = DAY;
    // A pulse of an open task never expires.
    let open_task = store.pulse(&pulse("dev", "#1", "parsed the scene"), T0).unwrap().entry;
    assert_eq!(open_task.expires, None);
    // Without a task: at + pulse_days.
    let loose = store.pulse(&pulse("qa", "", "looked around"), T0).unwrap().entry;
    assert_eq!(loose.expires, Some(T0 + 14 * day));
    // The task closes: close + pulse_days, not a second before.
    let closed = T0 + 3 * day;
    store.cleanup(T0 + 3 * day, &Closed(Some(closed))).unwrap();
    assert_eq!(store.get(&open_task.id, closed + 14 * day - 1).unwrap().map(|e| e.expires), Some(Some(closed + 14 * day)));
    assert!(store.get(&open_task.id, closed + 14 * day).unwrap().is_none(), "a read never returns an expired entry");
    // A reopen clears it.
    store.cleanup(T0 + 4 * day, &Closed(None)).unwrap();
    assert_eq!(store.get(&open_task.id, T0 + 400 * day).unwrap().unwrap().expires, None);
    // A read message: read_at + message_days; unread: at + unread_days.
    let read = store.send(&message("lead", "the build is green", false), None, T0).unwrap().entry;
    let unread = store.send(&message("owner", "please confirm the release", false), None, T0).unwrap().entry;
    assert_eq!(unread.expires, Some(T0 + 30 * day));
    let marked = store.mark_read(&read.id, &Viewer { team: Some("t".into()), name: "lead".into(), role: Role::Lead }, T0 + day).unwrap();
    assert_eq!(marked.expires, Some(T0 + day + 7 * day));
    // The unread one is not dropped silently: an event, an inbox item.
    let cleaned = store.cleanup(T0 + 30 * day, &OpenClock).unwrap();
    let event = cleaned.events.iter().find(|e| e["kind"] == "working_unread_expired").expect("the unread event");
    assert_eq!(event["body"]["to"], "owner");
    assert_eq!(event["body"]["inbox"], true);
    assert_eq!(event["body"]["text"], "please confirm the release");
    assert!(store.get(&unread.id, T0 + 30 * day).unwrap().is_none());
    // Briefs: the latest never; the older versions at the task's close.
    let v1 = store.brief("t", "#2", "lead", "first version", T0).unwrap().entry;
    let v2 = store.brief("t", "#2", "lead", "second version", T0 + 10).unwrap().entry;
    assert_eq!((v1.version, v2.version), (1, 2));
    store.set_task_closed("t", "#2", Some(T0 + day)).unwrap();
    assert!(store.get(&v1.id, T0 + day).unwrap().is_none(), "an old version goes at the close");
    assert_eq!(store.get(&v2.id, T0 + 1000 * day).unwrap().unwrap().expires, None, "the latest never expires by the TTL");
}

#[test]
fn a_pulse_within_five_minutes_replaces_the_previous_one_in_place() {
    let (_dir, store) = open(Settings::default());
    let first = store.pulse(&pulse("dev", "#3", "started"), T0).unwrap().entry;
    let second = store.pulse(&pulse("dev", "#3", "halfway"), T0 + 299).unwrap();
    assert_eq!(second.entry.id, first.id);
    assert_eq!(second.entry.at, T0 + 299);
    assert!(second.notes[0].starts_with("replaced"));
    let third = store.pulse(&pulse("dev", "#3", "done"), T0 + 299 + 301).unwrap().entry;
    assert_ne!(third.id, first.id, "after five minutes a new row");
    let viewer = Viewer::owner();
    let current = store.pulses(&viewer, None, None, None, false, T0 + 1000).unwrap();
    assert_eq!(current.len(), 1, "the current pulse per (by, task)");
    assert_eq!(current[0].done, "done");
    assert_eq!(store.pulses(&viewer, None, None, None, true, T0 + 1000).unwrap().len(), 2);
}

/// 11.15 test 5: each bound's refusal text.
#[test]
fn the_bounds_refuse_with_their_words() {
    let (_dir, store) = open(Settings { unread_normal: 2, unread_urgent: 1, ..Settings::default() });
    let long = "x".repeat(501);
    let error = store.pulse(&Pulse { blocked: long, ..pulse("dev", "", "") }, T0).unwrap_err();
    assert_eq!(error.message, "refused: `blocked` has 501 characters; a pulse field holds at most 500");
    let error = store.send(&message("lead", &"y".repeat(2001), false), None, T0).unwrap_err();
    assert_eq!(error.message, "refused: the message has 2001 characters; a message holds at most 2000");
    let error = store.brief("t", "#1", "lead", &"z".repeat(8 * 1024 + 1), T0).unwrap_err();
    assert!(error.message.contains("at most 8 KiB"), "{}", error.message);
    for n in 0..2 {
        store.send(&message("qa", &format!("note {n}"), false), None, T0).unwrap();
    }
    let error = store.send(&message("qa", "one more", false), None, T0).unwrap_err();
    assert_eq!(error.message, "refused: qa has 2 unread normal messages, the most it may have; it does not read them (the lead is told)");
    store.send(&message("qa", "now", true), None, T0).unwrap();
    assert!(store.send(&message("qa", "again", true), None, T0).is_err(), "urgent has its own cap");
    assert!(store.events(0, 10).unwrap().iter().any(|event| event["kind"] == "working_unread_cap"));
}

/// 11.15 test 5: the cleanup's order, its 500-row pass and what it protects.
#[test]
fn the_cleanup_is_bounded_and_keeps_what_it_must() {
    let (_dir, store) = open(Settings { pulses_per_team: 3, brief_versions: 2, ..Settings::default() });
    // Six pulses by six members: the current row of each is protected.
    for n in 0..6 {
        store.pulse(&pulse(&format!("m{n}"), "#1", "work"), T0 + n).unwrap();
    }
    // An older row of m0 that is not current.
    store.pulse(&pulse("m0", "#1", "later"), T0 + 1000).unwrap();
    let cleaned = store.cleanup(T0 + 2000, &OpenClock).unwrap();
    assert_eq!(cleaned.deleted, 1, "only the non-current pulse may go");
    let all = store.pulses(&Viewer::owner(), None, None, None, true, T0 + 2000).unwrap();
    assert_eq!(all.len(), 6);
    // Brief versions: the oldest go, the latest never.
    for v in 0..4 {
        store.brief("t", "#9", "lead", &format!("version {v}"), T0 + v).unwrap();
    }
    store.cleanup(T0 + 3000, &OpenClock).unwrap();
    let left: Vec<i64> = (1..=4).filter(|v| store.brief_of("t", "#9", Some(*v), T0 + 3000).unwrap().is_some()).collect();
    assert_eq!(left, [3, 4]);
    // At most 500 rows a pass; the next pass continues.
    let (_dir2, big) = open(Settings::default());
    for n in 0..520 {
        big.pulse(&pulse(&format!("m{n}"), "", "x"), T0).unwrap();
    }
    let first = big.cleanup(T0 + 15 * DAY, &OpenClock).unwrap();
    assert_eq!((first.deleted, first.more), (500, true));
    let second = big.cleanup(T0 + 15 * DAY, &OpenClock).unwrap();
    assert_eq!(second.deleted, 20);
    // One `working_cleaned` an hour at most, none when nothing was deleted.
    let cleaned_events = big.events(0, 100).unwrap().into_iter().filter(|e| e["kind"] == "working_cleaned").count();
    assert_eq!(cleaned_events, 1);
}

#[test]
fn working_text_is_redacted_not_refused() {
    let (_dir, store) = open(Settings::default());
    let written = store.send(&message("lead", "the key is ghp_0123456789abcdefghijABCDEFGHIJ012345 now", false), None, T0).unwrap();
    assert_eq!(written.entry.text, "the key is [redacted: github-token] now");
    assert!(written.notes[0].contains("stored redacted"));
}

struct Bell(RefCell<Vec<(String, String, bool)>>);
impl Doorbell for Bell {
    fn ring(&self, recipient: &str, text: &str, urgent: bool) -> Option<String> {
        self.0.borrow_mut().push((recipient.into(), text.into(), urgent));
        Some(if urgent { "hook".into() } else { "idle".into() })
    }
}

/// 11.11: the doorbell is one line, never the text; normal rings coalesce.
#[test]
fn the_doorbell_rings_a_line_and_coalesces_normal_messages() {
    let (_dir, store) = open(Settings::default());
    let bell = Bell(RefCell::new(Vec::new()));
    let first = store.send(&message("qa", "secret plan details", false), Some(&bell), T0).unwrap().entry;
    store.send(&message("qa", "more", false), Some(&bell), T0 + 60).unwrap();
    let urgent = store.send(&message("qa", "stop the build", true), Some(&bell), T0 + 61).unwrap().entry;
    let rings = bell.0.borrow();
    assert_eq!(rings.len(), 2, "the second normal one coalesced: {rings:?}");
    assert_eq!(rings[0].1, format!("[Zerus lore · message · from dev · normal · {}] read it with hgs lore messages --unread", first.id));
    assert!(!rings[0].1.contains("secret plan"), "the text stays in the store");
    assert!(rings[1].1.contains("URGENT") && rings[1].2);
    assert_eq!((urgent.state.as_str(), urgent.safe_point.as_str()), ("rung", "hook"));
}

#[test]
fn a_member_sees_its_messages_and_the_lead_all() {
    let (_dir, store) = open(Settings::default());
    store.send(&message("qa", "for qa", false), None, T0).unwrap();
    store.send(&message("ui", "for ui", false), None, T0).unwrap();
    store.send(&message("team", "for everyone", false), None, T0).unwrap();
    let qa = Viewer { team: Some("t".into()), name: "qa".into(), role: Role::Member };
    let texts: Vec<String> = store.messages(&qa, false, None, None, T0).unwrap().into_iter().map(|e| e.text).collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert!(texts.contains(&"for qa".to_owned()) && texts.contains(&"for everyone".to_owned()));
    let lead = Viewer { team: Some("t".into()), name: "lead".into(), role: Role::Lead };
    assert_eq!(store.messages(&lead, false, None, None, T0).unwrap().len(), 3);
    let other = Viewer { team: Some("other".into()), name: "lead".into(), role: Role::Lead };
    assert!(store.messages(&other, false, None, None, T0).unwrap().is_empty(), "no search crosses teams");
    // A team message is read once every known recipient read it.
    let team = store.send(&Message { recipients: vec!["qa".into(), "ui".into()], ..message("team", "two readers", false) }, None, T0).unwrap().entry;
    assert_eq!(store.mark_read(&team.id, &qa, T0 + 1).unwrap().state, "queued");
    let ui = Viewer { team: Some("t".into()), name: "ui".into(), role: Role::Member };
    assert_eq!(store.mark_read(&team.id, &ui, T0 + 2).unwrap().state, "read");
}

#[test]
fn a_read_marks_only_the_readers_own_copy() {
    // Task 186 (merger's review of 132): nobody hides or consumes another's
    // message.
    let (_dir, store) = open(Settings::default());
    let for_qa = store.send(&message("qa", "for qa", false), None, T0).unwrap().entry;
    let for_lead = store.send(&message("lead", "for the lead", false), None, T0).unwrap().entry;
    let for_owner = store.send(&message("owner", "for the owner", false), None, T0).unwrap().entry;
    let ui = Viewer { team: Some("t".into()), name: "ui".into(), role: Role::Member };
    let lead = Viewer { team: Some("t".into()), name: "lead".into(), role: Role::Lead };
    let refused = store.mark_read(&for_qa.id, &lead, T0 + 1).unwrap_err();
    assert!(refused.message.contains("only its recipient marks it read"), "{}", refused.message);
    assert!(store.mark_read(&for_qa.id, &ui, T0 + 1).is_err(), "a member never sees another's message");
    assert!(store.mark_read(&for_owner.id, &lead, T0 + 1).is_err(), "the lead does not read the owner's");
    assert!(store.mark_read(&for_lead.id, &Viewer::owner(), T0 + 1).is_err(), "the owner does not read the lead's");
    assert_eq!(store.mark_read(&for_lead.id, &lead, T0 + 1).unwrap().state, "read");
    assert_eq!(store.mark_read(&for_owner.id, &Viewer::owner(), T0 + 1).unwrap().state, "read");
    // A team message with no named recipients: each reader's own mark only.
    let all = store.send(&message("team", "for everyone", false), None, T0).unwrap().entry;
    assert_eq!(store.mark_read(&all.id, &ui, T0 + 2).unwrap().state, "queued", "the first reader does not settle it");
    let qa = Viewer { team: Some("t".into()), name: "qa".into(), role: Role::Member };
    assert_eq!(store.messages(&qa, true, None, None, T0 + 3).unwrap().iter().filter(|e| e.id == all.id).count(), 1);
    assert!(store.messages(&ui, true, None, None, T0 + 3).unwrap().iter().all(|e| e.id != all.id));
    assert!(store.messages(&qa, true, None, None, T0 + 3).unwrap().iter().any(|e| e.id == for_qa.id), "qa's stays unread");
}

#[test]
fn the_working_search_finds_by_words_and_terms() {
    let (_dir, store) = open(Settings::default());
    store.pulse(&Pulse { blocked: "the depth_buffer resolve crashes on Vulkan".into(), ..pulse("dev", "#4", "") }, T0).unwrap();
    store.send(&message("lead", "unrelated chatter", false), None, T0).unwrap();
    let found = store.search("depth_buffer crash", &Viewer::owner(), T0).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0.kind, "pulse");
    assert!(found[0].2.contains(&"words".to_owned()));
    assert!(store.search("depth_buffer", &Viewer::owner(), T0 + 400 * DAY).unwrap().len() == 1, "an open task's pulse stays");
}

#[test]
fn a_full_file_refuses_writes_and_reads_go_on() {
    let (_dir, store) = open(Settings { file_bytes: 1, ..Settings::default() });
    let error = store.pulse(&pulse("dev", "", "x"), T0).unwrap_err();
    assert_eq!(error.message, "the working tier is full (0 MiB)");
    assert!(store.pulses(&Viewer::owner(), None, None, None, true, T0).unwrap().is_empty());
}

#[test]
fn the_plan_clock_reads_closed_items() {
    let records = vec![
        json!({"kind": "item", "id": "ship#3", "status": "done", "updated": "2026-11-07"}),
        json!({"kind": "item", "id": "ship#4", "status": "open", "updated": "2026-11-07"}),
    ];
    let clock = PlanClock::from_records(&records);
    assert_eq!(clock.closed_at("", "ship#3"), Some(1_794_009_600));
    assert_eq!(clock.closed_at("", "#ship#3"), Some(1_794_009_600));
    assert_eq!(clock.closed_at("", "ship#4"), None);
}
