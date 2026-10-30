use super::*;
use super::support::*;

#[test]
fn a_legacy_all_decided_team_hears_once_on_resume() {
    let mut team = running_team();
    for task in &mut team.tasks {
        task.status = TaskStatus::Cancelled;
    }
    team.status = TeamStatus::Paused;
    team.idle = Notice::None;
    let mut events = Vec::new();
    let none = BTreeSet::new();
    apply(&mut team, "resume", &json!({}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((team.status, team.idle), (TeamStatus::Running, Notice::Pending));
    assert_eq!(events.last().unwrap().kind, "all_decided");
    // An already sent (or uncertain) message is never armed again.
    team.idle = Notice::Sent;
    apply(
        &mut team,
        "add-member",
        &json!({"member":{"id":"docs","role":"Writer","agent":"codex"}}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(team.idle, Notice::Sent);
    assert_eq!(events.iter().filter(|event| event.kind == "all_decided").count(), 1);
}

#[test]
fn finish_ends_a_decided_team_with_a_bounded_summary() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let error = apply(&mut team, "finish", &json!({"summary":"Done"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "2 task(s) are not decided: decide or cancel them first");
    for id in [1, 2] {
        apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
    }
    let error = apply(&mut team, "finish", &json!({"summary":"Done"}), &Actor::Member("dev".into()), &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Only the lead or you finish the team");
    let error = apply(&mut team, "finish", &json!({"summary":"  "}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "A summary is required");
    let error = apply(&mut team, "finish", &json!({"summary":"x".repeat(4001)}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "A summary is too long");
    let done = apply(&mut team, "finish", &json!({"summary":"  Shipped  "}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(done["status"], "finished");
    assert_eq!((team.status, team.idle), (TeamStatus::Done, Notice::None));
    let finish = team.finish.as_ref().unwrap();
    assert_eq!((finish.by.as_str(), finish.summary.as_str()), ("Lead", "Shipped"));
    assert_eq!(events.last().unwrap().kind, "finished");
    assert_eq!(events.last().unwrap().payload["accepted"], 0);
    assert_eq!(events.last().unwrap().payload["tasks"], 2);
    assert_eq!(events.last().unwrap().payload["summary_chars"], 7);
    // Done is final: neither finish nor stop works again.
    let error = apply(&mut team, "finish", &json!({"summary":"Again"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "The team is not running");
    assert!(apply(&mut team, "stop", &json!({}), &Actor::Human, &none, &mut events).is_err());
}

#[test]
fn you_finish_a_paused_decided_team() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    for id in [1, 2] {
        apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
    }
    apply(&mut team, "pause", &json!({}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.status, TeamStatus::Paused);
    let done = apply(&mut team, "finish", &json!({"summary":"Stopped by me"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(done["status"], "finished");
    assert_eq!(team.finish.unwrap().by, "You");
}

#[test]
fn an_unconfirmed_hand_off_is_settled_only_by_you() {
    let mut team = running_team();
    hand_out(&mut team, 1);
    team.task_mut(1).unwrap().attempts[0].state = AttemptState::Unconfirmed;
    team.task_mut(1).unwrap().status = TaskStatus::Assigned;
    let mut events = Vec::new();
    assert!(check(&mut team, &json!({"task":1,"outcome":"delivered"}), &Actor::Member("lead".into()), &mut events).is_err());
    check(&mut team, &json!({"task":1,"outcome":"not_delivered"}), &Actor::Human, &mut events).unwrap();
    let task = team.task(1).unwrap();
    assert_eq!((task.status, task.attempts[0].state), (TaskStatus::Pending, AttemptState::Refused));
}

#[test]
fn stop_cancels_open_tasks_and_archive_needs_a_stopped_team() {
    let mut team = running_team();
    let mut events = Vec::new();
    assert!(apply(&mut team, "archive", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).is_err());
    hand_out(&mut team, 1);
    apply(&mut team, "stop", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(team.tasks.iter().all(|t| t.status == TaskStatus::Cancelled));
    assert_eq!(team.task(1).unwrap().attempts[0].state, AttemptState::Revoked);
    apply(&mut team, "archive", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.status, TeamStatus::Archived);
}

#[test]
fn a_finished_team_holds_and_releases_nothing() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    team.status = TeamStatus::Done;
    let error = apply(&mut team, "hold", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Only a planning, running or paused team holds tasks");
    let error = apply(&mut team, "release", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert_eq!(error, "Only a planning, running or paused team releases tasks");
    // A paused team still holds and releases (L1).
    team.status = TeamStatus::Paused;
    apply(&mut team, "hold", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap();
    apply(&mut team, "release", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap();
}

#[test]
fn role_and_task_text_normalises_crlf_and_names_a_control_character() {
    // CRLF and a lone CR become LF.
    let task = parse_task(&json!({"id":1,"title":"A\r\nB","description":"C\rD","criteria":["E\r\nF"],"assignee":"dev"})).unwrap();
    assert_eq!((task.title.as_str(), task.description.as_str(), task.criteria[0].as_str()), ("A\nB", "C\nD", "E\nF"));
    let member = parse_member(&json!({"id":"dev","role":"Back\r\nend","agent":"codex"})).unwrap();
    assert_eq!(member.role, "Back\nend");
    // A refused control character names the field and its position.
    let error = parse_task(&json!({"id":1,"title":"A\u{7}B","assignee":"dev"})).unwrap_err().to_string();
    assert_eq!(error, "A task title has a control character at position 2");
    let error = parse_task(&json!({"id":1,"title":"A","description":"B\u{7}","assignee":"dev"})).unwrap_err().to_string();
    assert_eq!(error, "A task description has a control character at position 2");
    let error = parse_member(&json!({"id":"dev","role":"B\u{7}","agent":"codex"})).unwrap_err().to_string();
    assert_eq!(error, "A role has a control character at position 2");
    let error = parse_member(&json!({"id":"dev","role":"Backend","agent":"codex","prompt":"P\u{7}"})).unwrap_err().to_string();
    assert_eq!(error, "A role prompt has a control character at position 2");
}
