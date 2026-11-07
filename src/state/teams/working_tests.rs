// Task 134: the working tier's team messages in teams.sqlite3.
use super::*;
use crate::state::teams::model::Member;
use crate::state::teams::store::Store;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        members: vec![Member { id: "lead".into(), ..Default::default() }, Member { id: "dev".into(), ..Default::default() }],
        ..Default::default()
    };
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    (dir, store)
}

fn post_one(store: &mut Store, request: &str, text: &str, urgency: Urgency, at: f64) -> anyhow::Result<(Message, bool)> {
    store.mutate_working("t", |tx, team, _| {
        let posted = post(tx, "t", &NewMessage { request_id: request, by: "lead", to: "dev", kind: "message", urgency, text }, at)?;
        refresh(tx, team, "dev", at)?;
        Ok(posted)
    })
}

#[test]
fn a_message_is_stored_once_per_request_and_read_only_by_a_receipt() {
    let (_dir, mut store) = store();
    let (first, fresh) = post_one(&mut store, "r-1", "Use port 8080", Urgency::Normal, 100.0).unwrap();
    assert!(fresh && first.id.starts_with("w-message-") && first.state == "queued");
    assert_eq!(first.expires, 100.0 + UNREAD_DAYS * 86_400.0);
    // The same request answers the same message; another text under it is refused.
    let (again, fresh) = post_one(&mut store, "r-1", "Use port 8080", Urgency::Normal, 200.0).unwrap();
    assert!(!fresh && again.id == first.id);
    assert!(post_one(&mut store, "r-1", "Use port 9090", Urgency::Normal, 200.0).unwrap_err().to_string().contains("another message"));
    assert!(post_one(&mut store, "r-2", "", Urgency::Normal, 200.0).is_err());
    assert!(post_one(&mut store, "r-3", &"x".repeat(TEXT_LIMIT + 1), Urgency::Normal, 200.0).is_err());
    // The team record carries the summary status shows.
    let team = store.load("t").unwrap().unwrap();
    let summary = team.member("dev").unwrap().unread.clone().unwrap();
    assert_eq!((summary.count, summary.urgent, summary.oldest.as_str(), summary.rings), (1, 0, first.id.as_str(), 0));
    // Rung and put into a turn: still unread.
    store.mutate_working("t", |tx, _, _| { rung(tx, std::slice::from_ref(&first.id), "idle", 110.0)?; injected(tx, std::slice::from_ref(&first.id), "hook", 120.0) }).unwrap();
    let list = unread(store.connection(), "t", "dev", 130.0).unwrap();
    assert_eq!((list[0].state.as_str(), list[0].rings, list[0].safe_point.as_str()), ("rung", 1, "idle"));
    assert!(list[0].injected_at.is_some());
    // The receipt: only the recipient's own messages, once, with its request id.
    let marked = store.mutate_working("t", |tx, _, _| mark_read(tx, "t", "lead", std::slice::from_ref(&first.id), "inbox", "read-1", 140.0)).unwrap();
    assert!(marked.is_empty(), "another member's read marks nothing");
    let marked = store.mutate_working("t", |tx, team, _| {
        let marked = mark_read(tx, "t", "dev", std::slice::from_ref(&first.id), "inbox", "read-2", 150.0)?;
        refresh(tx, team, "dev", 150.0)?;
        Ok(marked)
    }).unwrap();
    assert_eq!(marked, vec![first.id.clone()]);
    let read = by_request(store.connection(), "r-1").unwrap().unwrap();
    assert_eq!((read.state.as_str(), read.read_at, read.read_via.as_str(), read.read_request.as_str()), ("read", Some(150.0), "inbox", "read-2"));
    assert_eq!(read.expires, 150.0 + MESSAGE_DAYS * 86_400.0, "a read message expires 7 days after its read");
    assert!(unread(store.connection(), "t", "dev", 160.0).unwrap().is_empty());
    assert!(store.load("t").unwrap().unwrap().member("dev").unwrap().unread.is_none());
}

