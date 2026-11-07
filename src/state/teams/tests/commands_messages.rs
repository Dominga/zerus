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

/// Task 173: the lead's team message naming a task goes only to the member
/// that holds it, bound to its attempt; anything else is refused with the
/// hand-out, and a message naming no task goes through unbound.
#[test]
fn a_lead_message_naming_a_task_binds_to_its_attempt_or_is_refused() {
    let (team, dev_attempt, _) = review_team();
    with_review_team(&team, || {
        as_lead(|| {
            let sent = call("message", json!({"to":"dev","text":"On #1: keep the old field name"})).unwrap();
            assert_eq!(sent["status"], "queued");
            let error = call("message", json!({"to":"qa","text":"Lead, for 3: write the docs too"})).unwrap_err().to_string();
            assert!(error.starts_with("Task #3 is not handed to qa"), "{error}");
            assert!(error.contains("hgs team retry --json"), "{error}");
            let error = call("message", json!({"to":"team","text":"Everyone help with #1"})).unwrap_err().to_string();
            assert!(error.contains("Task #1 is not handed to qa"), "a broadcast naming a task is refused: {error}");
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
