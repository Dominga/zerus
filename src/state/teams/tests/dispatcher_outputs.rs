// Task 155 (owner, 2026-11-07): tasks that write the same files never run
// apart, and a dependent task starts from its predecessors' accepted work.
use super::*;

fn outputs(team: &mut Team, id: u32, paths: &[&str]) {
    team.task_mut(id).unwrap().output = paths.iter().map(|path| path.to_string()).collect();
}

fn overlap_errors(team: &Team) -> Vec<plan::Finding> {
    plan::validate(team, &BTreeSet::new()).errors.into_iter().filter(|finding| finding.kind == "outputs").collect()
}

#[test]
fn overlapping_outputs_must_be_chained_by_the_plan() {
    let mut team = team();
    // #1 and #2 both write crates/lore/src; #3 writes elsewhere.
    outputs(&mut team, 1, &["crates/lore/src"]);
    outputs(&mut team, 2, &["crates/lore/src"]);
    outputs(&mut team, 3, &["docs/lore.md"]);
    let found = overlap_errors(&team);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!((found[0].task, found[0].tasks.clone()), (Some(2), vec![1, 2]));
    assert!(found[0].text.contains("#1 API and #2 More API both write crates/lore/src") && found[0].text.contains("chain them"), "{}", found[0].text);
    // A soft dependency lets both run at once: still an error.
    team.task_mut(2).unwrap().deps = vec![Dependency { task: 1, kind: DependencyKind::Soft }];
    assert_eq!(overlap_errors(&team).len(), 1);
    // A hard one, or every dependency hard, chains them.
    team.settings.all_hard = true;
    assert!(overlap_errors(&team).is_empty());
    team.settings.all_hard = false;
    team.task_mut(2).unwrap().deps = vec![Dependency { task: 1, kind: DependencyKind::Hard }];
    assert!(overlap_errors(&team).is_empty());
    // Through another task too: #3 waits for #2, which waits for #1.
    outputs(&mut team, 3, &["crates/lore/src/cli.rs"]);
    assert_eq!(overlap_errors(&team).iter().map(|finding| finding.tasks.clone()).collect::<Vec<_>>(), vec![vec![1, 3], vec![2, 3]]);
    team.task_mut(3).unwrap().deps = vec![Dependency { task: 2, kind: DependencyKind::Hard }];
    assert!(overlap_errors(&team).is_empty());
    // A decided task no longer holds its outputs.
    team.task_mut(3).unwrap().deps.clear();
    for id in [1, 2] {
        team.task_mut(id).unwrap().status = TaskStatus::Accepted;
    }
    assert!(overlap_errors(&team).is_empty());
}

/// Task 259 (owner, 2026-11-08): a folder-level output is advisory. A bare
/// `src`, `tray`, `client/src` or `crates/replica` names a whole area, so it
/// never holds another task; the plan warns and asks for files or module paths.
#[test]
fn a_folder_level_output_is_advisory_and_never_holds_a_whole_area() {
    for broad in ["src", "tray", "client", "client/src", "crates/lore", "mobile/android", "."] {
        assert!(Task::broad_output(broad), "{broad}");
    }
    // Files, module paths and known dotless root files are real outputs.
    for narrow in ["src/api.rs", "src/state/teams", "src/protocol", "Cargo.toml", "Makefile", "client/src/ui/teams/Team.tsx", "docs/specs/x.md"] {
        assert!(!Task::broad_output(narrow), "{narrow}");
    }
    let mut team = team();
    outputs(&mut team, 1, &["crates/lore"]);
    outputs(&mut team, 2, &["crates/lore/src/search.rs"]);
    // The overlap is only folder-level: a warning, never a chain error.
    assert!(overlap_errors(&team).is_empty());
    let warnings: Vec<plan::Finding> = plan::validate(&team, &BTreeSet::new()).warnings.into_iter().filter(|finding| finding.kind == "outputs").collect();
    assert!(
        warnings.iter().any(|finding| finding.task == Some(1) && finding.text.contains("names whole areas (crates/lore)") && finding.text.contains("advisory")),
        "{warnings:?}"
    );
    assert!(
        warnings.iter().any(|finding| finding.tasks == vec![1, 2] && finding.text.contains("soft dependency") && finding.text.contains("folder-level output is advisory")),
        "{warnings:?}"
    );
    // In progress, #1's whole crate holds nothing; #2 starts beside it.
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    assert!(!plan::holds_outputs(team.task(1).unwrap()));
    assert!(plan::output_holders(&team, team.task(2).unwrap()).is_empty());
    // A module path still holds the file inside it.
    outputs(&mut team, 1, &["crates/lore/src"]);
    outputs(&mut team, 2, &["crates/lore/src/search.rs"]);
    assert!(plan::holds_outputs(team.task(1).unwrap()));
    assert_eq!(plan::output_holders(&team, team.task(2).unwrap()), vec![1]);
}

