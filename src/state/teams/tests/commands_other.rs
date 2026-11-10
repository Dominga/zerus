use super::*;
use super::support::*;

/// U54: one owner action moves the members on the account through U53's
/// switch (keeping the session) and reports a switch that does not confirm.
#[test]
fn a_bulk_swap_moves_the_lead_through_u53_and_reports_a_refusal() {
    use super::*;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        live_sessions_override(Some(vec![crate::state::teams::dispatcher::Seen {
            name: "claude/x/lead".into(),
            run_id: "r0".into(),
            account_id: "x".into(),
            ..Default::default()
        }]));
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let team = running_team();
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        set_test_switch(Some(switched_switch));
        let receipt = run(
            "swap-member",
            &json!({"request_id":"22222222-2222-4222-8222-222222222222","team":id,
                "from_account":"x","to_account":"y","provider":"claude","by":"human"}),
            false,
        )
        .unwrap();
        assert_eq!(receipt["status"], "swapped", "{receipt}");
        assert_eq!(
            SWITCH_CALLS.with(|calls| calls.borrow().clone()),
            vec![("claude/x/lead".to_owned(), "y".to_owned())]
        );
        let stored = store.load(&id).unwrap().unwrap();
        let lead = stored.member("lead").unwrap();
        assert_eq!(lead.account, "y");
        assert!(lead.session.is_some());
        // A switch that does not confirm moves nobody and names the reason.
        set_test_switch(Some(uncertain_switch));
        let error = run(
            "swap-member",
            &json!({"request_id":"33333333-3333-4333-8333-333333333333","team":id,
                "from_account":"x","to_account":"z","provider":"claude","by":"human"}),
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("could move to account z"), "{error}");
        set_test_switch(None);
        live_sessions_override(None);
    });
}

/// U54: a bulk swap selects the members of one provider whose session
/// really runs on the account, the lead included, and the roster reads the
/// real account from the session record.
#[test]
fn a_bulk_swap_selects_members_by_their_sessions_real_account() {
    use crate::state::teams::dispatcher::Seen;
    use crate::state::teams::model::{Member, SessionRef, Team, TeamStatus};
    let seen = |name: &str, account: &str| Seen {
        name: name.into(),
        run_id: format!("{name}-run"),
        account_id: account.into(),
        ..Default::default()
    };
    let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
    let member = |id: &str, agent: &str, name: &str, account: &str| Member {
        id: id.into(),
        role: id.into(),
        agent: agent.into(),
        account: account.into(),
        session: Some(SessionRef { name: name.into(), run_id: format!("{name}-run"), ..Default::default() }),
        ..Default::default()
    };
    team.members = vec![
        member("lead", "claude", "claude/x/lead", "x"),
        member("dev", "claude", "claude/x/dev", "x"),
        member("qa", "codex", "codex/x/qa", "x"),
    ];
    let sessions = vec![seen("claude/x/lead", "x"), seen("claude/x/dev", "x"), seen("codex/x/qa", "x")];
    assert_eq!(bulk_swap_members(&team, &sessions, "x", "claude"), ["lead".to_owned(), "dev".to_owned()]);
    assert_eq!(bulk_swap_members(&team, &sessions, "x", ""), ["lead".to_owned(), "dev".to_owned(), "qa".to_owned()]);
    let elsewhere = vec![seen("claude/x/lead", "y"), seen("claude/x/dev", "x"), seen("codex/x/qa", "x")];
    assert_eq!(bulk_swap_members(&team, &elsewhere, "x", "claude"), ["dev".to_owned()]);
    let native = vec![seen("claude/x/lead", ""), seen("claude/x/dev", "x")];
    assert_eq!(bulk_swap_members(&team, &native, "native-claude", "claude"), ["lead".to_owned()]);
    // `hgs ls` names a default-account session "native-claude": "", "native"
    // and "native-claude" match it alike.
    let listed = vec![seen("claude/x/lead", "native-claude"), seen("claude/x/dev", "native"), seen("codex/x/qa", "x")];
    for from in ["", "native", "native-claude"] {
        assert_eq!(bulk_swap_members(&team, &listed, from, "claude"), ["lead".to_owned(), "dev".to_owned()], "{from}");
    }
    assert!(bulk_swap_members(&team, &listed, "x", "claude").is_empty());
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut record = json!({"version":1,"name":"claude/x/dev","agent":"claude","account_id":"y"});
        crate::state::storage::write(&mut record).unwrap();
        assert_eq!(session_account(team.member("dev").unwrap()), "y");
        // A session without a readable record keeps its configured account.
        assert_eq!(session_account(team.member("qa").unwrap()), "x");
    });
}

/// U54: the members a bulk move takes are all swapped: the moved ones keep
/// their session, the fresh ones get a new one on the target account, and
/// an all-fresh move is not read as a single-member swap.
#[test]
fn a_bulk_swap_swaps_moved_fresh_and_mixed_members() {
    let swap = |moved: &[&str], fresh: &[&str]| {
        let mut team = running_team();
        let owned = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        let notes = fresh.iter().map(|id| json!({"member":id,"note":"the old session was paused and saved as x"})).collect();
        let mut input = json!({"from_account":"x","to_account":"y"});
        fill_bulk_swap(&mut input, &owned(moved), &owned(fresh), notes, Vec::new(), "y");
        let mut events = Vec::new();
        let receipt = ops::apply(&mut team, "swap-member", &input, &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        (team, receipt, events)
    };
    // Moved only: the lead keeps its session and takes the new account.
    let (team, receipt, _) = swap(&["lead"], &[]);
    assert_eq!(receipt["status"], "swapped", "{receipt}");
    let lead = team.member("lead").unwrap();
    assert_eq!((lead.account.as_str(), lead.session.as_ref().map(|s| s.name.as_str())), ("y", Some("claude/x/lead")));
    assert!(!lead.launch_after_swap);
    // Fresh only: the member is swapped, not refused as a single swap.
    let (team, receipt, events) = swap(&[], &["dev"]);
    assert_eq!(receipt["fresh"], json!(["dev"]), "{receipt}");
    let dev = team.member("dev").unwrap();
    assert!(dev.session.is_none() && dev.launch_after_swap && dev.state == MemberState::NotCreated, "{dev:?}");
    assert_eq!(dev.account, "y");
    assert!(events.iter().any(|event| event.kind == "member_retired"));
    // Mixed: each one its own way.
    let (team, receipt, _) = swap(&["lead"], &["dev"]);
    assert_eq!(receipt["members"].as_array().map(Vec::len), Some(2), "{receipt}");
    assert!(team.member("lead").unwrap().session.is_some());
    assert!(team.member("dev").unwrap().session.is_none() && team.member("dev").unwrap().launch_after_swap);
}

/// U54: a bulk swap keeps each member's session, conversation and open
/// attempt and only changes the account.
#[test]
fn a_bulk_swap_keeps_each_members_session_and_open_attempt() {
    use super::*;
    use crate::state::teams::model::{Attempt, AttemptState, Member, SessionRef, Task, TaskStatus, Team, TeamStatus};
    let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
    let member = |id: &str| Member {
        id: id.into(),
        role: id.into(),
        agent: "claude".into(),
        account: "x".into(),
        session: Some(SessionRef { name: format!("claude/x/{id}"), run_id: "r".into(), ..Default::default() }),
        ..Default::default()
    };
    team.members = vec![member("lead"), member("dev")];
    team.tasks.push(Task {
        id: 1,
        title: "A".into(),
        assignee: "dev".into(),
        status: TaskStatus::InProgress,
        attempts: vec![Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() }],
        ..Default::default()
    });
    let mut events = Vec::new();
    let result = ops::apply(
        &mut team,
        "swap-member",
        &json!({"members":["lead","dev"],"bulk":true,"account":"y"}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(result["status"], "swapped", "{result}");
    assert_eq!(result["members"].as_array().unwrap().len(), 2);
    for id in ["lead", "dev"] {
        let found = team.member(id).unwrap();
        assert_eq!(found.account, "y");
        assert!(found.session.is_some());
        assert!(!found.launch_after_swap);
    }
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.attempts[0].state, AttemptState::Delivered);
    assert!(task.continuation.is_none());
    assert!(events.iter().any(|event| event.kind == "member_swapped"));
}

/// U54: a swap never interrupts a session that is busy or unreachable; the
/// member waits with that reason and the session keeps running.
#[test]
fn a_busy_old_session_is_never_interrupted_by_a_swap() {
    use super::*;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut record = json!({"version":1,"name":"claude/x/dev","agent":"claude","run_id":"r","conversation_id":"c"});
        crate::state::storage::write(&mut record).unwrap();
        let error = retire_old_session("claude/x/dev", "second").unwrap_err();
        assert!(error.contains("waiting for the old session"), "{error}");
        let saved = crate::state::storage::read("claude/x/dev").unwrap();
        assert!(saved.get("paused").is_none(), "a busy session is not paused");
    });
}

/// U54: a renamed session stays the member. The caller matches by its run
/// id or conversation id as well as by name, and the team record follows the
/// rename once (the next command matches by name again).
#[test]
fn a_renamed_session_stays_the_member_and_the_team_follows_it() {
    use crate::state::teams::model::{Member, SessionRef, Team, TeamStatus};
    let mut team = Team { id: "t".into(), name: "T".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
    team.members.push(Member {
        id: "lead".into(),
        role: "Lead".into(),
        session: Some(SessionRef {
            name: "claude/x/old".into(),
            run_id: "r1".into(),
            conversation_id: "c1".into(),
            ..Default::default()
        }),
        ..Default::default()
    });
    for record in [
        json!({"name":"claude/x/new","run_id":"r1"}),
        json!({"name":"claude/x/new","run_id":"","conversation_id":"c1"}),
        json!({"name":"claude/x/old","run_id":"r9"}),
    ] {
        assert_eq!(calling_member(&team, &record).as_deref(), Some("lead"), "{record}");
    }
    assert_eq!(calling_member(&team, &json!({"name":"claude/x/other","run_id":"r2","conversation_id":"c2"})), None);
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
        let record = json!({"name":"claude/x/new","run_id":"r1","conversation_id":"c1"});
        follow_session_rename(&mut store, "t", Some(&record)).unwrap();
        let stored = store.load("t").unwrap().unwrap();
        let session = stored.member("lead").unwrap().session.clone().unwrap();
        assert_eq!((session.name.as_str(), session.run_id.as_str()), ("claude/x/new", "r1"));
        let before = store.events("t", None, 100).unwrap();
        follow_session_rename(&mut store, "t", Some(&record)).unwrap();
        assert_eq!(store.events("t", None, 100).unwrap(), before);
    });
}

/// N39/F1.3: add-member checks the new member's folder against the team's
/// policy, as plan and approve do, and files nothing on a mismatch.
#[test]
fn adding_a_member_whose_folder_breaks_the_policy_is_refused() {
    use crate::state::teams::model::FolderPolicy;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.settings.folder_policy = FolderPolicy::Shared;
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let error = run(
            "add-member",
            &json!({"request_id":"22222222-2222-4222-8222-222222222222","team":id,"by":"human",
                "member":{"id":"ops","role":"Ops","agent":"codex","folder":"worktree"}}),
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("folder error(s) to add the member"), "{error}");
        assert!(error.contains("the shared folder policy leaves no own folder"), "{error}");
        let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert!(saved.member("ops").is_none(), "the refused member is not filed");
        // A member that fits the policy is added.
        run(
            "add-member",
            &json!({"request_id":"33333333-3333-4333-8333-333333333333","team":id,"by":"human",
                "member":{"id":"ops","role":"Ops","agent":"codex"}}),
            false,
        )
        .unwrap();
        let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert!(saved.member("ops").is_some());
    });
}

/// Task #48: in a sandbox every refused store write is the sandbox's, whatever
/// the error says (Codex's other user meets a private-folder check, not a
/// read-only database), so the member's request waits in its folder.
#[test]
fn a_sandbox_queues_whatever_blocked_its_write() {
    let error = anyhow::anyhow!("Missing team store parent: the folder is not private to this user");
    assert!(!super::read_only(&error), "outside a sandbox an unrelated error is the error");
    crate::platform::set_test_sandboxed(Some(true));
    let queued = super::read_only(&error);
    crate::platform::set_test_sandboxed(None);
    assert!(queued);
}

/// Task #28 (5): team 33fbafeb's dsh member asked for a worktree got one; its
/// sandbox could not write the git metadata (in the main checkout's `.git`),
/// and the preflight only warned. A dsh member's own folder is a clone now,
/// under the worktree policy too; another agent keeps its worktree.
#[test]
fn a_deepseek_member_given_a_worktree_gets_a_clone() {
    use crate::state::teams::model::{FolderPolicy, MemberFolder};
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.settings.folder_policy = FolderPolicy::Worktree;
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        run(
            "add-member",
            &json!({"request_id":"22222222-2222-4222-8222-222222222222","team":id,"by":"human",
                "member":{"id":"ui2","role":"Frontend","agent":"dsh","model":"deepseek-flash","folder":"worktree"}}),
            false,
        )
        .unwrap();
        run(
            "add-member",
            &json!({"request_id":"33333333-3333-4333-8333-333333333333","team":id,"by":"human",
                "member":{"id":"ops","role":"Ops","agent":"codex","folder":"worktree"}}),
            false,
        )
        .unwrap();
        let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert_eq!(saved.member("ui2").unwrap().folder, MemberFolder::Clone);
        assert_eq!(saved.member("ops").unwrap().folder, MemberFolder::Worktree);
        let found = super::super::plan::validate(&saved, &Default::default());
        assert!(!found.warnings.iter().any(|finding| finding.text.contains("cannot write its git metadata")), "{:?}", found.warnings);
    });
}

/// N39/F1.3: extend keeps a folder or policy mismatch instead of dropping
/// it (it names no task), so the phase that cannot run is not added.
#[test]
fn extending_a_team_whose_folders_break_the_policy_is_refused() {
    use crate::state::teams::model::FolderPolicy;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.settings.folder_policy = FolderPolicy::Worktree;
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111112", &json!({}), team, vec![]).unwrap();
        let error = run(
            "extend",
            &json!({"request_id":"22222222-2222-4222-8222-222222222223","team":id,"by":"human",
                "tasks":[{"id":2,"title":"More","assignee":"dev"}]}),
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("phase error(s) to extend"), "{error}");
        assert!(error.contains("the worktree policy gives every member its own folder"), "{error}");
        let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert!(saved.task(2).is_none(), "the refused phase is not added");
    });
}

