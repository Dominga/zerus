use super::*;
use super::support::*;

/// N14: the word is checked again inside the change, on the stored team. Two
/// requests sent at once both pass the CLI's pre-check on their own read;
/// the second must still refuse the word the first already spent, whatever
/// operation it is for.
#[test]
fn the_stored_team_refuses_a_word_another_request_spent() {
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Running,
        settings: Settings { max_members: 6, ..Default::default() },
        ..Default::default()
    };
    team.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    team.members.push(Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() });
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let word = |change: &str| {
        json!({"roster_word":true,"owner_word_event":7,"owner_word_change":change,
            "owner_word_checked":{"text":"убери dev из состава","at":1.0,"event":7}})
    };
    let mut remove = word("the roster change");
    remove["member"] = json!("dev");
    apply(&mut team, "remove-member", &remove, &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(team.owner_word_uses(7), 1);
    // The same sentence cannot pay for the cap change, even on a copy of the
    // team both requests loaded before either wrote.
    let mut settings = word("the roster change");
    settings["settings"] = json!({"max_members":7});
    let error = apply(&mut team, "settings", &settings, &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already used"), "{error}");
    assert_eq!(team.settings.max_members, 6, "the refused change moved nothing");
}

#[test]
fn the_lead_approves_only_on_the_owners_checked_word() {
    let (mut team, _) = create(
        "t",
        &json!({"name":"T","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    plan(
        &mut team,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],"tasks":[{"id":1,"title":"API","assignee":"dev"}]}),
        &Actor::Member("lead".into()),
        &mut events,
    )
    .unwrap();
    // F13: without the owner's sentence the lead cannot approve the plan.
    let refused = approve(&mut team, &json!({}), &Actor::Member("lead".into()), &none, &mut events).unwrap_err().to_string();
    assert_eq!(refused, "The lead approves only on the owner's word: pass \"owner_word\" with the owner's sentence");
    let word = json!({"text":"plan approved, go ahead","at":1000.0,"event":2});
    let approved = approve(
        &mut team,
        &json!({"owner_word_checked":word}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!((approved["status"].as_str(), approved["by"].as_str()), (Some("approved"), Some("Lead")));
    assert_eq!(team.approved_by, "Lead");
    assert!(team.approved_at.is_some());
    assert_eq!(team.owner_word.as_ref().unwrap().text, "plan approved, go ahead");
    let event = events.iter().rev().find(|event| event.kind == "approved").unwrap();
    assert_eq!(event.payload["by"], "Lead");
    assert_eq!(event.payload["owner_word"]["text"], "plan approved, go ahead");
    // A phase opens the same way: the owner's quote stays on the record.
    let mut team = running_team();
    extend(
        &mut team,
        &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap();
    for id in [1, 2] {
        apply(&mut team, "cancel", &json!({"task":id}), &Actor::Human, &none, &mut events).unwrap();
    }
    let refused = apply(
        &mut team,
        "approve-phase",
        &json!({"phase":2}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(refused, "The lead approves a phase only on the owner's word: pass \"owner_word\"");
    let opened = apply(
        &mut team,
        "approve-phase",
        &json!({"phase":2,"owner_word_checked":{"text":"next phase plan approved","at":1000.0,"event":7}}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(opened["status"], "phase_approved");
    let record = team.phases.iter().find(|phase| phase.n == 2).unwrap();
    assert_eq!((record.state, record.decided_by.as_str()), (PhaseState::Approved, "Lead"));
    assert_eq!(record.owner_word.as_ref().unwrap().text, "next phase plan approved");
    let event = events.iter().rev().find(|event| event.kind == "phase_approved").unwrap();
    assert_eq!(event.payload["owner_word"]["text"], "next phase plan approved");
    // The ordering rule (F12) still applies: a draft before the phase
    // refuses, before the word matters.
    let mut early = running_team();
    early.settings.approval = Approval::Review;
    for id in [3, 4] {
        extend(&mut early, &json!({"tasks":[{"id":id,"title":"Fix","assignee":"dev"}]}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    }
    let blocked = apply(
        &mut early,
        "approve-phase",
        &json!({"phase":3,"owner_word_checked":{"text":"next phase plan approved","at":1000.0,"event":7}}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(blocked, "Phase 3 opens after the previous phase is approved");
}

#[test]
fn the_start_timeout_is_bounded_and_defaults() {
    let lead = json!({"id":"lead","agent":"claude"});
    let (team, _) = create("t", &json!({"name":"T","lead":lead})).unwrap();
    assert_eq!(team.settings.start_timeout_seconds, 180);
    let (team, _) = create("t", &json!({"name":"T","lead":lead,"settings":{"start_timeout_seconds":60}})).unwrap();
    assert_eq!(team.settings.start_timeout_seconds, 60);
    for seconds in [0, 29, 3601] {
        let error = create("t", &json!({"name":"T","lead":lead,"settings":{"start_timeout_seconds":seconds}}))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "The start warning waits 30 to 3600 seconds after delivery", "{seconds}");
    }
    let (mut team, _) = create("t", &json!({"name":"T","lead":lead})).unwrap();
    let mut events = Vec::new();
    let error = plan(&mut team, &json!({"tasks":[],"settings":{"start_timeout_seconds":2}}), &Actor::Human, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "The start warning waits 30 to 3600 seconds after delivery");
    plan(&mut team, &json!({"tasks":[],"settings":{"start_timeout_seconds":3600}}), &Actor::Human, &mut events).unwrap();
    assert_eq!(team.settings.start_timeout_seconds, 3600);
    // The long-block warning defaults to 30 min and is bounded (N5).
    let (team, _) = create("t", &json!({"name":"T","lead":lead})).unwrap();
    assert_eq!(team.settings.blocked_timeout_seconds, 1800);
    for seconds in [59, 86_401] {
        let error = create("t", &json!({"name":"T","lead":lead,"settings":{"blocked_timeout_seconds":seconds}}))
            .unwrap_err()
            .to_string();
        assert_eq!(error, "The blocked warning waits 60 to 86400 seconds", "{seconds}");
    }
    let (team, _) = create("t", &json!({"name":"T","lead":lead,"settings":{"blocked_timeout_seconds":60}})).unwrap();
    assert_eq!(team.settings.blocked_timeout_seconds, 60);
}

#[test]
fn a_wider_profile_waits_for_the_owner() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    plan(
        &mut team,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","permissions_profile":"commit"}],
            "tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap();
    assert_eq!(team.profile_widening[0]["to"], "commit");
    let error = apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap_err().to_string();
    assert!(error.contains("confirm_widening"), "{error}");
    apply(&mut team, "approve", &json!({"confirm_widening":true}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert!(team.profile_widening.is_empty());
    // An unsupported profile is refused.
    let (mut fresh, _) = create("t2", &json!({"name":"T2","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let error = plan(
        &mut fresh,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","permissions_profile":"root"}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "A permissions profile is read, write, build, commit or network");
    // C3: `permissions` stays the launch mode; bypass is accepted.
    let (mut bypass, _) = create("t3", &json!({"name":"T3","lead":{"id":"lead","agent":"claude"}})).unwrap();
    plan(
        &mut bypass,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","permissions":"bypass"}],
            "tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap();
    assert_eq!(bypass.member("dev").unwrap().permissions, "bypass");
    let (mut fresh, _) = create("t4", &json!({"name":"T4","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let error = plan(
        &mut fresh,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex","permissions":"root"}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "A permissions mode is auto or bypass");
}

#[test]
fn a_member_cannot_forge_an_approval() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    plan(&mut team, &json!({"tasks":[{"id":1,"title":"A","assignee":"lead"}]}), &Actor::Human, &mut events).unwrap();
    // C1: a hand-written checked word is refused for anyone but the lead.
    let forged = json!({"owner_word_checked":{"text":"go ahead","at":1.0}});
    let error = apply(&mut team, "approve", &forged, &Actor::Member("dev".into()), &BTreeSet::new(), &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only the lead approves on the owner's word");
    // The owner's own approval needs no word.
    apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    // C1: the same guard for a phase approval.
    let mut running = running_team();
    extend(&mut running, &json!({"tasks":[{"id":3,"title":"Fix","assignee":"dev"}]}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    let error = apply(&mut running, "approve-phase", &forged, &Actor::Member("dev".into()), &BTreeSet::new(), &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only the lead approves on the owner's word");
}

#[test]
fn the_preflight_flags_a_sandboxed_worktree_and_a_forbidden_project() {
    let mut team = running_team();
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.agent = "dsh".into();
        dev.folder = MemberFolder::Worktree;
    }
    let validation = plan::validate(&team, &BTreeSet::new());
    assert!(validation.warnings.iter().any(|finding| finding.text.contains("sandboxed dsh agent in a worktree")), "{:?}", validation.warnings);
    team.settings.worktrees_forbidden = true;
    let validation = plan::validate(&team, &BTreeSet::new());
    assert!(validation.errors.iter().any(|finding| finding.text.contains("forbids worktrees")), "{:?}", validation.errors);
    team.settings.worktrees_forbidden = false;
    team.settings.folder_policy = FolderPolicy::Shared;
    let validation = plan::validate(&team, &BTreeSet::new());
    assert!(validation.errors.iter().any(|finding| finding.text.contains("shared folder policy")), "{:?}", validation.errors);
    // Auto allows a member's own folder.
    team.settings.folder_policy = FolderPolicy::Auto;
    let validation = plan::validate(&team, &BTreeSet::new());
    assert!(!validation.errors.iter().any(|finding| finding.text.contains("folder policy")), "{:?}", validation.errors);
}

#[test]
fn only_the_owner_sets_the_reminder_minutes() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    for field in ["remind_owner_minutes", "remind_owner_permission_minutes"] {
        let mut settings = serde_json::Map::new();
        settings.insert(field.to_owned(), json!(0));
        let error = apply(&mut team, "settings", &json!({"settings": Value::Object(settings)}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert!(error.contains("Only you change"), "{field}: {error}");
    }
    apply(&mut team, "settings", &json!({"settings":{"remind_owner_minutes":0}}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.settings.remind_owner_minutes, 0);
}

#[test]
fn a_subteam_cannot_turn_its_own_approvals_to_auto() {
    let mut team = Team {
        id: "child".into(), name: "Child".into(), lead: "dev".into(), status: TeamStatus::Running,
        parent: Some(Parent { team: "parent".into(), task: 1, lead: "lead".into(), member: "dev".into() }),
        ..Default::default()
    };
    team.members.push(Member { id: "dev".into(), role: "Lead".into(), agent: "codex".into(), ..Default::default() });
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let error = apply(&mut team, "settings", &json!({"settings":{"approval":"auto"}}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Only you change") || error.contains("never auto"), "{error}");
    // The same team's phase approval cannot be auto either.
    let error = apply(&mut team, "settings", &json!({"settings":{"phase_approval":"auto"}}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Only you change") || error.contains("never auto"), "{error}");
    // N13: the owner may choose auto for a sub-team.
    apply(&mut team, "settings", &json!({"settings":{"approval":"auto"}}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.settings.approval, Approval::Auto);
    // A sub-team already carrying auto can still change another setting.
    apply(&mut team, "settings", &json!({"settings":{"report_timeout_seconds":90}}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
    assert_eq!(team.settings.report_timeout_seconds, 90);
}

#[test]
fn settings_change_planning_members_and_keep_a_widening_for_the_owner() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    plan(
        &mut team,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"claude"}],"tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap();
    let value = apply(
        &mut team,
        "settings",
        &json!({"members_update":[{"id":"dev","role":"API","folder":"clone","permissions_profile":"commit"}],
            "settings":{"report_timeout_seconds":90}}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(value["status"], "updated");
    let dev = team.member("dev").unwrap();
    assert_eq!((dev.role.as_str(), dev.folder, dev.permissions_profile.as_str()), ("API", MemberFolder::Clone, "commit"));
    assert_eq!(team.settings.report_timeout_seconds, 90);
    assert_eq!(team.profile_widening[0]["to"], "commit");
    assert_eq!(events.last().unwrap().kind, "settings_changed");
    // A member with a session cannot change on a running team.
    let mut running = running_team();
    let error = apply(
        &mut running,
        "settings",
        &json!({"members_update":[{"id":"lead","role":"X"}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("without a session"), "{error}");
}

#[test]
fn only_the_owner_sets_a_members_context_before_a_task() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    plan(
        &mut team,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"claude"}],"tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap();
    let set = |team: &mut Team, actor: &Actor, value: &str| {
        let mut events = Vec::new();
        apply(team, "settings", &json!({"members_update":[{"id":"dev","context_before_task":value}]}), actor, &BTreeSet::new(), &mut events)
    };
    set(&mut team, &Actor::Human, "compact").unwrap();
    assert_eq!(team.member("dev").unwrap().context_before_task, "compact");
    // The lead never sets it (U46), and an unknown action is refused.
    let error = set(&mut team, &Actor::Member("lead".into()), "clear").unwrap_err().to_string();
    assert_eq!(error, "Only you set a member's context before a task");
    let error = set(&mut team, &Actor::Human, "sometimes").unwrap_err().to_string();
    assert_eq!(error, "A context before a task is none, compact or clear");
    // Empty inherits the role again; "none" overrides a role's action.
    set(&mut team, &Actor::Human, "").unwrap();
    assert_eq!(team.member("dev").unwrap().context_before_task, "");
    set(&mut team, &Actor::Human, "none").unwrap();
    assert_eq!(team.member("dev").unwrap().context_before_task, "none");
    // U46: a running member that already has a session takes this one field
    // (it only changes the next hand-out); other fields still need a member
    // without a session.
    let mut running = running_team();
    running.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
        Some(SessionRef { name: "codex/x/dev".into(), run_id: "r1".into(), ..Default::default() });
    let set_running = |team: &mut Team, update: Value| {
        let mut events = Vec::new();
        apply(team, "settings", &json!({"members_update":[update]}), &Actor::Human, &BTreeSet::new(), &mut events)
    };
    set_running(&mut running, json!({"id":"dev","context_before_task":"clear"})).unwrap();
    assert_eq!(running.member("dev").unwrap().context_before_task, "clear");
    // #581 L7: even on a running member only the owner sets the context.
    let mut events = Vec::new();
    let error = apply(
        &mut running,
        "settings",
        &json!({"members_update":[{"id":"dev","context_before_task":"compact"}]}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert_eq!(error, "Only you set a member's context before a task");
    assert_eq!(running.member("dev").unwrap().context_before_task, "clear");
    let error = set_running(&mut running, json!({"id":"dev","role":"API","context_before_task":"compact"})).unwrap_err().to_string();
    assert_eq!(error, "A running team changes only members without a session");
    let error = set_running(&mut running, json!({"id":"dev","role":"API"})).unwrap_err().to_string();
    assert_eq!(error, "A running team changes only members without a session");
}

/// #581 N6, spec 2: the owner edits a member's model list in the team
/// settings, a running member with a session included.
#[test]
fn the_owner_edits_a_running_members_models() {
    let mut team = running_team();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
        Some(SessionRef { name: "codex/x/dev".into(), run_id: "r1".into(), ..Default::default() });
    let set = |team: &mut Team, actor: &Actor, update: Value| {
        let mut events = Vec::new();
        apply(team, "settings", &json!({"members_update":[update]}), actor, &BTreeSet::new(), &mut events)
    };
    let models = json!([{"model":" gpt-5.5 ","effort":"high"},{"model":"opus"}]);
    let value = set(&mut team, &Actor::Human, json!({"id":"dev","models":models})).unwrap();
    assert_eq!(value["members"], json!(["dev"]), "{value}");
    let dev = team.member("dev").unwrap();
    assert_eq!(
        dev.models,
        vec![ModelChoice { model: "gpt-5.5".into(), effort: "high".into() }, ModelChoice { model: "opus".into(), effort: String::new() }]
    );
    // With the context action in the same update too.
    set(&mut team, &Actor::Human, json!({"id":"dev","models":[{"model":"opus"}],"context_before_task":"compact"})).unwrap();
    assert_eq!((team.member("dev").unwrap().models.len(), team.member("dev").unwrap().context_before_task.as_str()), (1, "compact"));
    // An empty list leaves only the launch model.
    set(&mut team, &Actor::Human, json!({"id":"dev","models":[]})).unwrap();
    assert!(team.member("dev").unwrap().models.is_empty());
    // The lead does not, a bad entry is refused, and other fields still
    // need a member without a session.
    let error = set(&mut team, &Actor::Member("lead".into()), json!({"id":"dev","models":[{"model":"opus"}]})).unwrap_err().to_string();
    assert_eq!(error, "Only you set a member's models");
    assert!(set(&mut team, &Actor::Human, json!({"id":"dev","models":[{"model":"two words"}]})).is_err());
    assert!(set(&mut team, &Actor::Human, json!({"id":"dev","models":"opus"})).is_err());
    let many: Vec<Value> = (0..21).map(|n| json!({"model":format!("m{n}")})).collect();
    assert!(set(&mut team, &Actor::Human, json!({"id":"dev","models":many})).unwrap_err().to_string().contains("at most"));
    let error = set(&mut team, &Actor::Human, json!({"id":"dev","models":[],"role":"API"})).unwrap_err().to_string();
    assert_eq!(error, "A running team changes only members without a session");
    assert!(team.member("dev").unwrap().models.is_empty());
}

#[test]
fn one_sentence_pays_for_a_member_and_then_its_tasks() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let word = |change: &str| {
        json!({"roster_word":true,"owner_word_event":9,"owner_word_change":change,
            "owner_word_checked":{"text":"add a Backend and its tasks","at":1.0,"event":9}})
    };
    let mut add = word("adding Backend");
    add["member"] = json!({"id":"dev2","role":"Backend","agent":"codex"});
    apply(&mut team, "add-member", &add, &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(team.owner_word_uses(9), 1);
    // Its tasks, on the same sentence: the pair's second half.
    let mut tasks = word("the request to add to a running phase");
    tasks["phase"] = json!(1);
    tasks["tasks"] = json!([{"id":3,"title":"Its task","assignee":"dev2"}]);
    let warned = apply(&mut team, "add-to-phase", &tasks, &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(warned["status"], "warning", "{warned}");
    assert_eq!(team.owner_word_uses(9), 2);
    // The sentence is spent: neither a second request nor another member add.
    let error = apply(&mut team, "add-to-phase", &tasks, &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already used"), "{error}");
    let mut again = word("adding Backend");
    again["member"] = json!({"id":"dev3","role":"Backend","agent":"codex"});
    let error = apply(&mut team, "add-member", &again, &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("already used"), "{error}");
}

#[test]
fn a_failed_change_does_not_spend_the_owners_word() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let word = |change: &str| {
        json!({"roster_word":true,"owner_word_event":11,"owner_word_change":change,
            "owner_word_checked":{"text":"add a Backend and its tasks","at":1.0,"event":11}})
    };
    // A failed change: a dependency on a task that does not exist, so nothing
    // is added and the word stays unspent.
    let mut bad = word("the request to add to a running phase");
    bad["phase"] = json!(1);
    bad["tasks"] = json!([{"id":9,"title":"Bad","assignee":"dev","deps":[{"task":42}]}]);
    let error = apply(&mut team, "add-to-phase", &bad, &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("neither in the plan nor among the new tasks"), "{error}");
    assert_eq!(team.owner_word_used(11), None, "the failed change spent the word");
    // The same word still pays for the valid change.
    let mut good = word("the request to add to a running phase");
    good["phase"] = json!(1);
    good["tasks"] = json!([{"id":3,"title":"Good","assignee":"dev"}]);
    let warned = apply(&mut team, "add-to-phase", &good, &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(warned["status"], "warning", "{warned}");
    assert_eq!(team.owner_word_uses(11), 1);
}

#[test]
fn the_lead_cannot_take_the_owners_settings() {
    // C4: approval, growth and unattended running stay the owner's; a
    // partial object keeps the fields it does not name.
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    team.settings.report_timeout_seconds = 77;
    for change in [json!({"phase_approval":"auto"}), json!({"approval":"auto"}), json!({"tree_sessions":99}), json!({"max_members":12})] {
        let error = apply(&mut team, "settings", &json!({"settings":change}), &Actor::Member("lead".into()), &none, &mut events)
            .unwrap_err()
            .to_string();
        assert!(error.starts_with("Only you change the team's"), "{error}");
    }
    apply(&mut team, "settings", &json!({"settings":{"remind_lead_minutes":9}}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!((team.settings.remind_lead_minutes, team.settings.report_timeout_seconds), (9, 77));
    apply(&mut team, "settings", &json!({"settings":{"phase_approval":"auto"}}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((team.settings.phase_approval, team.settings.report_timeout_seconds), (Some(Approval::Auto), 77));
    // A wider profile for a member a running team will launch is confirmed
    // by the owner at once.
    let widen = json!({"members_update":[{"id":"qa","permissions_profile":"network"}]});
    let qa = team.members.iter_mut().find(|member| member.id == "qa").unwrap();
    (qa.session, qa.permissions_profile) = (None, "read".into());
    // A refused change is not stored (the store drops the changed copy).
    let error = apply(&mut team.clone(), "settings", &widen, &Actor::Member("lead".into()), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("confirm_widening"), "{error}");
    let error = apply(&mut team.clone(), "settings", &widen, &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("confirm_widening"), "{error}");
    let mut confirmed = widen.clone();
    confirmed["confirm_widening"] = json!(true);
    apply(&mut team, "settings", &confirmed, &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.member("qa").unwrap().permissions_profile, "network");
    // Nor in Planning, where an auto plan would let the lead approve itself.
    let (mut planning, _) = create("p", &json!({"name":"P","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let error = apply(
        &mut planning,
        "plan",
        &json!({"tasks":[{"id":1,"title":"A","assignee":"lead"}],"settings":{"approval":"auto"}}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap_err()
    .to_string();
    assert!(error.starts_with("Only you change the team's approval"), "{error}");
    // Nor in a draft the owner creates; the lead may set the limits flag
    // (spec 6.1).
    let (mut draft, _) = create("d", &json!({"name":"D","lead":{"id":"lead","agent":"claude"}})).unwrap();
    draft.status = TeamStatus::Draft;
    draft.draft = Some(Draft { revision: 1, ..Default::default() });
    let mut auto = serde_json::to_value(&draft.settings).unwrap();
    auto["approval"] = json!("auto");
    let error = apply(&mut draft.clone(), "draft", &json!({"action":"update","revision":1,"set":{"settings":auto}}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("Only you change the team's approval"), "{error}");
    apply(&mut team, "settings", &json!({"settings":{"resume_after_limits":true}}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert!(team.settings.resume_after_limits);
}

/// U62: the test configuration's override is the owner's, an object the
/// project file merges under.
#[test]
fn only_the_owner_overrides_the_test_configuration() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let tests = json!({"full":{"policy":"standing"},"provider":[{"name":"tray","disabled":true}]});
    let error = apply(&mut team, "settings", &json!({"settings":{"tests":tests}}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("Only you change the team's tests"), "{error}");
    let error = apply(&mut team, "settings", &json!({"settings":{"tests":"all"}}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("The tests override is an object"), "{error}");
    apply(&mut team, "settings", &json!({"settings":{"tests":tests}}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.settings.tests.as_ref(), Some(&tests));
    // The lead still changes its own settings without touching it.
    apply(&mut team, "settings", &json!({"settings":{"remind_lead_minutes":9}}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(team.settings.tests.as_ref(), Some(&tests));
    let value = serde_json::to_value(&team.settings).unwrap();
    assert_eq!(value["tests"]["full"]["policy"], "standing");
}

/// U62 (#715): only the owner grants a full test run, one release at a time.
#[test]
fn only_the_owner_grants_a_full_test_run() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let error = apply(&mut team, "full-tests", &json!({"release":"v1.57.0"}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Only you"), "{error}");
    let receipt = apply(&mut team, "full-tests", &json!({"release":"v1.57.0"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["release"].as_str()), (Some("granted"), Some("v1.57.0")));
    assert_eq!(team.full_tests_grant.as_ref().unwrap().release, "v1.57.0");
    assert!(events.iter().any(|event| event.kind == "full_tests_granted"));
    let revoked = apply(&mut team, "full-tests", &json!({"revoke":true}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(revoked["status"], "revoked");
    assert!(team.full_tests_grant.is_none());
}
