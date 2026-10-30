use super::*;
use super::support::*;

#[test]
fn an_old_catalog_keeps_the_hand_off_text_unchanged() {
    let mut team = team();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().prompt = "You write the backend".into();
    let task = team.task(1).unwrap().clone();
    let attempt = Attempt { n: 1, id: "a1".into(), member: "dev".into(), ..Default::default() };
    let text = hand_off_text_with(&team, &task, &attempt, "", "", "");
    assert!(text.contains("Role: You write the backend"), "{text}");
    assert!(!text.contains("Project rules:") && !text.contains("Team protocol extra:"), "{text}");
}

#[test]
fn the_hand_off_names_the_attempt_and_how_to_report() {
    let team = team();
    let task = team.task(1).unwrap();
    let attempt = Attempt { n: 2, id: "A-ID".into(), member: "dev".into(), ..Default::default() };
    let text = hand_off_text(&team, task, &attempt, "msi", "msi-old");
    assert!(text.starts_with("[Zerus team \u{b7} from: Lead \u{b7} task: #1 \u{b7} attempt: 2]"));
    assert!(text.contains("hgs @msi team report --json") && text.contains("\"attempt\":\"A-ID\""));
    // H5: the hand-out names ask, current and the report command; the
    // attempt id authorizes a member on another computer (N2).
    assert!(text.contains("hgs @msi team ask --json") && text.contains("\"team\":\"t\""), "{text}");
    assert!(text.contains("\"attempt\":\"A-ID\""), "{text}");
    assert!(text.contains("Your task and attempt again: {\"attempt\":\"A-ID\"} | hgs @msi team current --json"), "{text}");
    assert!(text.contains("Report once at the end: hgs @msi team report --json"), "{text}");
    assert!(text.contains("A progress line is not a report; a question is not a report."), "{text}");
    let local = hand_off_text(&team, task, &attempt, "msi", "");
    assert!(local.contains("hgs team report --json"));
    assert!(local.contains("hgs team ask --json") && local.contains("hgs team current --json"), "{local}");
    assert!(local.contains("{\"attempt\":\"A-ID\"} | hgs team current --json"), "{local}");
}

#[test]
fn a_hand_off_lists_dependencies_with_their_folders_and_reports() {
    let mut team = team();
    team.folder = "work".into();
    let dep = team.task_mut(1).unwrap();
    dep.status = TaskStatus::Accepted;
    dep.attempts = vec![
        attempt_with_report("dev", "the accepted report", AttemptState::Reported),
        attempt_with_report("dev", "a later rejected report", AttemptState::Rejected),
    ];
    let task = team.task(3).unwrap().clone();
    let attempt = Attempt { n: 1, id: "A".into(), member: "qa".into(), ..Default::default() };
    let text = hand_off_text(&team, &task, &attempt, "", "");
    assert!(text.contains("From dependencies:\n#1 API (accepted) by dev (Backend), folder: work"), "{text}");
    // An accepted dependency shows the report that accepted it, not a later one.
    assert!(text.contains("the accepted report"), "{text}");
    assert!(!text.contains("a later rejected report"), "{text}");
    assert!(text.find("From dependencies:").unwrap() > text.find("Depends on:").unwrap());
    // A soft dependency counts too, and an unaccepted one shows its latest report.
    team.task_mut(1).unwrap().status = TaskStatus::Rejected;
    let text = hand_off_text(&team, &task, &attempt, "", "");
    assert!(text.contains("a later rejected report"), "{text}");
    assert!(!text.contains("the accepted report"), "{text}");
    // A member on another computer shows which; the lead shows the team's folder.
    team.members.iter_mut().find(|m| m.id == "dev").unwrap().computer = "msi".into();
    assert!(hand_off_text(&team, &task, &attempt, "", "").contains("folder: msi:work"));
    team.members.iter_mut().find(|m| m.id == "dev").unwrap().folder = MemberFolder::Worktree;
    team.members.iter_mut().find(|m| m.id == "dev").unwrap().computer.clear();
    assert!(hand_off_text(&team, &task, &attempt, "", "").contains("folder: work-team-t-dev"));
    team.members.iter_mut().find(|m| m.id == "lead").unwrap().directory = "lead-dir".into();
    team.task_mut(1).unwrap().attempts = vec![attempt_with_report("lead", "lead report", AttemptState::Reported)];
    let text = hand_off_text(&team, &task, &attempt, "", "");
    assert!(text.contains("by lead (Lead), folder: work"), "{text}");
    assert!(!text.contains("lead-dir"), "{text}");
    // A member on this computer in a team led from a peer is prefixed with
    // the local host name.
    team.members.iter_mut().find(|m| m.id == "lead").unwrap().computer = "peer".into();
    team.members.iter_mut().find(|m| m.id == "dev").unwrap().folder = MemberFolder::Shared;
    team.members.iter_mut().find(|m| m.id == "dev").unwrap().computer.clear();
    team.task_mut(1).unwrap().attempts = vec![attempt_with_report("dev", "dev report", AttemptState::Reported)];
    let text = hand_off_text(&team, &task, &attempt, "msi", "");
    assert!(text.contains("by dev (Backend), folder: msi:work"), "{text}");
}

