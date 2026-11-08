// Task 263 (owner, 2026-11-08): a ready task waiting for its busy member must
// not silently hold back other members' ready work. The dispatcher used to
// claim a task's files as soon as it passed the output checks, so 198 (queued
// on its busy ui2) kept 218-226 (ui3, idle, same files) from starting for an
// hour, and status named only "wait for ui3". Now only a task the tick hands
// out claims its files; once it can start, the later task's work holds it
// back instead, and every such hold is named in waits, in look and in the
// lead's idle item.
use super::*;

/// The reported shape: dev works on #1; #2 waits for dev (pinned, so it is not
/// handed to dev2) and #3, dev2's, writes a file inside #2's folder.
fn reported_shape() -> (Team, Vec<Seen>) {
    let (mut team, mut seen) = team_with_two_backends();
    delivered(&mut team);
    team.task_mut(2).unwrap().assignee = "dev".into();
    team.task_mut(2).unwrap().pinned = true;
    team.task_mut(2).unwrap().output = vec!["client/src/ui/teams".into()];
    team.task_mut(3).unwrap().assignee = "dev2".into();
    team.task_mut(3).unwrap().output = vec!["client/src/ui/teams/List.tsx".into()];
    // dev is busy with #1; dev2 is idle and free.
    seen[1].free = false;
    seen[1].idle = false;
    (team, seen)
}

#[test]
fn a_task_waiting_for_its_busy_member_never_holds_a_later_members_ready_task() {
    let (team, seen) = reported_shape();
    assert!(!plan::holds_outputs(team.task(2).unwrap()), "a queued task that never started holds nothing");
    // Task 263: #2 waits for dev, who is busy; it must not claim the files #3
    // (dev2, idle) writes, so #3 is handed out instead of waiting silently.
    let found = steps(&team, &seen, 100.0);
    assert!(found.contains(&Step::HandOut { task: 3, member: "dev2".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })), "{found:?}");
}

#[test]
fn a_handed_out_task_still_claims_the_files_for_the_tick() {
    let (mut team, seen) = team_with_two_backends();
    // Both members are free and #1, #2 overlap: only the first goes this tick.
    team.task_mut(1).unwrap().assignee = "dev".into();
    team.task_mut(2).unwrap().assignee = "dev2".into();
    team.task_mut(1).unwrap().output = vec!["client/src/ui/teams".into()];
    team.task_mut(2).unwrap().output = vec!["client/src/ui/teams/List.tsx".into()];
    let found = steps(&team, &seen, 100.0);
    assert!(found.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })), "{found:?}");
}

#[test]
fn a_returned_holder_waiting_for_its_member_names_the_hold_in_status_and_look() {
    let (mut team, seen) = reported_shape();
    // #2 came back and waits for dev again: it holds its files, so #3 cannot
    // start beside it - and the hold is named, never a silent "wait for dev2".
    let task = team.task_mut(2).unwrap();
    task.status = TaskStatus::Pending;
    task.rework_since = Some(1.0);
    task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Rejected, ..Default::default() });
    assert_eq!(plan::output_holders(&team, team.task(3).unwrap()), vec![2]);
    let wait = inbox::waits(&team, team.task(3).unwrap(), true).unwrap();
    assert_eq!(wait["for"], "outputs", "{wait}");
    assert_eq!(wait["on"], json!([2]), "{wait}");
    assert_eq!(wait["unblock"], "#2 waits for dev (Backend): hold or reassign #2", "{wait}");
    assert!(!steps(&team, &seen, 100.0).iter().any(|step| matches!(step, Step::HandOut { task: 3, .. } | Step::Rebalance { task: 3, .. })));
    // The idle member's look names the hold instead of "hands it the next".
    let dev2 = team.member("dev2").unwrap().clone();
    let next = crate::state::teams::look::next_action(&team, &dev2, "idle");
    assert!(next.contains("#3") && next.contains("#2") && next.contains("waits for dev (Backend)"), "{next}");
    // Once the returned #2 starts it runs, so the hold says to wait for it.
    team.task_mut(2).unwrap().status = TaskStatus::InProgress;
    team.task_mut(2).unwrap().attempts.last_mut().unwrap().state = AttemptState::Delivered;
    let wait = inbox::waits(&team, team.task(3).unwrap(), true).unwrap();
    assert_eq!(wait["unblock"], "wait for #2 to finish", "{wait}");
    assert_eq!(wait["on"], json!([2]), "{wait}");
}