/// N7: discarding a draft keeps its reason and stops the lead's session,
/// only when the owner asked for it (spec 3).
#[test]
fn discarding_a_draft_stops_its_lead_and_keeps_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.status = TeamStatus::Draft;
        team.draft = Some(Draft { revision: 3, ..Default::default() });
        team.brief = Notice::Pending;
        {
            let lead = team.members.iter_mut().find(|member| member.id == "lead").unwrap();
            lead.session = Some(SessionRef { name: "claude/x/lead".into(), run_id: "r0".into(), ..Default::default() });
            lead.state = MemberState::Active;
        }
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        super::record_lead_stops(true);
        let value = run(
            "draft",
            &json!({"team":id,"by":"human","action":"discard","stop_lead":true,"reason":"not worth it",
                "request_id":"22222222-2222-4222-8222-222222222222"}),
            false,
        )
        .unwrap();
        super::record_lead_stops(false);
        assert_eq!(value["lead_stopped"], true, "{value}");
        assert_eq!(super::lead_stops(), vec![(String::new(), "claude/x/lead".to_string())]);
        let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert_eq!((saved.status, saved.stop_reason.as_str()), (TeamStatus::Stopped, "draft_discarded"));
        assert!(saved.draft.is_none());
        // The durable feed names the reason and the stop request.
        let events = Store::open(false).unwrap().events(&id, None, 20).unwrap();
        let event = events["items"].as_array().unwrap().iter().find(|event| event["kind"] == "draft_discarded").expect("no event");
        assert_eq!(event["detail"]["reason"], "not worth it");
        assert_eq!(event["detail"]["stop_lead"], true);
        // A discard without stop_lead stops nothing.
        let mut second = running_team();
        second.id = "d2".into();
        second.status = TeamStatus::Draft;
        second.draft = Some(Draft { revision: 1, ..Default::default() });
        let other = second.id.clone();
        Store::open(true).unwrap().create("33333333-3333-4333-8333-333333333333", &json!({}), second, vec![]).unwrap();
        super::record_lead_stops(true);
        let value = run(
            "draft",
            &json!({"team":other,"by":"human","action":"discard","stop_lead":false,
                "request_id":"44444444-4444-4444-8444-444444444444"}),
            false,
        )
        .unwrap();
        super::record_lead_stops(false);
        assert!(value.get("lead_stopped").is_none(), "{value}");
        assert!(super::lead_stops().is_empty());
    });
}

/// N10: a member whose only open tasks are drafts leaves when the owner
/// names the member that takes them (spec 4).
#[test]
fn a_removed_member_drafts_move_to_the_named_member() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "codex".into(), ..Default::default() });
        team.tasks.push(Task { id: 2, title: "Later work".into(), assignee: "dev".into(), status: TaskStatus::Draft, phase: 2, ..Default::default() });
        team.tasks.push(Task { id: 3, title: "Started".into(), assignee: "dev".into(), status: TaskStatus::Pending, phase: 2, ..Default::default() });
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        // Without a target the removal is refused, and with a target only
        // the drafts may move.
        let error = run("remove-member", &json!({"team":id,"by":"human","member":"dev","request_id":"22222222-2222-4222-8222-222222222222"}), false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Reassign or cancel its open tasks first"), "{error}");
        let error = run(
            "remove-member",
            &json!({"team":id,"by":"human","member":"dev","reassign_drafts_to":"qa","request_id":"33333333-3333-4333-8333-333333333333"}),
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("started tasks first"), "{error}");
        // Its started work is cancelled: the draft moves to qa and dev goes.
        for (task, request) in [(1, "44444444-4444-4444-8444-444444444444"), (3, "55555555-5555-4555-8555-555555555555")] {
            run("cancel", &json!({"team":id,"by":"human","task":task,"request_id":request}), false).unwrap();
        }
        let value = run(
            "remove-member",
            &json!({"team":id,"by":"human","member":"dev","reassign_drafts_to":"qa","request_id":"66666666-6666-4666-8666-666666666666"}),
            false,
        )
        .unwrap();
        assert_eq!(value["status"], "removed", "{value}");
        let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert_eq!(saved.task(2).unwrap().assignee, "qa");
        assert_eq!(saved.member("dev").unwrap().state, MemberState::Removed);
    });
}

/// The owner, 2026-10-28: the lead or the owner records safe actions, a
/// member never does, and the team's journal names each change.
#[test]
fn only_the_lead_or_the_owner_records_safe_actions() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        for (id, conversation) in [("lead", "conv-lead"), ("dev", "conv-dev")] {
            let member = team.members.iter_mut().find(|member| member.id == id).unwrap();
            member.session = Some(SessionRef { name: format!("dsh/x/{id}"), run_id: "r1".into(), conversation_id: conversation.into(), ..Default::default() });
        }
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let add = json!({"team":id,"action":"add","role":"Backend","shape":"cargo test --locked --bin hgs {word}","reason":"its own tests"});
        super::set_test_caller_identities(Some((Some("conv-dev".into()), None, None)));
        assert!(run("safe-action", &add, false).is_err());
        super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
        let listed = run("safe-action", &add, false).unwrap();
        let action = listed["actions"][0].clone();
        assert_eq!((action["shape"].as_str(), action["by"].as_str()), (Some("cargo test --locked --bin hgs {word}"), Some("lead")), "{listed}");
        // A shape that is too short, or starts with a slot, is refused.
        let mut short = add.clone();
        short["shape"] = json!("ls {path}");
        assert!(run("safe-action", &short, false).is_err());
        super::set_test_caller_identities(Some((None, None, None)));
        let removed = run("safe-action", &json!({"team":id,"action":"remove","id":action["id"],"by":"human"}), false).unwrap();
        assert_eq!(removed["actions"], json!([]));
        let events = Store::open(false).unwrap().events(&id, None, 20).unwrap();
        let kinds: Vec<&str> = events["items"].as_array().unwrap().iter().filter_map(|event| event["kind"].as_str()).collect();
        assert!(kinds.contains(&"safe_action_added") && kinds.contains(&"safe_action_removed"), "{kinds:?}");
    });
}

/// F5: the owner's edit wins only over a lead edit made before the lead read
/// it; the lead's read clears the marks, so the lead can change the field.
#[test]
fn a_lead_changes_an_owner_edited_field_after_reading_the_draft() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.status = TeamStatus::Draft;
        team.draft = Some(Draft { revision: 5, owner_edited: vec!["members".into()], ..Default::default() });
        let lead_id = team.lead.clone();
        {
            let lead = team.members.iter_mut().find(|member| member.id == lead_id).unwrap();
            lead.session =
                Some(SessionRef { name: "dsh/x/lead".into(), run_id: "r1".into(), conversation_id: "conv-lead".into(), ..Default::default() });
        }
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let update = json!({"team":id,"action":"update","revision":5,"by":"human",
            "set":{"members":[{"id":"dev2","role":"Ops","agent":"codex"}]},
            "request_id":"22222222-2222-4222-8222-222222222222"});
        // As the lead, before it reads, the owner-edited field is refused.
        super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
        let error = run("draft", &update, false).unwrap_err().to_string();
        assert!(error.contains("Owner changed members"), "{error}");
        // The owner's read leaves the marks alone.
        super::set_test_caller_identities(Some((None, None, None)));
        let owner = run("draft", &json!({"team":id,"action":"get","by":"human"}), false).unwrap();
        assert_eq!(owner["draft"]["owner_edited"], json!(["members"]), "{owner}");
        // The lead's read clears them and names them in the answer (N22).
        super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
        let get = run("draft", &json!({"team":id,"action":"get","by":"human"}), false).unwrap();
        assert!(get["draft"]["owner_edited"].is_null() || get["draft"]["owner_edited"] == json!([]), "{get}");
        assert_eq!(get["owner_edited"], json!(["members"]), "{get}");
        // A second read has nothing left to clear.
        let again_get = run("draft", &json!({"team":id,"action":"get","by":"human"}), false).unwrap();
        assert_eq!(again_get["owner_edited"], json!([]), "{again_get}");
        let mut again = update;
        again["request_id"] = json!("33333333-3333-4333-8333-333333333333");
        let value = run("draft", &again, false).unwrap();
        assert_eq!(value["status"], "updated", "{value}");
        let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert!(saved.member("dev2").is_some(), "{:?}", saved.members.iter().map(|member| member.id.clone()).collect::<Vec<_>>());
        assert!(saved.draft.as_ref().unwrap().owner_edited.is_empty());
    });
}

/// Task #11 (team ede724c4, owner 2026-11-10): a draft update that does not
/// name the members (only settings, only the plan mode, only the tasks) saves,
/// for the owner and for the lead, and leaves the members as they were. The
/// session adoption used to insert `"members": null` into the update.
#[test]
fn a_draft_update_without_members_saves_and_keeps_them() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.status = TeamStatus::Draft;
        team.draft = Some(Draft { revision: 1, ..Default::default() });
        let lead_id = team.lead.clone();
        {
            let lead = team.members.iter_mut().find(|member| member.id == lead_id).unwrap();
            lead.session =
                Some(SessionRef { name: "dsh/x/lead".into(), run_id: "r1".into(), conversation_id: "conv-lead".into(), ..Default::default() });
        }
        let id = team.id.clone();
        let settings = serde_json::to_value(&team.settings).unwrap();
        let tasks = json!([{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"Tests","assignee":"qa"}]);
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let members = || serde_json::to_value(&Store::open(false).unwrap().load(&id).unwrap().unwrap().members).unwrap();
        let before = members();
        let mut request = 0u32;
        for lead in [false, true] {
            if lead {
                // The lead reads the draft first: the owner's edits above are
                // marked until it does (N22).
                super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
                run("draft", &json!({"team":id,"action":"get","by":"human"}), false).unwrap();
            }
            for set in [json!({"settings":settings}), json!({"plan_mode":"free"}), json!({"tasks":tasks})] {
                let caller = lead.then(|| "conv-lead".to_owned());
                super::set_test_caller_identities(Some((caller, None, None)));
                let revision = Store::open(false).unwrap().load(&id).unwrap().unwrap().draft.unwrap().revision;
                request += 1;
                let update = json!({"team":id,"action":"update","revision":revision,"by":"human","set":set,
                    "request_id":format!("22222222-2222-4222-8222-{request:012}")});
                let value = run("draft", &update, false).unwrap_or_else(|error| panic!("lead {lead}, {set}: {error:#}"));
                assert_eq!(value["status"], "updated", "{value}");
                assert_eq!(members(), before, "lead {lead}, {set}: the members stay");
            }
        }
        super::set_test_caller_identities(None);
        let saved = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert_eq!(saved.tasks.len(), 2);
        assert_eq!(saved.plan_mode, PlanMode::Free, "the owner's plan mode");
    });
}

/// F5/N6: a draft started with a new lead launches it in the team folder and
/// gives it the draft brief, like a desktop team's lead.
#[test]
fn a_started_draft_launches_and_briefs_its_new_lead() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let value = run(
            "draft",
            &json!({"action":"start","by":"human","request_id":"11111111-1111-4111-8111-111111111111",
                "folder":project.display().to_string(),"name":"New team",
                "lead":{"agent":"claude","model":"opus","effort":"high","account":"work"}}),
            false,
        )
        .unwrap();
        let id = value["team"].as_str().unwrap().to_owned();
        let team = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert_eq!((team.status, team.brief), (TeamStatus::Draft, Notice::Pending));
        let lead = team.member("lead").unwrap();
        assert_eq!((lead.state, lead.session.is_some(), lead.folder), (MemberState::NotCreated, false, MemberFolder::Shared));
        // The lead starts in the team's folder, never a worktree.
        assert_eq!(super::super::dispatcher::member_folder_path(&team, lead).unwrap(), project.display().to_string());
        // The dispatcher's next tick starts it, then briefs it once.
        let steps = super::super::dispatcher::steps(&team, &[], 100.0);
        assert!(steps.contains(&crate::state::teams::dispatcher::Step::Launch { member: "lead".into() }), "{steps:?}");
        let mut launched = team.clone();
        {
            let lead = launched.members.iter_mut().find(|member| member.id == "lead").unwrap();
            lead.launch_id = "L".into();
            lead.session = Some(SessionRef { name: "claude/x/newlead".into(), run_id: "r1".into(), ..Default::default() });
        }
        let free = crate::state::teams::dispatcher::Seen {
            name: "claude/x/newlead".into(),
            run_id: "r1".into(),
            free: true,
            idle: true,
            live: true,
            ..Default::default()
        };
        assert!(super::super::dispatcher::steps(&launched, &[free], 100.0).contains(&crate::state::teams::dispatcher::Step::Brief));
        let brief = super::super::dispatcher::brief_text(&launched);
        assert!(brief.contains("hgs guide designer") && brief.contains(&id), "{brief}");
    });
}

/// U19 (2): the lead answers a member's subagent request with hgs team
/// answer; the answer goes to that member's session for that exact card
/// (its tool use), as `hgs answer` with the card's id and hash.
#[test]
fn the_leads_answer_reaches_the_members_subagent_card() {
    use crate::state::teams::model::SessionRef;
    let (mut team, _, _) = review_team();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
        Some(SessionRef { name: "claude/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
    let card = super::super::dispatcher::subagent_card();
    with_review_team(&team, || {
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(Vec<String>, Value)>::new()));
        let record = calls.clone();
        let inspected = json!({"pending_questions":[card.clone()]}).to_string();
        super::super::dispatcher::set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            record.borrow_mut().push((args.iter().map(|arg| arg.to_string()).collect(), input.cloned().unwrap_or_default()));
            Ok(match args.first().copied() {
                Some("inspect") => super::super::dispatcher::fake_output(0, &inspected, ""),
                _ => super::super::dispatcher::fake_output(0, r#"{"status":"answered"}"#, ""),
            })
        })));
        let result = as_lead(|| {
            call(
                "answer",
                json!({"member":"dev","question_id":card["question_id"],"expected_question_hash":card["question_hash"],
                    "answers":[{"question_id":"approval","selected_option_ids":["1"]}]}),
            )
        });
        super::super::dispatcher::set_fake_hgs(None);
        result.unwrap();
        // Task #349: the lead's answer is in the approval journal, with its
        // source, its member and team, the question and the answer given.
        let journal = crate::approval_log::root();
        let entries = crate::approval_log::entries(&journal, &json!({"source":"team lead dsh/x/lead"})).unwrap();
        assert_eq!(entries["entries"].as_array().unwrap().len(), 1, "{entries}");
        assert_eq!(entries["entries"][0]["team"], "rv");
        assert_eq!(entries["entries"][0]["member"], "dev");
        assert_eq!(entries["entries"][0]["answer"], "allow: Yes");
        assert!(entries["entries"][0]["prompt"].as_str().unwrap().contains("Start 2 subagents"), "{entries}");
        let calls = calls.borrow();
        let answer = calls.iter().find(|(args, _)| args.first().map(String::as_str) == Some("answer")).expect("hgs answer was called");
        assert_eq!(answer.0, vec!["answer".to_owned(), "claude/x/dev".to_owned(), "--json".to_owned()]);
        assert_eq!(answer.1["question_id"], card["question_id"]);
        assert_eq!(answer.1["expected_question_hash"], card["question_hash"]);
        assert_eq!(answer.1["answers"][0]["selected_option_ids"], json!(["1"]));
        assert_eq!(answer.1["expected_run_id"], "r1");
    });
}

