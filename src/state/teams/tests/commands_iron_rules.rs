use super::support::*;
use super::*;
use crate::state::iron_rules;

#[test]
fn session_rules_require_exact_identity_and_replay_one_write() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let name = "claude/example/one";
        crate::state::storage::atomic(
            &crate::state::storage::record_path(name),
            &json!({"version":1,"agent":"claude",
            "name":name,"run_id":"r1","conversation_id":"c1"})
            .to_string(),
        )
        .unwrap();
        let mut input = json!({"by":"human","scope":"session","name":name,"expected_run_id":"r1","expected_conversation_id":"c1",
            "request_id":"11111111-1111-4111-8111-111111111111","expected_revision":0,"rules":["Session requirement"]});
        let saved = iron_rules::run("set", &input).unwrap();
        assert_eq!(iron_rules::run("set", &input).unwrap(), saved);
        input["expected_run_id"] = json!("foreign");
        input["request_id"] = json!("22222222-2222-4222-8222-222222222222");
        assert!(iron_rules::run("set", &input).is_err());
        assert_eq!(
            crate::state::with_session_project_binding(name, None, "r1", "c1", |record| Ok(
                record["iron_rules"].clone()
            ))
            .unwrap(),
            json!(["Session requirement"])
        );
    });
}

#[test]
fn project_rules_migrate_once_and_never_replace_an_owners_edits() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut input = json!({"by":"human","scope":"project","project":"p1","expected_revision":0,
            "request_id":"11111111-1111-4111-8111-111111111111","rules":["Keep existing rules"]});
        let first = iron_rules::run("migrate", &input).unwrap();
        assert_eq!(first["rules"], json!(["Keep existing rules"]));
        assert_eq!(iron_rules::run("migrate", &input).unwrap(), first);
        input["request_id"] = json!("22222222-2222-4222-8222-222222222222");
        input["expected_revision"] = first["revision"].clone();
        input["rules"] = json!([]);
        iron_rules::run("set", &input).unwrap();
        input["request_id"] = json!("33333333-3333-4333-8333-333333333333");
        input["rules"] = json!(["Keep existing rules"]);
        let kept = iron_rules::run("migrate", &input).unwrap();
        assert_eq!(
            kept["rules"],
            json!([]),
            "a deliberate empty list is authoritative"
        );
        set_test_caller_identities(Some((Some("agent-session".into()), None, None)));
        assert!(iron_rules::run("set", &input).is_err());
    });
}

#[test]
fn a_task_carries_all_four_iron_rule_levels_even_when_the_prompt_hides_rules() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        iron_rules::run(
            "set",
            &json!({"by":"human","scope":"project","project":"p1","expected_revision":0,
            "request_id":"11111111-1111-4111-8111-111111111111","rules":["Project requirement"]}),
        )
        .unwrap();
        let (mut team, _, _) = review_team();
        team.project_id = "p1".into();
        team.settings.iron_rules = vec!["Team addition".into()];
        let member = team.members.iter_mut().find(|m| m.id == "dev").unwrap();
        member.session = Some(SessionRef {
            name: "codex/x/dev".into(),
            run_id: "r1".into(),
            conversation_id: "c-dev".into(),
            ..Default::default()
        });
        member.iron_rules = vec!["Member addition".into()];
        member.prompt_snapshot = Some(PromptSnapshot {
            prompt_order: vec!["role".into(), "rules".into()],
            prompt_off: vec!["rules".into()],
            ..Default::default()
        });
        let session = member.session.as_ref().unwrap();
        let name = session.name.clone();
        crate::state::storage::atomic(&crate::state::storage::record_path(&name), &json!({"version":1,"agent":"codex",
            "name":name,"run_id":session.run_id,"conversation_id":session.conversation_id,"iron_rules":["Session addition"]}).to_string()).unwrap();
        let task = team.task(1).unwrap();
        let attempt = task.attempts.last().unwrap();
        let text = super::super::dispatcher::hand_off_text(&team, task, attempt, "", "");
        let at: Vec<_> = [
            "Project requirement",
            "Team addition",
            "Member addition",
            "Session addition",
        ]
        .iter()
        .map(|rule| {
            assert_eq!(text.matches(rule).count(), 1, "{text}");
            text.find(rule).unwrap()
        })
        .collect();
        assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{text}");
    });
}

