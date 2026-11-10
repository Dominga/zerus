// Task 134: hgs team current / inbox return a member's team messages and give
// the read receipt; hgs team message stores one with its urgency.
use super::support::*;
use super::*;
use crate::state::teams::working::{self, NewMessage, Urgency};

fn post(to: &str, request: &str, text: &str, urgency: Urgency) -> String {
    let mut store = Store::open(true).unwrap();
    let at = crate::state::now();
    store
        .mutate_working("rv", |tx, team, _| {
            let (message, _) = working::post(tx, "rv", &NewMessage { request_id: request, by: "lead", to, kind: "hand_out", urgency, text }, at)?;
            working::refresh(tx, team, to, at)?;
            Ok(message.id)
        })
        .unwrap()
}

#[test]
fn current_returns_the_unread_text_and_records_one_read_receipt() {
    let (team, dev_attempt, _) = review_team();
    with_review_team(&team, || {
        let id = post("dev", &dev_attempt, "[Zerus team \u{b7} from: Lead \u{b7} task: #1 \u{b7} attempt: 1]\nTask #1: API\nBuild it.", Urgency::Normal);
        // A peek shows it and marks nothing.
        let peek = run("current", &json!({"attempt":dev_attempt,"peek":true}), false).unwrap();
        assert_eq!(peek["messages"][0]["id"], id.as_str());
        assert!(peek["read"].is_null());
        assert_eq!(stored().member("dev").unwrap().unread.as_ref().unwrap().count, 1);
        // The member's own current: the text, the attempt, and the receipt.
        let request = uuid::Uuid::new_v4().to_string();
        let view = run("current", &json!({"attempt":dev_attempt,"request_id":request}), false).unwrap();
        assert_eq!(view["attempt"], dev_attempt.as_str());
        assert!(view["messages"][0]["text"].as_str().unwrap().contains("Build it."));
        assert_eq!(view["read"], json!({"read":[id],"request_id":request,"via":"current"}));
        // Task 287: the receipt changes the message rows only; the worker's
        // next tick brings the member's unread summary up to date.
        let before = stored();
        assert_eq!(before.member("dev").unwrap().unread.as_ref().map(|unread| unread.count), Some(1), "the team record is not rewritten");
        let mut store = Store::open(true).unwrap();
        crate::state::teams::dispatcher::refresh_unread(&mut store, &before).unwrap();
        assert!(stored().member("dev").unwrap().unread.is_none(), "status no longer shows it unread");
        let read = working::by_request(Store::open(false).unwrap().connection(), &dev_attempt).unwrap().unwrap();
        assert_eq!((read.state.as_str(), read.read_via.as_str(), read.read_request.as_str()), ("read", "current", request.as_str()));
        let events = Store::open(false).unwrap().events("rv", None, 50).unwrap();
        let receipt = events["items"].as_array().unwrap().iter().find(|event| event["kind"] == "message_read").expect("the receipt is journaled");
        assert_eq!(receipt["detail"]["request_id"], request.as_str());
        // Read once: the next current has nothing new and no receipt.
        let again = run("current", &json!({"attempt":dev_attempt}), false).unwrap();
        assert!(again["messages"].as_array().unwrap().is_empty() && again["read"].is_null());
    });
}