#[test]
fn a_plan_or_a_phase_with_unchained_overlapping_outputs_is_refused() {
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let (mut team, _) = ops::create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    ops::apply(&mut team, "plan", &json!({"tasks":[
        {"id":1,"title":"Lore core","assignee":"lead","output":["crates/lore/src"]},
        {"id":2,"title":"Lore search","assignee":"lead","output":["crates/lore/src/search.rs"]}]}), &Actor::Human, &none, &mut events).unwrap();
    let error = ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("both write crates/lore/src") && error.contains("hard dependency of #2 on #1"), "{error}");
    // Chained, the plan runs.
    team.task_mut(2).unwrap().deps = vec![Dependency { task: 1, kind: DependencyKind::Hard }];
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &none, &mut events).unwrap();
    // A later phase whose task writes the same module is refused until it is
    // chained to it.
    let error = ops::apply(&mut team, "extend", &json!({"tasks":[{"id":3,"title":"Lore CLI","assignee":"lead","output":["crates/lore/src/cli.rs"]}]}),
        &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("phase error") && error.contains("#3 Lore CLI"), "{error}");
    ops::apply(&mut team, "extend", &json!({"tasks":[{"id":3,"title":"Lore CLI","assignee":"lead","output":["crates/lore/src/cli.rs"],
        "deps":[{"task":2,"kind":"hard"}]}]}), &Actor::Human, &none, &mut events).unwrap();
    // Task 259: a whole area is no longer refused as a dependency chain - it
    // is advisory, and the extend's validation carries the warning to name
    // the files or module paths instead.
    let extended = ops::apply(&mut team, "extend", &json!({"tasks":[
        {"id":4,"title":"Crate sweep","assignee":"lead","output":["crates/lore"]}]}),
        &Actor::Human, &none, &mut events).unwrap();
    let warnings = extended["validation"]["warnings"].as_array().cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|warning| warning["kind"] == "outputs" && warning["text"].as_str().unwrap_or_default().contains("names whole areas (crates/lore)")),
        "{extended}"
    );
}

#[test]
fn a_task_never_starts_beside_an_open_task_writing_the_same_files() {
    let (mut team, seen) = team_with_two_backends();
    // #1 runs on dev; #2 is dev2's and writes inside #1's output.
    team.task_mut(1).unwrap().attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    team.task_mut(2).unwrap().assignee = "dev2".into();
    outputs(&mut team, 1, &["crates/lore/src"]);
    outputs(&mut team, 2, &["crates/lore/src/cli.rs"]);
    let started = |team: &Team| steps(team, &seen, 1.0).into_iter().filter(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })).count();
    assert_eq!(started(&team), 0, "a free member never runs it beside #1");
    assert_eq!(plan::output_holders(&team, team.task(2).unwrap()), vec![1]);
    assert!(!can_hand_out(&team, 2), "the hand-out itself rechecks it");
    // Reported and undecided, #1's work is still on its own branch.
    team.task_mut(1).unwrap().status = TaskStatus::Submitted;
    team.task_mut(1).unwrap().attempts[0].state = AttemptState::Reported;
    assert_eq!(started(&team), 0);
    // Sent back, #1 still holds its outputs while it waits for dev again.
    team.task_mut(1).unwrap().status = TaskStatus::Pending;
    team.task_mut(1).unwrap().attempts[0].state = AttemptState::Rejected;
    assert_eq!(plan::output_holders(&team, team.task(2).unwrap()), vec![1]);
    // Decided, #2 starts.
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    assert!(steps(&team, &seen, 1.0).contains(&Step::HandOut { task: 2, member: "dev2".into() }));
    assert!(can_hand_out(&team, 2));
    // Separate files run side by side as before.
    team.task_mut(1).unwrap().status = TaskStatus::InProgress;
    outputs(&mut team, 2, &["crates/watch/src"]);
    assert!(steps(&team, &seen, 1.0).contains(&Step::HandOut { task: 2, member: "dev2".into() }));
}

