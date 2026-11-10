// Task 271 (owner, 2026-11-08): the lead playbook - one check per rule (a)-(i),
// each raising its item and clearing when fixed - and the lead brief that comes
// back after a compaction.
use super::*;
use crate::state::teams::model::*;

fn lead_session(name: &str, run: &str) -> SessionRef {
    SessionRef { name: name.into(), run_id: run.into(), conversation_id: "conv-1".into(), ..Default::default() }
}

fn member(id: &str, session: Option<SessionRef>) -> Member {
    Member { id: id.into(), role: "Backend".into(), agent: "claude".into(), state: MemberState::Active, session, ..Default::default() }
}

/// A running team in the branch-pipeline mode with its settings in place.
fn team() -> Team {
    let mut team = Team {
        id: "t271".into(),
        name: "Coach".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        working_mode: "branch-pipeline".into(),
        mode_proposal: Some(json!({"preset":"branch-pipeline","title":"the branch pipeline"})),
        settings: Settings {
            branch_per_task: Some(BranchPerTask { template: "task/{id}".into() }),
            base_branch: "main".into(),
            ..Default::default()
        },
        members: vec![member("lead", Some(lead_session("claude/x/lead", "r1"))), member("dev", None)],
        ..Default::default()
    };
    team.playbook = Some(playbook(&team));
    team
}

fn task(id: u32, status: TaskStatus) -> Task {
    Task { id, phase: 1, title: format!("Task {id}"), assignee: "dev".into(), status, ..Default::default() }
}

fn rule(found: &[Breach], letter: &str) -> Option<Breach> {
    found.iter().find(|breach| breach.rule == letter).cloned()
}

#[test]
fn the_playbook_holds_the_preset_and_all_nine_rules() {
    let team = team();
    let playbook = playbook(&team);
    assert_eq!(playbook["mode"], "branch-pipeline");
    assert_eq!(playbook["rules"].as_array().map(Vec::len), Some(9));
    assert!(playbook["rules"].as_array().unwrap().iter().any(|rule| rule["id"] == "i"));
    assert!(playbook["settings"]["branch_per_task"]["template"] == "task/{id}");
}

#[test]
fn a_the_mode_its_project_needs_raises_an_item_and_clears_when_the_settings_follow() {
    let mut team = team();
    team.settings.branch_per_task = None;
    team.settings.base_branch.clear();
    assert!(rule(&breaches(&team), "a").is_some(), "{:?}", breaches(&team));
    // The owner's Allow writes the preset's settings.
    super::super::working_mode::apply(&mut team.settings, "branch-pipeline", "main").unwrap();
    team.working_mode = "branch-pipeline".into();
    assert!(rule(&breaches(&team), "a").is_none(), "{:?}", breaches(&team));
}

#[test]
fn b_accepted_work_off_the_line_raises_an_item_until_it_lands() {
    let mut team = team();
    let mut accepted = task(4, TaskStatus::Accepted);
    accepted.merge = Some(MergeEntry { state: "queued".into(), commit: "abc".into(), ..Default::default() });
    team.tasks.push(accepted);
    let found = rule(&breaches(&team), "b").expect("a breach for the queued landing");
    assert!(found.title.contains("#4"), "{found:?}");
    assert!(found.fix.contains("hgs team"), "{found:?}");
    // The queue landed it: the item clears.
    team.tasks[0].merge = Some(MergeEntry { state: "merged".into(), merged: "abc".into(), ..Default::default() });
    assert!(rule(&breaches(&team), "b").is_none(), "{:?}", breaches(&team));
}

#[test]
fn c_ready_work_with_nobody_on_it_raises_an_item() {
    let mut team = team();
    team.tasks.push(task(1, TaskStatus::Pending));
    assert!(rule(&breaches(&team), "c").is_some(), "{:?}", breaches(&team));
    // A member holds an attempt: the run is moving.
    team.tasks[0].attempts = vec![Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() }];
    assert!(rule(&breaches(&team), "c").is_none(), "{:?}", breaches(&team));
}

#[test]
fn d_a_whole_folder_output_raises_an_item_until_the_files_are_named() {
    let mut team = team();
    let mut broad = task(2, TaskStatus::Pending);
    broad.output = vec!["src".into()];
    team.tasks.push(broad);
    let found = rule(&breaches(&team), "d").expect("a breach for the folder output");
    assert!(found.title.contains("src"), "{found:?}");
    team.tasks[0].output = vec!["src/teams/playbook.rs".into()];
    assert!(rule(&breaches(&team), "d").is_none(), "{:?}", breaches(&team));
}

