//! Task 97 (owner, 2026-11-06): the owner's question queue, with fake native
//! prompts: a request left past `defer_minutes` is denied once and queued; an
//! unknown delivery is never answered again; the owner's later answer is one
//! guarded message, never a replayed tool; ordinary sessions only on request.
use super::super::owner_queue;
use super::support::*;
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

const MINUTE: f64 = 60.0;

fn approval_card() -> Value {
    json!({"question_id":"q1","question_hash":"h".repeat(64),"approval":true,"source":"claude_tool_approval","tool":"Bash",
        "tool_input":{"command":"cargo test --locked"},
        "questions":[{"id":"perm","header":"Bash","question":"Run cargo test --locked?","options":[
            {"id":"yes","label":"Yes","scope":"once"},{"id":"always","label":"Yes, and don't ask again","scope":"session"},
            {"id":"no","label":"No"}]}]})
}

fn question_card(source: &str) -> Value {
    json!({"question_id":"q2","question_hash":"k".repeat(64),"source":source,
        "questions":[{"id":"which","header":"Plan","question":"Which storage should I use?","options":[
            {"id":"a","label":"SQLite"},{"id":"b","label":"Postgres (Recommended)"}]}]})
}

/// The team with dev's approval asked at `at`.
fn asking(at: f64) -> Team {
    let mut t = team();
    t.status = TeamStatus::Running;
    let dev = t.members.iter_mut().find(|member| member.id == "dev").unwrap();
    let card = approval_card();
    dev.pending = Some(PendingQuestion { question_id: "q1".into(), question_hash: card["question_hash"].as_str().unwrap().into(),
        approval: true, at, card });
    t
}

/// Every hgs call the fake answered: (command, input).
type Calls = Rc<RefCell<Vec<(String, Value)>>>;

/// A fake hgs: `answer` exits with `answer_code` (stderr `answer_error`),
/// `send` is submitted.
fn fake(answer_code: i32, answer_error: &'static str) -> Calls {
    let calls: Calls = Rc::default();
    let seen = calls.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
        seen.borrow_mut().push((args[0].to_owned(), input.cloned().unwrap_or_default()));
        Ok(match args[0] {
            "answer" if answer_code == 0 => fake_output(0, r#"{"status":"answered"}"#, ""),
            "answer" => fake_output(answer_code, "", answer_error),
            "send" => fake_output(0, r#"{"status":"submitted"}"#, ""),
            other => panic!("unexpected hgs {other}"),
        })
    })));
    calls
}

fn commands(calls: &Calls) -> Vec<String> {
    calls.borrow().iter().map(|(command, _)| command.clone()).collect()
}