/// The owner, 2026-11-05: Kimi refused the lead's answer ("Another Kimi
/// request is waiting. Answer in Terminal.") and the request stayed in the
/// lead's inbox, which could never answer it. It moves to the owner.
#[test]
fn an_answer_only_the_terminal_takes_moves_the_request_to_the_owner() {
    use crate::state::teams::model::{PendingQuestion, SessionRef};
    let (mut team, _, _) = review_team();
    let card = json!({"question_id":"k1","question_hash":"hk","approval":true,"can_answer":true,
        "questions":[{"id":"approval","header":"Bash","question":"Run this command?","options":[{"id":"1","label":"Approve once"}]}]});
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.session = Some(SessionRef { name: "kimi/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
        dev.pending = Some(PendingQuestion { question_id: "k1".into(), question_hash: "hk".into(), approval: true, at: 1.0, card: card.clone() });
    }
    let id = team.id.clone();
    with_review_team(&team, || {
        let inspected = json!({"pending_questions":[card.clone()]}).to_string();
        super::super::dispatcher::set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
            Ok(match args.first().copied() {
                Some("inspect") => super::super::dispatcher::fake_output(0, &inspected, ""),
                _ => super::super::dispatcher::fake_output(1, "", "Another Kimi request is waiting. Answer in Terminal."),
            })
        })));
        let result = as_lead(|| {
            call("answer", json!({"member":"dev","question_id":"k1","expected_question_hash":"hk",
                "answers":[{"question_id":"approval","selected_option_ids":["1"]}]}))
        });
        super::super::dispatcher::set_fake_hgs(None);
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Nothing was answered") && error.contains("owner"), "{error}");
        let after = Store::open(false).unwrap().load(&id).unwrap().unwrap();
        let dev = after.members.iter().find(|member| member.id == "dev").unwrap();
        assert!(dev.pending.is_none());
        let moved = dev.terminal_question.as_ref().expect("the owner's now");
        assert_eq!((moved.question_id.as_str(), moved.card["can_answer"].as_bool()), ("k1", Some(false)));
        assert!(super::super::inbox::attention(&after, 10.0).iter().any(|item| item["kind"] == "permission_terminal"));
    });
}

/// N20: the lead's own permission card is the owner's to answer. The lead
/// answering it would decide the whole subagent group it was asked to
/// confirm, so `hgs team answer` refuses it before reading the card.
#[test]
fn the_lead_never_answers_its_own_permission_card() {
    use crate::state::teams::model::SessionRef;
    let (mut team, _, _) = review_team();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
        Some(SessionRef { name: "claude/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
    with_review_team(&team, || {
        let error = as_lead(|| {
            call("answer", json!({"member":"lead","question_id":"q1","expected_question_hash":"h1","answers":[]}))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("owner"), "{error}");
        // A member's card is still the lead's to answer.
        super::super::dispatcher::set_fake_hgs(Some(Box::new(|_, args: &[&str], _input: Option<&Value>| {
            Ok(match args.first().copied() {
                Some("inspect") => super::super::dispatcher::fake_output(0, r#"{"pending_questions":[]}"#, ""),
                _ => super::super::dispatcher::fake_output(0, r#"{"status":"answered"}"#, ""),
            })
        })));
        let error = as_lead(|| {
            call("answer", json!({"member":"dev","question_id":"q1","expected_question_hash":"h1","answers":[]}))
        })
        .unwrap_err()
        .to_string();
        super::super::dispatcher::set_fake_hgs(None);
        assert!(error.contains("no longer pending"), "{error}");
    });
}

#[test]
fn a_bad_finding_is_refused_with_the_report_not_at_accept() {
    // N13: "high" is no severity: the member hears it at once, and the
    // corrected report is accepted. Task #292: the validation runs on the
    // owner's path, not the sandbox's queued write; the test forces it so it
    // reads the same on a sandboxed machine.
    crate::platform::set_test_sandboxed(Some(false));
    let (team, attempt, _) = review_team();
    with_review_team(&team, || {
        let error = call("report", json!({"task":1,"attempt":attempt,"text":"Done",
            "report":{"summary":"Done","findings":[finding("high","Lost write")]}}))
        .unwrap_err()
        .to_string();
        // Task #229: a finding may name crit or minor too (the same severities
        // a spawned follow-up takes), so the refusal names all five.
        assert!(error.contains("A finding is critical (crit), normal (minor) or later"), "{error}");
        assert_eq!(stored().task(1).unwrap().attempts[0].report, None);
        let error = call("report", json!({"task":1,"attempt":attempt,"text":"Done",
            "report":{"summary":"Done","findings":[{"severity":"normal","title":"No scenario"}]}}))
        .unwrap_err()
        .to_string();
        assert!(error.contains("A finding needs its scenario"), "{error}");
        call("report", json!({"task":1,"attempt":attempt,"text":"Done",
            "report":{"summary":"Done","findings":[finding("critical","Lost write")]}}))
        .unwrap();
        call("accept", json!({"task":1,"by":"human"})).unwrap();
        let team = stored();
        assert_eq!(team.findings.len(), 1);
        assert_eq!(team.findings[0].severity, "critical");
    });
    crate::platform::set_test_sandboxed(None);
}

#[test]
fn fixed_and_dropped_findings_leave_room_under_the_cap() {
    // N14: 1000 closed findings no longer block the next accept; the
    // oldest closed ones make room and the store stays at 1000.
    use crate::state::teams::model::Finding;
    let (mut team, attempt, _) = review_team();
    for n in 1..=1000 {
        team.findings.push(Finding { id: format!("f{n}"), severity: "normal".into(), title: format!("F{n}"), scenario: "s".into(),
            state: if n % 2 == 0 { "fixed".into() } else { "dropped".into() }, ..Default::default() });
    }
    team.findings_seq = 1000;
    with_review_team(&team, || {
        call("report", json!({"task":1,"attempt":attempt,"text":"Done",
            "report":{"summary":"Done","findings":[finding("normal","New"),finding("later","Later")]}}))
        .unwrap();
        call("accept", json!({"task":1,"by":"human"})).unwrap();
        let team = stored();
        assert_eq!(team.findings.len(), 1000);
        assert_eq!(team.findings.last().unwrap().id, "f1002");
        assert!(!team.findings.iter().any(|finding| finding.id == "f1" || finding.id == "f2"));
    });
    // Live findings still count: 1000 open ones leave no room.
    let (mut team, attempt, _) = review_team();
    for n in 1..=1000 {
        team.findings.push(Finding { id: format!("f{n}"), severity: "normal".into(), title: format!("F{n}"), scenario: "s".into(),
            state: "open".into(), ..Default::default() });
    }
    with_review_team(&team, || {
        call("report", json!({"task":1,"attempt":attempt,"text":"Done","report":{"summary":"Done","findings":[finding("normal","New")]}}))
            .unwrap();
        let error = call("accept", json!({"task":1,"by":"human"})).unwrap_err().to_string();
        assert!(error.contains("at most 1000 findings that are not fixed or dropped"), "{error}");
    });
}

#[test]
fn the_owners_accept_after_the_leads_files_the_findings_once() {
    // N15
    let (team, attempt, _) = review_team();
    with_review_team(&team, || {
        call("report", json!({"task":1,"attempt":attempt,"text":"Done",
            "report":{"summary":"Done","findings":[finding("normal","Slow"),finding("critical","Lost write")]}}))
        .unwrap();
        as_lead(|| call("accept", json!({"task":1}))).unwrap();
        assert_eq!(stored().findings.len(), 2);
        call("accept", json!({"task":1,"by":"human"})).unwrap();
        let team = stored();
        assert_eq!(team.findings.len(), 2, "{:?}", team.findings);
        assert_eq!(team.task(1).unwrap().decided_by, Some(crate::state::teams::model::Decider::Human));
    });
}

#[test]
fn a_replan_keeps_its_proposal_and_fix_phase_its_phase() {
    // N17, N18
    let (team, attempt, _) = review_team();
    with_review_team(&team, || {
        call("report", json!({"task":1,"attempt":attempt,"text":"Done",
            "report":{"summary":"Done","findings":[finding("critical","Lost write"),finding("normal","Slow")]}}))
        .unwrap();
        call("accept", json!({"task":1,"by":"human"})).unwrap();
        // A critical finding is not parked in a draft phase.
        let error = call("finding", json!({"finding":"f1","action":"fix_phase","phase":2,"by":"human"})).unwrap_err().to_string();
        assert!(error.contains("Only a normal finding"), "{error}");
        call("finding", json!({"finding":"f2","action":"fix_phase","phase":2,"by":"human"})).unwrap();
        call("finding", json!({"finding":"f1","action":"replan","proposal":"Split the save into two steps","by":"human"})).unwrap();
        let team = stored();
        assert_eq!(team.findings[1].phase, Some(2));
        assert_eq!(team.findings[0].proposal, "Split the save into two steps");
        // Status shows both, and a replanned critical finding is still open.
        let status = run("status", &json!({"team":"rv"}), false).unwrap();
        let items = status["findings"]["items"].as_array().unwrap();
        let f1 = items.iter().find(|item| item["id"] == "f1").unwrap();
        assert_eq!(f1["proposal"], "Split the save into two steps");
        assert_eq!(items.iter().find(|item| item["id"] == "f2").unwrap()["phase"], 2);
        assert_eq!(status["findings_summary"]["open"]["critical"], 1);
        assert_eq!(status["findings_summary"]["open"]["normal"], 1);
        // The team waits paused, and the owner's attention names the proposal.
        assert_eq!(status["status"], "paused");
        let replan = status["attention"].as_array().unwrap().iter().find(|item| item["kind"] == "replan").cloned();
        assert_eq!(replan.as_ref().map(|item| item["proposal"].clone()), Some(json!("Split the save into two steps")), "{}", status["attention"]);
    });
}

#[test]
fn a_finding_in_a_replan_or_a_draft_phase_still_blocks_the_finish() {
    // N18: replan and fix_phase are not resolved.
    use crate::state::teams::model::{Finding, TaskStatus};
    for state in ["replan", "fix_phase"] {
        let (mut team, _, _) = review_team();
        for task in &mut team.tasks {
            task.status = if task.phase == 1 { TaskStatus::Accepted } else { TaskStatus::Cancelled };
        }
        team.findings.push(Finding { id: "f1".into(), severity: "critical".into(), title: "Lost write".into(), scenario: "s".into(),
            state: state.into(), ..Default::default() });
        with_review_team(&team, || {
            let error = call("finish", json!({"summary":"Done","by":"human"})).unwrap_err().to_string();
            assert!(error.contains("A critical finding is open"), "{state}: {error}");
        });
    }
}

#[test]
fn a_finished_team_takes_no_reopen() {
    // N20: reopen, urgent, replan and fix_phase need a running team;
    // closing a finding still works.
    use crate::state::teams::model::{Finding, TaskStatus, TeamStatus};
    let (mut team, _, _) = review_team();
    for task in &mut team.tasks {
        task.status = TaskStatus::Accepted;
    }
    team.status = TeamStatus::Done;
    team.findings.push(Finding { id: "f1".into(), severity: "normal".into(), title: "Slow".into(), scenario: "s".into(), task: Some(1),
        state: "open".into(), ..Default::default() });
    with_review_team(&team, || {
        for action in [json!({"action":"reopen"}), json!({"action":"replan","proposal":"x"}), json!({"action":"fix_phase","phase":2})] {
            let mut input = action.clone();
            input["finding"] = json!("f1");
            input["by"] = json!("human");
            let error = call("finding", input).unwrap_err().to_string();
            assert!(error.contains("need a running or paused team"), "{action}: {error}");
        }
        assert_eq!(stored().task(1).unwrap().status, TaskStatus::Accepted);
        call("finding", json!({"finding":"f1","action":"fixed","by":"human"})).unwrap();
        assert_eq!(stored().findings[0].state, "fixed");
    });
}

#[test]
fn a_removed_or_paused_member_is_not_swapped() {
    // N31
    use crate::state::teams::model::MemberState;
    let (mut team, _, _) = review_team();
    team.members.iter_mut().find(|member| member.id == "qa").unwrap().state = MemberState::Removed;
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().state = MemberState::Paused;
    with_review_team(&team, || {
        let error = call("swap-member", json!({"member":"qa","agent":"claude","by":"human"})).unwrap_err().to_string();
        assert!(error.contains("This member was removed"), "{error}");
        let error = call("swap-member", json!({"member":"dev","agent":"claude","by":"human"})).unwrap_err().to_string();
        assert!(error.contains("Resume dev before swapping it"), "{error}");
        let team = stored();
        assert_eq!(team.member("qa").unwrap().state, MemberState::Removed);
        assert!(!team.member("qa").unwrap().launch_after_swap);
        assert_eq!(team.member("dev").unwrap().state, MemberState::Paused);
    });
}

#[test]
fn a_swap_to_an_existing_session_takes_the_listings_folder_not_the_requests() {
    // N32: the adopted session's own folder comes from the live listing; the
    // request may not name a different folder or folder kind for it.
    use crate::state::teams::dispatcher::Seen;
    use crate::state::teams::model::MemberFolder;
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        super::live_sessions_override(Some(vec![Seen {
            name: "codex/x/other".into(),
            run_id: "r9".into(),
            conversation_id: "c9".into(),
            live: true,
            cwd: "/work/other-checkout".into(),
            ..Default::default()
        }]));
        let value = call(
            "swap-member",
            json!({"member":"dev","session":{"name":"codex/x/other","run_id":"r9"},
                "directory":"/work/forged","folder":"worktree","by":"human"}),
        )
        .unwrap();
        assert_eq!(value["status"], "swapped", "{value}");
        let saved = stored();
        let dev = saved.member("dev").unwrap();
        assert_eq!(dev.directory, "/work/other-checkout", "the listing's folder, never the request's");
        assert_eq!(dev.folder, MemberFolder::Shared, "the adopted session's own kind");
    });
    super::live_sessions_override(None);
}

#[test]
fn a_swap_to_an_existing_session_takes_its_folder() {
    // N32: the adopted session's folder replaces the member's worktree.
    use crate::state::teams::model::MemberFolder;
    let (mut team, _, _) = review_team();
    let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.folder = MemberFolder::Worktree;
    dev.directory = "/work/repo-team-rv-dev".into();
    let mut events = Vec::new();
    ops::apply(
        &mut team,
        "swap-member",
        &json!({"member":"dev","session":{"name":"codex/x/other","run_id":"r9","conversation_id":"c9"},
            "directory":"/work/other-checkout","folder":"shared"}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    let dev = team.member("dev").unwrap();
    assert_eq!(dev.directory, "/work/other-checkout");
    assert_eq!(dev.folder, MemberFolder::Shared);
    assert_eq!(dev.session.as_ref().unwrap().name, "codex/x/other");
}

#[test]
fn a_draft_behind_an_approved_running_phase_does_not_wait() {
    // N40 (spec 2.6): phase 1 still runs, phase 2 can be approved now.
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        let status = run("status", &json!({"team":"rv"}), false).unwrap();
        let phase = status["phases"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap().clone();
        // #515: it says it can run alongside, not that it waits.
        assert_eq!(phase["waiting"]["alongside"], true, "{phase}");
        assert_eq!(phase["waiting"]["text"], "Can run alongside phase 1", "{phase}");
        call("approve-phase", json!({"phase":2,"by":"human"})).unwrap();
    });
}

#[test]
fn a_confirmed_alongside_approval_hands_the_phase_out_now() {
    // N6: "Can run alongside phase 1" was false - approving phase 2 put it
    // in Queued and every task waited for phase 1. Task 158 (owner,
    // 2026-11-07): every approval now runs the phase alongside, confirmed
    // "alongside" or not.
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        let value = call("approve-phase", json!({"phase":2,"by":"human"})).unwrap();
        assert_eq!((value["phase_state"].as_str(), value["alongside"].as_bool()), (Some("running"), Some(false)), "{value}");
        assert!(crate::state::teams::plan::ready(&stored()).contains(&3), "the phase runs alongside phase 1");
    });
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        let value = call("approve-phase", json!({"phase":2,"alongside":true,"by":"human"})).unwrap();
        assert_eq!(
            (value["phase_state"].as_str(), value["alongside"].as_bool()),
            (Some("running"), Some(true)),
            "{value}"
        );
        let saved = stored();
        assert_eq!(saved.phases.iter().find(|phase| phase.n == 2).unwrap().state, PhaseState::Approved);
        assert!(crate::state::teams::plan::ready(&saved).contains(&3), "the phase really runs alongside phase 1");
    });
    // A barrier phase keeps its tasks for the end of phase 1 either way, so
    // its waiting text names that, not an alongside that cannot happen.
    let (mut team, _, _) = review_team();
    team.phases.iter_mut().find(|phase| phase.n == 2).unwrap().barrier = true;
    with_review_team(&team, || {
        let status = run("status", &json!({"team":"rv"}), false).unwrap();
        let phase = status["phases"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap().clone();
        assert_eq!(phase["waiting"]["text"], "Starts when phase 1 is finished", "{phase}");
        call("approve-phase", json!({"phase":2,"alongside":true,"by":"human"})).unwrap();
        assert!(!crate::state::teams::plan::ready(&stored()).contains(&3), "a barrier holds the phase's tasks");
    });
}

#[test]
fn a_planning_team_has_no_phase_in_work() {
    // N41 (spec 10.2): before Approve & Run every phase waits for you.
    let (mut team, _) = ops::create("rv", &json!({"name":"Review","lead":{"id":"lead","agent":"claude"}})).unwrap();
    ops::apply(&mut team, "plan", &json!({"tasks":[{"id":1,"title":"A","assignee":"lead"}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new())
        .unwrap();
    with_review_team(&team, || {
        let status = run("status", &json!({"team":"rv"}), false).unwrap();
        let first = status["phase_summary"].as_array().unwrap().iter().find(|phase| phase["n"] == 1).unwrap().clone();
        assert_eq!(first["status"], "draft", "{first}");
        assert_eq!(first["state"], "planned");
    });
}

#[test]
fn a_draft_starts_with_its_lead_and_nothing_before_it() {
    use crate::state::teams::model::TeamStatus;
    use crate::state::teams::store::Store;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.db"), true).unwrap();
    let request = "11111111-1111-4111-8111-111111111111";
    let input = json!({"request_id":request,"folder":dir.path().display().to_string(),"name":"Checkout","lead":{"agent":"claude"}});
    let value = super::draft_start_in(&mut store, request, &input, input["lead"].clone()).unwrap();
    let id = value["team"].as_str().unwrap().to_owned();
    assert_eq!(value["revision"], 1);
    let team = store.load(&id).unwrap().unwrap();
    assert_eq!(team.status, TeamStatus::Draft);
    assert_eq!(team.draft.as_ref().unwrap().revision, 1);
    assert_eq!(team.member("lead").map(|member| member.role.as_str()), Some("Lead"));
    assert_eq!(team.members.len(), 1);
    // A repeated request returns the first result and creates nothing twice.
    super::draft_start_in(&mut store, request, &input, input["lead"].clone()).unwrap();
    assert_eq!(store.teams(true, 10).unwrap().len(), 1);
}

#[test]
fn draft_get_returns_the_revision_and_preflight() {
    use crate::state::teams::store::Store;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.db"), true).unwrap();
    let request = "22222222-2222-4222-8222-222222222222";
    let input = json!({"request_id":request,"folder":dir.path().display().to_string(),"lead":{"agent":"claude"}});
    let value = super::draft_start_in(&mut store, request, &input, input["lead"].clone()).unwrap();
    let id = value["team"].as_str().unwrap().to_owned();
    let get = super::draft_get_in(&mut store, &json!({"team":id})).unwrap();
    assert_eq!(get["revision"], 1);
    assert_eq!(get["draft"]["revision"], 1);
    assert!(get["preflight"]["errors"].is_array());
    // Feedback 2026-11-04 (5): the answer carries what the draft holds.
    assert_eq!(get["fields"]["members"][0]["id"], "lead", "{get}");
    assert!(get["fields"]["settings"]["max_members"].is_u64(), "{get}");
    assert!(get["fields"]["tasks"].is_array() && get["fields"]["goal"].is_string(), "{get}");
}

/// N3/C6: the owner paths that create a team or change the catalog have a
/// test through the command entry: an agent session is refused, and you
/// (outside every session, saying `"by":"human"`) may still act.
#[test]
fn the_create_and_catalog_owner_paths_refuse_an_agent_session() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _ = Store::open(true).unwrap();
        let request = "11111111-1111-4111-8111-111111111111";
        let input = json!({"request_id":request,"name":"T","folder":dir.path().join("project").display().to_string(),
            "lead":{"id":"lead","agent":"claude"},"by":"human"});
        set_test_caller_identities(Some((Some("conversation".into()), None, None)));
        let error = format!("{:#}", run("create", &input, false).unwrap_err());
        assert!(error.contains("An agent session cannot act as the owner"), "{error}");
        // The catalog's owner path takes the same rule, for every caller.
        let error = super::catalog_owner(&json!({"by":"human"})).map_err(|error| error.to_string()).unwrap_err();
        assert!(error.contains("An agent session cannot act as the owner"), "{error}");
        // An hgs session and a run marker are as much an agent as a dsh one.
        for identities in [(None, Some("hgs/x/dev".to_owned()), None), (None, None, Some("run-1".to_owned()))] {
            set_test_caller_identities(Some(identities));
            let error = format!("{:#}", run("create", &input, false).unwrap_err());
            assert!(error.contains("An agent session cannot act as the owner"), "{error}");
        }
        // Without "by" an agent session reads the catalog as itself, and the
        // owner's own shell (outside every session) is the owner (feedback 17).
        set_test_caller_identities(Some((None, Some("hgs/x/dev".to_owned()), None)));
        assert!(!super::catalog_owner(&json!({})).unwrap());
        set_test_caller_identities(Some((None, None, None)));
        assert!(super::catalog_owner(&json!({})).unwrap());
        assert!(!super::catalog_owner(&json!({"by":"lead"})).unwrap());
        // Outside every session you create the team and reach the owner path.
        assert!(super::catalog_owner(&json!({"by":"human"})).unwrap());
        let created = run("create", &input, false).unwrap();
        assert_eq!(created["team"].as_str().map(str::len), Some(36), "{created}");
    });
}

/// The owner, 2026-11-01: Create team on a computer that never had a team
/// failed with "No teams yet" - the replay check read a store not made yet.
#[test]
fn the_first_team_on_a_computer_is_created_without_a_store() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        assert!(!crate::state::teams::store::path().exists());
        set_test_caller_identities(Some((None, None, None)));
        let input = json!({"request_id":"11111111-1111-4111-8111-111111111111","name":"First",
            "folder":dir.path().join("project").display().to_string(),"lead":{"id":"lead","agent":"claude"},"by":"human"});
        let created = run("create", &input, false).unwrap();
        // A replay of the same request returns the same team.
        assert_eq!(run("create", &input, false).unwrap()["team"], created["team"]);
    });
}