#[test]
fn e_the_lore_binary_needs_a_task_that_ships_it() {
    let mut team = team();
    team.settings.memory = Some(TeamMemory { mode: "project".into(), workspace: "w".into(), ..Default::default() });
    assert!(rule(&breaches(&team), "e").is_some(), "{:?}", breaches(&team));
    team.tasks.push(Task { title: "Ship the lore binary in the package".into(), description: "add it to the installer".into(), ..Default::default() });
    assert!(rule(&breaches(&team), "e").is_none(), "{:?}", breaches(&team));
}

#[test]
fn f_a_scratch_team_in_the_leads_session_is_flagged() {
    let mut team = team();
    team.scratch_parent = Some(ScratchParent { team: "parent".into(), task: 1, member: "qa".into() });
    // One session, one team: the scratch lead shares the lead's session.
    team.members[1].session = Some(lead_session("claude/x/lead", "r1"));
    let found = rule(&breaches(&team), "f").expect("a breach for the shared session");
    assert!(found.title.contains("scratch"), "{found:?}");
    // Its own session: the rule holds.
    team.members[1].session = Some(lead_session("claude/x/qa", "r2"));
    assert!(rule(&breaches(&team), "f").is_none(), "{:?}", breaches(&team));
}

#[test]
fn g_tests_only_in_a_report_raise_an_item_until_they_run() {
    let mut team = team();
    let mut accepted = task(5, TaskStatus::Accepted);
    accepted.attempts = vec![Attempt {
        n: 1,
        id: "a1".into(),
        member: "dev".into(),
        state: AttemptState::Reported,
        report: Some(Report { text: "done".into(), tests: Some(json!({"added":["t::tests::one"],"removed":[]})), ..Default::default() }),
        ..Default::default()
    }];
    team.tasks.push(accepted);
    assert!(rule(&breaches(&team), "g").is_some(), "{:?}", breaches(&team));
    // The merge queue ran the affected tests on the merged commit: it clears.
    team.tasks[0].merge = Some(MergeEntry { state: "testing".into(), requests: vec!["r1".into()], ..Default::default() });
    assert!(rule(&breaches(&team), "g").is_none(), "{:?}", breaches(&team));
}

#[test]
fn h_a_task_naming_another_base_raises_an_item() {
    let mut team = team();
    team.tasks.push(Task { id: 6, phase: 1, title: "Wire the view".into(), description: "Start from base branch: release, then land on the line".into(), ..Default::default() });
    let found = rule(&breaches(&team), "h").expect("a breach for the other base");
    assert!(found.title.contains("release"), "{found:?}");
    // The integration line is the base: no item.
    team.tasks[0].description = "Start from base branch: main, the integration line".into();
    assert!(rule(&breaches(&team), "h").is_none(), "{:?}", breaches(&team));
}

#[test]
fn i_a_real_format_token_raises_an_owner_item() {
    let mut team = team();
    team.tasks.push(Task { id: 7, phase: 1, title: "Fix the client".into(), description: "the vector is sk-abc123def456ghi789".into(), ..Default::default() });
    let found = rule(&breaches(&team), "i").expect("a breach for the token");
    assert!(found.owner, "a leaked secret is the owner's decision");
    assert!(found.title.contains("API key"), "{found:?}");
    team.tasks[0].description = "the vector is sk-test-placeholder".into();
    assert!(rule(&breaches(&team), "i").is_none(), "{:?}", breaches(&team));
}

#[test]
fn the_brief_carries_the_preset_the_decisions_the_breaches_and_the_landing() {
    let mut team = team();
    team.decisions.push(Decision {
        id: "d1".into(),
        question: "Wire the view mock".into(),
        choice: "the real API".into(),
        kind: "rule".into(),
        by: "You".into(),
        ..Default::default()
    });
    let mut landed = task(8, TaskStatus::Accepted);
    landed.merge = Some(MergeEntry { state: "merged".into(), merged: "abc123def456".into(), ..Default::default() });
    team.tasks.push(landed);
    let mut broad = task(9, TaskStatus::Pending);
    broad.output = vec!["client".into()];
    team.tasks.push(broad);
    let text = brief(&team);
    assert!(text.contains("branch-pipeline"), "{text}");
    assert!(text.contains("(d)"), "the breach's rule: {text}");
    assert!(text.contains("Wire the view mock -> the real API"), "the standing decision: {text}");
    assert!(text.contains("abc123def"), "the integration head: {text}");
    assert!(text.contains("Idle members"), "who idles: {text}");
    assert!(text.contains("After a compaction"), "{text}");
}