#[test]
fn a_lead_changes_iron_rules_only_on_the_owners_verified_word() {
    let dir = tempfile::tempdir().unwrap();
    let said = "Please change the iron rules for Checkout";
    owner_word_events(dir.path(), &[(crate::state::now() - 60.0, said)]);
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let (mut team, _, _) = review_team();
        team.name = "Checkout".into();
        team.members
            .iter_mut()
            .find(|m| m.id == "lead")
            .unwrap()
            .session
            .as_mut()
            .unwrap()
            .conversation_id = "c1".into();
        let id = team.id.clone();
        let mut store = Store::open(true).unwrap();
        store
            .create(
                "11111111-1111-4111-8111-111111111111",
                &json!({}),
                team,
                vec![],
            )
            .unwrap();
        set_test_caller_identities(Some((Some("c1".into()), None, None)));
        let mut input = json!({"request_id":"22222222-2222-4222-8222-222222222222","team":id,"settings":{"iron_rules":["Owner requirement"]}});
        assert!(run("settings", &input, false).is_err());
        input["request_id"] = json!("33333333-3333-4333-8333-333333333333");
        input["owner_word"] = json!({"text":said});
        // Task 162: the typed word is no longer taken; the iron-rules card is.
        let error = run("settings", &input, false).unwrap_err().to_string();
        assert!(error.contains("no longer taken for settings"), "{error}");
        assert!(Store::open(false).unwrap().load(&id).unwrap().unwrap().settings.iron_rules.is_empty());
        set_test_caller_identities(Some((None, None, None)));
        run(
            "settings",
            &json!({"request_id":"44444444-4444-4444-8444-444444444444","by":"human","team":id,
            "members_update":[{"id":"dev","iron_rules":["Member requirement"]}]}),
            false,
        )
        .unwrap();
        assert_eq!(
            Store::open(false)
                .unwrap()
                .load(&id)
                .unwrap()
                .unwrap()
                .member("dev")
                .unwrap()
                .iron_rules,
            vec!["Member requirement"]
        );
    });
}

#[test]
fn startup_proof_is_bound_to_the_current_native_identities() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let name = "claude/example/proof";
        let mut record = json!({"version":1,"agent":"claude","name":name,"run_id":"r1","conversation_id":"c1","iron_rules":["Own rule"]});
        let (_, text) = iron_rules::record_context(&record).unwrap();
        record["iron_rules_start_supported"] = json!(true);
        record["iron_rules_start_run"] = json!("r1"); record["iron_rules_start_conversation"] = json!("c1");
        record["iron_rules_start_digest"] = json!(super::super::store::digest(&json!(text)));
        for (run,conversation,valid) in [("r1","c1",true),("r2","c1",false),("r1","c2",false)] {
            record["run_id"] = json!(run); record["conversation_id"] = json!(conversation);
            crate::state::storage::atomic(&crate::state::storage::record_path(name),&record.to_string()).unwrap();
            let got = iron_rules::run("get",&json!({"scope":"session","name":name,"expected_run_id":run,"expected_conversation_id":conversation})).unwrap();
            assert_eq!(got["start_delivery_supported"],valid);
        }
    });
}

