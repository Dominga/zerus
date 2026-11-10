use super::*;
use super::support::*;

/// N5: a finished team's inbox still carries the owner's decision on the
/// lead's proposal (spec 4), so the lead hears the answer.
#[test]
fn a_done_team_reports_the_owners_proposal_decision_in_its_inbox() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.status = TeamStatus::Done;
        team.proposals.push(json!({"id":"p1","kind":"next-goal","by":"Lead","at":10.0,"state":"accepted",
            "decided_at":20.0,"note":"Go on","goal":"v2"}));
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let value = run("inbox", &json!({"team":id,"by":"human"}), false).unwrap();
        let items = value["items"].as_array().unwrap();
        let item = items.iter().find(|item| item["kind"] == "proposal_decided").expect("the decision is missing");
        assert!(
            item["summary"].as_str().unwrap_or_default().contains("accepted your next-goal proposal p1: Go on"),
            "{item}"
        );
        // A running team with the same proposal lists nothing stale twice.
        let mut running = running_team();
        running.proposals.push(json!({"id":"p2","kind":"next-goal","by":"Lead","at":10.0,"state":"rejected","decided_at":20.0,"note":"No"}));
        assert_eq!(super::super::inbox::items(&running).iter().filter(|item| item.kind == "proposal_decided").count(), 1);
    });
}

/// N3: a finished team starts one next goal, however many requests ask for
/// it; a replay of the first answers its receipt.
#[test]
fn a_finished_team_starts_only_one_next_goal() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        super::live_sessions_override(Some(Vec::new()));
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.status = TeamStatus::Done;
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let first = run("next-goal", &json!({"team":id,"goal":"v2","by":"human","request_id":"22222222-2222-4222-8222-222222222222"}), false).unwrap();
        assert_eq!(first["status"], "planned", "{first}");
        let error = run("next-goal", &json!({"team":id,"goal":"v3","by":"human","request_id":"33333333-3333-4333-8333-333333333333"}), false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("already started"), "{error}");
        let again = run("next-goal", &json!({"team":id,"goal":"v2","by":"human","request_id":"22222222-2222-4222-8222-222222222222"}), false).unwrap();
        assert_eq!(again["team"], first["team"]);
    });
    super::live_sessions_override(None);
}

/// N3: an accepted proposal's change runs under the proposal's own request
/// id, so a decision interrupted between its writes replays instead of
/// running the change twice.
#[test]
fn an_accepted_proposal_replays_instead_of_running_twice() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.members.push(Member { id: "ops".into(), role: "Ops".into(), agent: "codex".into(), ..Default::default() });
        team.proposals.push(json!({"id":"p1","kind":"remove-member","by":"Lead","at":10.0,"state":"open","reason":"bye",
            "member":"ops","operation_request":"44444444-4444-4444-8444-444444444444"}));
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let decide = |store: &mut Store| {
            super::decide_proposal_in(
                store,
                &json!({"request_id":"55555555-5555-4555-8555-555555555555","team":id,"proposal":"p1","accept":true,"reason":"bye"}),
                &Actor::Human,
            )
            .unwrap()
        };
        let first = decide(&mut store);
        assert_eq!(first["status"], "accepted", "{first}");
        let removals = |store: &Store| {
            let events = store.events(&id, None, 50).unwrap();
            events["items"].as_array().unwrap().iter().filter(|event| event["kind"] == "member_removed").count()
        };
        assert_eq!(removals(&store), 1);
        // The change went through but the acceptance was lost (a crash
        // between the two writes): the proposal is open again.
        store
            .mutate(None, &id, None, |team, _| {
                if let Some(proposal) = team.proposals.iter_mut().find(|proposal| proposal["id"] == "p1") {
                    proposal["state"] = json!("open");
                }
                Ok(Value::Null)
            })
            .unwrap();
        let again = decide(&mut store);
        assert_eq!(again["status"], "accepted", "{again}");
        assert_eq!(removals(&store), 1, "the change ran twice");
    });
}

/// N4: an accepted add-member proposal runs the same adoption checks as the
/// owner's own change (C8's owner_change), not a bare ops::apply.
#[test]
fn an_accepted_add_member_runs_the_adoption_checks() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        super::live_sessions_override(Some(Vec::new()));
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.proposals.push(json!({"id":"p1","kind":"add-member","by":"Lead","at":10.0,"state":"open","reason":"needs help",
            "member":{"id":"ops","role":"Ops","agent":"codex","session":{"name":"codex/x/ops","host":""}},
            "operation_request":"44444444-4444-4444-8444-444444444444"}));
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let error = super::decide_proposal_in(
            &mut store,
            &json!({"request_id":"55555555-5555-4555-8555-555555555555","team":id,"proposal":"p1","accept":true,"reason":"ok"}),
            &Actor::Human,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("is not running on this computer"), "{error}");
        let saved = store.load(&id).unwrap().unwrap();
        assert_eq!(saved.proposals[0]["state"], "open", "the proposal was accepted without the check");
        assert!(saved.member("ops").is_none());
    });
    super::live_sessions_override(None);
}