#[test]
fn two_ready_tasks_writing_the_same_files_start_one_at_a_time() {
    let (mut team, seen) = team_with_two_backends();
    team.task_mut(2).unwrap().assignee = "dev2".into();
    outputs(&mut team, 1, &["crates/lore/src"]);
    outputs(&mut team, 2, &["crates/lore/src"]);
    // Both members are free; only the first in the dispatch order starts.
    let now = steps(&team, &seen, 1.0);
    assert!(now.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{now:?}");
    assert!(!now.iter().any(|step| matches!(step, Step::HandOut { task: 2, .. } | Step::Rebalance { task: 2, .. })), "{now:?}");
    // Two returned tasks never wait on each other: the lower number goes first.
    for (id, member) in [(1, "dev"), (2, "dev2")] {
        team.task_mut(id).unwrap().attempts.push(Attempt { n: 1, member: member.into(), state: AttemptState::Rejected, ..Default::default() });
    }
    assert!(plan::output_holders(&team, team.task(1).unwrap()).is_empty());
    assert_eq!(plan::output_holders(&team, team.task(2).unwrap()), vec![1]);
    // A refused hand-out never reached anyone and holds nothing.
    let mut refused = team.clone();
    refused.task_mut(1).unwrap().attempts[0].state = AttemptState::Refused;
    assert!(!plan::holds_outputs(refused.task(1).unwrap()));
}

#[test]
fn a_dependent_task_starts_from_its_accepted_predecessors_commits() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        // dev2 works in its own clone: its accepted commit is not in dev's worktree.
        let other = root.path().join("dev2");
        git_in(root.path(), &["clone", "-q", &lead.display().to_string(), &other.display().to_string()]);
        git_in(&other, &["config", "user.name", "Zerus Test"]);
        git_in(&other, &["config", "user.email", "test@example.com"]);
        git_in(&other, &["checkout", "-q", "-b", "team/x/dev2"]);
        std::fs::write(other.join("lore.txt"), "core\n").unwrap();
        git_in(&other, &["add", "-A"]);
        git_in(&other, &["commit", "-q", "-m", "Lore core"]);
        let core = git_in(&other, &["rev-parse", "HEAD"]);
        std::fs::write(other.join("gate.txt"), "dev2\n").unwrap();
        git_in(&other, &["commit", "-q", "-am", "Gate by dev2"]);
        let gate = git_in(&other, &["rev-parse", "HEAD"]);
        let mut team = stage_team(&lead, &dev, "none", &|_| super::super::super::catalog::builtin());
        let mut dev2 = team.member("dev").unwrap().clone();
        dev2.id = "dev2".into();
        dev2.folder = MemberFolder::Clone;
        dev2.directory = other.display().to_string();
        dev2.session = Some(SessionRef { name: "codex/x/dev2".into(), run_id: "r3".into(), ..Default::default() });
        team.members.push(dev2);
        let accepted = |id: u32, title: &str, commit: &str| Task {
            id,
            title: title.into(),
            assignee: "dev2".into(),
            status: TaskStatus::Accepted,
            attempts: vec![Attempt { n: 1, member: "dev2".into(), state: AttemptState::Reported, folder_head: commit.into(), folder_branch: "team/x/dev2".into(), ..Default::default() }],
            ..Default::default()
        };
        team.tasks.push(accepted(3, "Lore core", &core));
        team.tasks.push(accepted(4, "Gate by dev2", &gate));
        team.task_mut(1).unwrap().deps = vec![Dependency { task: 3, kind: DependencyKind::Hard }];
        team.task_mut(2).unwrap().deps = vec![Dependency { task: 4, kind: DependencyKind::Hard }];
        let mut store = Store::open_at(&super::super::super::store::path(), true).unwrap();
        store.create(&uuid::Uuid::new_v4().to_string(), &json!({}), team, vec![]).unwrap();
        // #1 depends on the accepted #3: its commit is fetched from dev2's
        // clone and merged into dev's branch before the hand-out says so.
        let texts = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let sent = texts.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            assert_eq!(args.first().copied(), Some("send"));
            sent.borrow_mut().push(args.join(" "));
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        hand_out(&mut store, STAGE_TEAM, 1, "dev", None, "").unwrap();
        let saved = store.load(STAGE_TEAM).unwrap().unwrap();
        let attempt = saved.task(1).unwrap().attempts.last().unwrap().clone();
        assert_eq!(attempt.state, AttemptState::Delivered);
        let step = attempt.prepare.iter().find(|step| step.step == "dependencies").expect("the merge step goes with the attempt");
        assert_eq!((step.to.as_str(), step.result.as_str()), ("#3", "done"), "{step:?}");
        assert!(step.reason.contains(&format!("merged #3 ({})", &core[..9])), "{step:?}");
        assert!(!git_fails(&dev, &["merge-base", "--is-ancestor", &core, "HEAD"]), "dev's branch holds #3's accepted commit");
        assert_eq!(std::fs::read_to_string(dev.join("lore.txt")).unwrap().trim(), "core");
        assert_eq!(git_in(&dev, &["status", "--porcelain"]), "");
        let text = hand_off_text(&saved, saved.task(1).unwrap(), &attempt, "", "");
        assert!(text.contains("Zerus merged #3") && text.contains("into your branch before this hand-out"), "{text}");
        // Once it is there, the next hand-out finds nothing to merge.
        assert_eq!(prepare_dependencies(&saved, 1, "dev").unwrap().result, "skipped");
        // Nothing accepted yet, or the lead's own task: no step at all.
        assert!(prepare_dependencies(&saved, 3, "dev").is_none());
        assert!(prepare_dependencies(&saved, 1, "lead").is_none());
        // #2 depends on #4, whose commit conflicts with dev's own gate.txt: the
        // merge is aborted, the folder put back and the lead told; nothing is
        // left to the member and nothing is sent.
        std::fs::write(dev.join("gate.txt"), "dev\n").unwrap();
        git_in(&dev, &["commit", "-q", "-am", "Gate by dev"]);
        let before = git_in(&dev, &["rev-parse", "HEAD"]);
        let count = texts.borrow().len();
        hand_out(&mut store, STAGE_TEAM, 2, "dev", None, "").unwrap();
        set_fake_hgs(None);
        assert_eq!(texts.borrow().len(), count, "nothing is sent while the merge conflicts");
        assert_eq!(git_in(&dev, &["rev-parse", "HEAD"]), before);
        assert_eq!(git_in(&dev, &["status", "--porcelain", "--untracked-files=no"]), "");
        assert!(git_fails(&dev, &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"]), "no merge is left in progress");
        let saved = store.load(STAGE_TEAM).unwrap().unwrap();
        let task = saved.task(2).unwrap();
        assert!(task.attempts.is_empty());
        let waits = task.waits.as_ref().unwrap();
        assert_eq!((waits.on.as_slice(), waits.state.as_str()), (&["dependencies".to_owned()][..], "refused"));
        assert!(waits.reason.contains("#4's accepted commit") && waits.reason.contains("conflicts in gate.txt") && waits.reason.contains("put the folder back"), "{}", waits.reason);
        let item = inbox::items(&saved).into_iter().find(|item| item.kind == "task_waits" && item.task == Some(2)).expect("one inbox item for the lead");
        assert!(item.summary.contains("gate.txt") && item.command.contains("hgs team retry"), "{item:?}");
        assert!(!plan::ready(&saved).contains(&2), "never retried on its own");
    });
}