fn defer_steps(team: &Team, now: f64) -> Vec<(String, String)> {
    steps(team, &[], now)
        .into_iter()
        .filter_map(|step| match step {
            Step::Defer { member, question } => Some((member, question)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_safe_choice_is_deny_or_the_agents_own_option_and_never_allow() {
    let (answers, chosen) = owner_queue::safe_answer(&approval_card()).unwrap();
    assert_eq!(chosen, "deny");
    assert_eq!(answers, json!([{"question_id":"perm","selected_option_ids":["no"]}]));
    // A permission without a one-time Deny is left for the owner.
    let mut no_deny = approval_card();
    no_deny["questions"][0]["options"] = json!([{"id":"yes","label":"Yes","scope":"once"}]);
    assert!(owner_queue::safe_answer(&no_deny).is_none());
    // An agent's question takes its recommended option, else its first.
    let (answers, chosen) = owner_queue::safe_answer(&question_card("hook")).unwrap();
    assert_eq!((answers[0]["selected_option_ids"].clone(), chosen.as_str()), (json!(["b"]), "Postgres (Recommended)"));
    let mut plain = question_card("kimi_wire");
    plain["questions"][0]["options"][1]["label"] = json!("Postgres");
    assert_eq!(owner_queue::safe_answer(&plain).unwrap().0[0]["selected_option_ids"], json!(["a"]));
    // An optional Codex question is skipped.
    let mut optional = question_card("codex_async");
    optional["optional"] = json!(true);
    assert_eq!(owner_queue::safe_answer(&optional).unwrap().1, "skip");
    // Folder trust, updates, resumes and Terminal-only requests stay on screen.
    assert!(owner_queue::safe_answer(&question_card("claude_folder_trust")).is_none());
    let mut terminal = approval_card();
    terminal["can_answer"] = json!(false);
    assert!(owner_queue::safe_answer(&terminal).is_none());
    assert_eq!(owner_queue::request_text(&approval_card()).0, "Bash: cargo test --locked");
}

#[test]
fn a_request_unanswered_past_the_minutes_is_denied_once_and_queued() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut store = Store::open_at(&root.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = asking(1000.0);
        stored(&mut store, &t);
        assert_eq!(t.settings.defer_minutes, 10);
        // Within the minutes, or with the setting off, nothing happens.
        assert!(defer_steps(&t, 1000.0 + 9.0 * MINUTE).is_empty());
        let mut off = t.clone();
        off.settings.defer_minutes = 0;
        assert!(defer_steps(&off, 1000.0 + 60.0 * MINUTE).is_empty());
        let found = defer_steps(&t, 1000.0 + 11.0 * MINUTE);
        assert_eq!(found, vec![("dev".to_owned(), format!("q1:{}", "h".repeat(64)))]);
        let calls = fake(0, "");
        run_step(&mut store, &t, &Step::Defer { member: found[0].0.clone(), question: found[0].1.clone() }, "").unwrap();
        // One Deny through hgs answer with the exact identities, then the away message.
        assert_eq!(commands(&calls), ["answer", "send"]);
        let answer = calls.borrow()[0].1.clone();
        assert_eq!((answer["expected_run_id"].clone(), answer["question_id"].clone()), (json!("r1"), json!("q1")));
        assert_eq!(answer["answers"], json!([{"question_id":"perm","selected_option_ids":["no"]}]));
        assert_eq!(calls.borrow()[1].1["text"], owner_queue::AWAY_MESSAGE);
        let entry = owner_queue::entries().pop().unwrap();
        assert_eq!((entry.kind.as_str(), entry.safe.as_str(), entry.delivery.as_str(), entry.notice.as_str(), entry.state.as_str()),
            ("permission", "deny", "answered", "sent", "waiting"));
        assert_eq!((entry.text.as_str(), entry.session.as_str(), entry.role.as_str(), entry.asked_at), ("Bash: cargo test --locked", "codex/x/dev", "Backend", 1000.0));
        let after = store.load(&t.id).unwrap().unwrap();
        let dev = after.member("dev").unwrap();
        assert!(dev.pending.is_none());
        assert_eq!(dev.permission_answers.last().unwrap()["by"], "owner queue");
        assert!(event_kinds(&store, &t.id).contains(&"request_deferred".to_owned()));
        // Waiting for you > Deferred lists it, and the lead's row carries its phone alert.
        let items = owner_queue::waiting_items();
        assert_eq!((items[0]["kind"].clone(), items[0]["action"].clone()), (json!("deferred"), json!("answer_deferred")));
        assert!(items[0]["title"].as_str().unwrap().contains("Bash: cargo test --locked"), "{}", items[0]);
        let phone = owner_queue::mobile_items(&[(t.id.clone(), String::new(), "claude/x/lead".into())]);
        assert_eq!((phone[0].1.as_str(), phone[0].2["kind"].as_str()), ("claude/x/lead", Some("owner_deferred")));
        // The same request again (a stale tick) is never answered twice.
        assert!(run_step(&mut store, &t, &Step::Defer { member: "dev".into(), question: found[0].1.clone() }, "").is_err());
        assert_eq!(commands(&calls), ["answer", "send"]);
        set_fake_hgs(None);
    });
}

#[test]
fn an_answer_whose_delivery_is_unknown_is_never_sent_again() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut store = Store::open_at(&root.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = asking(0.0);
        stored(&mut store, &t);
        let calls = fake(2, "delivery uncertain: the Terminal did not confirm");
        let key = format!("q1:{}", "h".repeat(64));
        run_step(&mut store, &t, &Step::Defer { member: "dev".into(), question: key.clone() }, "").unwrap();
        assert_eq!(commands(&calls), ["answer"], "no away message after an unknown answer");
        let entry = owner_queue::entries().pop().unwrap();
        assert_eq!((entry.delivery.as_str(), entry.state.as_str()), ("uncertain", "waiting"));
        // The request may still be on screen: it stays the team's, but no tick
        // answers it again.
        let after = store.load(&t.id).unwrap().unwrap();
        assert!(after.member("dev").unwrap().pending.is_some());
        assert!(after.member("dev").unwrap().deferred.contains(&key));
        assert!(defer_steps(&after, 100.0 * MINUTE).is_empty());
        run_step(&mut store, &after, &Step::Defer { member: "dev".into(), question: key }, "").ok();
        assert_eq!(commands(&calls), ["answer"]);
        set_fake_hgs(None);
    });
}