#[test]
fn a_done_team_starts_the_next_goal_with_the_same_sessions() {
    use crate::state::teams::model::{Member, SessionRef, Team, TeamStatus};
    use crate::state::teams::store::Store;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.db"), true).unwrap();
    let old_id = "33333333-3333-4333-8333-333333333333";
    let mut done = Team {
        id: old_id.into(),
        name: "T".into(),
        lead: "lead".into(),
        status: TeamStatus::Done,
        folder: dir.path().display().to_string(),
        goal: "one".into(),
        ..Default::default()
    };
    done.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    done.members.push(Member {
        id: "dev".into(),
        role: "Backend".into(),
        agent: "claude".into(),
        session: Some(SessionRef { name: "claude/x/dev".into(), ..Default::default() }),
        ..Default::default()
    });
    store.create("44444444-4444-4444-8444-444444444444", &json!({"id":old_id}), done, Vec::new()).unwrap();
    // N8: the live sessions are read once; this test's listing has dev's.
    super::live_sessions_override(Some(vec![crate::state::teams::dispatcher::Seen {
        name: "claude/x/dev".into(),
        run_id: "r9".into(),
        conversation_id: "c9".into(),
        live: true,
        ..Default::default()
    }]));
    let value = super::next_goal_as(
        &mut store,
        "55555555-5555-4555-8555-555555555555",
        &json!({"request_id":"55555555-5555-4555-8555-555555555555","team":old_id,"goal":"two"}),
        &Actor::Human,
    )
    .unwrap();
    super::live_sessions_override(None);
    let next_id = value["team"].as_str().unwrap().to_owned();
    let next = store.load(&next_id).unwrap().unwrap();
    assert_eq!(next.status, TeamStatus::Planning);
    assert_eq!((next.previous.as_str(), next.goal.as_str()), (old_id, "two"));
    let dev = next.member("dev").unwrap();
    assert_eq!(dev.session.as_ref().map(|session| (session.name.as_str(), session.run_id.as_str())), Some(("claude/x/dev", "r9")));
    assert_eq!(dev.state, crate::state::teams::model::MemberState::Active);
    assert_eq!(store.load(old_id).unwrap().unwrap().next, next_id);
}

#[test]
fn a_lead_starts_the_next_goal_only_after_an_accepted_proposal() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let mut done = Team { id: "done".into(), name: "Done".into(), lead: "lead".into(), status: TeamStatus::Done, goal: "one".into(), ..Default::default() };
    done.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() });
    store.create("66666666-6666-4666-8666-666666666666", &json!({}), done.clone(), Vec::new()).unwrap();
    // N1: the lead alone cannot start the next goal.
    let error = super::next_goal_as(
        &mut store,
        "77777777-7777-4777-8777-777777777777",
        &json!({"request_id":"77777777-7777-4777-8777-777777777777","team":"done","goal":"two"}),
        &Actor::Member("lead".into()),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("accepted next-goal proposal"), "{error}");
    // With the owner's accepted proposal it can.
    let mut with_proposal = done.clone();
    with_proposal.id = "done2".into();
    with_proposal.proposals.push(json!({"id":"p1","kind":"next-goal","state":"accepted","goal":"two"}));
    store.create("88888888-8888-4888-8888-888888888888", &json!({}), with_proposal, Vec::new()).unwrap();
    let value = super::next_goal_as(
        &mut store,
        "99999999-9999-4999-8999-999999999999",
        &json!({"request_id":"99999999-9999-4999-8999-999999999999","team":"done2","goal":"other"}),
        &Actor::Member("lead".into()),
    )
    .unwrap();
    assert_eq!(value["status"], "planned");
    // N12: the accepted proposal's goal is used, and the done team cannot
    // start a second next goal.
    let next = store.load(value["team"].as_str().unwrap()).unwrap().unwrap();
    assert_eq!(next.goal, "two");
    let error = super::next_goal_as(
        &mut store,
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        &json!({"request_id":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa","team":"done2","goal":"three"}),
        &Actor::Member("lead".into()),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("already started"), "{error}");
}

/// U57: the polled list carries the open proposals with their change and
/// hash for the lead's cards, never a decided one or the replay request id.
#[test]
fn the_team_list_carries_the_open_proposals_for_their_cards() {
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    // #740: a proposal is checked when it is made: #2 and phase 2 exist.
    ops::apply(&mut team, "extend", &json!({"phase_title":"Next","tasks":[{"id":2,"title":"Later","assignee":"dev"}]}), &lead,
               &BTreeSet::new(), &mut Vec::new())
        .unwrap();
    for input in [
        json!({"kind":"cancel-tasks","reason":"drop","tasks":[2]}),
        json!({"kind":"discard-phase","reason":"nothing started","phase":2}),
    ] {
        ops::apply(&mut team, "propose", &input, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
    }
    team.proposals[1]["state"] = json!("rejected");
    // Task #6 (team ede724c4): the polled list counts the open proposals; their
    // cards come from status.
    let listed = summary(&team, false);
    assert_eq!(listed["open_proposals"], 1, "{listed}");
    assert!(listed.get("proposals").is_none(), "{listed}");
    assert_eq!(team.proposals[0]["state"], "open");
    assert_eq!(team.proposals[0]["change"]["tasks"], json!([2]));
}

/// U57: Allow applies exactly the proposed change once, bound to the id and
/// hash; a changed card is refused; Deny records the reason for the lead.
#[test]
fn an_allow_applies_the_proposed_change_once_and_a_changed_card_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = running_team();
    ops::apply(
        &mut team,
        "propose",
        &json!({"kind":"add-member","reason":"needs help",
            "members":[{"id":"x","role":"Ops","agent":"claude"},{"id":"y","role":"Ops","agent":"claude"}]}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut Vec::new(),
    )
    .unwrap();
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    let error = decide_proposal_in(
        &mut store,
        &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":"bad"}),
        &Actor::Human,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("changed"), "{error}");
    let receipt = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}), &Actor::Human).unwrap();
    assert_eq!(receipt["status"], "accepted");
    let team = store.load("t").unwrap().unwrap();
    assert!(team.member("x").is_some() && team.member("y").is_some());
    // A replay returns the stored state; no member is added twice.
    let replay = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}), &Actor::Human).unwrap();
    assert_eq!(replay["status"], "accepted");
    assert_eq!(store.load("t").unwrap().unwrap().members.iter().filter(|member| member.id == "x").count(), 1);
    // Deny records its reason for the lead and runs nothing.
    store
        .mutate(None, "t", None, |team, events| {
            // #740: a proposal names work that exists (it is checked when made).
            ops::apply(team, "propose", &json!({"kind":"cancel-tasks","reason":"drop","tasks":[1]}), &Actor::Member("lead".into()), &BTreeSet::new(), events)
        })
        .unwrap();
    let team = store.load("t").unwrap().unwrap();
    let hash = team.proposals[1]["hash"].as_str().unwrap().to_owned();
    let before = team.task(1).unwrap().status;
    let denied = decide_proposal_in(
        &mut store,
        &json!({"team":"t","proposal":"p2","accept":false,"reason":"not now","expected_hash":hash}),
        &Actor::Human,
    )
    .unwrap();
    assert_eq!(denied["status"], "rejected");
    let team = store.load("t").unwrap().unwrap();
    assert_eq!(team.proposals[1]["note"], "not now");
    assert_eq!(team.task(1).unwrap().status, before, "a Deny runs nothing");
}