/// #432 N8: a sub-team's create input cannot set an owner-only setting,
/// through the command entry itself - the stored child keeps its defaults.
#[test]
fn a_subteam_create_input_cannot_set_owner_settings() {
    use crate::state::teams::model::Settings;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let parent = running_team();
        let parent_id = parent.id.clone();
        let subteam_approval = parent.settings.subteam_approval;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), parent, vec![]).unwrap();
        let created = run(
            "create",
            &json!({"request_id":"22222222-2222-4222-8222-222222222222","name":"Sub","by":"human",
                "parent":{"team":parent_id,"task":1},
                "settings":{"auto_accept":true,"max_members":99,"tree_depth":8,"subteam_approval":"owner",
                    "report_timeout_seconds":90}}),
            false,
        )
        .unwrap();
        let child = Store::open(false).unwrap().load(created["team"].as_str().unwrap()).unwrap().unwrap();
        let defaults = Settings::default();
        assert_eq!(child.settings.max_members, defaults.max_members);
        assert_eq!(child.settings.tree_depth, defaults.tree_depth);
        assert!(!child.settings.auto_accept);
        // The parent's approval rule is inherited and the plan stays review.
        assert_eq!(child.settings.subteam_approval, subteam_approval);
        assert_eq!(child.settings.approval, Approval::Review);
        assert_eq!(child.settings.phase_approval, Some(Approval::Review));
        // The rest of the input is the owner's still.
        assert_eq!(child.settings.report_timeout_seconds, 90);
    });
}

#[test]
fn a_permission_escalation_carries_command_source_reason_and_ids() {
    let child = Team { id: "child".into(), name: "Child".into(), lead: "dev".into(), ..Default::default() };
    let pending = PendingQuestion {
        question_id: "claude-approval:x".into(),
        question_hash: "h".into(),
        approval: true,
        at: 1.0,
        card: json!({"tool_input":{"command":"cargo test --locked"},"questions":[{"id":"approval","header":"Bash","body":"Run the tests","options":[]}]}),
    };
    let question = permission_question(&child, "dev", &pending);
    for part in ["Permission request from dev", "Bash", "cargo test --locked", "from the tool input", "Reason: Run the tests", "claude-approval:x", "\"h\"", "hgs team answer --json", "\"team\":\"child\""] {
        assert!(question.text.contains(part), "{part} missing in {}", question.text);
    }
}

