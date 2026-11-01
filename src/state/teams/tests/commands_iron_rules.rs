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
            crate::state::with_session_project_binding(name, |record| Ok(
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
        run("settings", &input, false).unwrap();
        assert_eq!(
            Store::open(false)
                .unwrap()
                .load(&id)
                .unwrap()
                .unwrap()
                .settings
                .iron_rules,
            vec!["Owner requirement"]
        );
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