/// Task 287 (owner, 2026-11-08): marking messages read rewrote the whole team
/// record, and a mark that failed while the store was busy answered with an
/// error instead of the messages: an empty inbox. The messages are printed
/// whatever the mark does, and stay unread until a mark succeeds.
#[test]
fn an_inbox_under_a_held_lock_prints_its_messages() {
    let (team, dev_attempt, _) = review_team();
    with_review_team(&team, || {
        let id = post("dev", &dev_attempt, "Read me.", Urgency::Normal);
        let revision = stored().revision;
        // Another process holds the write lock.
        let path = crate::state::teams::store::path();
        let (held, ready) = std::sync::mpsc::channel();
        let (release, done) = std::sync::mpsc::channel::<()>();
        let holder = std::thread::spawn(move || {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch("BEGIN IMMEDIATE; UPDATE teams SET updated=updated;").unwrap();
            held.send(()).unwrap();
            let _ = done.recv_timeout(std::time::Duration::from_secs(20));
            db.execute_batch("COMMIT").unwrap();
        });
        ready.recv().unwrap();
        crate::state::teams::store::set_test_write_wait(Some(std::time::Duration::from_millis(300)));
        let view = run("inbox", &json!({"attempt":dev_attempt}), false);
        crate::state::teams::store::set_test_write_wait(None);
        release.send(()).unwrap();
        holder.join().unwrap();
        let view = view.unwrap();
        assert_eq!(view["messages"][0]["id"], id.as_str(), "the message is printed: {view}");
        assert_eq!(view["read"]["read"], json!([]), "{view}");
        assert!(!view["read"]["error"].as_str().unwrap_or_default().is_empty(), "{view}");
        assert_eq!(working::unread(Store::open(false).unwrap().connection(), "rv", "dev", crate::state::now()).unwrap().len(), 1, "still unread");
        // Once the store is free the next inbox marks it, and the team record
        // keeps its revision.
        let view = run("inbox", &json!({"attempt":dev_attempt}), false).unwrap();
        assert_eq!(view["read"]["read"], json!([id]), "{view}");
        assert_eq!(stored().revision, revision, "marking read never rewrites the team");
    });
}

#[test]
fn inbox_marks_only_the_callers_own_messages_read() {
    let (team, dev_attempt, qa_attempt) = review_team();
    with_review_team(&team, || {
        let to_dev = post("dev", "r-dev", "For dev", Urgency::Urgent);
        let to_lead = post("lead", "r-lead", "For the lead", Urgency::Normal);
        // You (the owner) see the lead's messages and mark nothing.
        let owner = call("inbox", json!({"by":"human"})).unwrap();
        assert_eq!(owner["for"], "lead");
        assert_eq!(owner["messages"][0]["id"], to_lead.as_str());
        assert!(owner["read"].is_null());
        // qa reads its inbox: nothing of dev's, and no lead items.
        let qa = run("inbox", &json!({"attempt":qa_attempt}), false).unwrap();
        assert_eq!(qa["for"], "qa");
        assert!(qa["messages"].as_array().unwrap().is_empty() && qa["items"].as_array().unwrap().is_empty());
        // dev reads its own: marked, urgent shown.
        let dev = run("inbox", &json!({"attempt":dev_attempt}), false).unwrap();
        assert_eq!((dev["messages"][0]["id"].as_str(), dev["messages"][0]["urgency"].as_str()), (Some(to_dev.as_str()), Some("urgent")));
        assert_eq!(dev["read"]["read"], json!([to_dev]));
        assert_eq!(dev["read"]["via"], "inbox");
        assert!(working::unread(Store::open(false).unwrap().connection(), "rv", "lead", crate::state::now()).unwrap().len() == 1, "the lead's stays unread");
    });
}

#[test]
fn a_caller_outside_the_team_never_peeks_at_the_leads_messages() {
    // Task 186 (11.6): an unverifiable caller with --team was read as the
    // owner and saw the lead's message texts; now it is refused.
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        post("lead", "r-lead", "For the lead only", Urgency::Normal);
        set_test_caller_identities(Some((None, Some("claude/x/outsider".into()), Some("r-outsider".into()))));
        let refused = call("inbox", json!({})).unwrap_err().to_string();
        set_test_caller_identities(Some((None, None, None)));
        assert!(refused.contains("agent session cannot act as the owner"), "{refused}");
        set_test_caller_identities(Some((None, Some("claude/x/stale".into()), None)));
        let stale = call("inbox", json!({})).unwrap_err().to_string();
        set_test_caller_identities(Some((None, None, None)));
        assert!(stale.contains("could not verify"), "{stale}");
        // The owner still peeks, marking nothing.
        let owner = call("inbox", json!({"by":"human"})).unwrap();
        assert_eq!(owner["messages"][0]["text"], "For the lead only");
        assert!(owner["read"].is_null());
    });
}