/// U57: Allow on an add-to-phase card for the running phase adds the tasks
/// at once: the Allow is the confirmation the CLI route asks for after its
/// warning (owner, 2026-10-30: the card was accepted and nothing was added).
#[test]
fn an_allowed_addition_to_the_running_phase_adds_its_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = running_team();
    let input = json!({"kind":"add-to-phase","reason":"a crash to hunt","phase":1,
        "tasks":[{"id":2,"title":"Find the crash","assignee":"dev","criteria":["Fixed"]}]});
    ops::apply(&mut team, "propose", &input, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    let receipt = decide_proposal_in(
        &mut store,
        &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}),
        &Actor::Human,
    )
    .unwrap();
    assert_eq!(receipt["status"], "accepted", "{receipt}");
    let team = store.load("t").unwrap().unwrap();
    let added = team.task(2).expect("the allowed task is in the plan");
    assert_eq!((added.phase, added.title.as_str()), (1, "Find the crash"));
}

/// U29: an Allow whose confirmation fails after its warning was stored
/// leaves no addition waiting that nobody can settle; the error is the
/// confirmation's.
#[test]
fn a_failed_allow_confirmation_leaves_no_addition_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), running_team(), vec![]).unwrap();
    let warned = store
        .mutate(None, "t", None, |team, events| {
            let input = json!({"phase":1,"tasks":[{"id":2,"title":"Find the crash","assignee":"dev"}]});
            ops::apply(team, "add-to-phase", &input, &Actor::Human, &BTreeSet::new(), events)
        })
        .unwrap();
    let pending = warned["pending"].as_str().unwrap().to_owned();
    assert_eq!(store.load("t").unwrap().unwrap().pending_additions.len(), 1);
    let confirm = json!({"team":"t","request_id":"22222222-2222-4222-8222-222222222222","by":"human","phase":1,
        "confirm":pending,"confirmation":"sure"});
    let error = super::confirm_allowed_addition(&mut store, "t", &confirm).unwrap_err().to_string();
    assert!(error.contains("Type \"add\""), "{error}");
    let team = store.load("t").unwrap().unwrap();
    assert!(team.pending_additions.is_empty(), "{:?}", team.pending_additions);
    assert!(team.task(2).is_none());
    assert!(store.events("t", None, 50).unwrap().to_string().contains("phase_addition_declined"));
}

/// U59: the desktop's switch dialog decides through decide-proposal; its
/// Allow switches the plan mode once, and a Deny keeps it and tells the lead.
#[test]
fn the_switch_dialog_switches_the_plan_mode_through_decide_proposal() {
    use super::super::model::PlanMode;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    for reason in ["short requests", "a stream"] {
        let input = json!({"kind":"switch-plan-mode","mode":"free","reason":reason});
        ops::apply(&mut team, "propose", &input, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
    }
    let first = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    let second = team.proposals[1]["hash"].as_str().unwrap().to_owned();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    // A guessed field never decides: no "accept" is an error, not a rejection.
    let error = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","decision":"allow","hash":first}), &Actor::Human)
        .unwrap_err()
        .to_string();
    assert!(error.contains("accept") && error.contains("nothing was decided"), "{error}");
    assert_eq!(store.load("t").unwrap().unwrap().proposals[0]["state"], "open");
    let denied = decide_proposal_in(
        &mut store,
        &json!({"team":"t","proposal":"p1","accept":false,"reason":"not yet","expected_hash":first}),
        &Actor::Human,
    )
    .unwrap();
    assert_eq!(denied["status"], "rejected");
    assert_eq!(store.load("t").unwrap().unwrap().plan_mode, PlanMode::Standard);
    let feed = store.events("t", None, 50).unwrap().to_string();
    assert!(feed.contains("plan_mode_denied") && feed.contains("not yet"), "{feed}");
    let receipt = decide_proposal_in(
        &mut store,
        &json!({"team":"t","proposal":"p2","accept":true,"by":"human","hash":second}),
        &Actor::Human,
    )
    .unwrap();
    assert_eq!(receipt["status"], "accepted", "{receipt}");
    assert_eq!(receipt["result"]["plan_mode"], "free");
    assert_eq!(store.load("t").unwrap().unwrap().plan_mode, PlanMode::Free);
    assert!(store.events("t", None, 50).unwrap().to_string().contains("plan_mode_switched"));
}

/// Task #58: a switch card naming a phase switches that phase only, through
/// the same decide-proposal; the owner's direct switch is one guarded op.
#[test]
fn a_phase_switch_card_and_the_owners_op_switch_one_phase() {
    use super::super::model::PlanMode;
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    let input = json!({"kind":"switch-plan-mode","mode":"free","phase":1,"reason":"support requests"});
    ops::apply(&mut team, "propose", &input, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    let receipt = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"expected_hash":hash}), &Actor::Human).unwrap();
    assert_eq!(receipt["status"], "accepted", "{receipt}");
    assert_eq!((receipt["result"]["phase"].as_u64(), receipt["result"]["mode"].as_str()), (Some(1), Some("free")));
    let stored = store.load("t").unwrap().unwrap();
    assert_eq!((stored.plan_mode, stored.phase_mode(1)), (PlanMode::Standard, PlanMode::Free));
    assert!(store.events("t", None, 50).unwrap().to_string().contains("phase_mode_switched"));
    // The owner locks it back with one guarded op; the same request id
    // answers its receipt again and never switches twice.
    let request = json!({"team":"t","request_id":"22222222-2222-4222-8222-222222222222","phase":1,"mode":"standard"});
    let first = store.mutate(Some(("22222222-2222-4222-8222-222222222222", &request)), "t", None, |team, events| {
        ops::apply(team, "phase-mode", &request, &Actor::Human, &BTreeSet::new(), events)
    })
    .unwrap();
    let again = store.mutate(Some(("22222222-2222-4222-8222-222222222222", &request)), "t", None, |team, events| {
        ops::apply(team, "phase-mode", &request, &Actor::Human, &BTreeSet::new(), events)
    })
    .unwrap();
    assert_eq!(first["mode"], "standard", "{first}");
    assert_eq!(again["mode"], first["mode"], "{again}");
    assert_eq!(store.load("t").unwrap().unwrap().phase_mode(1), PlanMode::Standard);
}

