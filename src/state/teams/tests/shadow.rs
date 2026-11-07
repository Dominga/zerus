use super::*;
use crate::state::teams::model::{Attempt, AttemptState, Report, TaskStatus};

/// dev holds #1 (open attempt a1); #2 is dev's but not handed; #3 is qa's.
fn team() -> Team {
    let mut team = Team { id: "t".into(), name: "Core".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
    for id in ["lead", "dev", "qa"] {
        team.members.push(Member { id: id.into(), role: id.into(), state: MemberState::Active, ..Default::default() });
    }
    team.tasks.push(Task { id: 1, title: "API".into(), assignee: "dev".into(), status: TaskStatus::InProgress,
        attempts: vec![Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, sent_at: 100.0, ..Default::default() }],
        ..Default::default() });
    team.tasks.push(Task { id: 2, title: "Docs".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
    team.tasks.push(Task { id: 145, title: "Merge".into(), assignee: "qa".into(), status: TaskStatus::Pending, ..Default::default() });
    team
}

#[test]
fn a_text_names_only_the_teams_tasks_in_their_usual_forms() {
    let team = team();
    assert_eq!(task_ids(&team, "Lead, for 145: build and measure it"), [145]);
    assert_eq!(task_ids(&team, "About #1 and Task #2, also task 145."), [1, 2, 145]);
    assert!(task_ids(&team, "wait 2 minutes, retry 145 times, version 1.2, #12a, #999, for 1 hour").is_empty());
    assert!(task_ids(&team, "Standup in 5").is_empty());
}

#[test]
fn a_lead_message_binds_to_the_handed_attempt_or_is_refused() {
    let team = team();
    let dev = team.member("dev").unwrap();
    let qa = team.member("qa").unwrap();
    assert_eq!(check_lead_message(&team, dev, "On #1: keep the old field").unwrap(),
        vec![Binding { team: "t".into(), task: 1, attempt: "a1".into(), member: "dev".into() }]);
    assert!(check_lead_message(&team, qa, "Please check the logs and reply").unwrap().is_empty(), "free-form: nothing to bind");
    let error = check_lead_message(&team, qa, "Lead, for 145: build, test and measure it").unwrap_err().to_string();
    assert!(error.starts_with("Task #145 is not handed to qa: it has no open attempt"), "{error}");
    assert!(error.contains("hgs team retry --json") && error.contains("hgs team reassign --json"), "{error}");
    let error = check_lead_message(&team, qa, "For #1 please review").unwrap_err().to_string();
    assert!(error.contains("it is handed to dev"), "{error}");
    let error = check_lead_message(&team, dev, "Then start #2 as well").unwrap_err().to_string();
    assert!(error.contains("Task #2 is not handed to dev"), "{error}");
    // The binding is kept on the attempt, bounded, with an event.
    let mut stored = team.clone();
    let mut events = Vec::new();
    for n in 0..25 {
        attach(&mut stored, &[Binding { team: "t".into(), task: 1, attempt: "a1".into(), member: "dev".into() }], &format!("note {n}"), "hgs send", 1.0, &mut events);
    }
    let kept = &stored.task(1).unwrap().attempts[0].lead_messages;
    assert_eq!(kept.len(), 20);
    assert_eq!(kept.last().unwrap()["text"], "note 24");
    assert_eq!(events[0].kind, "lead_message");
}

#[test]
fn a_member_job_needs_its_handed_work() {
    let team = team();
    let job = job_of(&team, "dev", "").unwrap();
    assert_eq!((job.task, job.attempt.as_str()), (1, "a1"));
    assert_eq!(job_of(&team, "dev", "a1").unwrap().task, 1);
    let error = job_of(&team, "dev", "other").unwrap_err().to_string();
    assert!(error.contains("Attempt other is not handed to you; your work is #1 (attempt a1)"), "{error}");
    let error = job_of(&team, "qa", "").unwrap_err().to_string();
    assert!(error.starts_with("No task is handed to you in team Core: ask the lead"), "{error}");
}

fn seen(free: bool, building: bool, turn_started: Option<f64>) -> super::super::dispatcher::Seen {
    super::super::dispatcher::Seen { name: "codex/x/qa".into(), free, live: true, building, build_label: if building { "cargo test".into() } else { String::new() },
        turn_started, activity: if free { "idle".into() } else { "busy".into() }, ..Default::default() }
}

#[test]
fn work_with_nothing_handed_is_unassigned_after_the_grace_and_a_build_at_once() {
    let mut team = team();
    let qa = team.member("qa").unwrap().clone();
    assert_eq!(unassigned(&team, &qa, &seen(false, true, None), 1_000.0).as_deref(), Some("builds cargo test"));
    assert_eq!(unassigned(&team, &qa, &seen(false, false, Some(950.0)), 1_000.0), None, "within the grace");
    assert_eq!(unassigned(&team, &qa, &seen(false, false, Some(800.0)), 1_000.0).as_deref(), Some("works (busy) since 3 min"));
    assert_eq!(unassigned(&team, &qa, &seen(true, false, None), 1_000.0), None, "an idle member is no work");
    // The rest of the turn its report ended is not unassigned.
    team.task_mut(145).unwrap().attempts.push(Attempt { n: 1, id: "q1".into(), member: "qa".into(), state: AttemptState::Reported,
        sent_at: 500.0, report: Some(Report { text: "done".into(), at: 900.0, ..Default::default() }), ..Default::default() });
    assert_eq!(unassigned(&team, &qa, &seen(false, false, Some(800.0)), 1_000.0), None);
    // Handed work, or the lead, is never flagged.
    let dev = team.member("dev").unwrap().clone();
    assert_eq!(unassigned(&team, &dev, &seen(false, true, Some(0.0)), 1_000.0), None);
    let lead = team.member("lead").unwrap().clone();
    assert_eq!(unassigned(&team, &lead, &seen(false, true, Some(0.0)), 1_000.0), None);
}