#[test]
fn a_message_is_stored_with_its_urgency_within_its_bounds() {
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        let sent = call("message", json!({"by":"human","to":"dev","text":"Pause the migration","urgent":true})).unwrap();
        assert_eq!((sent["status"].as_str(), sent["urgency"].as_str()), (Some("queued"), Some("urgent")));
        let unread = working::unread(Store::open(false).unwrap().connection(), "rv", "dev", crate::state::now()).unwrap();
        assert_eq!((unread[0].by.as_str(), unread[0].urgent(), unread[0].rings), ("owner", true, 0), "Zerus rings it at the next idle moment");
        assert_eq!(stored().member("dev").unwrap().unread.as_ref().unwrap().urgent, 1);
        // The same request applies once.
        let request = uuid::Uuid::new_v4().to_string();
        let once = json!({"by":"human","to":"lead","text":"Hello","team":"rv","request_id":request});
        run("message", &once, false).unwrap();
        run("message", &once, false).unwrap();
        assert_eq!(working::unread(Store::open(false).unwrap().connection(), "rv", "lead", crate::state::now()).unwrap().len(), 1);
        // To the team: one message per other member.
        let all = call("message", json!({"by":"human","to":"team","text":"Standup in 5"})).unwrap();
        assert_eq!(all["messages"].as_array().unwrap().len(), 3);
        // Bounds: the text, the recipient, the urgent cap.
        assert!(call("message", json!({"by":"human","to":"dev","text":"x".repeat(working::MEMBER_TEXT_LIMIT + 1)})).unwrap_err().to_string().contains("at most 2000"));
        assert!(call("message", json!({"by":"human","to":"nobody","text":"hi"})).is_err());
        assert!(call("message", json!({"by":"human","to":"dev","text":" "})).is_err());
        for n in 1..working::CAP_URGENT {
            call("message", json!({"by":"human","to":"dev","text":format!("urgent {n}"),"urgent":true})).unwrap();
        }
        let error = call("message", json!({"by":"human","to":"dev","text":"one too many","urgent":true})).unwrap_err().to_string();
        assert!(error.contains("5 unread urgent messages") && error.contains("at most 5"), "{error}");
    });
}

/// Task 173: the lead's team message naming a task the member holds is bound
/// to its attempt; a message naming no task goes through unbound. Task #30
/// (team ede724c4): a task the text only mentions is a reference and the
/// message goes with a note; only a "task" the member does not hold is refused,
/// naming hgs team reassign.
#[test]
fn a_lead_message_naming_a_task_binds_to_its_attempt_or_is_refused() {
    let (team, dev_attempt, _) = review_team();
    with_review_team(&team, || {
        as_lead(|| {
            let sent = call("message", json!({"to":"dev","text":"On #1: keep the old field name"})).unwrap();
            assert_eq!(sent["status"], "queued");
            assert!(sent.get("references").is_none(), "{sent}");
            // A mention of another member's task is a reference, not work.
            let sent = call("message", json!({"to":"qa","text":"Lead, for 3: see how #1 named it"})).unwrap();
            assert_eq!((sent["status"].as_str(), sent["references"].clone()), (Some("queued"), json!([3, 1])), "{sent}");
            assert!(sent["reference_note"].as_str().unwrap().contains("for reference only"), "{sent}");
            let sent = call("message", json!({"to":"team","text":"Everyone: #3 shows the pattern to follow"})).unwrap();
            assert_eq!(sent["status"], "queued", "a broadcast mentioning a task goes: {sent}");
            // The message's "task" naming a task the member does not hold is refused.
            let error = call("message", json!({"to":"qa","task":3,"text":"write the docs too"})).unwrap_err().to_string();
            assert!(error.starts_with("Task #3 is not handed to qa"), "{error}");
            assert!(error.contains("hgs team retry --json") && error.contains("hgs team reassign --json"), "{error}");
            assert!(call("message", json!({"to":"dev","task":"one","text":"x"})).unwrap_err().to_string().contains("task number"));
            call("message", json!({"to":"qa","text":"Standup in 5"})).unwrap();
        });
        let saved = stored();
        let attempt = saved.task(1).unwrap().attempts.iter().find(|attempt| attempt.id == dev_attempt).unwrap().clone();
        assert_eq!(attempt.lead_messages.len(), 1);
        assert_eq!(attempt.lead_messages[0]["text"], "On #1: keep the old field name");
        assert_eq!(attempt.lead_messages[0]["via"], "team message");
        assert!(saved.task(2).unwrap().attempts[0].lead_messages.is_empty());
        // The owner's own message is never checked.
        call("message", json!({"by":"human","to":"qa","text":"for 3: I changed my mind"})).unwrap();
    });
}