#[test]
fn an_accepted_swap_proposal_runs_with_its_fields() {
    // C8: the spec's shape (member id plus section 5 fields) is kept by
    // propose and run by the owner's decision without a panic.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = running_team();
    let proposal = json!({"kind":"swap-member","member":"dev","agent":"claude","model":"opus","reason":"Stuck three times"});
    ops::apply(&mut team, "propose", &proposal, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert_eq!(team.proposals[0]["member"], json!({"id":"dev","agent":"claude","model":"opus"}));
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    let decide = json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash});
    let error = decide_proposal_in(&mut store, &decide, &Actor::Member("lead".into())).unwrap_err().to_string();
    assert!(error.contains("Only you decide"), "{error}");
    let receipt = decide_proposal_in(&mut store, &decide, &Actor::Human).unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["kind"].as_str()), (Some("accepted"), Some("swap-member")));
    let team = store.load("t").unwrap().unwrap();
    let dev = team.member("dev").unwrap();
    assert_eq!((dev.agent.as_str(), dev.model.as_str(), dev.session.is_none()), ("claude", "opus", true));
    assert_eq!(team.proposals[0]["state"], "accepted");
    assert!(team.proposals[0]["decided_at"].as_f64().is_some());
    // An old proposal stored with a bare member id runs as well.
    assert_eq!(proposal_input("t", "swap-member", &json!("dev"), "11111111-1111-4111-8111-111111111111").unwrap()["member"], "dev");
}

#[test]
fn an_accepted_next_goal_runs_as_the_owner() {
    // N2: the decision does not re-derive its caller from the input.
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = running_team();
    ops::apply(&mut team, "propose", &json!({"kind":"next-goal","goal":"Ship v2","reason":"Done here"}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    team.status = TeamStatus::Done;
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    let receipt = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}), &Actor::Human).unwrap();
    let next = receipt["result"]["team"].as_str().unwrap().to_owned();
    assert_eq!(store.load(&next).unwrap().unwrap().goal, "Ship v2");
    assert_eq!(store.load("t").unwrap().unwrap().next, next);
}

/// #754 (owner, 2026-10-31: "I press Allow and nothing happens"): p11 named
/// #747 and #748, already taken, and Allow failed every time. Added tasks take
/// the team's next free numbers when Allow applies them, their dependencies
/// with them; an earlier Allow that stopped after its warning never leaves a
/// stale addition behind; a proposal Allow could never apply (a number given
/// twice, a dependency on an unknown task) is refused when the lead makes it,
/// and a dependency named twice is kept once.
#[test]
fn allowed_tasks_take_the_next_free_numbers_and_a_retry_never_meets_a_stale_warning() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    // #1 is taken: the proposal names it, and #7 depends on it (twice).
    let input = json!({"kind":"add-to-phase","reason":"two follow-ups","phase":1,
        "tasks":[{"id":1,"title":"First","assignee":"dev"},{"id":7,"title":"Second","assignee":"dev","deps":[{"task":1},{"task":1}]}]});
    ops::apply(&mut team, "propose", &input, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap();
    for (tasks, words) in [
        (json!([{"id":5,"title":"A","assignee":"dev"},{"id":5,"title":"B","assignee":"dev"}]), "Task #5 is given twice among the new tasks"),
        (json!([{"id":5,"title":"A","assignee":"dev","deps":[{"task":42}]}]), "Task #5 depends on #42, which is neither in the plan nor among the new tasks"),
    ] {
        let bad = json!({"kind":"add-to-phase","reason":"bad","phase":1,"tasks":tasks});
        let error = format!("{:#}", ops::apply(&mut team, "propose", &bad, &lead, &BTreeSet::new(), &mut Vec::new()).unwrap_err());
        assert!(error.contains("Allow could not apply") && error.contains(words), "{error}");
    }
    assert_eq!(team.proposals.len(), 1, "only the good proposal is made");
    let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
    let operation = team.proposals[0]["operation_request"].as_str().unwrap().to_owned();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
    // An earlier Allow's warning, stored under the proposal's request id with
    // an addition that no longer exists (it expired).
    let stale = json!({"team":"t","request_id":operation,"by":"human","phase":1,"tasks":input["tasks"]});
    store
        .mutate(Some((&operation, &stale)), "t", None, |_, _| Ok(json!({"status":"warning","pending":"gone-long-ago"})))
        .unwrap();
    let receipt = decide_proposal_in(
        &mut store,
        &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}),
        &Actor::Human,
    )
    .unwrap();
    assert_eq!(receipt["status"], "accepted", "{receipt}");
    let team = store.load("t").unwrap().unwrap();
    let first = team.task(2).expect("#1 joined as #2");
    let second = team.task(3).expect("#7 joined as #3");
    assert_eq!((first.title.as_str(), first.phase), ("First", 1));
    assert_eq!(second.title, "Second");
    assert_eq!(second.deps.iter().map(|dep| dep.task).collect::<Vec<_>>(), vec![2], "the dependency moved with its task");
    assert_eq!(team.task(1).unwrap().title, "API", "the taken task is untouched");
    // The decision is not applied twice when asked again.
    let again = decide_proposal_in(
        &mut store,
        &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}),
        &Actor::Human,
    )
    .unwrap();
    assert_eq!(again["status"], "accepted", "{again}");
    assert_eq!(store.load("t").unwrap().unwrap().tasks.len(), 3);
}