#[test]
fn the_owners_later_answer_is_one_message_never_a_replayed_tool() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let mut store = Store::open_at(&root.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = asking(0.0);
        stored(&mut store, &t);
        let calls = fake(0, "");
        run_step(&mut store, &t, &Step::Defer { member: "dev".into(), question: format!("q1:{}", "h".repeat(64)) }, "").unwrap();
        calls.borrow_mut().clear();
        let id = owner_queue::entries()[0].id.clone();
        // An agent session cannot answer for the owner.
        assert!(owner_queue::answer(&json!({"id":id,"decision":"allow"})).is_err());
        let receipt = owner_queue::answer(&json!({"id":id,"decision":"allow","by":"human"})).unwrap();
        assert_eq!(receipt["status"], "sent");
        assert_eq!(commands(&calls), ["send"], "a message, never an answer or a tool");
        assert_eq!(calls.borrow()[0].1["text"], "The owner now allows Bash: cargo test --locked: you may run it again.");
        assert_eq!(calls.borrow()[0].1["expected_run_id"], "r1");
        let entry = owner_queue::entries().pop().unwrap();
        assert_eq!((entry.state.as_str(), entry.answer.as_ref().unwrap().message.as_str()), ("answered", "sent"));
        assert!(owner_queue::waiting_items().is_empty());
        // Answered once: a second answer sends nothing.
        assert!(owner_queue::answer(&json!({"id":id,"decision":"deny","by":"human"})).is_err());
        assert_eq!(commands(&calls), ["send"]);
        // A permission takes allow or deny; a question takes the owner's words.
        assert!(owner_queue::answer_message(&entry, "deny", "").starts_with("The owner denies Bash: cargo test --locked"));
        set_fake_hgs(None);
    });
}

