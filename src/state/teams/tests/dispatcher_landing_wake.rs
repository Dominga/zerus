//! Task #464 (owner, 2026-11-09): accepted work never holds anyone. Its author
//! is free and takes the next task at once, whatever the landing state; with
//! the merge queue off the landing merger is woken once for each accepted task,
//! and the lead hears one line when nobody holds a landing task; the starved
//! digest names the state plainly ("N accepted tasks wait for a landing"),
//! never "waiting for the lead to review".
use super::*;
use crate::state::teams::model::{Dependency, DependencyKind};

const NOW: f64 = 10.0 + 30.0 * 60.0;
const COMMIT: &str = "46de9c4d5a6345395d221dcaa3eaf48aed0a1d71";

fn seen() -> Vec<Seen> {
    vec![
        session("claude/x/lead", "r0", true),
        session("codex/x/dev", "r1", true),
        session("claude/x/merger2", "r5", true),
    ]
}

/// A team with a merger that holds a rolling landing task, as the owner's p92
/// leaves it when the queue is off.
fn team_with_landing() -> Team {
    let mut team = team();
    team.members.push(Member {
        id: "merger2".into(),
        role: "Reviewer & Merger".into(),
        agent: "claude".into(),
        state: MemberState::Active,
        session: Some(SessionRef { name: "claude/x/merger2".into(), run_id: "r5".into(), ..Default::default() }),
        ..Default::default()
    });
    team.tasks.push(Task {
        id: 4,
        title: "Rolling landing batch 2: land #1".into(),
        assignee: "merger2".into(),
        status: TaskStatus::InProgress,
        phase: 1,
        ..Default::default()
    });
    team
}

/// #1 accepted by the lead, its commit never on a line (the queue is off).
fn accepted(team: &mut Team) {
    let task = team.task_mut(1).unwrap();
    task.attempts.push(Attempt {
        n: 1,
        id: "A1".into(),
        member: "dev".into(),
        state: AttemptState::Reported,
        folder_head: COMMIT.into(),
        folder_branch: "team/t-1".into(),
        report: Some(Report { text: "Done".into(), at: 5.0, ..Default::default() }),
        ..Default::default()
    });
    task.status = TaskStatus::Submitted;
    let mut events = Vec::new();
    ops::apply(team, "accept", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    assert!(super::super::super::merge_queue::waits_for_landing(team.task(1).unwrap()));
}

/// (a) The author of accepted work is free at once: its next task built on
/// that work is prepared without waiting for the landing; another member's
/// task built on the same work still starts only from the line.
#[test]
fn an_accepted_task_is_no_landing_wait_for_its_author() {
    let mut team = team();
    team.settings.integration_folder = "C:/no-such-line".into();
    accepted(&mut team);
    let task = team.task_mut(2).unwrap();
    task.deps = vec![Dependency { task: 1, kind: DependencyKind::Hard }];
    // Task #464: the author waits for nothing - its folder holds the commit it
    // made, so the dependent is handed out at once (no prepare step at all).
    assert!(prepare_dependencies(&team, 2, "dev").is_none(), "the author has nothing to wait for");
    // The other member is not the author: it starts only from what is landed.
    let other = prepare_dependencies(&team, 2, "qa").expect("the step exists");
    assert!(other.reason.contains("not on the integration line yet"), "{}", other.reason);
    assert_eq!(other.result, "refused");
}

/// (b) With the queue off, each accepted task wakes the merger that holds the
/// landing task once, naming the task and its commit; a landing it heard of is
/// never told again, and without a landing task the lead's inbox names the wait.
#[test]
fn an_accept_wakes_the_landing_merger_once_and_the_lead_sees_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut team = team_with_landing();
        accepted(&mut team);
        let found = steps(&team, &seen(), NOW);
        assert!(found.contains(&Step::WakeLanding { member: "merger2".into(), tasks: vec![1] }), "{found:?}");
        // A second tick before the step runs finds the same wake; the step
        // records it, and the next tick has nothing to tell.
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team.clone(), vec![]).unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            if args.first().copied() == Some("send") {
                record.borrow_mut().push(super::super::super::doorbell::posted_text(input.unwrap()));
            }
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        run_step(&mut store, &team, &Step::WakeLanding { member: "merger2".into(), tasks: vec![1] }, "").unwrap();
        set_fake_hgs(None);
        let text = sent.borrow().first().cloned().expect("the merger heard the landing");
        assert!(text.contains("#1") && text.contains("46de9c4d5") && text.contains("landing"), "{text}");
        assert!(text.contains("must not be asked to wait"), "{text}");
        let saved = store.load(&team.id).unwrap().unwrap();
        assert_eq!(saved.landing_told, vec![1]);
        assert!(event_kinds(&store, &team.id).contains(&"landing_woken".to_owned()));
        let again = steps(&saved, &seen(), NOW + 60.0);
        assert!(!again.iter().any(|step| matches!(step, Step::WakeLanding { .. })), "{again:?}");
    });
}

/// (c) The starved digest names the landing state plainly: how many accepted
/// tasks wait, the merger that holds the landing task and whether it is cold,
/// or that nobody has one; never "waiting for the lead to review".
#[test]
fn the_starved_digest_names_accepted_work_waiting_for_a_landing() {
    let mut team = team_with_landing();
    accepted(&mut team);
    team.members.iter_mut().find(|member| member.id == "merger2").unwrap().starved_since = Some(1.0);
    let actions = starved_actions(&team, &["merger2".into()]);
    let line = actions.iter().find(|line| line.contains("accepted, waiting to land")).expect("the landing line");
    assert!(line.contains("#1") && line.contains("merger2 (Reviewer & Merger) is cold with #4"), "{line}");
    let text = starved_text(&team, &["merger2".into()], NOW).expect("a digest");
    assert!(text.contains("accepted, waiting to land") && !text.to_lowercase().contains("waiting for the lead to review"), "{text}");
    // No merger task at all: the lead reads what to do with them by hand.
    team.task_mut(4).unwrap().status = TaskStatus::Accepted;
    let left = super::super::super::merge_queue::landing_line(&team, NOW).expect("the landing line");
    assert!(left.contains("no merger task is open") && left.contains("merged"), "{left}");
    // And the lead's inbox has the one ready line with its command.
    let item = super::super::super::inbox::items(&team)
        .into_iter()
        .find(|item| item.kind == "waiting_to_land")
        .expect("the lead's line");
    assert_eq!(item.value()["notice"], json!(false), "a notice, never a decision");
    assert!(item.summary.contains("accepted, waiting to land: #1"), "{}", item.summary);
    assert!(item.command.contains("hgs team landed --json"), "{}", item.command);
}
