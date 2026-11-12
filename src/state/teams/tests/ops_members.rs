use super::*;
use super::support::*;

#[test]
fn a_sub_lead_takes_reports_only_while_it_is_one_and_never_in_a_cycle() {
    let mut team = team_with_sub_lead();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    assert_eq!((team.reviewer_of("qa").as_str(), team.reviewer_of("dev").as_str(), team.reviewer_of("qalead").as_str()), ("qalead", "lead", "lead"));
    assert_eq!(team.reports_of("qalead").iter().map(|member| member.id.as_str()).collect::<Vec<_>>(), ["qa"]);
    assert_eq!(team.reports_of("lead").iter().map(|member| member.id.as_str()).collect::<Vec<_>>(), ["dev", "qalead"]);
    // Only a sub-lead takes reports, never itself or one below it.
    let error = apply(&mut team, "reports-to", &json!({"member":"dev","parent":"qa"}), &Actor::Human, &none, &mut events).unwrap_err();
    assert_eq!(error.to_string(), "dev reports to qa, which is not a sub-lead");
    apply(&mut team, "sub-lead", &json!({"member":"qa","sub":true}), &Actor::Human, &none, &mut events).unwrap();
    let error = apply(&mut team, "reports-to", &json!({"member":"qalead","parent":"qa"}), &Actor::Human, &none, &mut events).unwrap_err();
    assert!(error.to_string().ends_with("would report to itself"), "{error}");
    assert_eq!(team.member("qalead").unwrap().parent, "", "a refused change keeps the old parent");
    // A member cannot change the hierarchy, and the lead reports to nobody.
    let error = apply(&mut team, "sub-lead", &json!({"member":"dev","sub":true}), &Actor::Member("qalead".into()), &none, &mut events).unwrap_err();
    assert_eq!(error.to_string(), "Only the lead or you change who reports to whom");
    let error = apply(&mut team, "reports-to", &json!({"member":"lead","parent":"qalead"}), &Actor::Human, &none, &mut events).unwrap_err();
    assert_eq!(error.to_string(), "The lead reports to nobody");
    // A sub-lead with reports stays one, and is not removed.
    let error = apply(&mut team, "sub-lead", &json!({"member":"qalead","sub":false}), &Actor::Human, &none, &mut events).unwrap_err();
    assert_eq!(error.to_string(), "Move its reports to another lead first: qa (Tester)");
    let error = apply(&mut team, "remove-member", &json!({"member":"qalead"}), &Actor::Human, &none, &mut events).unwrap_err();
    assert_eq!(error.to_string(), "Move its reports to another lead first: qa (Tester)");
    // Add member names "Reports to"; an unknown one is refused.
    let error = apply(&mut team, "add-member", &json!({"member":{"id":"x","role":"Fuzzer","agent":"codex","parent":"nobody"}}), &Actor::Human, &none, &mut events)
        .unwrap_err();
    assert_eq!(error.to_string(), "x reports to unknown member nobody");
    assert!(team.member("x").is_none());
    apply(&mut team, "add-member", &json!({"member":{"id":"fuzz","role":"Fuzzer","agent":"codex","parent":"qalead"}}), &Actor::Human, &none, &mut events)
        .unwrap();
    assert_eq!(team.reviewer_of("fuzz"), "qalead");
    // Moved back to the lead, the Tester leaves the sub-lead's reports.
    apply(&mut team, "reports-to", &json!({"member":"qa","parent":"lead"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.member("qa").unwrap().parent, "");
    let feed: Vec<&Event> = events.iter().filter(|event| matches!(event.kind, "reports_to" | "sub_lead")).collect();
    assert_eq!(feed.last().unwrap().payload, json!({"by":"You","member":"Tester","to":"Lead"}));
    // Old JSON without the fields loads with everyone under the lead.
    let old: Member = serde_json::from_value(json!({"id":"m","role":"R","agent":"codex"})).unwrap();
    assert!(old.parent.is_empty() && !old.sub);
    assert!(serde_json::to_value(&old).unwrap().get("parent").is_none());
}

#[test]
fn a_sub_leads_member_interim_report_goes_to_the_sub_lead() {
    let mut team = team_with_sub_lead();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let tested = hand_out(&mut team, 2);
    apply(
        &mut team,
        "report",
        &json!({"task":2,"attempt":tested,"text":"Half the suite","interim":true}),
        &Actor::Member("qa".into()),
        &none,
        &mut events,
    )
    .unwrap();
    let theirs: Vec<_> = super::super::inbox::items_for(&team, "qalead").into_iter().filter(|item| item.kind == "interim_report").collect();
    assert_eq!(theirs.len(), 1, "{theirs:?}");
    assert_eq!(theirs[0].task, Some(2));
    assert!(super::super::inbox::items(&team).iter().all(|item| item.kind != "interim_report"));
    assert_eq!(events.iter().rev().find(|event| event.kind == "interim_report").unwrap().payload["to"], "QA lead");
}

#[test]
fn a_sub_lead_answers_and_decides_its_reports_work_and_the_lead_hears_only_its_own() {
    let mut team = team_with_sub_lead();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let sub = Actor::Member("qalead".into());
    let tested = hand_out(&mut team, 2);
    let built = hand_out(&mut team, 1);
    // The Tester's question goes to its sub-lead, who answers it.
    apply(&mut team, "ask", &json!({"text":"Which browser?"}), &Actor::Member("qa".into()), &none, &mut events).unwrap();
    assert!(super::super::inbox::items(&team).iter().all(|item| item.kind != "question"));
    let theirs = super::super::inbox::items_for(&team, "qalead");
    assert_eq!(theirs.iter().map(|item| item.kind).collect::<Vec<_>>(), ["question"]);
    apply(&mut team, "reply", &json!({"question":"q1","text":"Chrome"}), &sub, &none, &mut events).unwrap();
    assert_eq!(events.last().unwrap().payload["by"], "QA lead");
    // Both report: "Report to QA lead" and the plain one to the lead.
    apply(&mut team, "report", &json!({"task":2,"attempt":tested,"text":"Tested"}), &Actor::Member("qa".into()), &none, &mut events).unwrap();
    assert_eq!(events.iter().rev().find(|event| event.kind == "reported").unwrap().payload["to"], "QA lead");
    apply(&mut team, "report", &json!({"task":1,"attempt":built,"text":"Built"}), &Actor::Member("dev".into()), &none, &mut events).unwrap();
    assert_eq!(events.iter().rev().find(|event| event.kind == "reported").unwrap().payload["to"], "Lead");
    let lead_items: Vec<Option<u32>> = super::super::inbox::items(&team).iter().filter(|item| item.kind == "report").map(|item| item.task).collect();
    assert_eq!(lead_items, [Some(1)]);
    let sub_items: Vec<Option<u32>> = super::super::inbox::items_for(&team, "qalead").iter().filter(|item| item.kind == "report").map(|item| item.task).collect();
    assert_eq!(sub_items, [Some(2)]);
    // The sub-lead decides only its reports' work.
    let error = apply(&mut team, "accept", &json!({"task":1}), &sub, &none, &mut events).unwrap_err();
    assert_eq!(error.to_string(), "Only the lead or you decide on a report");
    let error = apply(&mut team, "accept", &json!({"task":2}), &Actor::Member("dev".into()), &none, &mut events).unwrap_err();
    assert_eq!(error.to_string(), "Only the lead or you decide on a report");
    apply(&mut team, "accept", &json!({"task":2}), &sub, &none, &mut events).unwrap();
    let accepted = events.iter().rev().find(|event| event.kind == "accepted").unwrap();
    assert_eq!((&accepted.payload["by"], &accepted.payload["task"]), (&json!("QA lead"), &json!(2)));
    assert_eq!(team.task(2).unwrap().status, TaskStatus::Accepted);
    // The lead still decides anything; holding follows the same rule.
    apply(&mut team, "accept", &json!({"task":1}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    let error = apply(&mut team, "retry", &json!({"task":1}), &sub, &none, &mut events).unwrap_err();
    assert_eq!(error.to_string(), "Only the lead or you retry tasks");
}

#[test]
fn a_member_may_adopt_an_existing_session() {
    let mut team = running_team();
    let mut events = Vec::new();
    let added = apply(
        &mut team,
        "add-member",
        &json!({"member":{"id":"dev2","role":"Backend","agent":"dsh",
            "session":{"name":"dsh/x/dev2","run_id":"r9","conversation_id":"c9"},
            "directory":"D:/w","folder":"shared"}}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(added["status"], "added");
    let member = team.member("dev2").unwrap();
    assert_eq!(member.state, MemberState::Active);
    assert_eq!(member.session.as_ref().map(|s| (s.name.as_str(), s.run_id.as_str())), Some(("dsh/x/dev2", "r9")));
    assert_eq!((member.directory.as_str(), member.folder), ("D:/w", MemberFolder::Shared));
    // A member without a session still starts its own.
    apply(&mut team, "add-member", &json!({"member":{"id":"dev3","role":"Backend","agent":"dsh"}}), &Actor::Human, &BTreeSet::new(), &mut events)
        .unwrap();
    assert_eq!(team.member("dev3").unwrap().state, MemberState::NotCreated);
}

#[test]
fn members_pause_and_leave_only_without_open_tasks() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    assert!(apply(&mut team, "pause-member", &json!({"member":"lead"}), &Actor::Human, &none, &mut events).is_err());
    apply(&mut team, "pause-member", &json!({"member":"qa"}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(team.member("qa").unwrap().state, MemberState::Paused);
    apply(&mut team, "resume-member", &json!({"member":"qa"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.member("qa").unwrap().state, MemberState::NotCreated);
    let error = apply(&mut team, "remove-member", &json!({"member":"qa"}), &Actor::Human, &none, &mut events).unwrap_err();
    assert!(error.to_string().contains("#2"), "{error}");
    cancel(&mut team, &json!({"task":2}), &Actor::Human, &mut events).unwrap();
    assert!(apply(&mut team, "remove-member", &json!({"member":"qa"}), &Actor::Member("lead".into()), &none, &mut events).is_err());
    apply(&mut team, "remove-member", &json!({"member":"qa"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.member("qa").unwrap().state, MemberState::Removed);
    // A removed member frees its place for a new one, which waits for a task.
    let docs = json!({"member":{"id":"docs","role":"Writer","agent":"codex"}});
    assert!(apply(&mut team, "add-member", &docs, &Actor::Member("qa".into()), &none, &mut events).is_err());
    apply(&mut team, "add-member", &docs, &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(team.member("docs").unwrap().state, MemberState::NotCreated);
    assert!(apply(&mut team, "add-member", &docs, &Actor::Human, &none, &mut events).is_err());
}

#[test]
fn scale_adds_a_roles_members_at_once_within_the_roster() {
    // U49: scale adds count members of one role, numbered, all or none.
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let scaled = apply(
        &mut team,
        "scale",
        &json!({"role":"Backend","agent":"codex","model":"gpt-5","effort":"high","account":"work","count":2}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!((scaled["status"].as_str(), scaled["members"].clone()), (Some("scaled"), json!(["backend", "backend2"])));
    let added = team.member("backend").unwrap();
    assert_eq!((added.role.as_str(), added.agent.as_str()), ("Backend", "codex"));
    assert_eq!((added.model.as_str(), added.effort.as_str(), added.account.as_str()), ("gpt-5", "high", "work"));
    assert_eq!((added.state, added.draining), (MemberState::NotCreated, false));
    assert_eq!(team.member("backend2").unwrap().role, "Backend");
    assert_eq!(events.iter().filter(|event| event.kind == "member_added").count(), 2);
    // The roster's cap counts them (not the lead): one more than the room
    // left does not fit and adds nothing.
    let room = team.settings.max_members - team.capped_members();
    let error = apply(&mut team, "scale", &json!({"role":"Backend","agent":"codex","count":room + 1}), &Actor::Human, &none, &mut events);
    let error = error.unwrap_err().to_string();
    assert!(error.contains("of 6 members besides the lead"), "{error}");
    assert!(team.member("backend3").is_none(), "a scale that does not fit adds nothing");
    // A role, an agent and a count are required; only the lead or you scale.
    for input in [
        json!({"role":"Backend","agent":"codex","count":0}),
        json!({"role":"Backend","agent":"codex"}),
        json!({"agent":"codex","count":1}),
        json!({"role":"Backend","count":1}),
    ] {
        assert!(apply(&mut team, "scale", &input, &Actor::Human, &none, &mut events).is_err(), "{input}");
    }
    let one = json!({"role":"Backend","agent":"codex","count":1});
    assert!(apply(&mut team, "scale", &one, &Actor::Member("dev".into()), &none, &mut events).is_err());
    // The lead scales on the owner's verified word (its check is in commands).
    let mut worded = one.clone();
    worded["roster_word"] = Value::Bool(true);
    worded["owner_word_checked"] = json!({"text":"Please add one more backend to the roster"});
    apply(&mut team, "scale", &worded, &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert!(team.member("backend3").unwrap().added.is_some(), "the roster says the lead added it on your word");
}

/// U56: the lead relaunches a stuck member only on the owner's setting;
/// otherwise the refusal names the command that sets it. Task 209: the
/// relaunch gives a fresh conversation with a handoff brief, not a restart of
/// the old one.
#[test]
fn a_stuck_member_is_relaunched_by_the_lead_only_with_the_owners_setting() {
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    let none = BTreeSet::new();
    let mut events = Vec::new();
    {
        let qa = team.members.iter_mut().find(|member| member.id == "qa").unwrap();
        qa.stuck = Some(MemberStuck { at: 1.0, reason: "no_progress".into(), phase: "tool".into(), ..Default::default() });
        qa.session = Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
    }
    // Owner, 2026-11-10: the setting is no longer needed for a stuck member.
    let receipt = apply(&mut team, "relaunch", &json!({"member":"qa"}), &lead, &none, &mut events).unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["fresh"].as_bool(), receipt["from"].as_str()), (Some("relaunch"), Some(true), Some("kimi/x/qa")));
    let qa = team.members.iter().find(|member| member.id == "qa").unwrap();
    assert!(qa.session.is_none(), "the fresh session starts on the next tick");
    assert!(qa.launch_after_swap);
    let handoff = qa.handoff.as_ref().expect("the new session gets the handoff brief");
    assert_eq!((handoff.why.as_str(), handoff.from.as_str()), ("relaunch", "kimi/x/qa"));
    assert_eq!(qa.replaced.as_ref().map(|session| session.name.as_str()), Some("kimi/x/qa"));
    assert!(events.iter().any(|event| event.kind == "fresh_session"));
    assert!(events.iter().any(|event| event.kind == "relaunch"));
}

/// Task 209: the owner relaunches a running member; it is not refused for
/// "did not fail to start", and the old conversation is set aside.
#[test]
fn the_owner_relaunches_a_running_member_into_a_fresh_conversation() {
    let mut team = running_team();
    let mut events = Vec::new();
    let qa = team.members.iter_mut().find(|member| member.id == "qa").unwrap();
    qa.session = Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
    let receipt = apply(&mut team, "relaunch", &json!({"member":"qa"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["fresh"].as_bool()), (Some("relaunch"), Some(true)));
    let qa = team.members.iter().find(|member| member.id == "qa").unwrap();
    assert!(qa.session.is_none() && qa.launch_after_swap && qa.handoff.is_some(), "a fresh session with its brief");
}

/// Task 209: the lead may propose a relaunch for a stuck member (the owner
/// allows it with a click); the card's trial runs the owner's own path, so a
/// running member no longer refuses it with "did not fail to start".
#[test]
fn the_lead_proposes_a_relaunch_for_a_stuck_member() {
    use crate::state::teams::model::MemberStuck;
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    let mut events = Vec::new();
    {
        let qa = team.members.iter_mut().find(|member| member.id == "qa").unwrap();
        qa.stuck = Some(MemberStuck { at: 1.0, reason: "no_progress".into(), phase: "tool".into(), ..Default::default() });
        qa.session = Some(SessionRef { name: "kimi/x/qa".into(), run_id: "r1".into(), conversation_id: "c".into(), ..Default::default() });
    }
    let receipt = apply(
        &mut team,
        "propose",
        &json!({"kind":"relaunch","member":"qa","reason":"no progress for an hour"}),
        &lead,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(receipt["status"], "proposed");
    let proposal = team.proposals.last().unwrap();
    assert_eq!((proposal["kind"].as_str(), proposal["state"].as_str()), (Some("relaunch"), Some("open")));
}

/// RogueDOD (2026-11-12, team 75549318): the lead proposed relaunching sec2
/// and sec3 when they stopped, and sec1 twice. The sessions came back by
/// themselves, the cards stayed, and the owner's Allow of all of them
/// replaced working sessions. One card per member; a card for a member that
/// had stopped withdraws once it works again.
#[test]
fn a_relaunch_card_is_one_per_member_and_withdraws_once_the_member_works_again() {
    use crate::state::teams::model::MemberSeen;
    let mut team = running_team();
    let lead = Actor::Member("lead".into());
    let none = BTreeSet::new();
    let mut events = Vec::new();
    {
        let qa = team.members.iter_mut().find(|member| member.id == "qa").unwrap();
        qa.session = Some(SessionRef { name: "dsh/x/qa".into(), run_id: "r1".into(), ..Default::default() });
        qa.seen = Some(MemberSeen { at: 1.0, live: false });
    }
    let card = json!({"kind":"relaunch","member":"qa","reason":"it stopped mid-task"});
    apply(&mut team, "propose", &card, &lead, &none, &mut events).unwrap();
    let id = team.proposals.last().unwrap()["id"].as_str().unwrap().to_owned();
    let error = apply(&mut team, "propose", &card, &lead, &none, &mut events).unwrap_err().to_string();
    assert!(error.starts_with(&format!("Card {id} already proposes relaunching")), "{error}");
    // Still stopped: the card stays.
    assert!(withdraw_inapplicable(&mut team, &mut events).is_empty());
    // The session runs again after the card was made: the card withdraws.
    let at = team.proposals.last().unwrap()["at"].as_f64().unwrap();
    team.members.iter_mut().find(|member| member.id == "qa").unwrap().seen = Some(MemberSeen { at: at + 1.0, live: true });
    assert_eq!(withdraw_inapplicable(&mut team, &mut events), vec![id.clone()]);
    let withdrawn = team.proposals.iter().find(|proposal| proposal["id"] == id.as_str()).unwrap();
    assert_eq!(withdrawn["state"], "withdrawn");
    assert!(withdrawn["withdrawn_reason"].as_str().unwrap().contains("works again"), "{withdrawn}");
    // A card for a member that runs (the owner wants a fresh conversation)
    // stays, until the member is relaunched another way.
    apply(&mut team, "propose", &card, &lead, &none, &mut events).unwrap();
    let id = team.proposals.last().unwrap()["id"].as_str().unwrap().to_owned();
    assert!(withdraw_inapplicable(&mut team, &mut events).is_empty());
    apply(&mut team, "relaunch", &json!({"member":"qa"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(withdraw_inapplicable(&mut team, &mut events), vec![id]);
    let withdrawn = team.proposals.last().unwrap();
    assert!(withdrawn["withdrawn_reason"].as_str().unwrap().contains("relaunched or swapped"), "{withdrawn}");
}

/// RogueDOD (2026-11-12): binding the session a member already has does not
/// make it the session the swap replaced, so the launch never renames it.
#[test]
fn a_swap_that_binds_the_members_own_session_sets_nothing_aside() {
    let mut team = running_team();
    let own = SessionRef { name: "dsh/x/qa".into(), run_id: "r1".into(), ..Default::default() };
    team.members.iter_mut().find(|member| member.id == "qa").unwrap().session = Some(own.clone());
    let input = json!({"member":"qa","session":{"name":"dsh/x/qa","run_id":"r1"}});
    apply(&mut team, "swap-member", &input, &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    let qa = team.member("qa").unwrap();
    assert_eq!((qa.session.as_ref().map(|session| session.name.as_str()), qa.replaced.as_ref()), (Some("dsh/x/qa"), None));
    // A relaunch is setting it aside; the swap binds it again instead.
    let mut team = running_team();
    {
        let qa = team.members.iter_mut().find(|member| member.id == "qa").unwrap();
        qa.session = None;
        qa.replaced = Some(own);
        qa.replaced_busy = true;
    }
    apply(&mut team, "swap-member", &input, &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    let qa = team.member("qa").unwrap();
    assert_eq!((qa.session.is_some(), qa.replaced.is_none(), qa.replaced_busy), (true, true, false));
}

#[test]
fn drain_waits_for_the_work_and_can_be_cancelled() {
    // U49: drain marks a member that then finishes its attempt and leaves;
    // the dispatcher pauses its session and removes it (its test is there).
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    assert!(apply(&mut team, "drain", &json!({"member":"lead"}), &Actor::Human, &none, &mut events).is_err());
    assert!(apply(&mut team, "drain", &json!({"member":"nobody"}), &Actor::Human, &none, &mut events).is_err());
    assert!(apply(&mut team, "drain", &json!({"member":"qa"}), &Actor::Member("dev".into()), &none, &mut events).is_err());
    // The lead drains on the owner's verified word (its check is in commands).
    let started = apply(&mut team, "drain", &json!({"member":"qa","roster_word":true}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!((started["status"].as_str(), started["by"].as_str()), (Some("draining"), Some("Lead on your word")));
    assert!(team.member("qa").unwrap().draining);
    assert_eq!(events.last().unwrap().kind, "member_draining");
    assert_eq!(events.last().unwrap().payload["member"], "Tester");
    // Undo before it ends: the same command with draining false needs no word.
    let cancelled = apply(&mut team, "drain", &json!({"member":"qa","draining":false}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(cancelled["status"], "draining_cancelled");
    assert!(!team.member("qa").unwrap().draining);
    assert_eq!(events.last().unwrap().kind, "drain_cancelled");
    // A sub-lead with reports moves them first, as a removal does.
    team.members.iter_mut().find(|member| member.id == "qa").unwrap().sub = true;
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().parent = "qa".into();
    let error = apply(&mut team, "drain", &json!({"member":"qa"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("reports"), "{error}");
    // A team that ended drains nobody.
    team.status = TeamStatus::Done;
    let error = apply(&mut team, "drain", &json!({"draining":false,"member":"dev"}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("running or paused"), "{error}");
}

/// #581 N3: a drain moves the member's not-started tasks, a pinned one
/// included, to same-role peers, so it always ends.
#[test]
fn a_drain_moves_not_started_tasks_pinned_included_to_role_peers() {
    let mut team = running_team();
    team.members.push(Member { id: "qa2".into(), role: "Tester".into(), agent: "claude".into(), ..Default::default() });
    {
        let task = team.task_mut(2).unwrap();
        task.pinned = true;
        task.status = TaskStatus::Pending;
    }
    let mut events = Vec::new();
    let receipt = apply(&mut team, "drain", &json!({"member":"qa"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["waiting"].clone()), (Some("draining"), json!([])));
    let task = team.task(2).unwrap();
    assert_eq!((task.assignee.as_str(), task.pinned), ("qa2", false));
    assert!(events.iter().any(|event| event.kind == "drain_moved"));
    assert!(crate::state::teams::inbox::items(&team).iter().all(|item| item.kind != "drain_held"));
}

/// #581 N3: with no same-role peer, the tasks that keep the drain open are
/// named in the answer and in one lead inbox item.
#[test]
fn a_drain_with_no_peer_names_the_tasks_that_keep_it_open() {
    let mut team = running_team();
    {
        let task = team.task_mut(2).unwrap();
        task.pinned = true;
        task.status = TaskStatus::Pending;
    }
    let mut events = Vec::new();
    let receipt = apply(&mut team, "drain", &json!({"member":"qa"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(receipt["waiting"], json!([2]));
    let items = crate::state::teams::inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "drain_held").unwrap();
    assert!(item.summary.contains("#2") && item.summary.contains("no other Tester"), "{}", item.summary);
    assert!(item.detail.contains("#2"), "{}", item.detail);
}

#[test]
fn a_team_without_a_draining_member_keeps_todays_flow() {
    // U49 opt-in: a team stored before this field loads unchanged, and its
    // hand-out, report, stage and accept produce the events of today's flow.
    let mut team = running_team();
    {
        let task = team.task_mut(1).unwrap();
        task.pipeline = "code".into();
        task.pipeline_run = Some(PipelineRun {
            id: "code".into(),
            name: "Code: review, test, lead".into(),
            stages: vec![
                PipelineStage { kind: "review".into(), role: "Reviewer".into(), member: "qa".into(), ..Default::default() },
                PipelineStage { kind: "test".into(), role: "Tester".into(), member: "qa".into(), ..Default::default() },
            ],
            max_rounds: 3,
            folder: "hold".into(),
            accept: "lead".into(),
            round: 1,
            hold_from_report: false,
            trimmed_returns: 0,
            runs: Vec::new(),
        });
    }
    let stored = serde_json::to_string(&team).unwrap();
    assert!(!stored.contains("draining"), "{stored}");
    team = serde_json::from_str(&stored).unwrap();
    assert!(team.members.iter().all(|member| !member.draining));
    let mut events = Vec::new();
    let attempt = hand_out(&mut team, 1);
    events.clear();
    report(&mut team, &json!({"task":1,"attempt":attempt,"text":"Done"}), &Actor::Member("dev".into()), &mut events).unwrap();
    super::super::pipeline::give_stage(&mut team, 1, "qa").unwrap();
    let stage = team.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap().id.clone();
    report(&mut team, &json!({"task":1,"attempt":stage,"verdict":"pass","text":"Read it"}), &Actor::Member("qa".into()), &mut events).unwrap();
    super::super::pipeline::give_stage(&mut team, 1, "qa").unwrap();
    let stage = team.task(1).unwrap().pipeline_run.as_ref().unwrap().open_run().unwrap().id.clone();
    report(&mut team, &json!({"task":1,"attempt":stage,"verdict":"pass","text":"Green"}), &Actor::Member("qa".into()), &mut events).unwrap();
    apply(&mut team, "accept", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    let kinds: Vec<&str> = events.iter().map(|event| event.kind).collect();
    assert_eq!(kinds, ["reported", "stage_started", "stage_passed", "stage_started", "stage_passed", "stages_passed", "accepted"]);
    assert_eq!(team.task(1).unwrap().status, TaskStatus::Accepted);
    assert!(!serde_json::to_string(&team).unwrap().contains("draining"));
}

#[test]
fn auto_gives_the_agents_default_folder() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    plan(
        &mut team,
        &json!({"settings":{"folder_policy":"auto"},
            "members":[{"id":"dev","role":"Backend","agent":"dsh"},{"id":"qa","role":"Tester","agent":"claude"},
                {"id":"api","role":"Backend","agent":"codex","folder":"shared"}],
            "tasks":[{"id":1,"title":"A","assignee":"dev"}]}),
        &Actor::Human,
        &mut events,
    )
    .unwrap();
    let folder = |id: &str| team.member(id).unwrap().folder;
    // F1.2: auto keeps the agent's default folder; an explicit one stays.
    assert_eq!(folder("dev"), MemberFolder::Clone);
    assert_eq!(folder("qa"), MemberFolder::Worktree);
    assert_eq!(folder("api"), MemberFolder::Shared);
    // A stored team keeps its members' folders.
    let (mut old, _) = create("t2", &json!({"name":"T2","lead":{"id":"lead","agent":"claude"}})).unwrap();
    plan(&mut old, &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"}],"tasks":[{"id":1,"title":"A","assignee":"dev"}]}), &Actor::Human, &mut events).unwrap();
    assert_eq!(old.member("dev").unwrap().folder, MemberFolder::Shared);
}

#[test]
fn a_lead_swaps_a_member_only_on_the_word() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let error = apply(&mut team, "swap-member", &json!({"member":"qa"}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("your word"), "{error}");
    let swapped = apply(&mut team, "swap-member", &json!({"member":"qa","roster_word":true,"owner_word_checked":{"text":"swap qa for kimi"}}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(swapped["by"], "Lead on your word");
    assert!(events.iter().any(|event| event.kind == "member_swapped" && event.payload["owner_word"] == "swap qa for kimi"));
}

#[test]
fn a_lead_removes_a_member_or_raises_the_cap_only_on_the_word() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    // Without the verified word the lead cannot remove a member.
    let error = apply(&mut team, "remove-member", &json!({"member":"qa"}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("your word"), "{error}");
    apply(&mut team, "cancel", &json!({"task":2}), &Actor::Human, &none, &mut events).unwrap();
    let removed = apply(&mut team, "remove-member", &json!({"member":"qa","roster_word":true}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(removed["by"], "Lead on your word");
    // max_members moves only with the word too.
    let error = apply(&mut team, "settings", &json!({"settings":{"max_members":5}}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Only you change"), "{error}");
    apply(&mut team, "settings", &json!({"settings":{"max_members":5},"roster_word":true}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(team.settings.max_members, 5);
}

#[test]
fn the_worktree_policy_refuses_a_shared_member() {
    let mut team = running_team();
    team.settings.folder_policy = FolderPolicy::Worktree;
    for member in team.members.iter_mut() {
        if member.id != "lead" {
            member.folder = MemberFolder::Worktree;
        }
    }
    let validation = plan::validate(&team, &BTreeSet::new());
    assert!(!validation.errors.iter().any(|finding| finding.text.contains("worktree policy")), "{:?}", validation.errors);
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().folder = MemberFolder::Shared;
    let validation = plan::validate(&team, &BTreeSet::new());
    assert!(validation.errors.iter().any(|finding| finding.text.contains("worktree policy")), "{:?}", validation.errors);
}

#[test]
fn adopting_an_existing_session_is_an_event() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    let plan_with_session = json!({"members":[{"id":"dev","role":"Backend","agent":"codex",
            "session":{"name":"codex/x/dev","run_id":"r1","conversation_id":"c1"}}],
        "tasks":[{"id":1,"title":"API","assignee":"dev"}]});
    plan(&mut team, &plan_with_session, &Actor::Human, &mut events).unwrap();
    let adopted: Vec<_> = events.iter().filter(|event| event.kind == "member_adopted").collect();
    assert_eq!(adopted.len(), 1, "{events:?}");
    assert_eq!((adopted[0].payload["member"].as_str(), adopted[0].payload["session"].as_str()), (Some("Backend"), Some("codex/x/dev")));
    // Planning the same session again does not adopt it twice (L2).
    events.clear();
    plan(&mut team, &plan_with_session, &Actor::Human, &mut events).unwrap();
    assert!(!events.iter().any(|event| event.kind == "member_adopted"), "{events:?}");
    // A new member with a session is adopted.
    events.clear();
    apply(
        &mut team,
        "add-member",
        &json!({"member":{"id":"qa","role":"Tester","agent":"kimi","session":{"name":"kimi/x/qa","run_id":"r2"}}}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert!(events.iter().any(|event| event.kind == "member_adopted" && event.payload["member"] == "Tester"), "{events:?}");
}

#[test]
fn a_next_goal_team_keeps_members_and_adopts_their_sessions() {
    let mut team = running_team();
    team.status = TeamStatus::Done;
    // N8: the old team's session-bound state must not follow the member.
    let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    member.session = Some(SessionRef { name: "claude/x/dev".into(), ..Default::default() });
    member.pending = Some(Default::default());
    member.permission = Some(json!({"tool":"Bash"}));
    member.permission_answers = vec![json!({"decision":"allow"})];
    member.limited = Some(Default::default());
    member.limit_resume = true;
    member.budget = Some(Default::default());
    member.seen = Some(Default::default());
    member.error = "old launch failed".into();
    member.launch_id = "L".into();
    member.settings_applied = true;
    member.quieted = "c-old".into();
    let live = [crate::state::teams::dispatcher::Seen {
        name: "claude/x/dev".into(),
        run_id: "r9".into(),
        conversation_id: "c9".into(),
        live: true,
        ..Default::default()
    }];
    let next = next_team(&team, "Ship v2", "Checkout 2", &live, &[]);
    assert_eq!(next.status, TeamStatus::Planning);
    assert_eq!((next.folder.as_str(), next.goal.as_str(), next.previous.as_str()), (team.folder.as_str(), "Ship v2", team.id.as_str()));
    assert_eq!(next.lead, team.lead);
    assert_eq!(next.members.len(), team.members.len());
    assert!(next.tasks.is_empty());
    assert_eq!(next.brief, Notice::Pending);
    let dev = next.member("dev").unwrap();
    assert_eq!(dev.state, MemberState::Active);
    assert_eq!(
        dev.session.as_ref().map(|session| (session.name.as_str(), session.run_id.as_str(), session.conversation_id.as_str())),
        Some(("claude/x/dev", "r9", "c9"))
    );
    assert!(dev.pending.is_none() && dev.permission.is_none() && dev.permission_answers.is_empty());
    assert!(dev.limited.is_none() && !dev.limit_resume && dev.budget.is_none() && dev.seen.is_none());
    assert!(dev.error.is_empty() && dev.launch_id.is_empty() && !dev.settings_applied && dev.quieted.is_empty());
    // A session that is gone leaves the member NotCreated.
    let next = next_team(&team, "Ship v2", "", &[], &[]);
    let dev = next.member("dev").unwrap();
    assert_eq!((dev.state, dev.session.is_some()), (MemberState::NotCreated, false));
    // A session that now serves another active team stays there.
    let mut other = running_team();
    other.id = "other".into();
    other.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
        Some(SessionRef { name: "claude/x/dev".into(), ..Default::default() });
    let next = next_team(&team, "Ship v2", "", &live, &[other]);
    assert_eq!(next.member("dev").unwrap().state, MemberState::NotCreated);
}

#[test]
fn a_next_goal_team_leaves_out_a_removed_member() {
    let mut team = running_team();
    team.status = TeamStatus::Done;
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().state = MemberState::Removed;
    // N21: the next goal copies the living roster only; a revived member
    // would count against the tree budget and block adoption elsewhere.
    let next = next_team(&team, "Ship v2", "", &[], &[]);
    assert!(next.member("dev").is_none());
    assert_eq!(next.members.len(), team.members.len() - 1);
    assert!(next.members.iter().all(|member| member.state != MemberState::Removed));
}

#[test]
fn the_owner_decides_the_leads_membership_proposal() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let value = apply(
        &mut team,
        "propose",
        &json!({"kind":"add-member","reason":"needs help","member":{"id":"extra","role":"Helper","agent":"claude"}}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap();
    let id = value["proposal"].as_str().unwrap().to_owned();
    let hash = value["hash"].as_str().unwrap().to_owned();
    // Only the owner decides.
    let error = apply(&mut team, "decide-proposal", &json!({"proposal":id,"accept":true}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only you decide a proposal");
    apply(&mut team, "decide-proposal", &json!({"proposal":id,"accept":true,"expected_hash":hash}), &Actor::Human, &none, &mut events).unwrap();
    assert!(team.member("extra").is_some());
    assert_eq!(team.proposals[0]["state"], "accepted");
    // F8: a swap-member proposal is stored with the member it changes.
    let value = apply(
        &mut team,
        "propose",
        &json!({"kind":"swap-member","reason":"replace the session","member":{"id":"dev","agent":"claude","account":"work"}}),
        &Actor::Member("lead".into()),
        &none,
        &mut events,
    )
    .unwrap();
    assert!(value["proposal"].as_str().is_some());
    let swap = team.proposals.last().unwrap();
    assert_eq!((swap["kind"].as_str(), swap["member"]["agent"].as_str()), (Some("swap-member"), Some("claude")));
}

#[test]
fn swapping_a_member_keeps_its_work_and_continues_the_open_attempt() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    // The member works on #1 with half a report and an open question.
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session = Some(SessionRef {
        host: String::new(),
        name: "codex/x/dev".into(),
        run_id: "r1".into(),
        conversation_id: "c1".into(),
    });
    let task = team.task_mut(1).unwrap();
    task.status = TaskStatus::InProgress;
    task.attempts.push(Attempt {
        n: 1,
        id: "a1".into(),
        member: "dev".into(),
        state: AttemptState::Delivered,
        sent_at: 5.0,
        delivered_at: Some(6.0),
        report: Some(Report { text: "Half done: the API is next.".into(), at: 10.0, ..Default::default() }),
        questions: vec![Question { id: "q1".into(), text: "Which database?".into(), at: 11.0, ..Default::default() }],
        ..Default::default()
    });
    let role = team.member("dev").unwrap().role.clone();
    let new_session = json!({"name":"codex/x/new","run_id":"r2","conversation_id":"c2"});
    let result = apply(
        &mut team,
        "swap-member",
        &json!({"member":"dev","session":new_session}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!((result["status"].as_str(), result["to"].as_str()), (Some("swapped"), Some("codex/x/new")));
    // H6: the member keeps its id, role, tasks and folder; the old session
    // is listed for the owner, which keeps running.
    let member = team.member("dev").unwrap();
    assert_eq!((member.id.as_str(), member.role.as_str()), ("dev", role.as_str()));
    assert_eq!(member.session.as_ref().map(|session| session.name.as_str()), Some("codex/x/new"));
    assert!(!member.launch_after_swap);
    assert_eq!(team.swapped.last().unwrap()["old"], "codex/x/dev");
    assert_eq!(team.swapped.last().unwrap()["member"], role.as_str());
    assert_eq!(team.members.len(), 3);
    // The open attempt is revoked as swapped and the task waits again with
    // what the new session needs to continue.
    let task = team.task(1).unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    let old = task.attempts.iter().find(|attempt| attempt.member == "dev").unwrap();
    assert_eq!((old.state, old.error.as_str()), (AttemptState::Revoked, "swapped"));
    let continuation = task.continuation.as_ref().unwrap();
    assert_eq!(continuation.report, "Half done: the API is next.");
    assert_eq!(continuation.questions, vec!["Which database?".to_owned()]);
    assert_eq!(continuation.since, old.sent_at);
    assert!(events.iter().any(|event| event.kind == "member_swapped" && event.payload["from"] == "codex/x/dev"));
    // Without a session the member is launched again with the new agent;
    // it does not count twice against max_members.
    let result = apply(
        &mut team,
        "swap-member",
        &json!({"member":"dev","agent":"kimi","model":"k2"}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!(result["to"], "");
    let member = team.member("dev").unwrap();
    assert_eq!((member.agent.as_str(), member.model.as_str()), ("kimi", "k2"));
    assert!(member.session.is_none() && member.launch_id.is_empty() && member.state == MemberState::NotCreated);
    // H6: it starts again even before its next task.
    assert!(member.launch_after_swap && !team.member("dev").unwrap().session.is_some());
    assert_eq!(team.members.len(), 3);
    // A member cannot swap, only the owner.
    let error = apply(&mut team, "swap-member", &json!({"member":"dev"}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only you swap a member; the lead needs your word: propose {\"kind\":\"swap-member\"} as a card (hgs guide team-propose); the owner allows it with a click");
}

#[test]
fn swapping_the_lead_revokes_its_task_and_briefs_the_new_session() {
    let mut team = running_team();
    let mut events = Vec::new();
    let none = BTreeSet::new();
    // The lead has its own task in progress.
    let task = team.tasks.first_mut().unwrap();
    task.assignee = "lead".into();
    task.status = TaskStatus::InProgress;
    task.attempts.push(Attempt {
        n: 1,
        id: "lead-a1".into(),
        member: "lead".into(),
        state: AttemptState::Delivered,
        sent_at: 5.0,
        delivered_at: Some(6.0),
        ..Default::default()
    });
    let result = apply(
        &mut team,
        "swap-member",
        &json!({"member":"lead","session":{"name":"claude/x/lead2","run_id":"r9"}}),
        &Actor::Human,
        &none,
        &mut events,
    )
    .unwrap();
    assert_eq!((result["lead"].as_bool(), result["to"].as_str()), (Some(true), Some("claude/x/lead2")));
    assert_eq!(team.lead, "lead");
    let task = team.task(1).unwrap();
    let attempt = task.attempts.last().unwrap();
    assert_eq!((attempt.state, attempt.error.as_str()), (AttemptState::Revoked, "swapped"));
    // H6: nothing is handed out for the lead; its task waits in the inbox
    // and the new session gets one brief.
    assert!(task.continuation.is_none());
    assert_eq!(team.brief, Notice::Pending);
    assert_eq!(team.swapped.last().unwrap()["old"], "claude/x/lead");
    assert!(events.iter().any(|event| event.kind == "member_swapped" && event.payload["lead"] == true));
}

#[test]
fn a_subteam_is_approved_by_its_parent_lead_or_the_owner() {
    fn subteam(approval: SubteamApproval) -> Team {
        let mut team = Team {
            id: "child".into(),
            name: "Child".into(),
            lead: "dev".into(),
            status: TeamStatus::Planning,
            parent: Some(Parent { team: "t".into(), task: 1, lead: "lead".into(), member: "dev".into() }),
            ..Default::default()
        };
        team.settings.subteam_approval = approval;
        team.members.push(Member {
            id: "dev".into(),
            role: "Backend".into(),
            agent: "codex".into(),
            session: Some(SessionRef { name: "codex/x/dev".into(), run_id: "r1".into(), conversation_id: "c".into(), host: String::new() }),
            ..Default::default()
        });
        team.tasks.push(Task { id: 1, title: "Sub work".into(), assignee: "dev".into(), ..Default::default() });
        team
    }
    let mut events = Vec::new();
    let none = BTreeSet::new();
    // Section 9.3: the sub-lead cannot approve its own plan; the parent
    // lead can, without the owner's word.
    let mut child = subteam(SubteamApproval::Parent);
    let error = apply(&mut child, "approve", &json!({}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("parent lead approves"), "{error}");
    // C6: a sub-team member whose id is the parent lead's is not it.
    child.members.push(Member { id: "lead".into(), role: "Lead".into(), agent: "codex".into(), ..Default::default() });
    let error = apply(&mut child, "approve", &json!({}), &Actor::Member("lead".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("parent lead approves"), "{error}");
    child.members.retain(|member| member.id != "lead");
    apply(&mut child, "approve", &json!({}), &Actor::ParentLead, &none, &mut events).unwrap();
    assert_eq!(child.status, TeamStatus::Running);
    // The owner always approves, whatever the setting.
    let mut owner = subteam(SubteamApproval::Owner);
    apply(&mut owner, "approve", &json!({}), &Actor::Human, &none, &mut events).unwrap();
    let mut strict = subteam(SubteamApproval::Owner);
    let error = apply(&mut strict, "approve", &json!({}), &Actor::ParentLead, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("owner approves"), "{error}");
    // A phase of a sub-team: the parent lead approves it without the
    // owner's word; the sub-lead cannot.
    let mut phases = subteam(SubteamApproval::Parent);
    apply(&mut phases, "approve", &json!({}), &Actor::ParentLead, &none, &mut events).unwrap();
    apply(&mut phases, "cancel", &json!({"task":1}), &Actor::Human, &none, &mut events).unwrap();
    extend(
        &mut phases,
        &json!({"tasks":[{"id":2,"title":"Next","assignee":"dev"}]}),
        &Actor::Member("dev".into()),
        &none,
        &mut events,
    )
    .unwrap();
    let phase = phases.phases.last().unwrap().n;
    let error = apply(&mut phases, "approve-phase", &json!({"phase":phase}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("parent lead approves"), "{error}");
    apply(&mut phases, "approve-phase", &json!({"phase":phase}), &Actor::ParentLead, &none, &mut events).unwrap();
    // A flat team is unchanged: its own lead still needs the owner's word.
    let mut flat = subteam(SubteamApproval::Parent);
    flat.parent = None;
    let error = apply(&mut flat, "approve", &json!({}), &Actor::ParentLead, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("owner's word"), "{error}");
}

#[test]
fn tree_limits_bound_depth_children_and_sessions() {
    fn with_session(id: &str, parent: Option<(&str, u32)>, sessions: usize) -> Team {
        let mut team = Team { id: id.into(), name: id.into(), lead: "lead".into(), ..Default::default() };
        for index in 0..sessions {
            team.members.push(Member {
                id: format!("m{index}"),
                role: "Backend".into(),
                agent: "codex".into(),
                session: Some(SessionRef {
                    name: format!("codex/x/{id}-{index}"),
                    run_id: "r".into(),
                    conversation_id: "c".into(),
                    host: String::new(),
                }),
                ..Default::default()
            });
        }
        if let Some((parent, task)) = parent {
            team.parent = Some(Parent { team: parent.into(), task, lead: "lead".into(), member: "m0".into() });
        }
        team
    }
    let mut master = with_session("m", None, 0);
    master.settings.tree_depth = 2;
    master.settings.tree_sessions = 2;
    let a = with_session("a", Some(("m", 1)), 1);
    let b = with_session("b", Some(("a", 1)), 1);
    let all = vec![master.clone(), a.clone(), b.clone()];
    assert_eq!((depth_of(&a, &all), depth_of(&b, &all)), (1, 2));
    assert_eq!(root_of(&b, &all).id, "m");
    assert_eq!(tree_session_count(&master, &all), 2);
    // A sub-team of b would be the third level below the master.
    let c = with_session("c", Some(("b", 1)), 0);
    let error = ensure_tree_create(&b, &master, &all, &c).unwrap_err().to_string();
    assert!(error.contains("at most 2 levels"), "{error}");
    // A new session beyond the tree budget is refused.
    let d = with_session("d", Some(("m", 2)), 1);
    let error = ensure_tree_create(&master, &master, &all, &d).unwrap_err().to_string();
    assert!(error.contains("at most 2 sessions"), "{error}");
    // add-member and swap-member ask ensure_tree_sessions for the addition.
    assert!(ensure_tree_sessions(&a, &all, 1).is_err());
    assert!(ensure_tree_sessions(&a, &all, 0).is_ok());
    // At most 32 sub-teams.
    let mut full = master.clone();
    full.children = (0..32).map(|index| format!("s{index}")).collect();
    let error = ensure_tree_create(&full, &master, &all, &with_session("e", Some(("m", 3)), 0))
        .unwrap_err()
        .to_string();
    assert!(error.contains("at most 32 sub-teams"), "{error}");
    // Creating a sub-team in the parent task's place records the link in
    // the child and the parent keeps the child id (section 9.1).
    let (mut link, _) = create(
        "child",
        &json!({"name":"Child","lead":{"id":"dev","role":"Backend","agent":"codex"},
            "parent":{"team":"m","task":1,"lead":"lead","member":"dev"}}),
    )
    .unwrap();
    link.children.push("grand".into());
    assert_eq!(link.parent.as_ref().map(|parent| (parent.team.as_str(), parent.task)), Some(("m", 1)));
    assert_eq!(link.children, vec!["grand".to_owned()]);
}

#[test]
fn a_finished_subteam_files_its_summary_and_a_reject_reopens_it() {
    let mut parent = running_team();
    let mut child = Team {
        id: "child".into(),
        name: "Child".into(),
        lead: "dev".into(),
        status: TeamStatus::Done,
        parent: Some(Parent { team: parent.id.clone(), task: 1, lead: "lead".into(), member: "dev".into() }),
        finish: Some(Finish { by: "Lead".into(), summary: "Sub work is done".into(), at: 10.0 }),
        ..Default::default()
    };
    let task = file_subteam_report(&child, &mut parent).unwrap();
    assert_eq!(task, 1);
    let parent_task = parent.task(1).unwrap();
    assert_eq!(parent_task.status, TaskStatus::Submitted);
    let report = parent_task.attempts.last().unwrap().report.as_ref().unwrap();
    assert!(report.text.contains("[Sub-team child Child]") && report.text.contains("Sub work is done"), "{}", report.text);
    assert_eq!(parent_task.attempts.last().unwrap().member, "dev");
    // N22: the filed report waits for the parent lead; it is not open.
    assert_eq!(parent_task.attempts.last().unwrap().state, AttemptState::Reported);
    // N23: a replay files no second report.
    let before = parent_task.attempts.len();
    file_subteam_report(&child, &mut parent).unwrap();
    assert_eq!(parent.task(1).unwrap().attempts.len(), before);
    // C5: the parent lead rejected the first report (not final) and the
    // sub-team reworked and finished again: the new summary is filed
    // instead of the replay check swallowing it.
    {
        let task = parent.task_mut(1).unwrap();
        task.attempts.last_mut().unwrap().state = AttemptState::Rejected;
        task.status = TaskStatus::InProgress;
    }
    let mut reworked = child.clone();
    reworked.finish = Some(Finish { by: "Lead".into(), summary: "The migration is fixed".into(), at: 20.0 });
    file_subteam_report(&reworked, &mut parent).unwrap();
    let task = parent.task(1).unwrap();
    assert_eq!((task.attempts.len(), task.status), (before + 1, TaskStatus::Submitted));
    assert!(task.attempts.last().unwrap().report.as_ref().unwrap().text.contains("The migration is fixed"));
    assert_eq!(task.attempts.last().unwrap().state, AttemptState::Reported);
    // Section 9.5: the rejection reopens the sub-team with a Rework draft
    // that carries the reason for its lead.
    let phase = reopen_subteam(&mut child, "Fix the migration").unwrap();
    assert_eq!(child.status, TeamStatus::Running);
    assert!(child.finish.is_none());
    let draft = child.phases.iter().find(|p| p.n == phase).unwrap();
    assert_eq!((draft.title.as_str(), draft.reason.as_str(), draft.state), ("Rework", "Fix the migration", PhaseState::Draft));
    // N24: a stopped sub-team stays stopped on a reject.
    child.status = TeamStatus::Stopped;
    assert!(reopen_subteam(&mut child, "again").is_err());
    // A flat team cannot be reopened.
    assert!(reopen_subteam(&mut Team::default(), "x").is_err());
}

#[test]
fn a_draft_members_update_keeps_the_plans_member_checks() {
    // C3: unique ids, the auto folder default and the widening the owner
    // confirms before a create that runs the team.
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    team.status = TeamStatus::Draft;
    team.draft = Some(Draft { revision: 1, ..Default::default() });
    team.settings.folder_policy = FolderPolicy::Auto;
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let twice = json!({"action":"update","revision":1,"set":{"members":[{"id":"dev","role":"A","agent":"dsh"},{"id":"dev","role":"B","agent":"dsh"}]}});
    let error = apply(&mut team, "draft", &twice, &Actor::Member("lead".into()), &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("used twice"), "{error}");
    let wide = json!({"action":"update","revision":1,"set":{"members":[{"id":"dev","role":"Backend","agent":"dsh","permissions_profile":"network"}],
        "tasks":[{"id":1,"title":"A","assignee":"dev"}]}});
    apply(&mut team, "draft", &wide, &Actor::Member("lead".into()), &none, &mut events).unwrap();
    assert_eq!(team.member("dev").unwrap().folder, MemberFolder::Clone);
    assert_eq!(team.profile_widening, vec![json!({"member":"dev","from":"write","to":"network"})]);
    let error = apply(&mut team, "draft", &json!({"action":"create","approve":true}), &Actor::Human, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("confirm_widening"), "{error}");
    apply(&mut team, "draft", &json!({"action":"create","approve":true,"confirm_widening":true}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!(team.status, TeamStatus::Running);
}

/// Owner feedback 2026-11-04 (14): a swap to a fresh session remembers the
/// session it replaced, so the dispatcher sets it aside before the new
/// session takes the member's name.
#[test]
fn a_swap_to_a_fresh_session_remembers_the_one_it_replaced() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let old = SessionRef { host: String::new(), name: "codex/x/dev".into(), run_id: "r1".into(), conversation_id: "c1".into() };
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session = Some(old.clone());
    apply(&mut team, "swap-member", &json!({"member":"dev","account":"additional"}), &Actor::Human, &none, &mut Vec::new()).unwrap();
    let member = team.member("dev").unwrap();
    assert_eq!(member.session, None);
    assert!(member.launch_after_swap);
    assert_eq!(member.replaced.as_ref(), Some(&old));
    assert_eq!(member.account, "additional");
}

/// Owner feedback 2026-11-04 (12, 18): a swap card names what changes; an
/// unknown field or an empty swap is refused, and one card swaps several
/// members.
#[test]
fn a_swap_card_names_what_changes_and_may_swap_several_members() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let lead = Actor::Member("lead".into());
    let propose = |team: &mut Team, input: Value| apply(team, "propose", &input, &lead, &none, &mut Vec::new());
    let error = propose(&mut team, json!({"kind":"swap-member","reason":"r","member":"dev","with":{"account":"additional"}}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no field \"with\"") && error.contains("account"), "{error}");
    let error = propose(&mut team, json!({"kind":"swap-member","reason":"r","member":"dev"})).unwrap_err().to_string();
    assert!(error.contains("changes nothing") && error.contains("\"fresh\": true"), "{error}");
    let error = propose(&mut team, json!({"kind":"swap-member","reason":"r","members":[{"id":"dev","acount":"x"}]}))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no field \"acount\""), "{error}");
    assert!(team.proposals.is_empty(), "nothing was proposed");
    // A fresh session with the same settings is said explicitly.
    propose(&mut team, json!({"kind":"swap-member","reason":"r","member":"dev","fresh":true})).unwrap();
    // One card for several members, each with its own change and prompt.
    team.members.push(Member { id: "qa".into(), role: "QA".into(), agent: "claude".into(), ..Default::default() });
    propose(
        &mut team,
        json!({"kind":"swap-member","reason":"move to additional","members":[
            {"id":"dev","account":"additional"},{"id":"qa","account":"additional","prompt":"Test the menu"}]}),
    )
    .unwrap();
    let card = team.proposals.last().unwrap();
    assert_eq!(card["change"]["members"].as_array().map(Vec::len), Some(2), "{card}");
    assert_eq!(card["member"]["id"], "dev");
    assert_eq!(card["change"]["members"][1]["prompt"], "Test the menu");
}

/// Feedback 12: a swap's prompt changes the member's prompt.
#[test]
fn a_swap_changes_the_role_prompt() {
    let mut team = running_team();
    apply(&mut team, "swap-member", &json!({"member":"dev","prompt":"Review only"}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert_eq!(team.member("dev").unwrap().prompt, "Review only");
}

/// Owner feedback 2026-11-04 (6, 7): a draft changes one member without
/// resending the list, and takes the plan mode.
#[test]
fn a_draft_changes_one_member_and_sets_the_plan_mode() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    team.status = TeamStatus::Draft;
    team.draft = Some(Draft { revision: 1, ..Default::default() });
    let none = BTreeSet::new();
    let lead = Actor::Member("lead".into());
    let update = |team: &mut Team, revision: u64, set: Value, actor: &Actor| {
        apply(team, "draft", &json!({"action":"update","revision":revision,"set":set}), actor, &none, &mut Vec::new())
    };
    update(&mut team, 1, json!({"members":[{"id":"dev","role":"Backend","agent":"claude"},{"id":"qa","role":"QA","agent":"claude"}]}), &lead).unwrap();
    // One member's model, a new member and a removal, by id.
    update(&mut team, 2, json!({"members":{"dev":{"model":"opus","account":"additional"},"ux":{"role":"UX","agent":"kimi"},"qa":null}}), &lead).unwrap();
    let ids: Vec<&str> = team.members.iter().map(|member| member.id.as_str()).collect();
    assert_eq!(ids, ["lead", "dev", "ux"]);
    let dev = team.member("dev").unwrap();
    assert_eq!((dev.role.as_str(), dev.model.as_str(), dev.account.as_str()), ("Backend", "opus", "additional"));
    let error = update(&mut team, 3, json!({"members":{"qa":null}}), &lead).unwrap_err().to_string();
    assert!(error.contains("no member qa"), "{error}");
    let error = update(&mut team, 3, json!({"members":{"dev":{"text":"x"}}}), &lead).unwrap_err().to_string();
    assert!(error.contains("no field \"text\""), "{error}");
    // The lead offers a plan mode; the owner chooses it.
    update(&mut team, 3, json!({"plan_mode":"free","plan_mode_reason":"one-off"}), &lead).unwrap();
    assert_eq!(team.plan_mode_offer, Some(PlanMode::Free));
    update(&mut team, 4, json!({"plan_mode":"free"}), &Actor::Human).unwrap();
    assert_eq!((team.plan_mode, team.plan_mode_offer), (PlanMode::Free, None));
}

/// Task #18 (owner, 2026-11-05): the New team form's first save is accepted as
/// it sends it (the form's four settings only, its members and template tasks);
/// a draft over its member limit is a preflight error and its create is refused
/// naming the limit, never "Preflight: ok" beside a create that does nothing.
#[test]
fn a_draft_over_its_member_limit_is_refused_at_create_and_by_the_preflight() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    team.status = TeamStatus::Draft;
    team.draft = Some(Draft { revision: 1, ..Default::default() });
    let none = BTreeSet::new();
    let members: Vec<Value> = (1..=3)
        .map(|n| {
            json!({"id":format!("dev-{n}"),"role":"Backend","agent":"claude","account":"","model":"","effort":"","permissions":"",
                   "computer":"","folder":"shared","prompt":"Build it."})
        })
        .collect();
    let set = json!({"goal":"Ship","addendum":"","members":members,
        "tasks":[{"id":1,"title":"Plan","assignee":"lead","deps":[]},{"id":2,"title":"Build","assignee":"dev-1","deps":[{"task":1,"kind":"hard"}]}],
        "settings":{"folder_policy":"auto","approval":"review","phase_approval":"auto","max_members":2}});
    apply(&mut team, "draft", &json!({"action":"update","revision":1,"set":set}), &Actor::Human, &none, &mut Vec::new()).unwrap();
    let validation = plan::validate(&team, &none);
    assert!(
        validation.errors.iter().any(|finding| finding.kind == "members" && finding.text.contains("over the team limit of 2")),
        "{:?}",
        validation.errors
    );
    assert!(validation.warnings.iter().all(|finding| finding.kind != "members"), "{:?}", validation.warnings);
    let error = apply(&mut team, "draft", &json!({"action":"create"}), &Actor::Human, &none, &mut Vec::new()).unwrap_err().to_string();
    assert!(error.contains("over the team limit of 2"), "{error}");
    assert_eq!(team.status, TeamStatus::Draft);
    // One member fewer: the same draft creates.
    apply(&mut team, "draft", &json!({"action":"update","revision":2,"set":{"members":{"dev-3":null}}}), &Actor::Human, &none, &mut Vec::new()).unwrap();
    let value = apply(&mut team, "draft", &json!({"action":"create"}), &Actor::Human, &none, &mut Vec::new()).unwrap();
    assert_eq!(value["status"], "planning", "{value}");
}

/// Task #26: team 33fbafeb's lead drafted the catalog's Reviewer with its
/// role's folder, `read_only`, and the draft was refused with "A member is
/// invalid" and an example, naming no field; the reviewer had to be made
/// shared. `read_only` is the shared folder with no own checkout, and a value
/// the draft refuses names its member and its field.
#[test]
fn a_draft_member_takes_the_catalogs_read_only_folder_and_a_refusal_names_the_field() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    team.status = TeamStatus::Draft;
    team.draft = Some(Draft { revision: 1, ..Default::default() });
    team.settings.folder_policy = FolderPolicy::Auto;
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let lead = Actor::Member("lead".into());
    let reviewer = json!({"action":"update","revision":1,"set":{"members":[
        {"id":"merger","role":"Reviewer & Merger","agent":"claude","model":"claude-sonnet-5-5","folder":"read_only"}]}});
    apply(&mut team, "draft", &reviewer, &lead, &none, &mut events).unwrap();
    assert_eq!(team.member("merger").unwrap().folder, MemberFolder::Shared, "no own checkout");
    // Written back, the folder reads as the shared folder it is.
    assert_eq!(serde_json::to_value(team.member("merger").unwrap()).unwrap()["folder"], "shared");
    // A folder the catalog does not know names the member and the field.
    let revision = team.draft.as_ref().unwrap().revision;
    let unknown = json!({"action":"update","revision":revision,"set":{"members":[
        {"id":"merger","role":"Reviewer","agent":"claude","folder":"readonly"}]}});
    let error = apply(&mut team, "draft", &unknown, &lead, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("A member \"merger\" is invalid: field \"folder\": unknown variant `readonly`"), "{error}");
    assert!(error.contains("`read_only`") || error.contains("`shared`"), "the accepted values are named: {error}");
    // So does a task field.
    let task = json!({"action":"update","revision":revision,"set":{"tasks":[{"id":1,"title":"Review","assignee":"merger","deps":"1"}]}});
    let error = apply(&mut team, "draft", &task, &lead, &none, &mut events).unwrap_err().to_string();
    assert!(error.contains("A task \"1\" is invalid: field \"deps\""), "{error}");
}

/// Task #6: a swap to another session drops the changes pending for the
/// session it replaced.
#[test]
fn a_swap_to_a_new_session_drops_the_pending_changes() {
    let mut team = running_team();
    let mut events = Vec::new();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().session =
        Some(SessionRef { name: "codex/x/dev".into(), run_id: "r1".into(), ..Default::default() });
    apply(
        &mut team,
        "settings",
        &json!({"members_update":[{"id":"dev","model":"gpt-6"},{"id":"dev","prompt":"Ship it"}]}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert!(team.member("dev").unwrap().model_switch.is_some() && team.member("dev").unwrap().prompt_change.is_some());
    apply(&mut team, "swap-member", &json!({"member":"dev","fresh":true}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    let dev = team.member("dev").unwrap();
    assert!(dev.model_switch.is_none() && dev.prompt_change.is_none(), "the replaced session's changes do not follow the swap: {dev:?}");
}

/// Task #6: a swap to an agent that lacks the member's permissions mode is
/// refused (Claude's auto is no mode for Codex).
#[test]
fn a_swap_to_an_agent_without_the_permissions_mode_is_refused() {
    let mut team = running_team();
    let mut events = Vec::new();
    apply(
        &mut team,
        "add-member",
        &json!({"member":{"id":"rev","role":"Reviewer","agent":"claude","permissions":"auto"}}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(team.member("rev").unwrap().permissions, "auto");
    let error = apply(&mut team.clone(), "swap-member", &json!({"member":"rev","agent":"codex"}), &Actor::Human, &BTreeSet::new(), &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Member \"rev\": field \"permissions\": Codex has no \"auto\" permissions mode; it takes bypass");
    assert_eq!(team.member("rev").unwrap().agent, "claude", "the refused swap changed nothing");
}

/// Task #13 (team ede724c4, owner 2026-11-10): the lead's draft values for the
/// owner's settings (base_branch, branch_per_task, max_members) are offers,
/// never a refusal: the owner's values stay, the offers wait for one click,
/// and an applied or otherwise-set field drops its offer.
#[test]
fn the_leads_owner_settings_in_a_draft_are_offers() {
    let (mut team, _) = create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
    team.status = TeamStatus::Draft;
    team.draft = Some(Draft { revision: 1, ..Default::default() });
    let none = BTreeSet::new();
    let lead = Actor::Member("lead".into());
    let mut settings = serde_json::to_value(&team.settings).unwrap();
    settings["base_branch"] = json!("main");
    settings["branch_per_task"] = json!({"template":"task/{id}"});
    settings["max_members"] = json!(9);
    settings["restart_crashed"] = json!(false);
    settings["folder_policy"] = json!("worktree");
    let before = team.settings.clone();
    let receipt = apply(&mut team, "draft", &json!({"action":"update","revision":1,"set":{"settings":settings}}), &lead, &none, &mut Vec::new()).unwrap();
    assert_eq!(receipt["status"], "updated", "{receipt}");
    assert!(receipt["note"].as_str().unwrap().contains("base_branch"), "{receipt}");
    // The owner's settings stay; the lead's own field (folder_policy) is taken.
    assert_eq!((team.settings.base_branch.as_str(), team.settings.max_members), (before.base_branch.as_str(), before.max_members));
    assert_eq!(team.settings.branch_per_task, before.branch_per_task);
    assert_eq!(team.settings.folder_policy, FolderPolicy::Worktree);
    let offers = team.draft.as_ref().unwrap().setting_offers.clone();
    assert_eq!(offers.get("base_branch"), Some(&json!("main")));
    assert_eq!(offers.get("branch_per_task"), Some(&json!({"template":"task/{id}"})));
    assert_eq!(offers.get("max_members"), Some(&json!(9)));
    assert!(team.draft.as_ref().unwrap().unseen.contains(&"settings".to_owned()), "the field is marked for the owner");
    // The owner changes something else: the offers stay.
    let mut owner = serde_json::to_value(&team.settings).unwrap();
    owner["approval"] = json!("auto");
    apply(&mut team, "draft", &json!({"action":"update","revision":2,"set":{"settings":owner.clone()}}), &Actor::Human, &none, &mut Vec::new()).unwrap();
    assert_eq!(team.draft.as_ref().unwrap().setting_offers.len(), 4);
    // Apply (one click): base_branch and max_members as offered; branch_per_task set otherwise.
    owner["base_branch"] = json!("main");
    owner["max_members"] = json!(9);
    owner["branch_per_task"] = json!({"template":"work/{id}"});
    let receipt = apply(&mut team, "draft", &json!({"action":"update","revision":3,"set":{"settings":owner}}), &Actor::Human, &none, &mut Vec::new()).unwrap();
    assert_eq!((team.settings.base_branch.as_str(), team.settings.max_members), ("main", 9));
    let left: Vec<String> = team.draft.as_ref().unwrap().setting_offers.keys().cloned().collect();
    assert_eq!(left, ["restart_crashed"], "{receipt}");
}

/// Task #13: a member whose session failed to start is relaunched by the lead
/// itself; a running member still needs the owner's card.
#[test]
fn the_lead_relaunches_a_member_that_failed_to_start() {
    let mut team = running_team();
    let none = BTreeSet::new();
    let lead = Actor::Member(team.lead.clone());
    let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.session = None;
    dev.state = MemberState::Active;
    dev.error = "Another worktree operation is running. Try again after it finishes.".into();
    let value = apply(&mut team, "relaunch", &json!({"member":"dev"}), &lead, &none, &mut Vec::new()).unwrap();
    assert_eq!(value["status"], "relaunch", "{value}");
    let qa = team.members.iter_mut().find(|member| member.id == "qa").unwrap();
    qa.session = Some(SessionRef { name: "claude/x/qa".into(), run_id: "r9".into(), ..Default::default() });
    qa.error.clear();
    let error = apply(&mut team, "relaunch", &json!({"member":"qa"}), &lead, &none, &mut Vec::new()).unwrap_err().to_string();
    assert!(error.contains("proposes"), "{error}");
}