#[test]
fn the_brief_comes_at_start_at_a_phase_and_after_a_compaction() {
    use crate::state::teams::dispatcher::{steps_with, Seen, Step};
    let mut team = team();
    // The planning brief went; this is the method brief after it.
    team.brief = Notice::None;
    team.brief_id = "brief-1".into();
    let seen = Seen { host: String::new(), name: "claude/x/lead".into(), run_id: "r1".into(), conversation_id: "conv-1".into(), free: true, ..Default::default() };
    let now = crate::state::now();
    // The team started: the first brief.
    let steps = steps_with(&team, &[seen.clone()], now, None);
    assert!(
        steps.iter().any(|step| matches!(step, Step::LeadBrief { reason } if reason == "the team started")),
        "{steps:?}"
    );
    // The brief went: the same session and phase ask for nothing more.
    team.lead_briefed = Some(json!({"reason":"the team started","run":"r1","conversation":"conv-1","phase":0,"used":800_000,"at":now}));
    let steps = steps_with(&team, &[seen.clone()], now, None);
    assert!(!steps.iter().any(|step| matches!(step, Step::LeadBrief { .. })), "{steps:?}");
    // A new phase starts: a pending task sits in phase 2.
    team.tasks.push(task(10, TaskStatus::Pending));
    team.tasks[0].phase = 2;
    let steps = steps_with(&team, &[seen.clone()], now, None);
    assert!(
        steps.iter().any(|step| matches!(step, Step::LeadBrief { reason } if reason == "the phase started")),
        "{steps:?}"
    );
    // The context was compacted: the reading fell well below the last brief.
    team.lead_briefed = Some(json!({"reason":"the phase started","run":"r1","conversation":"conv-1","phase":2,"used":800_000,"at":now}));
    team.members[0].context = Some(MemberContext::of(120_000, 1_000_000, now));
    let steps = steps_with(&team, &[seen], now, None);
    assert!(
        steps.iter().any(|step| matches!(step, Step::LeadBrief { reason } if reason == "the lead's context was compacted or cleared")),
        "{steps:?}"
    );
}

#[test]
fn the_items_reach_the_leads_inbox_and_clear() {
    let mut team = team();
    team.playbook = Some(playbook(&team));
    team.tasks.push(Task { id: 11, phase: 1, title: "Big".into(), output: vec!["docs".into()], ..Default::default() });
    let items = crate::state::teams::inbox::all_items(&team);
    let playbook: Vec<&crate::state::teams::inbox::Item> = items.iter().filter(|item| item.key == "playbook:d").collect();
    assert_eq!(playbook.len(), 1, "{items:?}");
    assert!(playbook[0].command.contains("hgs team"), "{:?}", playbook[0].command);
    team.tasks[0].output = vec!["docs/reference.md".into()];
    let items = crate::state::teams::inbox::all_items(&team);
    assert!(!items.iter().any(|item| item.key == "playbook:d"), "{items:?}");
}

/// Task #404 (owner, 2026-11-09): the brief that comes back after a compaction
/// carries the lead's speed rules.
#[test]
fn the_brief_says_to_answer_the_owner_first_and_run_long_work_in_the_background() {
    let team = team();
    let text = brief(&team).to_lowercase();
    for rule in ["answer the owner first", "one team command per tool call", "run in the background", "hgs team status --task", "bottleneck"] {
        assert!(text.contains(rule), "no rule: {rule}");
    }
}

/// Task #19 (team ede724c4, owner 2026-11-10): a lead who lands by hand on a
/// named line, or a team created with working mode "none", gets no (a), (b) or
/// (h): every one of them used to fire for each accepted task.
#[test]
fn a_manual_line_and_working_mode_none_raise_no_a_b_or_h() {
    let loud = |team: &Team| -> Vec<&'static str> {
        breaches(team).iter().map(|breach| breach.rule).filter(|rule| matches!(*rule, "a" | "b" | "h")).collect()
    };
    let mut team = team();
    // Settings off the mode (a), accepted work with a stale landing (b) and a
    // task naming another base (h).
    team.settings.branch_per_task = None;
    let mut accepted = task(4, TaskStatus::Accepted);
    accepted.merge = Some(MergeEntry { state: "queued".into(), commit: "abc".into(), ..Default::default() });
    team.tasks.push(accepted);
    team.tasks.push(Task { id: 6, phase: 1, title: "Wire the view".into(), description: "Start from base branch: release, then land on the line".into(), ..Default::default() });
    let before = loud(&team);
    assert!(before.contains(&"a") && before.contains(&"b") && before.contains(&"h"), "{before:?}");
    let mut manual = team.clone();
    manual.settings.manual_line = "team/ede724c4/line".into();
    assert!(loud(&manual).is_empty(), "{:?}", loud(&manual));
    let mut none = team.clone();
    none.working_mode = "none".into();
    assert!(loud(&none).is_empty(), "{:?}", loud(&none));
    // A team New team created with "Keep the settings as they are" (task #13).
    let mut kept = team.clone();
    kept.working_mode.clear();
    kept.mode_kept = Some(json!({"preset":"branch-pipeline","fingerprint":"f","at":1.0,"by":"New team"}));
    assert!(loud(&kept).is_empty(), "{:?}", loud(&kept));
}