#[test]
fn once_the_later_task_runs_it_holds_the_first_back() {
    let (mut team, seen) = reported_shape();
    // #3 started (dev2 delivered); #2 can no longer start beside it, even when
    // dev is free again - the overlap rule holds it back instead.
    team.task_mut(3).unwrap().status = TaskStatus::InProgress;
    team.task_mut(3).unwrap().attempts.push(Attempt { n: 1, member: "dev2".into(), state: AttemptState::Delivered, ..Default::default() });
    let first = team.task_mut(1).unwrap();
    first.status = TaskStatus::Accepted;
    first.attempts[0].state = AttemptState::Reported;
    let mut free = seen.clone();
    free[1].free = true;
    free[1].idle = true;
    assert_eq!(plan::output_holders(&team, team.task(2).unwrap()), vec![3]);
    let found = steps(&team, &free, 200.0);
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })), "{found:?}");
    let wait = inbox::waits(&team, team.task(2).unwrap(), true).unwrap();
    assert_eq!((wait["for"].clone(), wait["on"].clone()), (json!("outputs"), json!([3])), "{wait}");
}

#[test]
fn an_idle_members_held_work_is_one_lead_item_naming_the_holder() {
    let (mut team, _) = reported_shape();
    // #2 came back and waits for dev; its role family is another one, so the
    // member cannot take #2 itself and the item is the held-work one.
    team.task_mut(2).unwrap().status = TaskStatus::Pending;
    team.task_mut(2).unwrap().rework_since = Some(1.0);
    team.task_mut(2).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Rejected, ..Default::default() });
    team.members.iter_mut().find(|member| member.id == "dev2").unwrap().role = "Frontend".into();
    let dev2 = team.members.iter_mut().find(|member| member.id == "dev2").unwrap();
    dev2.seen_activity = "idle".into();
    dev2.seen = Some(MemberSeen { at: 100.0, live: true });
    let items = inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "idle_with_ready_work" && item.task == Some(3)).expect("the held-work item");
    assert!(item.member.contains("dev2"), "{item:?}");
    assert!(item.detail.contains("#2") && item.detail.contains("waits for dev (Backend)"), "{item:?}");
    assert!(item.command.contains("\"task\":2") && item.command.contains("hgs team hold"), "{item:?}");
    // A holder that runs is named too, but its item only says to wait.
    let mut running = team.clone();
    running.task_mut(2).unwrap().status = TaskStatus::InProgress;
    running.task_mut(2).unwrap().attempts.last_mut().unwrap().state = AttemptState::Delivered;
    let item = inbox::items(&running).into_iter().find(|item| item.kind == "idle_with_ready_work" && item.task == Some(3)).expect("the held-work item");
    assert!(item.detail.contains("runs now") && item.command.contains("hgs team look"), "{item:?}");
}

#[test]
fn the_held_item_is_not_invented_for_a_member_that_is_not_idle() {
    let (mut team, _) = reported_shape();
    team.task_mut(2).unwrap().status = TaskStatus::Pending;
    team.task_mut(2).unwrap().rework_since = Some(1.0);
    team.task_mut(2).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Rejected, ..Default::default() });
    // No idle listing (seen_activity stays empty) and no starved episode.
    assert!(inbox::items(&team).iter().all(|item| item.kind != "idle_with_ready_work" || item.task != Some(3)));
}