/// #754: the lead's extend takes the next free numbers too, with the
/// dependencies on its own tasks.
#[test]
fn an_extend_with_taken_numbers_takes_the_next_free_ones() {
    let mut team = running_team();
    let receipt = ops::apply(
        &mut team,
        "extend",
        &json!({"tasks":[{"id":1,"title":"Docs","assignee":"dev"},{"id":9,"title":"Review the docs","assignee":"dev","deps":[{"task":1}]}]}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(receipt["renumbered"], json!({"1":2,"9":3}), "{receipt}");
    assert_eq!(team.task(2).unwrap().title, "Docs");
    assert_eq!(team.task(3).unwrap().deps.iter().map(|dep| dep.task).collect::<Vec<_>>(), vec![2]);
}

/// Task #21 (owner, 2026-11-05): every owner-gated operation is a card. The
/// lead proposes it, the card names the change, the owner's Allow runs it
/// once as the owner's own request (a replay applies nothing twice) and a
/// Reject changes nothing; the audit names the operation and its request.
#[test]
fn owner_operation_cards_run_once_on_allow_and_never_on_reject() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        super::live_sessions_override(Some(Vec::new()));
        super::set_test_caller_identities(Some((None, None, None)));
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        // A member whose session failed to start: the one a relaunch is for.
        team.members.push(Member { id: "ops".into(), role: "Ops".into(), agent: "claude".into(), error: "launch failed".into(), ..Default::default() });
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let propose = |store: &mut Store, proposal: Value| -> anyhow::Result<(String, String)> {
            let receipt = store.mutate(None, "t", None, |team, events| ops::apply(team, "propose", &proposal, &Actor::Member("lead".into()), &BTreeSet::new(), events))?;
            Ok((receipt["proposal"].as_str().unwrap().to_owned(), receipt["hash"].as_str().unwrap().to_owned()))
        };
        let allow = |store: &mut Store, id: &str, hash: &str, accept: bool| {
            decide_proposal_in(
                store,
                &json!({"team":"t","proposal":id,"accept":accept,"by":"human","expected_hash":hash,"request_id":"66666666-6666-4666-8666-666666666666"}),
                &Actor::Human,
            )
        };
        // Relaunch: the card names the member; Allow clears its start error.
        let (id, hash) = propose(&mut store, json!({"kind":"relaunch","member":"ops","reason":"it failed to start"})).unwrap();
        let card = store.load("t").unwrap().unwrap().proposals.iter().find(|p| p["id"] == id.as_str()).unwrap().clone();
        assert_eq!(card["change"]["op"], "relaunch", "{card}");
        assert_eq!(card["change"]["summary"], "Relaunch ops (Ops)", "{card}");
        let receipt = allow(&mut store, &id, &hash, true).unwrap();
        assert_eq!(receipt["status"], "accepted", "{receipt}");
        let team = store.load("t").unwrap().unwrap();
        assert!(team.member("ops").unwrap().error.is_empty());
        // A replayed Allow answers the stored state and relaunches nothing again.
        assert_eq!(allow(&mut store, &id, &hash, true).unwrap()["status"], "accepted");
        let events = store.events("t", None, 200).unwrap();
        let items = events["items"].as_array().unwrap();
        assert_eq!(items.iter().filter(|event| event["kind"] == "relaunch").count(), 1, "{events}");
        let decided = items.iter().find(|event| event["kind"] == "proposal_decided" && event["detail"]["proposal"] == id.as_str()).expect("no decision");
        assert_eq!((decided["detail"]["op"].as_str(), decided["detail"]["summary"].as_str()), (Some("relaunch"), Some("Relaunch ops (Ops)")), "{decided}");
        assert!(decided["detail"]["request"].as_str().is_some_and(|request| !request.is_empty()), "{decided}");
        // Settings: an owner-only limit, as a settings card and as a switch card.
        let (id, hash) = propose(&mut store, json!({"kind":"settings","settings":{"max_members":9},"reason":"more hands"})).unwrap();
        let card = store.load("t").unwrap().unwrap().proposals.iter().find(|p| p["id"] == id.as_str()).unwrap().clone();
        assert!(card["change"]["lines"][0].as_str().unwrap_or_default().contains("max_members: 6 → 9"), "{card}");
        allow(&mut store, &id, &hash, true).unwrap();
        assert_eq!(store.load("t").unwrap().unwrap().settings.max_members, 9);
        let (id, hash) = propose(&mut store, json!({"kind":"switch-setting","setting":"base_branch","value":"main","reason":"land on main"})).unwrap();
        allow(&mut store, &id, &hash, true).unwrap();
        assert_eq!(store.load("t").unwrap().unwrap().settings.base_branch, "main");
        // Scale and drain run as the owner's own.
        let (id, hash) = propose(&mut store, json!({"kind":"scale","role":"Tester","agent":"claude","count":2,"reason":"tests lag"})).unwrap();
        allow(&mut store, &id, &hash, true).unwrap();
        let team = store.load("t").unwrap().unwrap();
        assert_eq!(team.members.iter().filter(|member| member.role == "Tester").count(), 2);
        let (id, hash) = propose(&mut store, json!({"kind":"drain","member":"dev","reason":"done soon"})).unwrap();
        allow(&mut store, &id, &hash, true).unwrap();
        assert!(store.load("t").unwrap().unwrap().member("dev").unwrap().draining);
        // Reject changes nothing and the card stays decided.
        let before = store.load("t").unwrap().unwrap().settings.max_members;
        let (id, hash) = propose(&mut store, json!({"kind":"settings","settings":{"max_members":3},"reason":"smaller"})).unwrap();
        assert_eq!(allow(&mut store, &id, &hash, false).unwrap()["status"], "rejected");
        let team = store.load("t").unwrap().unwrap();
        assert_eq!(team.settings.max_members, before);
        assert_eq!(team.proposals.iter().find(|p| p["id"] == id.as_str()).unwrap()["state"], "rejected");
        // A card Allow could not apply is refused when the lead proposes it.
        let error = propose(&mut store, json!({"kind":"relaunch","member":"nobody","reason":"x"})).unwrap_err().to_string();
        assert!(error.contains("Allow could not apply"), "{error}");
        let error = propose(&mut store, json!({"kind":"settings","reason":"x"})).unwrap_err().to_string();
        assert!(error.contains("settings proposal names"), "{error}");
    });
    super::live_sessions_override(None);
    super::set_test_caller_identities(None);
}

