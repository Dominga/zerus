//! Task 295 (owner, 2026-11-08 ~06:00): ui4 sat idle while its ready #226
//! waited. ui3's #219 and then #225 wrote the same files (client/src/ui/teams),
//! so task 155's rule held #226 until #225 was accepted at 06:38:12; it was
//! handed out three seconds later. Every view said otherwise: the session
//! state "Waiting for Lead to hand out #226", status `waits` {for: member, on:
//! [ui4]}, look "Hands it the next ready task". They now name the task it
//! waits for; it is handed out on the tick after that one ends; and the lead
//! can propose a relaunch for an idle member that never takes its task.
use super::super::ops::Actor;
use super::support::*;
use super::*;

const FILES: &str = "client/src/ui/teams";

/// #1 runs on dev (ui3's #219); #2 is dev2's ready task writing the same
/// folder (ui4's #226); both members are idle and free.
fn reproduced() -> (Team, Vec<Seen>) {
    let (mut team, seen) = team_with_two_backends();
    let one = team.task_mut(1).unwrap();
    one.output = vec![FILES.into()];
    one.status = TaskStatus::InProgress;
    one.attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    let two = team.task_mut(2).unwrap();
    two.output = vec![FILES.into()];
    two.assignee = "dev2".into();
    (team, seen)
}

fn reason_of(team: &Team, session: &str) -> Value {
    super::super::waiting::reasons(std::slice::from_ref(team), 0.0)
        .into_iter()
        .find(|waiting| waiting.name == session)
        .and_then(|waiting| waiting.reason)
        .unwrap_or(Value::Null)
}

fn hands_out_two(team: &Team, seen: &[Seen]) -> bool {
    steps(team, seen, 1.0).iter().any(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. }))
}

#[test]
fn an_idle_members_ready_task_names_the_task_writing_its_files() {
    let (mut team, seen) = reproduced();
    assert!(!hands_out_two(&team, &seen), "task 155: never beside #1");
    // The session state names #1, not the lead.
    let reason = reason_of(&team, "codex/x/dev2");
    assert_eq!(reason["kind"], "team_output", "{reason}");
    assert_eq!(reason["text"], "Waiting for #1 API: it writes the same files as #2");
    assert_eq!(reason["holders"], json!([1]));
    // Status names it too, not the member.
    let waits = super::super::inbox::waits(&team, team.task(2).unwrap(), true).unwrap();
    assert_eq!((waits["for"].as_str(), waits["on"].clone(), waits["unblock"].as_str()), (Some("deps"), json!([1]), Some("finish #1")), "{waits}");
    // And look says what Zerus does next.
    let next = super::super::look::next_action(&team, team.member("dev2").unwrap(), "idle");
    assert_eq!(next, "Its task #2 waits for #1, which writes the same files; Zerus hands it out when #1 ends.");
    // #1 ends: the next tick hands #2 to its idle member.
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    team.task_mut(1).unwrap().attempts[0].state = AttemptState::Reported;
    assert!(steps(&team, &seen, 1.0).contains(&Step::HandOut { task: 2, member: "dev2".into() }));
    assert_eq!(reason_of(&team, "codex/x/dev2")["kind"], "team_hand_out", "only an untaken ready task waits for the hand-out");
}

/// ui4's second wait: #225 and #226 both ready and both writing the same
/// files; the lower number starts first and the other names it.
#[test]
fn of_two_ready_tasks_writing_the_same_files_the_later_names_the_first() {
    let (mut team, _) = reproduced();
    team.task_mut(1).unwrap().status = TaskStatus::Pending;
    team.task_mut(1).unwrap().attempts.clear();
    assert!(plan::output_queue(&team, team.task(1).unwrap()).is_empty());
    assert_eq!(plan::output_queue(&team, team.task(2).unwrap()), vec![1]);
    assert_eq!(reason_of(&team, "codex/x/dev2")["text"], "Waiting for #1 API: it writes the same files as #2");
    // Separate files: nothing holds it, and it waits for the hand-out.
    team.task_mut(2).unwrap().output = vec!["client/src/ui/sessions".into()];
    assert!(plan::output_queue(&team, team.task(2).unwrap()).is_empty());
}

/// A hand-out step that was refused (an accepted dependency's merge
/// conflicted in the member's folder) is named, not "waiting for the lead".
#[test]
fn a_refused_hand_out_step_names_its_reason() {
    let (mut team, _) = reproduced();
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    team.task_mut(2).unwrap().waits = Some(TaskWait::zerus(
        "dependencies",
        "refused",
        "#273's accepted commit 28c983537 conflicts in client/src/ui/sessions/model.ts",
        1.0,
    ));
    let reason = reason_of(&team, "codex/x/dev2");
    assert_eq!(reason["kind"], "team_task_waits", "{reason}");
    assert!(reason["text"].as_str().unwrap().starts_with("Waiting for Lead to retry or reassign #2: #273's accepted commit"), "{reason}");
    let next = super::super::look::next_action(&team, team.member("dev2").unwrap(), "idle");
    assert!(next.starts_with("Its task #2 waits: #273's accepted commit") && next.ends_with("(retry or reassign it)."), "{next}");
}

/// The lead proposes a relaunch for an idle member that never takes its task
/// (not stuck, its session running); the owner allows it with a click, and
/// the relaunch gives it a fresh conversation with its handoff brief.
#[test]
fn the_lead_proposes_a_relaunch_for_an_idle_member_that_never_takes_its_task() {
    let (mut team, _) = reproduced();
    assert!(team.member("dev2").unwrap().stuck.is_none());
    let mut events = Vec::new();
    let receipt = ops::apply(
        &mut team,
        "propose",
        &json!({"kind":"relaunch","member":"dev2","reason":"idle 145 min; never took its ready #2"}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(receipt["status"], "proposed", "{receipt}");
    let proposal = team.proposals.last().unwrap();
    assert_eq!((proposal["kind"].as_str(), proposal["state"].as_str()), (Some("relaunch"), Some("open")));
    // The owner's click runs the relaunch itself: a fresh session.
    let relaunched = ops::apply(&mut team, "relaunch", &json!({"member":"dev2"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(relaunched["fresh"], true);
    assert!(team.member("dev2").unwrap().session.is_none() && team.member("dev2").unwrap().handoff.is_some());
}