#[test]
fn a_hand_off_lists_at_most_twenty_dependencies() {
    let mut team = team();
    team.folder = "work".into();
    let first = team.task_mut(1).unwrap();
    first.status = TaskStatus::Accepted;
    first.attempts = vec![attempt_with_report("dev", "first", AttemptState::Reported)];
    let mut deps = vec![Dependency { task: 1, kind: DependencyKind::Hard }];
    for id in 4..=27 {
        team.tasks.push(Task {
            id,
            title: format!("Dep {id}"),
            status: TaskStatus::Accepted,
            assignee: "dev".into(),
            attempts: vec![attempt_with_report("dev", "rest", AttemptState::Reported)],
            ..Default::default()
        });
        deps.push(Dependency { task: id, kind: DependencyKind::Hard });
    }
    team.task_mut(3).unwrap().deps = deps;
    let task = team.task(3).unwrap().clone();
    let attempt = Attempt { n: 1, id: "A".into(), member: "qa".into(), ..Default::default() };
    let text = hand_off_text(&team, &task, &attempt, "", "");
    // 1 + 19 more header lines, then only the count of the rest.
    assert!(text.contains("#22 Dep 22 (accepted)"), "{text}");
    assert!(!text.contains("#23 Dep 23 (accepted) by"), "{text}");
    assert!(text.contains("[5 more dependencies omitted]"), "{text}");
}

#[test]
fn a_dependency_without_a_report_is_not_listed() {
    let mut team = team();
    team.folder = "work".into();
    team.task_mut(3).unwrap().deps =
        vec![Dependency { task: 1, kind: DependencyKind::Hard }, Dependency { task: 2, kind: DependencyKind::Soft }];
    let task = team.task(3).unwrap().clone();
    let attempt = Attempt { n: 1, id: "A".into(), member: "qa".into(), ..Default::default() };
    let text = hand_off_text(&team, &task, &attempt, "", "");
    assert!(text.contains("Depends on: #1 API (pending), #2 More API (pending)"), "{text}");
    assert!(!text.contains("From dependencies:"), "{text}");
}

#[test]
fn dependency_reports_are_cut_to_two_thousand_characters() {
    let mut team = team();
    team.folder = "work".into();
    let dep = team.task_mut(1).unwrap();
    dep.status = TaskStatus::Accepted;
    dep.attempts = vec![attempt_with_report("dev", &"é".repeat(2500), AttemptState::Reported)];
    let task = team.task(3).unwrap().clone();
    let attempt = Attempt { n: 1, id: "A".into(), member: "qa".into(), ..Default::default() };
    let text = hand_off_text(&team, &task, &attempt, "", "");
    // Cut at a character boundary, with the note.
    assert_eq!(text.matches('é').count(), 2000);
    assert!(text.contains("[cut: shown 2000 of 2500 characters; full report: hgs team status --json, task #1]"), "{text}");
    // A report within the bound is whole, with no note.
    team.task_mut(1).unwrap().attempts = vec![attempt_with_report("dev", &"ζ".repeat(2000), AttemptState::Reported)];
    let text = hand_off_text(&team, &task, &attempt, "", "");
    assert_eq!(text.matches('ζ').count(), 2000);
    assert!(!text.contains("[cut:"), "{text}");
}

#[test]
fn dependency_reports_stop_at_the_total_limit() {
    let mut team = team();
    team.folder = "work".into();
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    team.task_mut(1).unwrap().attempts = vec![attempt_with_report("dev", &"α".repeat(500), AttemptState::Reported)];
    for (id, letter) in [(4, 'β'), (5, 'γ'), (6, 'δ'), (7, 'ε')] {
        team.tasks.push(Task {
            id,
            title: format!("Dep {id}"),
            status: TaskStatus::Accepted,
            assignee: "dev".into(),
            attempts: vec![attempt_with_report("dev", &letter.to_string().repeat(3000), AttemptState::Reported)],
            ..Default::default()
        });
    }
    team.task_mut(3).unwrap().deps = [1, 4, 5, 6, 7]
        .into_iter()
        .map(|task| Dependency { task, kind: DependencyKind::Hard })
        .collect();
    let task = team.task(3).unwrap().clone();
    let attempt = Attempt { n: 1, id: "A".into(), member: "qa".into(), ..Default::default() };
    let text = hand_off_text(&team, &task, &attempt, "", "");
    // 500 shown whole, then 2,000 + 2,000, then the last 1,500 of the 6,000.
    assert!(text.contains("α".repeat(500).as_str()), "{text}");
    assert!(!text.contains("task #1]"), "{text}");
    assert!(text.contains("[cut: shown 2000 of 3000 characters; full report: hgs team status --json, task #4]"), "{text}");
    assert!(text.contains("[cut: shown 1500 of 3000 characters; full report: hgs team status --json, task #6]"), "{text}");
    assert!(text.contains("#7 Dep 7 (accepted) by dev (Backend), folder: work\n[report omitted: limit reached]"), "{text}");
    assert_eq!(text.matches('α').count(), 500);
    assert_eq!(text.matches('β').count(), 2000);
    assert_eq!(text.matches('γ').count(), 2000);
    assert_eq!(text.matches('δ').count(), 1500);
    assert_eq!(text.matches('ε').count(), 0);
}