/// Task #21: the plan approval is a card too; Allow runs the planned team.
#[test]
fn a_plan_approval_card_runs_the_team() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        super::live_sessions_override(Some(Vec::new()));
        super::set_test_caller_identities(Some((None, None, None)));
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let (mut team, _) = ops::create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}})).unwrap();
        ops::apply(&mut team, "plan", &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],"tasks":[{"id":1,"title":"API","assignee":"dev"}]}),
            &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
        assert_eq!(team.status, TeamStatus::Planning);
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let receipt = store
            .mutate(None, "t", None, |team, events| ops::apply(team, "propose", &json!({"kind":"approve","reason":"the plan is ready"}), &Actor::Member("lead".into()), &BTreeSet::new(), events))
            .unwrap();
        let (id, hash) = (receipt["proposal"].as_str().unwrap().to_owned(), receipt["hash"].as_str().unwrap().to_owned());
        assert_eq!(store.load("t").unwrap().unwrap().status, TeamStatus::Planning, "proposing changes nothing");
        let receipt = decide_proposal_in(&mut store, &json!({"team":"t","proposal":id,"accept":true,"by":"human","expected_hash":hash}), &Actor::Human).unwrap();
        assert_eq!(receipt["status"], "accepted", "{receipt}");
        assert_eq!(store.load("t").unwrap().unwrap().status, TeamStatus::Running);
    });
    super::live_sessions_override(None);
    super::set_test_caller_identities(None);
}

/// Task 111 (owner, 2026-11-06, rejecting p47: "a notification must not reach
/// me if it physically cannot be carried out or stopped being possible"): an
/// add-to-phase proposal waits for the owner while its phase runs; the change
/// that closes the phase withdraws it with the reason, so it leaves Waiting for
/// you (and its notification), and the lead's inbox says why, once.
#[test]
fn a_pending_addition_whose_phase_closes_is_withdrawn_and_the_lead_told_once() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        let input = json!({"kind":"add-to-phase","reason":"a crash to hunt","phase":1,
            "tasks":[{"id":2,"title":"Find the crash","assignee":"dev","criteria":["Fixed"]}]});
        ops::apply(&mut team, "propose", &input, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let waiting = || run("waiting", &json!({}), false).unwrap()["items"].as_array().unwrap().iter()
            .filter_map(|item| item["id"].as_str().map(str::to_owned)).collect::<Vec<_>>();
        assert!(waiting().contains(&"t/owner_word_asked/proposal-p1".to_owned()), "{:?}", waiting());
        // Phase 1 closes: its only task is accepted.
        store.mutate(None, "t", None, |team, _| {
            team.task_mut(1).unwrap().status = TaskStatus::Accepted;
            Ok(Value::Null)
        }).unwrap();
        let saved = store.load("t").unwrap().unwrap();
        assert_eq!(saved.proposals[0]["state"], "withdrawn", "{}", saved.proposals[0]);
        let why = saved.proposals[0]["note"].as_str().unwrap().to_owned();
        assert!(why.contains("Phase 1 is closed (done)"), "{why}");
        assert!(store.events("t", None, 50).unwrap().to_string().contains("proposal_withdrawn"));
        assert!(!waiting().iter().any(|id| id.contains("proposal-p1")), "{:?}", waiting());
        // Never offered again: the status lists no open proposal.
        let status = run("status", &json!({"team":"t"}), false).unwrap();
        assert!(!status.to_string().contains("\"state\":\"open\""), "{status}");
        // The lead is told why, in one line.
        let inbox = run("inbox", &json!({"team":"t","by":"human"}), false).unwrap();
        let told: Vec<&Value> = inbox["items"].as_array().unwrap().iter().filter(|item| item["key"] == "proposal:p1" || item["summary"].as_str().is_some_and(|text| text.contains("proposal p1"))).collect();
        assert_eq!(told.len(), 1, "{inbox}");
        let line = told[0]["summary"].as_str().unwrap();
        assert!(line.contains("withdrawn before the owner saw it") && line.contains("Phase 1 is closed"), "{line}");
        // Another change keeps it withdrawn and tells nothing new.
        store.mutate(None, "t", None, |_, _| Ok(Value::Null)).unwrap();
        assert_eq!(store.events("t", None, 50).unwrap().to_string().matches("proposal_withdrawn").count(), 1);
    });
}

/// Task 111: a proposal that stopped being possible without a team change
/// since is withdrawn when it is read: it never waits for the owner, and an
/// Allow on a card shown before is refused with the reason and stores the
/// withdrawal.
#[test]
fn an_inapplicable_proposal_is_withdrawn_at_read_time_and_its_allow_refused() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        let input = json!({"kind":"add-to-phase","reason":"a crash to hunt","phase":1,
            "tasks":[{"id":2,"title":"Find the crash","assignee":"dev","criteria":["Fixed"]}]});
        ops::apply(&mut team, "propose", &input, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
        let hash = team.proposals[0]["hash"].as_str().unwrap().to_owned();
        // Stored as it was before this fix: open, its phase already closed.
        team.task_mut(1).unwrap().status = TaskStatus::Accepted;
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        assert_eq!(store.load("t").unwrap().unwrap().proposals[0]["state"], "open");
        let items = run("waiting", &json!({}), false).unwrap();
        assert!(!items.to_string().contains("proposal-p1"), "{items}");
        let error = decide_proposal_in(&mut store, &json!({"team":"t","proposal":"p1","accept":true,"by":"human","expected_hash":hash}), &Actor::Human)
            .unwrap_err()
            .to_string();
        assert!(error.contains("can no longer apply") && error.contains("withdrawn"), "{error}");
        let saved = store.load("t").unwrap().unwrap();
        assert_eq!(saved.proposals[0]["state"], "withdrawn");
        assert!(saved.task(2).is_none(), "nothing was added");
    });
}