#[test]
fn a_refused_message_may_be_answered_again_an_unknown_one_never() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let session = SessionRef { host: String::new(), name: "claude/p/solo".into(), run_id: "r9".into(), conversation_id: "c9".into() };
        let pending = PendingQuestion { question_id: "q2".into(), question_hash: "k".repeat(64), approval: false, at: 0.0, card: question_card("hook") };
        let calls = fake(0, "");
        let entry = owner_queue::defer(&owner_queue::Origin { session: &session, team: None, member: "" }, &pending, 700.0).unwrap().unwrap();
        assert_eq!((entry.kind.as_str(), entry.safe.as_str()), ("question", "Postgres (Recommended)"));
        let outcomes = Rc::new(RefCell::new(vec![(1, "the session is busy"), (2, "delivery uncertain")]));
        let sends = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
            sends.borrow_mut().push((args[0].to_owned(), input.cloned().unwrap_or_default()));
            let (code, error) = outcomes.borrow_mut().remove(0);
            Ok(fake_output(code, "", error))
        })));
        calls.borrow_mut().clear();
        // Refused before it reached the session: nothing was sent, answer again.
        let refused = owner_queue::answer(&json!({"id":entry.id,"decision":"answer","text":"Use SQLite","by":"human"}));
        assert!(refused.unwrap_err().to_string().contains("Nothing was sent"));
        assert!(owner_queue::entries()[0].answer.is_none());
        // May have arrived: answered, and never sent again.
        let unknown = owner_queue::answer(&json!({"id":entry.id,"decision":"answer","text":"Use SQLite","by":"human"})).unwrap();
        assert_eq!(unknown["status"], "uncertain");
        assert_eq!(calls.borrow()[1].1["text"], "The owner answers your question (Which storage should I use?): Use SQLite");
        assert!(owner_queue::answer(&json!({"id":entry.id,"decision":"answer","text":"Use SQLite","by":"human"})).is_err());
        assert_eq!(commands(&calls), ["send", "send"]);
        set_fake_hgs(None);
    });
}

#[test]
fn an_ordinary_session_is_deferred_only_when_the_owner_turns_it_on() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let solo = Seen { asking: true, phase: "approval".into(), ..session("claude/p/solo", "r9", false) };
        // Off by default: the worker never even reads it.
        assert!(owner_queue::watched().is_empty());
        let calls = fake(0, "");
        defer_watched(&[], std::slice::from_ref(&solo));
        assert!(commands(&calls).is_empty());
        // Only the owner turns it on.
        assert!(owner_queue::watch(&json!({"session":"claude/p/solo","minutes":10})).is_err());
        owner_queue::watch(&json!({"session":"claude/p/solo","minutes":10,"by":"human"})).unwrap();
        assert_eq!(owner_queue::minutes_for(&owner_queue::watched(), "", "claude/p/solo"), Some(10));
        // First seen now; deferred once past its minutes.
        let session = SessionRef { host: String::new(), name: "claude/p/solo".into(), run_id: "r9".into(), conversation_id: "c".into() };
        let cards = [approval_card()];
        assert!(owner_queue::tick_session(&session, 10, &cards, 5000.0).unwrap().is_empty());
        assert!(owner_queue::tick_session(&session, 10, &cards, 5000.0 + 9.0 * MINUTE).unwrap().is_empty());
        let deferred = owner_queue::tick_session(&session, 10, &cards, 5000.0 + 10.0 * MINUTE).unwrap();
        assert_eq!((deferred.len(), deferred[0].asked_at, deferred[0].team.as_str()), (1, 5000.0, ""));
        assert!(owner_queue::tick_session(&session, 10, &cards, 5000.0 + 30.0 * MINUTE).unwrap().is_empty());
        assert_eq!(commands(&calls), ["answer", "send"]);
        // Its phone alert is on its own row.
        assert_eq!(owner_queue::mobile_items(&[])[0].1, "claude/p/solo");
        // Off again.
        owner_queue::watch(&json!({"session":"claude/p/solo","minutes":0,"by":"human"})).unwrap();
        assert!(owner_queue::watched().is_empty());
        set_fake_hgs(None);
    });
}

#[test]
fn only_the_owner_sets_the_defer_minutes() {
    let mut t = asking(0.0);
    let mut events = Vec::new();
    let refused = ops::apply(&mut t, "settings", &json!({"settings":{"defer_minutes":0}}), &ops::Actor::Member("lead".into()), &BTreeSet::new(), &mut events);
    assert!(refused.unwrap_err().to_string().contains("defer_minutes"));
    ops::apply(&mut t, "settings", &json!({"settings":{"defer_minutes":30}}), &ops::Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(t.settings.defer_minutes, 30);
}