#[test]
fn stale_project_and_session_writes_do_not_consume_the_owners_word() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let name = "claude/example/stale";
        crate::state::storage::atomic(&crate::state::storage::record_path(name),&json!({"version":1,"agent":"claude","name":name,
            "run_id":"r1","conversation_id":"c1","iron_rules_revision":2}).to_string()).unwrap();
        // Without a team or owner_word, an agent would fail authorization if
        // the mutation reached it. Revision refusal must happen first.
        set_test_caller_identities(Some((Some("agent".into()),None,None)));
        let error = iron_rules::run("set",&json!({"scope":"session","name":name,"expected_run_id":"r1","expected_conversation_id":"c1",
            "expected_revision":0,"request_id":"11111111-1111-4111-8111-111111111111","rules":["New"]})).unwrap_err().to_string();
        assert!(error.contains("Session rules changed"),"{error}");
        set_test_caller_identities(Some((None,None,None)));
        iron_rules::run("set",&json!({"by":"human","scope":"project","project":"p1","expected_revision":0,
            "request_id":"22222222-2222-4222-8222-222222222222","rules":["Owner rule"]})).unwrap();
        set_test_caller_identities(Some((Some("agent".into()),None,None)));
        let error = iron_rules::run("set",&json!({"scope":"project","project":"p1","expected_revision":0,
            "request_id":"33333333-3333-4333-8333-333333333333","rules":["New"]})).unwrap_err().to_string();
        assert!(error.contains("Project rules changed"),"{error}");
    });
}

#[test]
fn the_owner_can_resolve_a_blocked_teams_project_identity() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let _worker = hold_worker_lock();
        let (mut team,_,_) = review_team(); team.members.iter_mut().find(|m|m.id=="lead").unwrap().session.as_mut().unwrap().conversation_id="c1".into(); let id=team.id.clone(); team.iron_rules_error="Ambiguous project label".into();
        let mut store=Store::open(true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111",&json!({}),team,vec![]).unwrap();
        let before=store.load(&id).unwrap().unwrap();
        assert!(super::super::inbox::items(&before).iter().any(|item|item.key=="iron_rules" && item.command.contains("project_id")));
        run("settings",&json!({"by":"human","team":id,"project_id":"p1","request_id":"22222222-2222-4222-8222-222222222222"}),false).unwrap();
        let after=Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert_eq!(after.project_id,"p1"); assert!(after.iron_rules_error.is_empty());
        set_test_caller_identities(Some((Some("c1".into()),None,None)));
        assert!(run("settings",&json!({"team":id,"project_id":"p2","request_id":"33333333-3333-4333-8333-333333333333"}),false).is_err());
    });
}

#[test]
fn remote_rules_membership_matches_alias_and_name_even_when_native_ids_collide() {
    let dir=tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(),|| {
        let (mut team,_,_)=review_team();
        for (id,host,name) in [("dev","peer-a","claude/x/one"),("other","peer-b","claude/x/one"),("third","peer-a","claude/x/two")] {
            let mut member=team.member("dev").unwrap().clone(); member.id=id.into(); member.computer=host.into();
            member.session=Some(SessionRef{host:String::new(),name:name.into(),run_id:"same-run".into(),conversation_id:"same-conversation".into()});
            if id=="dev" {*team.members.iter_mut().find(|m|m.id==id).unwrap()=member;} else {team.members.push(member);}
        }
        let mut store=Store::open(true).unwrap(); store.create("11111111-1111-4111-8111-111111111111",&json!({}),team,vec![]).unwrap();
        assert_eq!(store.rules_session("peer-a","claude/x/one","same-run","same-conversation","").unwrap().unwrap().1,"dev");
        assert_eq!(store.rules_session("peer-b","claude/x/one","same-run","same-conversation","").unwrap().unwrap().1,"other");
        assert_eq!(store.rules_session("peer-a","claude/x/two","same-run","same-conversation","").unwrap().unwrap().1,"third");
        assert!(store.rules_session("peer-c","claude/x/one","same-run","same-conversation","").unwrap().is_none());
    });
}