/// Task 111: proposing into a closed phase is refused at once, as it is for
/// a draft, with the reason.
#[test]
fn a_proposal_into_a_done_phase_is_refused() {
    let mut team = running_team();
    team.task_mut(1).unwrap().status = TaskStatus::Accepted;
    let input = json!({"kind":"add-to-phase","reason":"late work","phase":1,
        "tasks":[{"id":2,"title":"Late","assignee":"dev","criteria":["Done"]}]});
    let error = ops::apply(&mut team, "propose", &input, &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Phase 1 is closed (done)") && error.contains("extend"), "{error}");
    assert!(team.proposals.is_empty());
}

/// Task 112 (owner, 2026-11-06: "I paused it in the morning, unpaused it in
/// the afternoon, but Zerus still thinks it is paused"): a pause says since
/// when and by whom wherever it shows (status, Waiting for you, the sessions'
/// state, the lead's inbox) and leaves with the team's Resume; a team paused
/// before this record reads it from its pause event.
#[test]
fn a_paused_team_says_since_when_and_by_whom_and_resume_team_ends_it() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.tasks.push(Task { id: 2, title: "Integrate".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        store.mutate(None, "t", None, |team, events| ops::apply(team, "pause", &json!({}), &Actor::Human, &BTreeSet::new(), events)).unwrap();
        let saved = store.load("t").unwrap().unwrap();
        let pause = saved.paused.clone().expect("the pause is recorded");
        assert_eq!((pause.by.as_str(), pause.reason.as_str()), ("You", "paused"));
        // Status carries it; Waiting for you lists it with Resume team, unnotified by kind.
        let status = run("status", &json!({"team":"t"}), false).unwrap();
        assert_eq!(status["paused"]["by"], "You", "{}", status["paused"]);
        let waiting = run("waiting", &json!({}), false).unwrap();
        let item = waiting["items"].as_array().unwrap().iter().find(|item| item["kind"] == "team_paused").cloned().expect("listed");
        assert_eq!(item["action"], "resume_team");
        let title = item["title"].as_str().unwrap();
        assert!(title.starts_with("Team paused since ") && title.contains(" by You: members get no tasks"), "{title}");
        // The sessions' state reason says so too.
        let reasons: Vec<String> = super::super::waiting::reasons(std::slice::from_ref(&saved), 0.0)
            .into_iter()
            .filter_map(|waiting| waiting.reason.map(|reason| reason.to_string()))
            .collect();
        assert!(!reasons.is_empty() && reasons.iter().all(|reason| reason.contains("paused since") && reason.contains("members get no tasks")), "{reasons:?}");
        // The lead is told once that hand-outs wait for the team's Resume.
        let inbox = run("inbox", &json!({"team":"t","by":"human"}), false).unwrap();
        let told: Vec<&Value> = inbox["items"].as_array().unwrap().iter().filter(|item| item["kind"] == "team_paused").collect();
        assert_eq!(told.len(), 1, "{inbox}");
        let line = told[0]["summary"].as_str().unwrap();
        assert!(line.contains("The team is paused since") && line.contains("by You") && line.contains("#2"), "{line}");
        // Resume team ends it everywhere.
        store.mutate(None, "t", None, |team, events| ops::apply(team, "resume", &json!({}), &Actor::Human, &BTreeSet::new(), events)).unwrap();
        assert!(store.load("t").unwrap().unwrap().paused.is_none());
        let waiting = run("waiting", &json!({}), false).unwrap();
        assert!(!waiting.to_string().contains("team_paused"), "{waiting}");
        // A team paused before the record reads it from its pause event.
        store.mutate(None, "t", None, |team, events| ops::apply(team, "pause", &json!({}), &Actor::Member("lead".into()), &BTreeSet::new(), events)).unwrap();
        store.mutate(None, "t", None, |team, _| { team.paused = None; Ok(Value::Null) }).unwrap();
        assert!(store.load("t").unwrap().unwrap().paused.is_none(), "kept only while paused, and dropped here by hand");
        let status = run("status", &json!({"team":"t"}), false).unwrap();
        assert_eq!(status["paused"]["by"], "Lead", "{}", status["paused"]);
        assert!(status["paused"]["at"].as_f64().unwrap() > 0.0);
    });
}

/// Task 88: the Tests settings read the project's providers and templates,
/// check a candidate override without writing it, and the owner's Save
/// (settings) refuses one hgs test could not load.
#[test]
fn the_tests_settings_read_check_and_save_through_hgs() {
    let dir = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".zerus")).unwrap();
    std::fs::write(project.path().join(".zerus").join("tests.toml"),
        "version = 1\n[[provider]]\nname = \"engine\"\nkind = \"ctest\"\nbuild_dir = \"build\"\nresults = { format = \"junit\", path = \"build/ctest.xml\" }\n\
         [[template]]\nid = \"make-all\"\nprogram = \"make\"\nargs = [\"all\"]\n").unwrap();
    std::fs::write(project.path().join("pyproject.toml"), "[tool.pytest.ini_options]\n").unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        team.folder = project.path().display().to_string();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let read = run("test-config", &json!({"team":"t"}), false).unwrap();
        assert_eq!((read["ok"].clone(), read["source"].clone()), (json!(true), json!("file")), "{read}");
        assert_eq!(read["providers"][0]["kind"], "ctest");
        assert_eq!(read["providers"][0]["results"]["format"], "junit");
        assert_eq!((read["templates"][0]["id"].clone(), read["templates"][0]["accepted"].clone()), (json!("make-all"), json!(false)));
        assert!(read["proposal"].as_str().unwrap().contains("pytest"), "{read}");
        // A candidate is checked, never written.
        let candidate = json!({"provider":[{"name":"python","kind":"pytest","command":"python -m pytest {targets}",
            "results":{"format":"junit","path":"out/pytest.xml"}}]});
        let checked = run("test-config", &json!({"team":"t","action":"check","tests":candidate}), false).unwrap();
        assert_eq!(checked["ok"], true, "{checked}");
        assert!(checked["providers"].as_array().unwrap().iter().any(|provider| provider["name"] == "python"));
        assert!(store.load("t").unwrap().unwrap().settings.tests.is_none(), "a check writes nothing");
        let bad = json!({"provider":[{"name":"x","kind":"pytest","results":{"format":"html","path":"r.html"}}]});
        let refused = run("test-config", &json!({"team":"t","action":"check","tests":bad}), false).unwrap();
        assert_eq!(refused["ok"], false);
        assert!(refused["error"].as_str().unwrap().contains("junit, trx or unreal"), "{refused}");
        // The owner's Save goes through settings, checked the same way.
        store.mutate(None, "t", None, |team, events| ops::apply(team, "settings", &json!({"settings":{"tests":candidate}}), &Actor::Human, &BTreeSet::new(), events)).unwrap();
        assert_eq!(store.load("t").unwrap().unwrap().settings.tests, Some(candidate.clone()));
        let error = store
            .mutate(None, "t", None, |team, events| ops::apply(team, "settings", &json!({"settings":{"tests":bad}}), &Actor::Human, &BTreeSet::new(), events))
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not load"), "{error}");
        let saved = run("test-config", &json!({"team":"t"}), false).unwrap();
        assert_eq!(saved["overridden"], true);
        assert!(saved["providers"].as_array().unwrap().iter().any(|provider| provider["name"] == "python"));
    });
}