/// N34 (spec 6.2): PowerShell 5.1 pipes JSON with a UTF-8 BOM. Every
/// reader that parses JSON from stdin drops it: here through the shared
/// helper, and below by reading each reader's source, so a new reader
/// that forgets it fails this test.
#[test]
fn every_stdin_json_reader_drops_a_bom() {
    let bom = |text: &str| [&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat();
    assert_eq!(crate::state::without_bom(&bom("{\"a\":1}")), b"{\"a\":1}");
    assert_eq!(crate::state::without_bom(b"{}"), b"{}");
    let parsed: Value = serde_json::from_slice(crate::state::without_bom(&bom("{\"text\":\"hi\"}"))).unwrap();
    assert_eq!(parsed["text"], "hi");
    assert!(serde_json::from_slice::<Value>(&bom("{}")).is_err(), "serde refuses a BOM, which is the bug");
    // Streams and framed protocols that never parse JSON from stdin.
    let streams = ["peer_channel.rs", "screen/mod.rs", "platform/windows.rs", "cli.rs", "desktop/feed/io.rs",
        // A length-framed request between Zerus's own processes, not typed JSON.
        "desktop/windows_registration/native.rs",
        // Its test child reads length-framed binary requests from stdin.
        "platform/worktree_membership/objects/loan/peer/tests.rs"];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![root.clone()];
    let mut missing = Vec::new();
    let mut readers = 0;
    while let Some(path) = files.pop() {
        if path.is_dir() {
            files.extend(std::fs::read_dir(&path).unwrap().flatten().map(|entry| entry.path()));
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let relative = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if streams.contains(&relative.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            if !line.contains("stdin()") || line.contains("Stdio::") || line.contains("is_terminal") || line.trim_start().starts_with("//") {
                continue;
            }
            let window = lines[at..(at + 12).min(lines.len())].join("\n");
            if !(window.contains("from_slice") || window.contains("parse(") || window.contains("submit(") || window.contains("import(")
                || window.contains("enveloped("))
            {
                continue;
            }
            readers += 1;
            if !(window.contains("without_bom") || window.contains("json_request") || window.contains("submit(")) {
                missing.push(format!("{relative}:{}", at + 1));
            }
        }
    }
    assert!(readers >= 25, "only {readers} readers found");
    assert!(missing.is_empty(), "stdin JSON readers that keep a BOM: {missing:?}");
    // `send` strips it inside submit_core (every origin goes through it), before its digest.
    let input = std::fs::read_to_string(root.join("state/input.rs")).unwrap();
    let submit = input.find("fn submit_core(").unwrap();
    assert!(input[submit..submit + 400].contains("without_bom(input)"));
}

#[test]
fn a_json_request_drops_a_bom_and_refuses_non_utf8() {
    let mut with_bom = vec![0xEF, 0xBB, 0xBF];
    with_bom.extend_from_slice(br#"{"a":1}"#);
    assert_eq!(crate::state::json_request(&with_bom).unwrap()["a"], 1);
    let error = crate::state::json_request(&[0xFF, 0xFE, b'{']).unwrap_err().to_string();
    assert_eq!(error, "Input is not UTF-8 (PowerShell 5.1: use --json-file)");
    // A `--json-file` request is read and goes through the same reader.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("request.json");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(br#"{"task":342}"#);
    std::fs::write(&path, bytes).unwrap();
    let read = std::fs::read(&path).unwrap();
    assert_eq!(crate::state::json_request(&read).unwrap()["task"], 342);
}

#[test]
fn a_refused_write_is_recognised_in_any_language() {
    for message in [
        "attempt to write a readonly database",
        "Отказано в доступе. (os error 5)",
        "Permission denied (os error 13)",
        "unable to open database file: teams.sqlite3",
    ] {
        assert!(read_only(&anyhow::anyhow!(message.to_owned())), "{message}");
    }
    assert!(!read_only(&anyhow::anyhow!("This attempt is no longer current")));
}

#[test]
fn a_phase_whose_tasks_are_all_cancelled_reads_cancelled_and_is_hidden() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    ops::apply(
        &mut team,
        "extend",
        &json!({"phase_title":"Next","tasks":[{"id":2,"title":"D","assignee":"dev"},{"id":3,"title":"E","assignee":"dev"}]}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    ops::apply(&mut team, "approve-phase", &json!({"phase":2}), &Actor::Human, &none, &mut events).unwrap();
    for id in [2, 3] {
        ops::apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
    }
    // The status hides the cancelled phase and its tasks by default.
    let hidden = view_with_cancelled(&team, &[], None, false);
    let phases: Vec<u64> = hidden["phases"].as_array().unwrap().iter().map(|phase| phase["n"].as_u64().unwrap()).collect();
    assert!(!phases.contains(&2), "{hidden}");
    assert!(hidden["tasks"].as_array().unwrap().iter().all(|task| task["phase"] != 2), "{hidden}");
    assert!(hidden["phase_summary"].as_array().unwrap().iter().all(|phase| phase["n"] != 2), "{hidden}");
    // Asked for, the phase reads cancelled and its tasks are listed.
    let shown = view_with_cancelled(&team, &[], None, true);
    let phase = shown["phases"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap();
    assert_eq!(phase["state"], "cancelled", "{shown}");
    assert_eq!(shown["tasks"].as_array().unwrap().iter().filter(|task| task["phase"] == 2).count(), 2);
    let header = shown["phase_summary"].as_array().unwrap().iter().find(|phase| phase["n"] == 2).unwrap();
    assert_eq!((header["status"].clone(), header["state"].clone()), (json!("cancelled"), json!("cancelled")), "{shown}");
    // A phase with one task still open is not cancelled and stays visible.
    ops::apply(&mut team, "extend", &json!({"phase":2,"tasks":[{"id":4,"title":"Back","assignee":"dev"}]}), &Actor::Human, &none, &mut events).unwrap();
    let partial = view_with_cancelled(&team, &[], None, false);
    assert!(partial["phases"].as_array().unwrap().iter().any(|phase| phase["n"] == 2), "{partial}");
    // The plain view (used inside the dispatcher) keeps the whole team.
    assert!(view_with(&team, &[], None)["phases"].as_array().unwrap().iter().any(|phase| phase["n"] == 2));
}

#[test]
fn a_queued_report_keeps_its_whole_text_and_a_budget_queues_too() {
    // U42 (#512): the queued route stored the placeholder "poll" over the
    // whole report. One request id keeps the first whole text; a later
    // request under it is refused, never queued in its place. A budget
    // from the sandbox queues like a report.
    let _env = crate::state::ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        let member_folder = dir.path().join("dev");
        std::fs::create_dir_all(&member_folder).unwrap();
        {
            let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
            dev.agent = "dsh".into();
            dev.session = Some(SessionRef { name: "dsh/x/dev".into(), run_id: "r1".into(), conversation_id: "conv-dev".into(), ..Default::default() });
            dev.folder = crate::state::teams::model::MemberFolder::Shared;
            dev.directory = member_folder.display().to_string();
        }
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        super::set_test_caller_identities(Some((Some("conv-dev".into()), None, None)));
        let request = "00000000-0000-4000-8000-000000000901";
        let attempt = "00000000-0000-4000-8000-000000000001";
        let whole = "The whole report: 512 checks and their totals";
        let first = super::queue_write(
            "report",
            &json!({"request_id":request,"team":id,"attempt":attempt,"task":1,"text":whole}),
            false,
        )
        .unwrap();
        assert_eq!(first["status"], "queued");
        // A poll that carries a placeholder neither replaces it nor queues again.
        let error = super::queue_write(
            "report",
            &json!({"request_id":request,"team":id,"attempt":attempt,"task":1,"text":"poll"}),
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("already queues another request"), "{error}");
        let queued = crate::state::teams::outbox::pending(&member_folder, 10).unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].1["input"]["text"], whole);
        // The same request again is a retry: it answers its state, one file.
        let again = super::queue_write(
            "report",
            &json!({"request_id":request,"team":id,"attempt":attempt,"task":1,"text":whole}),
            false,
        )
        .unwrap();
        assert_eq!(again["status"], "queued");
        assert_eq!(crate::state::teams::outbox::pending(&member_folder, 10).unwrap().len(), 1);
        // U42: a budget from the member's sandbox queues in its folder too.
        let budget = super::queue_write(
            "expect",
            &json!({"request_id":"00000000-0000-4000-8000-000000000902","team":id,"label":"the suites","minutes":15}),
            false,
        )
        .unwrap();
        assert_eq!(budget["status"], "queued");
        let queued = crate::state::teams::outbox::pending(&member_folder, 10).unwrap();
        assert_eq!(queued.len(), 2);
        assert_eq!(
            (queued[1].1["op"].as_str(), queued[1].1["input"]["minutes"].as_u64()),
            (Some("expect"), Some(15))
        );
        // The gate that sends a write to the queue holds for both kinds.
        for op in ["report", "ask", "commit", "expect"] {
            assert!(super::queues_instead(op), "{op}");
        }
        for op in ["accept", "cancel", "scale", "drain"] {
            assert!(!super::queues_instead(op), "{op}");
        }
        super::set_test_caller_identities(None);
    });
}

#[test]
fn a_leaked_session_env_never_names_the_caller() {
    let mut team = running_team();
    let attempt = team.task(1).unwrap().attempts[0].id.clone();
    let human = json!({"request_id":"11111111-1111-4111-8111-111111111111","by":"human"});
    let foreign = Some("codex/AIBalanceTools/AI BalancerUpdate");
    // Its own session ancestry still proves the member, whatever the env says.
    let own = json!({"name":"codex/x/dev","run_id":"r1"});
    assert_eq!(resolve_actor(&team, "ask", &json!({}), Some(&own), None, foreign, None).unwrap(), Actor::Member("dev".into()));
    // A dsh member with a leaked env is proven by its conversation.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session.as_mut().unwrap().conversation_id = "c1".into();
    assert_eq!(resolve_actor(&team, "ask", &json!({}), None, Some("c1"), foreign, None).unwrap(), Actor::Member("dev".into()));
    // A report by attempt id is proven too.
    assert_eq!(resolve_actor(&team, "report", &json!({"attempt":attempt}), None, None, foreign, None).unwrap(), Actor::Member("dev".into()));
    // The leaked env alone proves nothing: it cannot act as the owner.
    let error = resolve_actor(&team, "stop", &human, None, None, foreign, None).unwrap_err().to_string();
    assert_eq!(error, "Zerus could not verify the calling session");
    // Outside every session, the owner may act.
    assert_eq!(resolve_actor(&team, "stop", &human, None, None, None, None).unwrap(), Actor::Human);
    // U12: a dsh session outside this team, and any session carrying the
    // hgs run marker, never acts as the owner - whatever "by" says.
    for op in ["stop", "approve-phase", "swap-member"] {
        let error = resolve_actor(&team, op, &human, None, Some("session-other"), None, None).unwrap_err().to_string();
        assert_eq!(error, "An agent session cannot act as the owner", "{op}");
        let error = resolve_actor(&team, op, &human, None, None, None, Some("run-other")).unwrap_err().to_string();
        assert_eq!(error, "An agent session cannot act as the owner", "{op}");
    }
}

#[test]
fn a_captured_report_is_replaced_through_the_report_operation() {
    let mut team = running_team();
    {
        let attempt = &mut team.task_mut(1).unwrap().attempts[0];
        attempt.state = AttemptState::Reported;
        attempt.report = Some(Report { text: "Building and running...".into(), at: 20.0, captured: true, ..Default::default() });
    }
    team.task_mut(1).unwrap().status = TaskStatus::Submitted;
    let mut events = Vec::new();
    let receipt = ops::apply(
        &mut team,
        "report",
        &json!({"task":1,"attempt":"00000000-0000-4000-8000-000000000001","text":"Done: the tests pass."}),
        &Actor::Member("dev".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(receipt["status"], "submitted");
    let report = team.task(1).unwrap().attempts[0].report.as_ref().unwrap();
    assert!(!report.captured);
    assert_eq!(report.text, "Done: the tests pass.");
    assert_eq!(report.replaced, vec![ReplacedReport { text: "Building and running...".into(), at: 20.0 }]);
    assert_eq!(events.last().unwrap().kind, "report_replaced");
}

#[test]
fn an_existing_session_is_adopted_with_its_own_folder() {
    let mut team = running_team();
    team.folder = "D:/work/shop".into();
    let sessions = vec![found_session("dsh/x/dev2", "r9", true, "D:/work/shop/module")];
    let adopted = adopted_session(&team, &existing_member(), &[], &sessions).unwrap();
    assert_eq!(adopted.directory, "D:/work/shop/module");
    assert!(!adopted.worktree);
    // The run and conversation come from the live session when omitted.
    assert_eq!((adopted.session.run_id.as_str(), adopted.session.conversation_id.as_str()), ("r9", "conversation-1"));
    // A session that is not running, or on another computer, is refused.
    let stopped = adopted_session(&team, &existing_member(), &[], &[found_session("dsh/x/dev2", "r9", false, "D:/w")]);
    assert_eq!(stopped.unwrap_err().to_string(), "dsh/x/dev2 is not running on this computer");
    let missing = adopted_session(&team, &existing_member(), &[], &[]);
    assert_eq!(missing.unwrap_err().to_string(), "dsh/x/dev2 is not running on this computer");
    let mut elsewhere = existing_member();
    elsewhere.computer = "msi".into();
    let remote = adopted_session(&team, &elsewhere, &[], &sessions);
    assert_eq!(remote.unwrap_err().to_string(), "dsh/x/dev2 is not running on msi");
    let mut mismatched = existing_member();
    mismatched.computer = "msi".into();
    mismatched.session.as_mut().unwrap().host = "peer".into();
    let wrong = adopted_session(&team, &mismatched, &[], &sessions);
    assert_eq!(wrong.unwrap_err().to_string(), "dsh/x/dev2 runs on peer, not on msi");
    // A worktree member needs the session inside the team's repository.
    let mut worktree = existing_member();
    worktree.folder = MemberFolder::Worktree;
    let refused = adopted_session(&team, &worktree, &[], &sessions);
    assert!(refused.unwrap_err().to_string().contains("is not the team's repository"));
    // A session on another computer joins in its own shared folder, but a
    // worktree member is refused clearly: git runs here, not there (L5).
    let mut remote = existing_member();
    remote.computer = "msi".into();
    remote.session.as_mut().unwrap().host = "msi".into();
    let remote_sessions = vec![Seen {
        host: "msi".into(),
        name: "dsh/x/dev2".into(),
        live: true,
        cwd: "/work/w".into(),
        ..Default::default()
    }];
    let adopted = adopted_session(&team, &remote, &[], &remote_sessions).unwrap();
    assert_eq!((adopted.directory.as_str(), adopted.worktree), ("/work/w", false));
    remote.folder = MemberFolder::Worktree;
    let refused = adopted_session(&team, &remote, &[], &remote_sessions);
    assert!(refused.unwrap_err().to_string().contains("must be on this computer"));
}

#[test]
fn a_session_busy_in_another_active_team_is_refused() {
    let team = running_team();
    let mut other = running_team();
    other.id = "other".into();
    other.name = "Other".into();
    other.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
        Some(SessionRef { name: "dsh/x/dev2".into(), run_id: "r9".into(), ..Default::default() });
    let sessions = vec![found_session("dsh/x/dev2", "r9", true, "D:/w")];
    let taken = adopted_session(&team, &existing_member(), &[team.clone(), other.clone()], &sessions);
    assert_eq!(taken.unwrap_err().to_string(), "dsh/x/dev2 is a member of team Other");
    // The lead of another active team is refused too.
    other.members.iter_mut().find(|member| member.id == "dev").unwrap().session = None;
    other.members.iter_mut().find(|member| member.id == "lead").unwrap().session =
        Some(SessionRef { name: "dsh/x/dev2".into(), ..Default::default() });
    let lead = adopted_session(&team, &existing_member(), &[team.clone(), other.clone()], &sessions);
    assert_eq!(lead.unwrap_err().to_string(), "dsh/x/dev2 is the lead of team Other");
    // A finished team does not keep the session.
    other.status = TeamStatus::Done;
    assert!(adopted_session(&team, &existing_member(), &[team.clone(), other], &sessions).is_ok());
}

#[test]
fn a_retried_adopted_session_request_returns_its_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = running_team();
    team.status = TeamStatus::Planning;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    // The first plan records its receipt for a member with a session.
    const PLAN: &str = "44444444-4444-4444-8444-444444444444";
    let input = json!({"members":[{"id":"dev2","role":"Backend","agent":"dsh","session":{"name":"dsh/x/dev2"}}],
        "tasks":[{"id":1,"title":"A","assignee":"dev2"}]});
    let receipt = store
        .mutate(Some((PLAN, &input)), "t", None, |team, events| {
            ops::apply(team, "plan", &input, &Actor::Human, &BTreeSet::new(), events)
        })
        .unwrap();
    // The retry returns the receipt before listing sessions (the named
    // session is not live, so adoption would refuse it) - L4.
    let mut filled = input.clone();
    adopt_existing_sessions(&store, PLAN, &input, "plan", &mut filled).unwrap();
    assert_eq!(filled, input);
    assert_eq!(store.receipt(PLAN, &input).unwrap().unwrap(), receipt);
}

#[test]
fn a_subteam_caller_is_the_assignee_parent_lead_or_owner() {
    let mut parent = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        ..Default::default()
    };
    let session = |name: &str, run: &str, conversation: &str| SessionRef {
        name: name.into(),
        run_id: run.into(),
        conversation_id: conversation.into(),
        host: String::new(),
    };
    parent.members = vec![
        Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), session: Some(session("claude/x/lead", "r0", "")), ..Default::default() },
        Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), session: Some(session("codex/x/dev", "r1", "")), ..Default::default() },
        Member {
            id: "dshdev".into(),
            role: "Backend".into(),
            agent: "dsh".into(),
            session: Some(session("dsh/x/dshdev", "r2", "conv-dev")),
            ..Default::default()
        },
        Member {
            id: "dshqa".into(),
            role: "Tester".into(),
            agent: "dsh".into(),
            session: Some(session("dsh/x/dshqa", "r3", "conv-qa")),
            ..Default::default()
        },
    ];
    // A DeepSeek assignee is proven by its conversation id, not by an hgs
    // session record.
    assert_eq!(parent_caller(&parent, "dshdev", None, Some("conv-dev"), None, None, "").unwrap(), ParentCaller::Assignee);
    // A dsh non-assignee cannot pass by:human as the owner.
    let error = parent_caller(&parent, "dshdev", None, Some("conv-qa"), None, None, "human")
        .unwrap_err()
        .to_string();
    assert!(error.contains("assignee"), "{error}");
    // The owner path still needs no session env and says it is you.
    assert_eq!(parent_caller(&parent, "dev", None, None, None, None, "human").unwrap(), ParentCaller::Owner);
    // U12: a dsh session outside the parent, or a run marker, may not claim
    // the owner path either.
    let error = parent_caller(&parent, "dev", None, Some("session-other"), None, None, "human").unwrap_err().to_string();
    assert_eq!(error, "An agent session cannot act as the owner");
    let error = parent_caller(&parent, "dev", None, None, None, Some("run-other"), "human").unwrap_err().to_string();
    assert_eq!(error, "An agent session cannot act as the owner");
    let error = parent_caller(&parent, "dev", None, None, Some("HGS"), None, "human")
        .unwrap_err()
        .to_string();
    assert!(error.contains("could not verify"), "{error}");
    // A session record is checked against the parent team: the assignee
    // and the parent lead are accepted, a stranger is refused.
    let dev = json!({"name":"codex/x/dev","run_id":"r1"});
    assert_eq!(parent_caller(&parent, "dev", Some(&dev), None, Some(""), None, "").unwrap(), ParentCaller::Assignee);
    let lead = json!({"name":"claude/x/lead","run_id":"r0"});
    assert_eq!(parent_caller(&parent, "dev", Some(&lead), None, None, None, "").unwrap(), ParentCaller::ParentLead);
    let stranger = json!({"name":"codex/x/other","run_id":"r9"});
    assert!(parent_caller(&parent, "dev", Some(&stranger), None, None, None, "").is_err());
}

