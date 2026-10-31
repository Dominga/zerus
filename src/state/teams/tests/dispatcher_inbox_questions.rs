use super::*;

#[test]
fn a_question_reaches_the_lead_once_and_its_answer_the_member() {
    let mut team = team();
    delivered(&mut team);
    let mut events = Vec::new();
    let asked = ops::apply(
        &mut team,
        "ask",
        &json!({"text":"Must I keep the old field?"}),
        &Actor::Member("dev".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!((asked["question"].as_str(), asked["task"].as_u64()), (Some("q1"), Some(1)));
    let lead = session("claude/x/lead", "r0", true);
    let dev = session("codex/x/dev", "r1", false);
    let found = steps(&team, &[lead.clone(), dev.clone()], 20.0);
    // The question is one inbox item, not a message of its own (C2).
    assert!(found.contains(&Step::Inbox), "{found:?}");
    let items = super::inbox::items(&team);
    assert_eq!(items[0].kind, "question");
    assert!(items[0].summary.contains("Must I keep the old field?"), "{}", items[0].summary);
    assert!(items[0].command.contains("\"question\":\"q1\"") && items[0].command.contains("hgs team reply --json"));
    // The lead hears once per question.
    team.inbox.told.push(items[0].key.clone());
    team.inbox.told_at = 20.0;
    assert!(!steps(&team, &[lead.clone(), dev.clone()], 21.0).iter().any(|s| matches!(s, Step::Inbox)));
    // The answer closes the question and waits for the live member.
    ops::apply(&mut team, "reply", &json!({"question":"q1","text":"Yes, keep it."}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    let question = &team.task(1).unwrap().attempts[0].questions[0];
    assert_eq!((question.answer.as_str(), question.notice), ("Yes, keep it.", Notice::Pending));
    let found = steps(&team, &[lead.clone(), dev.clone()], 20.0);
    assert!(found.contains(&Step::AnswerQuestion { task: 1, question: "q1".into() }), "{found:?}");
    // An offline member is not messaged; a sent answer never repeats.
    let mut offline = dev.clone();
    offline.live = false;
    assert!(!steps(&team, &[lead, offline], 20.0).iter().any(|s| matches!(s, Step::AnswerQuestion { .. })));
    team.task_mut(1).unwrap().attempts[0].questions[0].notice = Notice::Sent;
    assert!(!steps(&team, &[session("claude/x/lead", "r0", true), dev], 20.0)
        .iter()
        .any(|s| matches!(s, Step::AnswerQuestion { .. })));
    // The answer message names the question, the answer and the report route.
    let task = team.task(1).unwrap();
    let text = answer_to_member_text(task, &task.attempts[0].questions[0]);
    assert!(text.contains("#1") && text.contains("answer q1") && text.contains("Yes, keep it."), "{text}");
    assert!(text.contains("hgs team report --json") && text.contains("hgs team current --json"), "{text}");
}

#[test]
fn the_lead_hears_once_when_a_member_stops_on_its_task() {
    let mut team = team();
    let lead = session("claude/x/lead", "r0", true);
    let mut gone = session("codex/x/dev", "r1", false);
    gone.live = false;
    // A member with an open attempt whose session stopped; the dispatcher
    // recorded the stop on its previous tick (L3).
    let task = team.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    let id = task.id;
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().seen = Some(MemberSeen { at: 0.5, live: false });
    let steps_now = steps(&team, &[lead.clone(), gone.clone()], 1.0);
    // The stopped member is one inbox item, not a message of its own (C2).
    assert!(steps_now.contains(&Step::Inbox), "{steps_now:?}");
    let items = super::inbox::items(&team);
    assert_eq!(items[0].kind, "stalled");
    assert!(items[0].summary.contains("dev (Backend)'s session stopped"), "{}", items[0].summary);
    assert_eq!(items[0].task, Some(id));
    team.inbox.told.push(items[0].key.clone());
    team.inbox.told_at = 1.0;
    assert!(!steps(&team, &[lead, gone], 2.0).iter().any(|s| matches!(s, Step::Inbox)));
}

#[test]
fn a_lead_task_goes_to_the_lead_without_a_message_or_an_idle_lead() {
    let mut team = team();
    team.tasks.push(Task { id: 4, title: "Plan review".into(), assignee: "lead".into(), status: TaskStatus::Pending, ..Default::default() });
    // The lead is busy with its members: its own task still reaches it,
    // because no message is sent (C1).
    let lead = session("claude/x/lead", "r0", false);
    let found = steps(&team, std::slice::from_ref(&lead), 10.0);
    assert!(found.contains(&Step::HandOut { task: 4, member: "lead".into() }), "{found:?}");
    // A busy lead's task is listed in the inbox once the attempt exists.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    lead_hand_out(&mut store, &team.id, 4).unwrap();
    let saved = store.load(&team.id).unwrap().unwrap();
    let task = saved.task(4).unwrap();
    let attempt = task.attempts.last().unwrap();
    assert_eq!((attempt.member.as_str(), task.status), ("lead", TaskStatus::InProgress));
    // The lead works when it suits: started, never a start warning.
    assert!(attempt.started_at.is_some() && !attempt.start_warned);
    let items = super::inbox::items(&saved);
    assert!(items.iter().any(|item| item.kind == "lead_task" && item.task == Some(4)), "{items:?}");
    // The hand-out is recorded as Zerus's, without a message (C1).
    let events = store.events(&team.id, None, 10).unwrap();
    assert!(
        events["items"].as_array().unwrap().iter().any(|event| event["kind"] == "handed_out" && event["detail"]["no_message"] == true),
        "{events}"
    );
}

#[test]
fn an_undecided_report_reminds_the_lead_once_after_the_configured_minutes() {
    let mut team = team();
    {
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt {
            n: 1,
            id: "A1".into(),
            member: "dev".into(),
            state: AttemptState::Reported,
            report: Some(Report { text: "Done and checked".into(), at: 1000.0, ..Default::default() }),
            ..Default::default()
        });
        task.status = TaskStatus::Submitted;
    }
    let lead = session("claude/x/lead", "r0", true);
    let dev = session("codex/x/dev", "r1", false);
    let after = team.settings.remind_lead_minutes as f64 * 60.0;
    // The default is 15 minutes; not a second earlier.
    assert_eq!(after, 900.0);
    assert!(!steps(&team, &[lead.clone(), dev.clone()], 1000.0 + after - 1.0).iter().any(|s| matches!(s, Step::RemindReport { .. })));
    assert!(steps(&team, &[lead.clone(), dev.clone()], 1000.0 + after).iter().any(|s| matches!(s, Step::RemindReport { task: 1 })));
    // Once: the attempt records the reminder.
    team.task_mut(1).unwrap().attempts.last_mut().unwrap().reminded_at = Some(1000.0 + after);
    assert!(!steps(&team, &[lead.clone(), dev.clone()], 1000.0 + after + 60.0).iter().any(|s| matches!(s, Step::RemindReport { .. })));
    // 0 turns the reminder off.
    team.settings.remind_lead_minutes = 0;
    team.task_mut(1).unwrap().attempts.last_mut().unwrap().reminded_at = None;
    assert!(!steps(&team, &[lead, dev], 100_000.0).iter().any(|s| matches!(s, Step::RemindReport { .. })));
}

#[test]
fn members_replies_are_muted_for_the_owner_once_per_conversation() {
    let mut team = team();
    let mut dev = session("codex/x/dev", "r1", true);
    dev.notify_revision = Some(3);
    let mut lead = session("claude/x/lead", "r0", true);
    lead.notify_revision = Some(0);
    let quiet = |steps: &[Step]| steps.iter().filter_map(|s| match s { Step::Quiet { member, revision, muted, .. } => Some((member.clone(), *revision, *muted)), _ => None }).collect::<Vec<_>>();
    // The member's replies are for the lead; the lead's are for you.
    assert_eq!(quiet(&steps(&team, &[lead.clone(), dev.clone()], 1.0)), [("dev".to_owned(), 3, false)]);
    // Once muted for this conversation, never again: you may unmute it.
    team.members.iter_mut().find(|m| m.id == "dev").unwrap().quieted = "c".into();
    assert!(quiet(&steps(&team, &[lead.clone(), dev.clone()], 1.0)).is_empty());
    // A new conversation is muted again; a host that does not know the
    // preference is left alone.
    dev.conversation_id = "c2".into();
    assert_eq!(quiet(&steps(&team, &[lead.clone(), dev.clone()], 1.0)).len(), 1);
    dev.notify_revision = None;
    assert!(quiet(&steps(&team, &[lead, dev], 1.0)).is_empty());
}

#[test]
fn a_members_permission_request_is_read_into_the_inbox() {
    let mut team = team();
    let mut asking = session("codex/x/dev", "r1", false);
    asking.asking = true;
    let lead = session("claude/x/lead", "r0", true);
    let steps_now = steps(&team, &[lead.clone(), asking.clone()], 1.0);
    // Read once, whether the lead is free or not: the inbox message waits.
    assert!(steps_now.contains(&Step::Permission { member: "dev".into() }), "{steps_now:?}");
    let busy = session("claude/x/lead", "r0", false);
    assert!(steps(&team, &[busy, asking.clone()], 1.0).iter().any(|s| matches!(s, Step::Permission { .. })));
    // While the stored request is fresh, no inspect runs again.
    team.members.iter_mut().find(|m| m.id == "dev").unwrap().pending = Some(PendingQuestion {
        question_id: "q1".into(),
        question_hash: "h1".into(),
        approval: true,
        at: now(),
        card: json!({"questions":[{"id":"0","header":"Access","question":"How should I proceed?"}]}),
    });
    assert!(!steps(&team, &[lead.clone(), asking.clone()], now()).iter().any(|s| matches!(s, Step::Permission { .. })));
    // Status carries the hash hgs team answer expects with the request.
    let record = permission_record(team.members.iter().find(|m| m.id == "dev").unwrap().pending.as_ref().unwrap());
    assert_eq!((record["question_id"].as_str(), record["question_hash"].as_str()), (Some("q1"), Some("h1")), "{record}");
    assert_eq!(record["tool"], "Access");
    // A member that stopped asking clears the stored request.
    let mut working = session("codex/x/dev", "r1", false);
    working.phase = "working".into();
    assert!(steps(&team, &[lead.clone(), working], now()).contains(&Step::ClearPermission { member: "dev".into() }));
    // DeepSeek reports questions and permission requests as input; the others say which.
    let listing = json!([{"sessions":[{"name":"dsh/x/a","state":"running","activity":"attention","phase":"input"},
        {"name":"codex/x/b","process_state":"running","activity":"idle","phase":"question"},
        {"name":"codex/x/c","process_state":"running","activity":"idle","phase":"approval"}]}]);
    let asked: Vec<bool> = seen(&listing).iter().map(|s| s.asking).collect();
    assert_eq!(asked, [true, true, true]);
}

#[test]
fn a_permission_record_carries_the_native_answer_ids() {
    // C4: Claude's approval card: question id "approval", numbered options;
    // the deny label starts with "No" (the same rule as commands::denies).
    let claude = json!({"question_id":"claude-approval:h","question_hash":"h","approval":true,
        "questions":[{"id":"approval","header":"Bash","question":"Bash command","body":"npm test","allow_other":false,
            "options":[{"id":"1","label":"Yes"},{"id":"2","label":"No, and tell Claude what to do differently (esc)"}]}]});
    let pending = PendingQuestion {
        question_id: "claude-approval:h".into(),
        question_hash: "h".into(),
        approval: true,
        at: 10.0,
        card: claude.clone(),
    };
    let record = permission_record(&pending);
    assert_eq!(record["answer"], json!({"question_id":"approval","allow":"1","deny":"2"}), "{record}");
    // hgs team answer forwards these exact ids to the native answer.
    assert_eq!(record["answer"]["question_id"], claude["questions"][0]["id"]);
    for key in ["allow", "deny"] {
        let id = record["answer"][key].as_str().unwrap();
        assert!(
            claude["questions"][0]["options"].as_array().unwrap().iter().any(|option| option["id"] == id),
            "{key} {id}"
        );
    }
    // The answers hgs team answer forwards name the card question and an
    // option of that question, so the native answer accepts them.
    let answers = json!([{"question_id":record["answer"]["question_id"],
        "selected_option_ids":[record["answer"]["allow"]]}]);
    assert_eq!(answers[0]["question_id"], claude["questions"][0]["id"]);
    assert!(answers[0]["selected_option_ids"][0].is_string());
    // DeepSeek's card uses its own ids: the native host names them allow/deny.
    let deepseek = json!({"question_id":"q9","question_hash":"h9","approval":true,
        "questions":[{"id":"decision","header":"Permission","question":"Allow the command?",
            "options":[{"id":"allow","label":"Allow"},{"id":"deny","label":"Deny"}]}]});
    let pending = PendingQuestion {
        question_id: "q9".into(),
        question_hash: "h9".into(),
        approval: true,
        at: 20.0,
        card: deepseek,
    };
    assert_eq!(permission_record(&pending)["answer"], json!({"question_id":"decision","allow":"allow","deny":"deny"}));
    // Claude's outside-read card lists broader options first; only the
    // one-time ones answer, never a persistent setting.
    let outside = json!({"questions":[{"id":"approval","options":[
        {"id":"1","label":"Yes, and keep allowing any reads outside the working directories","scope":"broader"},
        {"id":"2","label":"No, and block reads outside the working directories from now on","scope":"broader"},
        {"id":"3","label":"No, and ask again next time","scope":"once"},
        {"id":"4","label":"Yes, but ask again next time","scope":"once"}]}]});
    assert_eq!(permission_answer(&outside), Some(json!({"question_id":"approval","allow":"4","deny":"3"})));
    // A card whose options are all broader offers no answer.
    let broader = json!({"questions":[{"id":"approval","options":[
        {"id":"1","label":"Yes, and always allow","scope":"broader"}]}]});
    assert_eq!(permission_answer(&broader), None);
    // A card without options has no answer shape to offer.
    let pending = PendingQuestion {
        question_id: "q0".into(),
        question_hash: "h0".into(),
        approval: false,
        at: 30.0,
        card: json!({"questions":[{"id":"0","header":"Access","question":"How should I proceed?"}]}),
    };
    assert!(permission_record(&pending).get("answer").is_none());
}

#[test]
fn the_desktop_fixture_is_a_real_status_question_with_the_native_answer() {
    // Saved from a real `hgs team status --json` call (the member question
    // of a pending Claude Bash approval), for the desktop test.
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../tray/tests/fixtures/hgs-team-member-question.json"
    ))
    .expect("the desktop fixture parses");
    assert_eq!(fixture["tool"], "Bash");
    assert_eq!(fixture["command"], "cargo test --locked");
    assert_eq!(fixture["command_source"], "tool_input");
    assert_eq!(fixture["answer"], json!({"question_id":"approval","allow":"1","deny":"2"}));
    // The same native card produces exactly that answer shape.
    let card = json!({"tool_input":{"command":"cargo test --locked"},
        "questions":[{"id":"approval","header":"Bash","body":"cargo test --locked",
            "options":[{"id":"1","label":"Yes"},{"id":"2","label":"No, and tell Claude what to do differently (esc)"}]}]});
    assert_eq!(permission_answer(&card), Some(fixture["answer"].clone()));
}

#[test]
fn a_permission_names_its_command_and_source() {
    let team = team();
    let member = team.member("qa").unwrap();
    let bash = json!({"approval":true,"questions":[{"id":"approval","header":"Bash","question":"Allow?","body":"Bash asks"}],
        "tool_input":{"command":"rm -rf build"}});
    assert_eq!(permission_command(&bash), (Some("rm -rf build".to_owned()), "tool_input"));
    let text = question_text(&team, member, &bash);
    assert!(text.contains("Command (from the tool input):\nrm -rf build"), "{text}");
    let patch = json!({"approval":true,"tool_input":{"patch":"*** Begin Patch\n*** Update File: src/a.rs\n*** End Patch"}});
    assert_eq!(permission_command(&patch), (Some("src/a.rs".to_owned()), "tool_input"));
    let dsh = json!({"approval":true,"command":"cargo build"});
    assert_eq!(permission_command(&dsh), (Some("cargo build".to_owned()), "process"));
    let unknown = json!({"approval":true});
    assert_eq!(permission_command(&unknown), (None, "unknown"));
    assert!(question_text(&team, member, &unknown).contains("command not shown by the agent - read it with hgs inspect"));
    // B2: the member's message names the tool and the reason.
    assert_eq!(denial_text("Bash", "build only"), "[Zerus team \u{b7} from: Zerus \u{b7} permission denied]\nLead: denied Bash: build only");
}

#[test]
fn a_vanished_question_is_recorded_as_unknown() {
    let mut answers = Vec::new();
    for index in 0..25 {
        answers.push(json!({"question_id":format!("q{index}"),"decision":"allow"}));
    }
    let answer = permission_lost(&mut answers, "q25", 30.0);
    assert_eq!(
        (answer["decision"].as_str(), answer["by"].as_str(), answer["at"].as_f64()),
        (Some("unknown"), Some("unknown"), Some(30.0))
    );
    assert_eq!(answers.len(), 20);
    assert_eq!(answers.last().unwrap()["decision"], "unknown");
    assert_eq!(answers.last().unwrap()["question_id"], "q25");
}