#[test]
fn removing_a_member_with_rules_needs_only_the_roster_word() {
    let dir=tempfile::tempdir().unwrap(); let said="Please remove m1 from the team";
    owner_word_events(dir.path(), &[(crate::state::now()-60.0,said)]);
    crate::state::with_test_state_root(dir.path(),|| {
        let _worker=hold_worker_lock(); let mut team=full_team(); let id=team.id.clone();
        team.members.iter_mut().find(|m|m.id=="m1").unwrap().iron_rules=vec!["Member rule".into()];
        Store::open(true).unwrap().create("11111111-1111-4111-8111-111111111111",&json!({}),team,vec![]).unwrap();
        set_test_caller_identities(Some((Some("c1".into()),None,None)));
        // Task 162: the typed word is no longer taken; the remove-member card is.
        let error=run("remove-member",&json!({"team":id,"member":"m1","owner_word":{"text":said},"request_id":"22222222-2222-4222-8222-222222222222"}),false).unwrap_err().to_string();
        assert!(error.contains("no longer taken for remove-member"),"{error}");
        let after=Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert!(after.member("m1").is_some_and(|m|m.state!=MemberState::Removed));
    });
}

#[test]
fn generic_rules_words_do_not_authorize_iron_rules() {
    let dir=tempfile::tempdir().unwrap(); let said="Please change the rules for T";
    owner_word_events(dir.path(), &[(crate::state::now()-60.0,said)]);
    crate::state::with_test_state_root(dir.path(),|| {
        let _worker=hold_worker_lock(); let team=full_team(); let id=team.id.clone();
        Store::open(true).unwrap().create("11111111-1111-4111-8111-111111111111",&json!({}),team,vec![]).unwrap();
        set_test_caller_identities(Some((Some("c1".into()),None,None)));
        assert!(run("settings",&json!({"team":id,"settings":{"iron_rules":["New rule"]},"owner_word":{"text":said},
            "request_id":"22222222-2222-4222-8222-222222222222"}),false).is_err());
    });
}

#[test]
fn failed_child_rules_authorization_releases_its_parent_reservation() {
    let dir=tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(),|| {
        let _worker=hold_worker_lock(); let mut parent=running_team(); let id=parent.id.clone();
        let lead=parent.members.iter_mut().find(|m|m.id==parent.lead).unwrap();
        lead.agent="dsh".into();lead.session=Some(SessionRef{name:"dsh/x/lead".into(),conversation_id:"c1".into(),..Default::default()});
        Store::open(true).unwrap().create("11111111-1111-4111-8111-111111111111",&json!({}),parent,vec![]).unwrap();
        set_test_caller_identities(Some((Some("c1".into()),None,None)));
        let error=run("create",&json!({"parent":{"team":id,"task":1},"name":"Child","settings":{"iron_rules":["Injected"]},
            "request_id":"22222222-2222-4222-8222-222222222222"}),false).unwrap_err().to_string();
        assert!(error.contains("iron rules") || error.contains("owner") || error.contains("member"),"{error}");
        let parent=Store::open(false).unwrap().load(&id).unwrap().unwrap();
        assert!(parent.task(1).unwrap().subteam.is_none(),"a failed policy gate cannot strand the reservation");
        let events=Store::open(false).unwrap().events(&id,None,100).unwrap();
        assert!(events["items"].as_array().unwrap().iter().any(|event|event["kind"]=="subteam_reserved"),"the test must pass the actual reservation entry point");
    });
}

#[test]
fn an_old_drafts_project_can_be_resolved_without_enabling_general_settings() {
    let dir=tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(),|| {
        let _worker=hold_worker_lock();let (mut team,_,_)=review_team();let id=team.id.clone();
        team.status=TeamStatus::Draft;team.project="Ambiguous label".into();
        Store::open(true).unwrap().create("11111111-1111-4111-8111-111111111111",&json!({}),team,vec![]).unwrap();
        run("settings",&json!({"by":"human","team":id,"project_id":"p1","request_id":"22222222-2222-4222-8222-222222222222"}),false).unwrap();
        assert_eq!(Store::open(false).unwrap().load(&id).unwrap().unwrap().project_id,"p1");
        assert!(run("settings",&json!({"by":"human","team":id,"project_id":"p1","settings":{"iron_rules":[]},
            "request_id":"33333333-3333-4333-8333-333333333333"}),false).is_err());
    });
}