/// Task #15 (team ede724c4; lead of team 2b863c49, 2026-11-10): a team with a
/// base branch and no integration folder. C1 and T03 both append to
/// tools/check.py and the lead resolved them on svn-art: a dependent of both
/// hands out from the line's head without a conflict and merges neither
/// commit again. A predecessor not on the line (D1) is still merged, and one
/// that really conflicts with the line (E1) still refuses with its files.
#[test]
fn a_hand_out_starts_from_the_line_head_not_commit_by_commit() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        // The line: svn-art, with the file every task appends to.
        git_in(&lead, &["checkout", "-q", "-b", "svn-art"]);
        std::fs::create_dir_all(lead.join("tools")).unwrap();
        std::fs::write(lead.join("tools/check.py"), "base\n").unwrap();
        git_in(&lead, &["add", "-A"]);
        git_in(&lead, &["commit", "-q", "-m", "check.py"]);
        // dev2's clone: four accepted commits, each on its own branch from the line.
        let other = root.path().join("dev2");
        git_in(root.path(), &["clone", "-q", &lead.display().to_string(), &other.display().to_string()]);
        git_in(&other, &["config", "user.name", "Zerus Test"]);
        git_in(&other, &["config", "user.email", "test@example.com"]);
        let commit = |branch: &str, file: &str, text: &str| -> String {
            git_in(&other, &["checkout", "-q", "-b", branch, "origin/svn-art"]);
            let path = other.join(file);
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            std::fs::write(&path, format!("{old}{text}\n")).unwrap();
            git_in(&other, &["add", "-A"]);
            git_in(&other, &["commit", "-q", "-m", branch]);
            git_in(&other, &["rev-parse", "HEAD"])
        };
        let c1 = commit("c1", "tools/check.py", "c1");
        let t03 = commit("t03", "tools/check.py", "t03");
        let d1 = commit("d1", "d1.txt", "d1");
        let e1 = commit("e1", "tools/check.py", "e1");
        // The lead merges C1 and T03 into svn-art and resolves their conflict once.
        git_in(&lead, &["fetch", "-q", &other.display().to_string(), "c1", "t03"]);
        git_in(&lead, &["merge", "-q", "--no-ff", "--no-edit", &c1]);
        assert!(git_fails(&lead, &["merge", "-q", "--no-ff", "--no-edit", &t03]), "C1 and T03 conflict");
        std::fs::write(lead.join("tools/check.py"), "base\nc1\nt03\n").unwrap();
        git_in(&lead, &["add", "-A"]);
        git_in(&lead, &["commit", "-q", "--no-edit"]);
        let line = git_in(&lead, &["rev-parse", "HEAD"]);
        let mut team = stage_team(&lead, &dev, "none", &|_| super::super::super::catalog::builtin());
        team.settings.base_branch = "svn-art".into();
        let mut dev2 = team.member("dev").unwrap().clone();
        dev2.id = "dev2".into();
        dev2.folder = MemberFolder::Clone;
        dev2.directory = other.display().to_string();
        dev2.session = Some(SessionRef { name: "codex/x/dev2".into(), run_id: "r3".into(), ..Default::default() });
        team.members.push(dev2);
        let accepted = |id: u32, branch: &str, commit: &str| Task {
            id,
            title: branch.into(),
            assignee: "dev2".into(),
            status: TaskStatus::Accepted,
            attempts: vec![Attempt { n: 1, member: "dev2".into(), state: AttemptState::Reported, folder_head: commit.into(), folder_branch: branch.into(), ..Default::default() }],
            ..Default::default()
        };
        team.tasks.push(accepted(3, "c1", &c1));
        team.tasks.push(accepted(4, "t03", &t03));
        team.tasks.push(accepted(5, "d1", &d1));
        team.tasks.push(accepted(6, "e1", &e1));
        let hard = |task: u32| Dependency { task, kind: DependencyKind::Hard };
        team.task_mut(1).unwrap().deps = vec![hard(3), hard(4)];
        team.task_mut(2).unwrap().deps = vec![hard(5)];
        team.tasks.push(Task { id: 8, title: "After E1".into(), assignee: "dev".into(), deps: vec![hard(6)], ..Default::default() });
        // 1. Both resolved predecessors: the branch comes up to svn-art's head,
        // with no conflict, and neither commit is merged again.
        let step = prepare_dependencies(&team, 1, "dev").expect("a step");
        assert_eq!(step.result, "done", "{step:?}");
        assert!(step.reason.contains(&format!("the line svn-art ({})", &line[..9])), "{step:?}");
        assert!(!step.reason.contains("#3 (") && !step.reason.contains("#4 ("), "on the line already, not merged again: {step:?}");
        assert_eq!(std::fs::read_to_string(dev.join("tools/check.py")).unwrap().replace("\r\n", "\n"), "base\nc1\nt03\n");
        assert!(!git_fails(&dev, &["merge-base", "--is-ancestor", &line, "HEAD"]));
        assert_eq!(git_in(&dev, &["status", "--porcelain"]), "");
        // Again: nothing to do.
        assert_eq!(prepare_dependencies(&team, 1, "dev").unwrap().result, "skipped");
        // 2. A predecessor the line does not hold yet is still merged.
        let step = prepare_dependencies(&team, 2, "dev").expect("a step");
        assert_eq!(step.result, "done", "{step:?}");
        assert!(step.reason.contains(&format!("#5 ({})", &d1[..9])), "{step:?}");
        assert!(dev.join("d1.txt").exists());
        // 3. A real conflict with the line still refuses with its files and
        // leaves the folder as it was.
        let before = git_in(&dev, &["rev-parse", "HEAD"]);
        let step = prepare_dependencies(&team, 8, "dev").expect("a step");
        assert_eq!(step.result, "refused", "{step:?}");
        assert!(step.reason.contains("#6's accepted commit") && step.reason.contains("conflicts in tools/check.py"), "{step:?}");
        assert_eq!(git_in(&dev, &["rev-parse", "HEAD"]), before);
        assert!(git_fails(&dev, &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"]), "no merge is left in progress");
    });
}