#[test]
fn phase_approval_card_requires_the_owners_explicit_override() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        super::live_sessions_override(Some(Vec::new()));
        super::set_test_caller_identities(Some((None, None, None)));
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        ops::apply(&mut team, "settings", &json!({"settings":{"phase_checks":{"unresolved_prerequisites":true}}}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
        ops::apply(&mut team, "extend", &json!({"tasks":[{"id":3,"title":"Later","assignee":"dev","requires":[{"task":999}]}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
        let team_id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let proposed = store.mutate(None, &team_id, None, |team, events| ops::apply(team, "propose",
            &json!({"kind":"approve-phase","phase":2,"approve_anyway":true,"reason":"ready"}),
            &Actor::Member("lead".into()), &BTreeSet::new(), events)).unwrap();
        let saved = store.load(&team_id).unwrap().unwrap();
        let card = saved.proposals.iter().find(|p| p["id"] == proposed["proposal"]).unwrap();
        assert!(card["change"]["input"].get("approve_anyway").is_none(), "{card}");
        let mut decision = json!({"team":team_id,"proposal":proposed["proposal"],"accept":true,"expected_hash":proposed["hash"]});
        let error = decide_proposal_in(&mut store, &decision, &Actor::Human).unwrap_err().to_string();
        assert!(error.contains("Unresolved prerequisites"), "{error}");
        assert_eq!(store.load(&team_id).unwrap().unwrap().phases[0].state, super::super::model::PhaseState::Draft);
        decision["approve_anyway"] = json!(true);
        let receipt = decide_proposal_in(&mut store, &decision, &Actor::Human).unwrap();
        assert_eq!(receipt["status"], "accepted", "{receipt}");
        assert!(store.load(&team_id).unwrap().unwrap().phases[0].state.is_accepted());
        assert_eq!(decide_proposal_in(&mut store, &decision, &Actor::Human).unwrap()["status"], "accepted");
    });
    super::live_sessions_override(None);
    super::set_test_caller_identities(None);
}

#[test]
fn phase_card_trial_override_preserves_other_approval_guards() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        ops::apply(&mut team, "settings", &json!({"settings":{"phase_checks":{"unresolved_prerequisites":true}}}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
        ops::apply(&mut team, "extend", &json!({"tasks":[{"id":2,"title":"Later","assignee":"dev","requires":[{"task":999}]}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
        let team_id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let propose = |team: &mut Team, events: &mut Vec<Event>| ops::apply(team, "propose", &json!({"kind":"approve-phase","phase":2,"reason":"ready"}), &Actor::Member("lead".into()), &BTreeSet::new(), events);
        store.mutate(None, &team_id, None, |team, _| {
            team.members.retain(|member| member.id != "dev");
            Ok(Value::Null)
        }).unwrap();
        let error = store.mutate(None, &team_id, None, propose).unwrap_err().to_string();
        assert!(error.contains("Allow could not apply"), "{error}");
        assert!(!error.contains("Unresolved prerequisites"), "{error}");
        assert!(store.load(&team_id).unwrap().unwrap().proposals.is_empty());
    });
}

#[test]
fn an_open_phase_approval_card_is_rechecked_without_running_phase_checks() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let mut store = Store::open(true).unwrap();
        let mut team = running_team();
        ops::apply(&mut team, "settings", &json!({"settings":{"phase_checks":{"unresolved_prerequisites":true}}}),
            &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
        ops::apply(&mut team, "extend", &json!({"tasks":[{"id":3,"title":"Later","assignee":"dev","requires":[{"task":999}]}]}),
            &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
        let id = team.id.clone();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team, vec![]).unwrap();
        let proposed = store.mutate(None, &id, None, |team, events| ops::apply(team, "propose",
            &json!({"kind":"approve-phase","phase":2,"reason":"Review the prerequisite override"}),
            &Actor::Member("lead".into()), &BTreeSet::new(), events)).unwrap();
        let mut saved = store.load(&id).unwrap().unwrap();
        let before = super::super::plan::PHASE_CHECK_CALLS.with(std::cell::Cell::get);
        assert!(ops::withdraw_inapplicable(&mut saved, &mut Vec::new()).is_empty());
        let status = run("status", &json!({"team":id}), false).unwrap();
        let card = status["proposals"].as_array().unwrap().iter()
            .find(|card| card["id"] == proposed["proposal"]).unwrap();
        assert_eq!(card["state"], "open");
        assert_eq!(super::super::plan::PHASE_CHECK_CALLS.with(std::cell::Cell::get), before);
    });
}