/// Task 161: the lead's message telling a member to wait for an outside
/// event is refused with the park; an ordinary one goes.
#[test]
fn the_lead_never_messages_a_wait() {
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        as_lead(|| {
            let error = call("message", json!({"to":"dev","text":"Keep your task and wait for the 1.58.0 install, then retest"})).unwrap_err().to_string();
            assert!(error.starts_with("Waiting is a park, never an instruction") && error.contains("hgs team park --json"), "{error}");
            call("message", json!({"to":"dev","text":"Rename the module first, then retest"})).unwrap();
        });
    });
}

/// Task 156: the owner's decide is recorded at once and binds the work; a
/// second one on the same question supersedes it; the lead cannot hand a
/// decision down as a message.
#[test]
fn a_decision_is_recorded_listed_and_never_messaged() {
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        let first = call("decide", json!({"by":"human","question":"Which HTTP client does the API use?","choice":"ureq only",
            "why":"one blocking client","tasks":[1],"forbids":["reqwest"]})).unwrap();
        assert_eq!(first["status"], "recorded");
        assert_eq!(first["decision"]["id"], "d1");
        let second = call("decide", json!({"by":"human","question":"Which HTTP client does the API use?","choice":"ureq or hyper",
            "why":"hyper for the relay","tasks":[1]})).unwrap();
        assert_eq!(second["decision"]["id"], "d2");
        let saved = stored();
        assert_eq!(saved.decisions[0].superseded_by, "d2");
        let list = call("decisions", json!({})).unwrap();
        assert_eq!(list["decisions"][0]["decision"]["id"], "d2");
        assert_eq!(list["decisions"][0]["binds"], json!([1]));
        assert_eq!(list["decisions"][1]["binds"], json!([]), "a superseded decision binds nothing");
        assert!(call("decide", json!({"by":"human","question":"","choice":"x"})).is_err());
        as_lead(|| {
            let error = call("message", json!({"to":"dev","text":"The owner decided: no reqwest anywhere"})).unwrap_err().to_string();
            assert!(error.contains("hgs team decide --json"), "{error}");
            // Task #36 (team ede724c4): a hand-off that quotes the owner and
            // the binding decisions as context is delivered.
            let sent = call("message", json!({"to":"dev","text":"Owner, 2026-11-10. Slice docs/reference.md into records, one per heading.\nDecisions that bind this task (the owner's; follow them):\n- d1 (owner, 2026-11-10): ureq or hyper"})).unwrap();
            assert_eq!(sent["status"], "queued", "{sent}");
            // Without the owner's word the lead records nothing.
            let error = call("decide", json!({"question":"Q","choice":"C"})).unwrap_err().to_string();
            assert!(error.contains("owner_word"), "{error}");
        });
    });
}