/// Task #15: a predecessor the line holds as an equivalent commit (the lead
/// cherry-picked it) is not merged again, and a member in a separate clone
/// gets the line's head from the team folder.
#[test]
fn a_cherry_picked_predecessor_on_the_line_is_not_merged_again() {
    let root = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(root.path(), || {
        let (lead, dev) = stage_repo(root.path());
        git_in(&lead, &["checkout", "-q", "-b", "line"]);
        let other = root.path().join("dev2");
        git_in(root.path(), &["clone", "-q", &lead.display().to_string(), &other.display().to_string()]);
        git_in(&other, &["config", "user.name", "Zerus Test"]);
        git_in(&other, &["config", "user.email", "test@example.com"]);
        git_in(&other, &["checkout", "-q", "-b", "w1", "origin/line"]);
        std::fs::write(other.join("w1.txt"), "w1\n").unwrap();
        git_in(&other, &["add", "-A"]);
        git_in(&other, &["commit", "-q", "-m", "w1"]);
        let w1 = git_in(&other, &["rev-parse", "HEAD"]);
        // The lead cherry-picks it: the line holds the change, not the commit.
        git_in(&lead, &["fetch", "-q", &other.display().to_string(), "w1"]);
        git_in(&lead, &["cherry-pick", &w1]);
        let mut team = stage_team(&lead, &dev, "none", &|_| super::super::super::catalog::builtin());
        team.settings.base_branch = "line".into();
        let mut dev2 = team.member("dev").unwrap().clone();
        dev2.id = "dev2".into();
        dev2.folder = MemberFolder::Clone;
        dev2.directory = other.display().to_string();
        team.members.push(dev2);
        team.tasks.push(Task {
            id: 3,
            title: "w1".into(),
            assignee: "dev2".into(),
            status: TaskStatus::Accepted,
            attempts: vec![Attempt { n: 1, member: "dev2".into(), state: AttemptState::Reported, folder_head: w1.clone(), folder_branch: "w1".into(), ..Default::default() }],
            ..Default::default()
        });
        team.task_mut(1).unwrap().deps = vec![Dependency { task: 3, kind: DependencyKind::Hard }];
        let step = prepare_dependencies(&team, 1, "dev").expect("a step");
        assert!(matches!(step.result.as_str(), "done" | "skipped"), "{step:?}");
        assert!(!step.reason.contains("#3 ("), "the cherry-picked commit is on the line: {step:?}");
        assert!(dev.join("w1.txt").exists(), "the line brought the change");
    });
}