#[test]
fn a_draft_phase_can_run_alongside_a_running_phase() {
    let mut team = running_team();
    // F12/N40: phase 1 still runs, and the draft after it says it can run
    // alongside - not that it waits; the owner is warned by the desktop.
    team.tasks.push(Task { id: 3, title: "Fix".into(), phase: 2, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() });
    team.phases.push(Phase {
        n: 2,
        order: 2,
        title: "Fixes".into(),
        approval: Approval::Review,
        state: PhaseState::Draft,
        tasks: vec![3],
        ..Default::default()
    });
    let value = view_with(&team, &[], None);
    let phase = &value["phases"][0];
    assert_eq!(phase["waiting"]["text"], "Can run alongside phase 1");
    assert_eq!(phase["waiting"]["alongside"], true);
    assert_eq!((phase["waiting"]["phase"].as_u64(), phase["waiting"]["open"].clone()), (Some(1), json!([1])));
    // The server's rule is unchanged: a draft opens when no earlier phase is
    // a draft, so this one can be approved while phase 1 still runs; task
    // 158: it then runs alongside phase 1.
    let mut approved = team.clone();
    let mut events = Vec::new();
    ops::apply(&mut approved, "approve-phase", &json!({"phase":2}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(approved.phases[0].state, PhaseState::Approved);
    // Once the previous phase is decided, the draft is simply a draft.
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    let value = view_with(&team, &[], None);
    assert!(value["phases"][0].get("waiting").is_none(), "{value}");
    // A second draft waits for the first draft by its number.
    team.phases.push(Phase {
        n: 3,
        order: 3,
        title: "Later".into(),
        approval: Approval::Review,
        state: PhaseState::Draft,
        tasks: vec![],
        ..Default::default()
    });
    // F12: a draft behind a draft says it waits for the approval, not for
    // readiness.
    let value = view_with(&team, &[], None);
    assert_eq!(value["phases"][1]["waiting"]["phase"], 2);
    assert_eq!(value["phases"][1]["waiting"]["open"], json!([3]));
    assert_eq!(value["phases"][1]["waiting"]["text"], "Waiting for the previous phase to be approved");
    assert!(value["phases"][1]["waiting"].get("alongside").is_none(), "{value}");
    // The approval refusal stays specific about which phase is in the way.
    let mut events = Vec::new();
    let error = ops::apply(&mut team, "approve-phase", &json!({"phase":3}), &Actor::Human, &BTreeSet::new(), &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Phase 3 opens after the previous phase is approved");
}

#[test]
fn a_draft_and_phase_edits_keep_their_own_revision() {
    // C1: "revision" of these operations is not the team's revision, so a
    // change elsewhere in the team never locks the draft or the phases.
    for op in ["draft", "edit-draft", "move-phase", "discard-phase", "extend"] {
        assert_eq!(expected_revision(op, &json!({"revision":3})), None, "{op}");
    }
    assert_eq!(expected_revision("plan", &json!({"revision":3})), Some(3));
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let (mut team, _) = ops::create("d", &json!({"name":"D","lead":{"id":"lead","agent":"claude"}})).unwrap();
    team.status = TeamStatus::Draft;
    team.draft = Some(Draft { revision: 1, ..Default::default() });
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    // The owner marks a field seen: the team revision moves, the draft's not.
    let seen = json!({"action":"seen","fields":["goal"]});
    store
        .mutate(None, "d", expected_revision("draft", &seen), |team, events| ops::apply(team, "draft", &seen, &Actor::Human, &BTreeSet::new(), events))
        .unwrap();
    let update = json!({"action":"update","revision":1,"set":{"goal":"Ship"}});
    let receipt = store
        .mutate(Some(("22222222-2222-4222-8222-222222222222", &update)), "d", expected_revision("draft", &update), |team, events| {
            ops::apply(team, "draft", &update, &Actor::Member("lead".into()), &BTreeSet::new(), events)
        })
        .unwrap();
    // The reply names the draft's revision apart from the team's.
    assert_eq!(receipt["draft_revision"], 2);
    assert_ne!(receipt["revision"], receipt["draft_revision"]);
    let again = json!({"action":"update","revision":2,"set":{"goal":"Ship it"}});
    store
        .mutate(None, "d", expected_revision("draft", &again), |team, events| {
            ops::apply(team, "draft", &again, &Actor::Member("lead".into()), &BTreeSet::new(), events)
        })
        .unwrap();
    assert_eq!(store.load("d").unwrap().unwrap().goal, "Ship it");
}

#[test]
fn only_the_owner_outside_every_session_starts_a_draft() {
    // C2: no team exists yet to prove a member by.
    let owner = json!({"by":"human"});
    owner_without_team(&owner, false, false, false, false).unwrap();
    for (record, dsh, env, run) in [(true, false, false, false), (false, true, false, false), (false, false, true, false), (false, false, false, true)] {
        assert!(owner_without_team(&owner, record, dsh, env, run).is_err(), "{record} {dsh} {env} {run}");
    }
    // Feedback 17: the owner's own shell needs no "by"; another value is refused.
    owner_without_team(&json!({}), false, false, false, false).unwrap();
    assert!(owner_without_team(&json!({}), false, false, true, false).is_err());
    assert!(owner_without_team(&json!({"by":"lead"}), false, false, false, false).is_err());
}

#[test]
fn the_parent_lead_is_proven_in_the_parent_team_by_record_or_dsh() {
    // C6: never by a sub-team member that carries the parent lead's id.
    let mut parent = running_team();
    parent.id = "parent".into();
    parent.members[0].session.as_mut().unwrap().conversation_id = "dsh-lead".into();
    let mut child = running_team();
    child.id = "child".into();
    child.parent = Some(Parent { team: "parent".into(), task: 1, lead: "lead".into(), member: "dev".into() });
    let lead = json!({"name":"claude/x/lead","run_id":"r0"});
    let stranger = json!({"name":"claude/x/other","run_id":"r9"});
    // The sub-team's own lead runs in the same session as the parent's in
    // running_team(); a distinct sub-lead shows the proof.
    child.members[0].session = Some(SessionRef { name: "claude/x/sublead".into(), run_id: "r5".into(), ..Default::default() });
    assert!(parent_lead_proven(&child, Some(&parent), Some(&lead), None));
    assert!(!parent_lead_proven(&child, Some(&parent), Some(&stranger), None));
    assert!(parent_lead_proven(&child, Some(&parent), None, Some("dsh-lead")));
    assert!(!parent_lead_proven(&child, Some(&parent), None, Some("dsh-other")));
    assert!(!parent_lead_proven(&child, None, Some(&lead), None));
    let mut unlinked = child.clone();
    unlinked.parent = None;
    assert!(!parent_lead_proven(&unlinked, Some(&parent), Some(&lead), None));
}

/// U51: the owner's key adds one bounded urgent phase and nothing else.
#[test]
fn an_intake_key_adds_one_urgent_phase_and_no_other_operation() {
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        // A team with none of the new fields behaves as today.
        let before = stored();
        assert!(before.intakes.is_empty() && before.intakes_seq == 0, "{before:?}");
        assert!(before.settings.restart_crashed);
        assert!(before.members.iter().all(|member| member.restarts.is_empty() && member.crash.is_none() && !member.crash_told));
        let request = "11111111-2222-4333-8444-555555555551";
        let created = run("intake-key", &json!({"request_id":request,"team":"rv","label":"health watch","by":"human"}), false).unwrap();
        assert_eq!((created["status"].clone(), created["id"].clone()), (json!("created"), json!("i1")));
        let key = created["key"].as_str().unwrap().to_owned();
        assert!(!key.is_empty());
        let saved = stored();
        assert_eq!(saved.intakes.len(), 1);
        assert_eq!(saved.intakes[0].label, "health watch");
        assert_eq!(saved.intakes[0].hash, ops::intake_hash(&key));
        assert_ne!(saved.intakes[0].hash, key);
        // A replay returns its first receipt, and the key is not shown again.
        let again = run("intake-key", &json!({"request_id":request,"team":"rv","label":"health watch","by":"human"}), false).unwrap();
        assert_eq!(again["status"], json!("created"));
        assert!(again.get("key").is_none(), "{again}");
        // The key adds one urgent phase: barrier off, approved at once.
        let added = call("intake", json!({"key":key,"tasks":[{"title":"Hotfix","assignee":"dev"},{"title":"Probe","assignee":"qa"}]})).unwrap();
        assert_eq!(added["status"], json!("intake"));
        let saved = stored();
        let phase = saved.phases.last().unwrap();
        assert!(phase.urgent && !phase.barrier, "{phase:?}");
        assert_eq!(phase.approval, Approval::Auto);
        assert_eq!(phase.title, "Urgent: health watch");
        assert_eq!(phase.state, PhaseState::Approved);
        assert_eq!(phase.tasks, vec![4, 5]);
        // The feed names the key's label, never the key.
        let feed = Store::open(false).unwrap().events("rv", None, 50).unwrap().to_string();
        assert!(feed.contains("Intake health watch"), "{feed}");
        assert!(!feed.contains(&key), "{feed}");
        // Bounded: one to five tasks, and never a named phase.
        for tasks in [vec![json!({"title":"A","assignee":"dev"}); 6], Vec::new()] {
            let error = call("intake", json!({"key":key,"tasks":tasks})).unwrap_err();
            assert!(error.to_string().contains("1 to 5"), "{error}");
        }
        let error = call("intake", json!({"key":key,"phase":2,"tasks":[{"title":"X","assignee":"dev"}]})).unwrap_err();
        assert!(error.to_string().contains("cannot name"), "{error}");
        // The key opens no other operation.
        for op in ["status", "inbox", "extend", "plan", "report", "accept", "settings", "intake-key", "relaunch", "stop"] {
            let error = call(op, json!({"key":key})).unwrap_err();
            assert!(error.to_string().contains("intake key adds urgent tasks only"), "{op}: {error}");
        }
        // A member cannot create a key; the owner revokes it by id.
        let denied = as_lead(|| {
            run("intake-key", &json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":"rv","label":"x","by":"human"}), false)
        })
        .unwrap_err();
        assert!(denied.to_string().contains("Only you"), "{denied}");
        let revoked = call("intake-key", json!({"revoke":"i1","by":"human"})).unwrap();
        assert_eq!(revoked["status"], json!("revoked"));
        assert!(stored().intakes.is_empty());
        let after = call("intake", json!({"key":key,"tasks":[{"title":"Late","assignee":"dev"}]})).unwrap_err();
        assert!(after.to_string().contains("Unknown or revoked"), "{after}");
        // Without a key nothing changes.
        let error = call("intake", json!({"tasks":[{"title":"Late","assignee":"dev"}]})).unwrap_err();
        assert!(error.to_string().contains("carries the owner's key"), "{error}");
    });
}

/// #581 N4/L3: an intake task keeps only the plan fields the key is given,
/// and a key has at most one open urgent phase.
#[test]
fn an_intake_task_keeps_only_its_plan_fields_and_one_phase_is_open_per_key() {
    use crate::state::teams::model::TaskKind;
    let (team, _, _) = review_team();
    with_review_team(&team, || {
        let created = run(
            "intake-key",
            &json!({"request_id":"11111111-2222-4333-8444-555555555552","team":"rv","label":"watch","by":"human"}),
            false,
        )
        .unwrap();
        let key = created["key"].as_str().unwrap().to_owned();
        let added = call(
            "intake",
            json!({"key":key,"tasks":[{
                "title":"Fix","description":"d","criteria":["c"],"assignee":"dev",
                "pipeline":"none","pinned":true,"phase":9,"models":["m"],"kind":"check",
                "check":{"command":"true"},"deps":[{"task":1}],"lane":{"id":"x"},"files":["a"]}]}),
        )
        .unwrap();
        let saved = stored();
        let task = saved.task(added["tasks"][0].as_u64().unwrap() as u32).unwrap();
        assert_eq!((task.title.as_str(), task.description.as_str()), ("Fix", "d

Files: a"));
        assert_eq!(task.criteria, vec!["c"]);
        assert_eq!(task.assignee, "dev");
        // The owner's plan fields are not the key's; the pipeline resolves
        // as for a plan task with no choice.
        assert!(task.pipeline.is_empty(), "pipeline: {}", task.pipeline);
        assert!(!task.pinned && task.models.is_empty() && task.lane.is_none() && task.check.is_none());
        assert!(task.deps.is_empty() && task.kind == TaskKind::Task);
        assert_eq!(task.phase, saved.phases.last().unwrap().n);
        // L3: the key's phase is still open, so it adds no second one.
        let error = call("intake", json!({"key":key,"tasks":[{"title":"More","assignee":"dev"}]})).unwrap_err();
        assert!(error.to_string().contains("still open"), "{error}");
        // Once its work is decided, the key may add another urgent phase.
        for id in added["tasks"].as_array().unwrap() {
            let id = id.as_u64().unwrap() as u32;
            call("cancel", json!({"task":id,"by":"human"})).unwrap();
        }
        let again = call("intake", json!({"key":key,"tasks":[{"title":"Later","assignee":"dev"}]})).unwrap();
        assert_eq!(again["status"], json!("intake"));
    });
}

/// Task 158 (owner, 2026-11-07): start-ahead is gone; the CLI still routes it
/// so an older lead or desktop hears why, and nothing changes.
#[test]
fn start_ahead_is_refused_with_the_reason_through_the_cli() {
    use crate::state::teams::model::{Approval, Phase, PhaseState, Task, TaskStatus};
    let (mut team, _, _) = review_team();
    team.settings.auto_accept = true;
    team.phases.push(Phase {
        n: 9,
        order: 9,
        title: "Next".into(),
        approval: Approval::Auto,
        state: PhaseState::Draft,
        tasks: vec![99],
        ..Default::default()
    });
    team.tasks.push(Task { id: 99, title: "Next".into(), phase: 9, status: TaskStatus::Draft, assignee: "dev".into(), ..Default::default() });
    with_review_team(&team, || {
        for error in [as_lead(|| call("start-ahead", json!({"phase":9}))), call("start-ahead", json!({"phase":9,"by":"human"}))] {
            let error = error.unwrap_err().to_string();
            assert!(error.contains("Start ahead is gone"), "{error}");
        }
        assert_eq!(stored().phases.iter().find(|phase| phase.n == 9).unwrap().state, PhaseState::Draft);
        assert!(!crate::state::teams::commands::USAGE.contains("start-ahead"));
    });
}

/// U61/U52: a team stored before this phase carries none of the new fields,
/// loads with today's defaults and writes none of them back.
#[test]
fn a_stored_team_without_the_new_fields_loads_and_behaves_as_today() {
    let (team, _, _) = review_team();
    let mut value = serde_json::to_value(&team).unwrap();
    for key in ["intakes", "intakes_seq"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value["settings"].as_object_mut().unwrap().remove("restart_crashed");
    for member in value["members"].as_array_mut().unwrap() {
        for key in ["restarts", "crash", "crash_told"] {
            member.as_object_mut().unwrap().remove(key);
        }
    }
    let mut legacy: Team = serde_json::from_value(value).unwrap();
    assert!(legacy.intakes.is_empty() && legacy.intakes_seq == 0);
    // Owner, 2026-11-05: restart_crashed is on by default, for old teams too.
    assert!(legacy.settings.restart_crashed);
    assert!(legacy.members.iter().all(|member| member.restarts.is_empty() && member.crash.is_none() && !member.crash_told));
    // The same plan, hand-out and settings work; none of the new fields is
    // written for a team that does not set them.
    let mut events = Vec::new();
    let refused = ops::apply(&mut legacy, "settings", &json!({"settings":{"restart_crashed":false}}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap_err();
    assert!(refused.to_string().contains("restart_crashed"), "{refused}");
    ops::apply(&mut legacy, "settings", &json!({"settings":{"restart_crashed":false}}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(!legacy.settings.restart_crashed);
    let written = serde_json::to_value(&legacy).unwrap();
    assert!(written.get("intakes").is_none() && written.get("intakes_seq").is_none(), "{written}");
    // An explicit off is written and survives a reload.
    assert_eq!(written["settings"]["restart_crashed"], false, "{written}");
    assert!(!serde_json::from_value::<Team>(written.clone()).unwrap().settings.restart_crashed);
    assert!(written["members"].as_array().unwrap().iter().all(|member| member.get("restarts").is_none()
        && member.get("crash").is_none() && member.get("crash_told").is_none()), "{written}");
}

/// U62 (#715): only an unnarrowed test template is a full run; the team's
/// policy and the owner's one-time grant decide who may start one.
#[test]
fn a_full_test_run_needs_the_teams_policy_or_the_owners_grant() {
    use crate::state::teams::model::{FullTests, FullTestsGrant};
    assert!(super::full_test_run("cargo-test", &json!({})));
    assert!(!super::full_test_run("cargo-test", &json!({"filter":"teams"})));
    // Task 53: the request hgs test affected prints for a client crate is
    // narrowed by its filters; without filters (skips or a manifest alone,
    // or only empty filters) it is still the whole suite.
    let printed = json!({"filters":["teams::tests::","teams::catalog_edit::"],"skips":["teams::tests::slow"],"manifest":["client/core/Cargo.toml"]});
    assert!(!super::full_test_run("cargo-test", &printed));
    assert!(super::full_test_run("cargo-test", &json!({"manifest":["client/core/Cargo.toml"]})));
    assert!(super::full_test_run("cargo-test", &json!({"skips":["teams::"],"manifest":["client/core/Cargo.toml"]})));
    assert!(super::full_test_run("cargo-test", &json!({"filters":[],"manifest":["client/core/Cargo.toml"]})));
    assert!(super::full_test_run("cargo-test", &json!({"filters":[" "]})));
    assert!(!super::full_test_run("qt-test", &json!({"suite":"test_teams"})));
    assert!(super::full_test_run("android-test", &json!({})));
    assert!(!super::full_test_run("cargo-build", &json!({})));
    let mut team = running_team();
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_path_buf();
    // Ask without a grant: refused with the estimate and the command.
    team.settings.full_tests = FullTests::Ask;
    let error = super::check_full_run(&team, &folder).unwrap_err().to_string();
    assert!(
        error.contains("full test run") && error.contains("full-tests") && error.contains("unknown time"),
        "{error}"
    );
    // The owner's one-time grant opens one run.
    team.full_tests_grant = Some(FullTestsGrant { release: "v1.57.0".into(), at: 1.0, by: "You".into() });
    assert!(super::check_full_run(&team, &folder).unwrap());
    // "allowed" opens every full run; "never" refuses even a grant.
    team.settings.full_tests = FullTests::Allowed;
    assert!(!super::check_full_run(&team, &folder).unwrap());
    team.settings.full_tests = FullTests::Never;
    assert!(super::check_full_run(&team, &folder).is_err(), "never refuses even a grant");
    // Status shows the policy and a grant.
    team.settings.full_tests = FullTests::Ask;
    let value = super::view_with(&team, &[], None);
    assert_eq!(value["full_tests"], "ask");
    assert_eq!(value["full_tests_grant"]["release"], "v1.57.0");
}

/// #740: a proposal is checked when the lead makes it, by the code Allow
/// runs. p6 wrote a dependency as a bare number: propose took it and Allow
/// failed with "Invalid task", leaving a card nobody could allow.
#[test]
fn a_proposal_allow_could_not_apply_is_refused_when_made() {
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    let bare = json!({"kind":"add-to-phase","reason":"p6","phase":1,
        "tasks":[{"id":2,"title":"Follow-up","assignee":"dev","deps":[1]}]});
    let error = ops::apply(&mut team, "propose", &bare, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("Allow could not apply") && error.contains("A task \"2\" is invalid: field \"deps\""), "{error}");
    assert!(team.proposals.is_empty(), "nothing was proposed");
    // Unknown work is refused too.
    let unknown = json!({"kind":"cancel-tasks","reason":"drop","tasks":[99]});
    assert!(ops::apply(&mut team, "propose", &unknown, &lead, &BTreeSet::new(), &mut Vec::new()).is_err());
    // The same dependency as the plan writes it is proposed and allowed.
    let good = json!({"kind":"add-to-phase","reason":"p6","phase":1,
        "tasks":[{"id":2,"title":"Follow-up","assignee":"dev","deps":[{"task":1}]}]});
    ops::apply(&mut team, "propose", &good, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert_eq!(team.proposals.len(), 1);
    assert!(team.task(2).is_none(), "the check ran on a copy");
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    let receipt = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}), &Actor::Human)
        .unwrap();
    assert_eq!(receipt["status"], "accepted", "{receipt}");
}

/// #740: the lead asks the owner a question with options and a recommended
/// one; the owner's answer, an option or words of the owner's own, goes to
/// the lead's inbox with the question.
#[test]
fn the_lead_asks_the_owner_a_question() {
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    let bad = json!({"kind":"question","question":"Where?","options":["D:","C:"],"recommended":"E:"});
    assert!(ops::apply(&mut team, "propose", &bad, &lead, &BTreeSet::new(), &mut Vec::new()).is_err(), "the recommended one is offered");
    assert!(ops::apply(&mut team, "propose", &json!({"kind":"question","options":["D:"]}), &lead, &BTreeSet::new(), &mut Vec::new()).is_err());
    let input = json!({"kind":"question","question":"Install the build cache on D:?","options":["Yes, on D:","Yes, on C:","No"],
        "recommended":"Yes, on D:"});
    let receipt = ops::apply(&mut team, "propose", &input, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert_eq!(receipt["change"]["recommended"], "Yes, on D:");
    assert_eq!(team.proposals[0]["change"]["options"], json!(["Yes, on D:","Yes, on C:","No"]));
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    // Waiting for you shows the question itself.
    let listed = super::super::owner_waiting::items(&[store.load("t").unwrap().unwrap()], super::super::super::now());
    assert!(listed.iter().any(|item| item["title"] == "The lead asks: Install the build cache on D:?"), "{listed:?}");
    let error = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"expected_hash":hash}), &Actor::Human)
        .unwrap_err()
        .to_string();
    assert!(error.contains("answer"), "an answer is required: {error}");
    let answered = decide_proposal_in(
        &mut store,
        &json!({"team":"t","proposal":"p1","accept":true,"expected_hash":hash,"answer":"Yes, on D:, but 30 GB"}),
        &Actor::Human,
    )
    .unwrap();
    assert_eq!(answered["status"], "answered", "{answered}");
    let team = store.load("t").unwrap().unwrap();
    assert_eq!((team.proposals[0]["state"].as_str(), team.proposals[0]["answer"].as_str()), (Some("answered"), Some("Yes, on D:, but 30 GB")));
    let items = super::super::inbox::items(&team);
    let told = items.iter().find(|item| item.kind == "proposal_decided").expect("the answer reaches the lead");
    assert!(told.summary.contains("answered your question p1"), "{}", told.summary);
    assert_eq!(told.detail, "Yes, on D:, but 30 GB");
}

/// Owner 2026-10-31: every task carries its estimate in status and in its
/// timeline, and a running one the time spent against it.
#[test]
fn status_and_spans_carry_each_tasks_estimate() {
    let (mut team, _, _) = review_team();
    if let Some(task) = team.task_mut(1) {
        task.estimate_minutes = Some(40.0);
        task.estimate_source = "lead".into();
    }
    if let Some(task) = team.task_mut(2) {
        task.status = crate::state::teams::model::TaskStatus::InProgress;
        task.attempts = vec![crate::state::teams::model::Attempt {
            n: 1,
            member: "qa".into(),
            sent_at: crate::state::now() - 3600.0,
            delivered_at: Some(crate::state::now() - 3600.0),
            ..Default::default()
        }];
    }
    with_review_team(&team, || {
        let status = call("status", json!({})).unwrap();
        let one = status["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 1).unwrap();
        assert_eq!(one["estimate_minutes"], json!(40.0));
        assert_eq!(one["estimate_source"], json!("lead"));
        assert_eq!(one["estimate_text"], json!("40 min"));
        let running = status["tasks"].as_array().unwrap().iter().find(|task| task["id"] == 2).unwrap();
        assert!(running["estimate_spent_minutes"].as_f64().unwrap() >= 59.0, "{running}");
        assert!(running["estimate_minutes"].is_null(), "no history yet: {running}");
        assert_eq!(running["estimate_text"], json!("no estimate yet"));
        let spans = call("spans", json!({"task":1})).unwrap();
        assert_eq!(spans["tasks"][0]["estimate_minutes"], json!(40.0));
        assert_eq!(spans["tasks"][0]["estimate_text"], json!("40 min"));
    });
}

/// Task 184: a working-tier write is redacted, never refused, and says so.
#[test]
fn a_note_and_a_message_with_a_secret_are_stored_redacted() {
    let (team, attempt, _) = review_team();
    with_review_team(&team, || {
        let answer = call("note", json!({"attempt":attempt, "done":"rotate DB_PASSWORD=Tr0ub4dor&3x now", "next":"deploy"})).unwrap();
        assert_eq!(answer["status"], "noted", "{answer}");
        assert_eq!(answer["redacted"], json!(["secret-assignment"]), "{answer}");
        assert!(answer["pulse"]["done"].as_str().unwrap().contains("[redacted: secret-assignment]"), "{answer}");
        assert!(!answer["pulse"]["done"].as_str().unwrap().contains("Tr0ub4dor"), "{answer}");
        let store = crate::state::teams::store::Store::open(false).unwrap();
        let latest = store.latest_pulses("rv", crate::state::now());
        assert!(latest[0].done.contains("[redacted: secret-assignment]"), "{}", latest[0].done);
        assert!(!latest[0].done.contains("Tr0ub4dor"), "{}", latest[0].done);

        let answer = call("message", json!({"to":"dev","text":concat!("the key is AKIA", "ABCDEFGHIJKLWXYZ")})).unwrap();
        assert_eq!(answer["redacted"], json!(["aws-access-key-id"]), "{answer}");
        let messages = crate::state::teams::working::unread(&store.connection(), "rv", "dev", crate::state::now()).unwrap();
        assert!(messages.iter().any(|message| message.text.contains("[redacted: aws-access-key-id]")), "{messages:?}");
        assert!(messages.iter().all(|message| !message.text.contains(concat!("AKIA", "ABCDEFGHIJKLWXYZ"))), "{messages:?}");
    });
}

/// Task #269: `hgs team look` reads the member's own build (and a tool call)
/// from the live listing, so a quiet build is named, not called uncertain.
#[test]
fn look_reads_the_members_build_and_tool_call_from_the_live_listing() {
    use crate::state::teams::dispatcher::{tool_stuck_minutes, Seen};
    use crate::state::teams::look;
    let mut team = Team::default();
    team.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "kimi".into(), ..Default::default() });
    let qa = team.member("qa").unwrap();
    let at = 10_000.0;
    let seen = Seen {
        live: true,
        activity: "busy".into(),
        phase: "working".into(),
        building: true,
        build_label: "cargo-build".into(),
        last_event_at: Some(at - 1500.0),
        ..Default::default()
    };
    let hooks = look_hooks(&team, qa, Some(&seen), at);
    assert!(hooks.building);
    assert_eq!(hooks.build_label, "cargo-build");
    assert_eq!(hooks.quiet, 1500.0);
    assert_eq!(hooks.limit, super::super::ops::stuck_minutes(&team) * 60.0);
    assert_eq!(look::waiting(&hooks).as_deref(), Some("its own Zerus build (cargo-build)"));
    // A tool call in flight gets the dispatcher's longer tool bound and is named.
    let seen = Seen {
        live: true,
        activity: "busy".into(),
        phase: "tool".into(),
        tool: "Bash".into(),
        tool_started: Some(at - 3000.0),
        turn_started: Some(at - 4000.0),
        last_event_at: Some(at - 3000.0),
        ..Default::default()
    };
    let hooks = look_hooks(&team, qa, Some(&seen), at);
    assert_eq!(hooks.limit, tool_stuck_minutes(&team) * 60.0);
    assert_eq!(look::waiting(&hooks).as_deref(), Some("a tool call (Bash)"));
    // Without a listing the stored fields and the usual bound are used.
    let hooks = look_hooks(&team, qa, None, at);
    assert!(!hooks.building && hooks.tool.is_empty());
    assert_eq!(hooks.limit, super::super::ops::stuck_minutes(&team) * 60.0);
}
/// Task #275: a queued call's answer names the request id, where its result
/// stays and that a refusal or failure reaches the member as a team message.
#[test]
fn a_queued_call_says_where_its_result_arrives() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("work");
        std::fs::create_dir_all(&folder).unwrap();
        let (mut team, _, _) = review_team();
        team.folder = folder.to_string_lossy().into_owned();
        team.status = TeamStatus::Running;
        team.members.iter_mut().find(|member| member.id == "dev").unwrap().session = Some(SessionRef {
            name: "codex/x/dev".into(),
            run_id: "r1".into(),
            conversation_id: "dev-conversation".into(),
            ..Default::default()
        });
        Store::open(true).unwrap().create("66666666-6666-4666-8666-666666666666", &json!({}), team.clone(), vec![]).unwrap();
        set_test_caller_identities(Some((Some("dev-conversation".into()), None, None)));
        let request_id = "77777777-7777-4777-8777-777777777777";
        let queued = queue_write("ask", &json!({"request_id":request_id,"team":team.id,"text":"How?","attempt":""}), false).unwrap();
        set_test_caller_identities(None);
        assert_eq!(queued["status"], json!("queued"));
        assert_eq!(queued["request_id"], json!(request_id));
        assert!(queued["result"].as_str().unwrap().ends_with(&format!("{request_id}.result.json")), "{queued}");
        assert!(queued["delivery"].as_str().unwrap().contains("team message"), "{queued}");
        assert!(queued["note"].as_str().unwrap().contains("team message"), "{queued}");
    });
}

/// Task #466 (owner, 2026-11-09): the startup trust requests stay the owner's to
/// answer, never a team lead's (CLAUDE.md: who answers, not where).
#[test]
fn a_startup_trust_request_is_the_owners_to_answer_in_person() {
    let trust = json!({"trust_request":true,"source":"claude_bypass_warning","can_answer":true});
    let approval = json!({"approval":true,"source":"claude_tool_approval","can_answer":true});
    assert!(!owner_only_trust(&trust, &Actor::Human));
    assert!(owner_only_trust(&trust, &Actor::Member("lead".into())));
    assert!(owner_only_trust(&trust, &Actor::ParentLead));
    // An ordinary approval is the lead's to answer, as before.
    assert!(!owner_only_trust(&approval, &Actor::Member("lead".into())));
}

/// Task #13 (team ede724c4, owner 2026-11-10): the whole path from New team to
/// running members, through the real command entry and the dispatcher's own
/// steps, in an isolated state folder, meets no refusal: Start planning, the
/// lead's draft (members, tasks and the owner's settings as offers), the
/// owner's one-click apply and Create team, members started at once (one
/// waits for the worktree lock), the lead adding a task to the running phase
/// and relaunching a member that failed to start.
#[test]
fn new_team_to_running_members_meets_no_refusal() {
    use crate::state::teams::dispatcher::{self as worker, Seen, Step};
    let dir = tempfile::tempdir().unwrap();
    worker::set_git_ceiling(&dir.path().display().to_string());
    let repo = dir.path().join("project");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git").arg("-C").arg(&repo).args(["-c", "commit.gpgsign=false"]).args(args).output().unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(repo.join("a.txt"), "a").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    crate::state::with_test_state_root(&state, || {
        let _worker = hold_worker_lock();
        let refusals = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let step = |what: &str, result: anyhow::Result<Value>| -> Value {
            result.unwrap_or_else(|error| {
                refusals.borrow_mut().push(format!("{what}: {error:#}"));
                Value::Null
            })
        };
        // The fake computer: `worktrees create` is busy twice (members
        // created at once), then makes the worktree; qa's first start fails.
        let busy = std::rc::Rc::new(std::cell::Cell::new(2));
        let qa_fails = std::rc::Rc::new(std::cell::Cell::new(true));
        let (busy_left, qa_first) = (busy.clone(), qa_fails.clone());
        worker::set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            if args.first() == Some(&"worktrees") && args.get(1) == Some(&"create") {
                if busy_left.get() > 0 {
                    busy_left.set(busy_left.get() - 1);
                    return Ok(worker::fake_output(1, "", "Another worktree operation is running. Try again after it finishes."));
                }
                let value = |key: &str| args.iter().position(|arg| *arg == key).map(|at| args[at + 1].to_owned()).unwrap();
                let output = std::process::Command::new("git")
                    .arg("-C")
                    .arg(value("--path"))
                    .args(["worktree", "add", "--quiet", "-b", &value("--branch"), &value("--destination"), "HEAD"])
                    .output()?;
                return Ok(output);
            }
            if args.first() == Some(&"worktrees") {
                return Ok(worker::fake_output(0, r#"{"state":"folder_unavailable"}"#, ""));
            }
            if args.iter().any(|arg| arg.ends_with("-qa")) && qa_first.get() {
                qa_first.set(false);
                return Ok(worker::fake_output(1, "", "the agent did not start"));
            }
            Ok(worker::fake_output(0, r#"{"status":"started"}"#, ""))
        })));
        let load = |id: &str| Store::open(false).unwrap().load(id).unwrap().unwrap();
        // 1. New team, Start planning: only the draft and its lead.
        super::set_test_caller_identities(Some((None, None, None)));
        let started = step(
            "start planning",
            run(
                "draft",
                &json!({"action":"start","by":"human","request_id":"13131313-1313-4131-8131-000000000001",
                    "folder":repo.display().to_string(),"name":"Walk","lead":{"agent":"claude"}}),
                false,
            ),
        );
        let id = started["team"].as_str().unwrap_or_default().to_owned();
        assert!(!id.is_empty(), "{:?}", refusals.borrow());
        let team = load(&id);
        let mut store = Store::open(true).unwrap();
        assert_eq!(team.members.len(), 1, "only the lead");
        step("launch the lead", worker::test_run_step(&mut store, &team, &Step::Launch { member: "lead".into() }).map(|_| Value::Null));
        // What the session listing gives the worker: the lead's session.
        store
            .mutate(None, &id, None, |team, _| {
                let lead = team.members.iter_mut().find(|member| member.id == "lead").unwrap();
                lead.session =
                    Some(SessionRef { name: "claude/x/walk-lead".into(), run_id: "r0".into(), conversation_id: "conv-lead".into(), ..Default::default() });
                lead.state = MemberState::Active;
                Ok(Value::Null)
            })
            .unwrap();
        // 2. The lead drafts members, tasks and settings, the owner's among them.
        super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
        step("the lead reads the draft", run("draft", &json!({"team":id,"action":"get","by":"human"}), false));
        let mut settings = serde_json::to_value(&load(&id).settings).unwrap();
        settings["branch_per_task"] = json!({"template":"task/{id}"});
        settings["max_members"] = json!(5);
        settings["base_branch"] = json!(walk_head(&repo));
        let set = json!({"goal":"Walk the path","settings":settings,
            "members":[{"id":"dev","role":"Backend","agent":"codex","folder":"worktree"},{"id":"qa","role":"Tester","agent":"codex","folder":"worktree"}],
            "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"Tests","assignee":"qa"}]});
        let revision = load(&id).draft.unwrap().revision;
        let drafted = step(
            "the lead drafts",
            run(
                "draft",
                &json!({"team":id,"action":"update","revision":revision,"by":"human","set":set,"request_id":"13131313-1313-4131-8131-000000000002"}),
                false,
            ),
        );
        assert_eq!(drafted["setting_offers"]["max_members"], json!(5), "the lead's owner settings are offers: {drafted}");
        // 3. The owner applies the offers with one click and creates the team.
        super::set_test_caller_identities(Some((None, None, None)));
        let draft = load(&id);
        let mut owner = serde_json::to_value(&draft.settings).unwrap();
        for (field, value) in &draft.draft.as_ref().unwrap().setting_offers {
            owner[field] = value.clone();
        }
        step(
            "the owner applies the offers",
            run(
                "draft",
                &json!({"team":id,"action":"update","revision":draft.draft.as_ref().unwrap().revision,"by":"human","set":{"settings":owner},
                    "request_id":"13131313-1313-4131-8131-000000000003"}),
                false,
            ),
        );
        step(
            "Create team",
            run("draft", &json!({"team":id,"action":"create","approve":true,"by":"human","request_id":"13131313-1313-4131-8131-000000000004"}), false),
        );
        let team = load(&id);
        assert_eq!(team.status, TeamStatus::Running, "{:?}", refusals.borrow());
        assert!(team.settings.branch_per_task.is_some() && team.settings.max_members == 5, "the applied offers");
        // 4. The worker starts every member at once.
        let lead_seen = Seen { name: "claude/x/walk-lead".into(), run_id: "r0".into(), live: true, free: true, idle: true, ..Default::default() };
        let launches: Vec<Step> = worker::steps(&team, std::slice::from_ref(&lead_seen), crate::state::now())
            .into_iter()
            .filter(|step| matches!(step, Step::Launch { .. }))
            .collect();
        assert_eq!(launches.len(), 2, "both members start: {launches:?}");
        for launch in &launches {
            step(&format!("{launch:?}"), worker::test_run_step(&mut store, &load(&id), launch).map(|_| Value::Null));
        }
        assert_eq!(busy.get(), 0, "the busy worktree lock was waited for");
        let team = load(&id);
        assert!(team.member("dev").unwrap().error.is_empty(), "dev started: {}", team.member("dev").unwrap().error);
        assert!(team.member("qa").unwrap().failed_to_start(), "qa's start failed (the scripted failure)");
        // 5. The lead adds a task to the running phase, by itself.
        super::set_test_caller_identities(Some((Some("conv-lead".into()), None, None)));
        step(
            "the lead adds a task",
            run(
                "add-to-phase",
                &json!({"team":id,"phase":1,"tasks":[{"id":3,"title":"More API","assignee":"dev"}],"request_id":"13131313-1313-4131-8131-000000000005"}),
                false,
            ),
        );
        // 6. The lead relaunches the member that failed to start, by itself.
        step("the lead relaunches qa", run("relaunch", &json!({"team":id,"member":"qa","request_id":"13131313-1313-4131-8131-000000000006"}), false));
        let team = load(&id);
        let relaunch = worker::steps(&team, std::slice::from_ref(&lead_seen), crate::state::now())
            .into_iter()
            .find(|step| matches!(step, Step::Launch { member } if member == "qa"));
        match relaunch {
            Some(launch) => step("qa starts again", worker::test_run_step(&mut store, &team, &launch).map(|_| Value::Null)),
            None => step("qa starts again", Err(anyhow::anyhow!("no Launch step for qa after the relaunch"))),
        };
        super::set_test_caller_identities(None);
        worker::set_fake_hgs(None);
        assert!(refusals.borrow().is_empty(), "refusals on the happy path: {:#?}", refusals.borrow());
        let team = load(&id);
        assert!(team.task(3).is_some(), "the lead's task joined the phase");
        assert!(team.member("qa").unwrap().error.is_empty(), "qa started: {}", team.member("qa").unwrap().error);
    });
}

fn walk_head(repo: &std::path::Path) -> String {
    let output = std::process::Command::new("git").arg("-C").arg(repo).args(["symbolic-ref", "--short", "HEAD"]).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
