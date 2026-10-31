use super::*;
use super::support::*;

#[test]
fn the_hand_out_says_how_to_read_a_run_result() {
    // N4: a new UUID per run, and asking again with the same one only reads.
    let t = team();
    let task = t.tasks.iter().find(|task| task.assignee == "dev").unwrap().clone();
    let attempt = Attempt { n: 1, id: "A1".into(), member: "dev".into(), ..Default::default() };
    let text = hand_off_text(&t, &task, &attempt, "", "");
    assert!(text.contains("a new request_id for every run"), "{text}");
    assert!(text.contains("Ask again with the same request_id to read the result"), "{text}");
    assert!(text.contains("it never runs the command twice"), "{text}");
}

#[test]
fn a_queued_ask_from_the_members_folder_reaches_the_lead() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    let request = json!({"op":"ask","request_id":"66666666-6666-4666-8666-666666666666",
        "team":t.id,"member":"dev","attempt":"A1",
        "input":{"request_id":"66666666-6666-4666-8666-666666666666","team":t.id,"task":id,"attempt":"A1","text":"Which test suite first?"}});
    super::super::outbox::push(&folder, &request).unwrap();
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member]).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let attempt = saved.task(id).unwrap().attempts.last().unwrap();
    assert_eq!(attempt.questions.iter().filter(|question| question.open()).count(), 1, "{:?}", attempt.questions);
    let result = super::super::outbox::result(&folder, "66666666-6666-4666-8666-666666666666").unwrap();
    assert!(result.get("error").is_none() || result["status"] != "refused", "{result}");
}

#[test]
fn a_queued_report_applied_twice_is_one_report() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    let task = t.tasks.iter_mut().find(|t| t.assignee == "dev").unwrap();
    task.attempts.push(Attempt { n: 1, id: "A1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    let id = task.id;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
    let folder = dir.path().join("work");
    let request = json!({"op":"report","request_id":"55555555-5555-4555-8555-555555555555",
        "team":t.id,"member":"dev","attempt":"A1",
        "input":{"request_id":"55555555-5555-4555-8555-555555555555","team":t.id,"task":id,"attempt":"A1","text":"Done"}});
    let mut member = session("codex/x/dev", "r1", true);
    member.cwd = folder.to_string_lossy().into_owned();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member.clone()]).unwrap();
    // The worker died before it removed the file: the same request again.
    super::super::outbox::push(&folder, &request).unwrap();
    let current = store.load(&t.id).unwrap().unwrap();
    take_outbox(&mut store, &current, &[member]).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let task = saved.task(id).unwrap();
    assert_eq!(task.attempts.len(), 1);
    assert_eq!(task.attempts[0].state, AttemptState::Reported);
    let result = super::super::outbox::result(&folder, "55555555-5555-4555-8555-555555555555").unwrap();
    assert_eq!(result["status"], "submitted");
}

#[test]
fn a_quiet_session_without_a_reply_mark_is_captured_after_the_timeout() {
    // U20/#343 attempt 2: the member finished its turn and stopped, but the
    // listing carried no reply_at (its replies were never read), so no turn
    // end could be recorded. After the report timeout the attempt must be
    // decided, not left in progress.
    let mut t = team();
    delivered(&mut t);
    t.task_mut(1).unwrap().attempts[0].started_at = Some(20.0);
    let mut seen = session("codex/x/dev", "r1", true);
    seen.phase = "idle".into();
    assert!(seen.reply_at.is_none());
    let timeout = t.settings.report_timeout_seconds as f64;
    let waiting = steps(&t, &[seen.clone()], 10.0 + timeout - 1.0);
    assert!(!waiting.iter().any(|step| matches!(step, Step::NoReport { .. })), "{waiting:?}");
    let late = steps(&t, &[seen], 10.0 + timeout + 1.0);
    assert!(late.contains(&Step::NoReport { task: 1 }), "{late:?}");
}