#[test]
fn the_cleanup_is_bounded_and_names_what_expired_unread() {
    let (_dir, mut store) = store();
    for n in 0..(CLEANUP_ROWS + 20) {
        post_one(&mut store, &format!("r-{n}"), &format!("note {n}"), Urgency::Normal, n as f64).unwrap();
    }
    let read = format!("r-{}", CLEANUP_ROWS + 19);
    let id = by_request(store.connection(), &read).unwrap().unwrap().id;
    store.mutate_working("t", |tx, _, _| mark_read(tx, "t", "dev", &[id], "current", "x", 10.0)).unwrap();
    // Nothing expires before its time; an expired one never reads back.
    let now = UNREAD_DAYS * 86_400.0 + (CLEANUP_ROWS + 30) as f64;
    assert_eq!(unread(store.connection(), "t", "dev", now).unwrap().len(), 0, "past its expiry a message is not returned");
    let count = |store: &Store| store.connection().query_row("SELECT COUNT(*) FROM working_messages", [], |row| row.get::<_, i64>(0)).unwrap();
    let first = store.mutate_working("t", |tx, _, _| cleanup(tx, "t", now)).unwrap();
    assert_eq!(count(&store), 20, "one pass deletes at most 500 rows");
    // The read one went first (it expired earliest), silently; each unread one
    // is named once.
    assert_eq!(first.len() as i64, CLEANUP_ROWS - 1);
    assert!(first.iter().all(|event| event.kind == "working_unread_expired"));
    assert_eq!(first[0].payload["text"], "note 0");
    let second = store.mutate_working("t", |tx, _, _| cleanup(tx, "t", now)).unwrap();
    assert_eq!(second.len(), 20);
    assert_eq!(store.connection().query_row("SELECT COUNT(*) FROM working_messages", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn the_doorbell_is_one_line_and_never_the_text() {
    let bell = doorbell("[Zerus team \u{b7} from: Lead \u{b7} task: #7 \u{b7} attempt: 2]", 1, 0);
    assert_eq!(bell, "[Zerus team \u{b7} from: Lead \u{b7} task: #7 \u{b7} attempt: 2] 1 new message for you: hgs team inbox (or hgs team current)");
    let urgent = doorbell("", 3, 1);
    assert_eq!(urgent, "[Zerus team \u{b7} URGENT] 3 new messages for you: hgs team inbox (or hgs team current)");
    assert_eq!(header_of("[Zerus team \u{b7} from: Zerus \u{b7} starved]\nBackend has had no task"), "[Zerus team \u{b7} from: Zerus \u{b7} starved]");
    assert_eq!(header_of("Plain text"), "");
    assert!(!doorbell(&"[Zerus team ".repeat(80), 1, 0).contains('\n'));
    assert!(doorbell(&"[Zerus team ".repeat(80), 1, 0).chars().count() <= 300);
}

#[test]
fn a_turn_gets_each_unread_message_once_within_its_bound() {
    let message = |id: &str, text: &str, urgent: bool, injected: Option<f64>| Message {
        id: id.into(), request_id: id.into(), team: "t".into(), by: "lead".into(), to: "dev".into(), kind: "message".into(),
        urgency: if urgent { "urgent" } else { "normal" }.into(), text: text.into(), at: 1.0, state: "rung".into(), rung_at: None,
        rings: 1, safe_point: String::new(), injected_at: injected, read_at: None, read_via: String::new(), read_request: String::new(), expires: 9.0,
    };
    let list = vec![message("a", "First", false, Some(5.0)), message("b", "Second", true, None), message("c", "Third", false, None)];
    let (text, ids) = turn_context(&list, false).unwrap();
    assert_eq!(ids, vec!["b".to_owned(), "c".to_owned()], "a message already in a turn is not repeated");
    assert!(text.contains("Second") && text.contains("URGENT") && text.contains("stay unread until you read them"), "{text}");
    assert_eq!(turn_context(&list, true).unwrap().1, vec!["b".to_owned()]);
    let long = vec![message("x", &"y".repeat(TURN_CONTEXT_BYTES), false, None), message("z", "after", false, None)];
    assert_eq!(turn_context(&long, false).unwrap().1, vec!["x".to_owned()], "bounded: the rest waits for the next turn");
    assert!(turn_context(&[message("a", "First", false, Some(5.0))], false).is_none());
}
