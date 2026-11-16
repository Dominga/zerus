//! Live-team edits through the actual operation/receipt/journal writers.
use super::*;
use zerus_team_plan::catalog::AccountChoice;

fn add() -> Operation {
    Operation::AddMember {
        role: "developer".into(),
        agent: "codex".into(),
        model: "example-model".into(),
        effort: "high".into(),
        account: AccountChoice::Named {
            account: "example-account".into(),
        },
    }
}
#[test]
fn adding_a_member_keeps_the_catalog_and_existing_member_bytes() {
    let mut h = Harness::new();
    let before = h.store.members(TEAM, 200, None).unwrap().rows;
    let bundle = h.store.team(TEAM).unwrap().unwrap().bundle;
    let reply = h.run(&person(), add()).unwrap();
    assert_eq!(reply.member.as_deref(), Some("m4"));
    let after = h.store.members(TEAM, 200, None).unwrap().rows;
    assert_eq!(&after[..before.len()], before);
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().bundle, bundle);
    let added = h.store.member(TEAM, "m4").unwrap().unwrap();
    assert_eq!(added.role, "developer");
    assert_eq!(added.agent, "codex");
    let launch = added.launch.unwrap();
    assert_eq!(launch.model, "example-model");
    assert_eq!(launch.effort, "high");
    assert_eq!(
        launch.account,
        AccountChoice::Named {
            account: "example-account".into()
        }
    );
    assert!(added.session.is_none());
    let journal = h.store.journal(TEAM, 200, None).unwrap().rows;
    assert!(journal
        .iter()
        .any(|j| j.kind == "add_member" && j.refs.contains(&"member:m4".to_owned())));
}
#[test]
fn only_the_person_edits_and_stale_or_replayed_requests_keep_identity() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    for actor in [lead, other(), service()] {
        for op in [
            add(),
            Operation::RenameTeam {
                name: "Renamed".into(),
            },
            Operation::SetGoal {
                goal: "New goal".into(),
            },
        ] {
            refused(h.run(&actor, op), code::NOT_ALLOWED);
        }
    }
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    let r = request(revision, add());
    let first = super::apply_fixture(&mut h.store, &person(), &r, 1234).unwrap();
    let replay = super::apply_fixture(&mut h.store, &person(), &r, 2234).unwrap();
    assert_eq!(first, replay);
    refused(
        super::apply_fixture(&mut h.store, &person(), &request(revision, add()), 3234),
        code::STALE_REVISION,
    );
    assert_eq!(h.store.members(TEAM, 200, None).unwrap().rows.len(), 5);
}
#[test]
fn rename_and_goal_change_display_metadata_only_and_validate_names() {
    let mut h = Harness::new();
    h.bind("lead");
    let members = h.store.members(TEAM, 200, None).unwrap().rows;
    let old = h.store.team(TEAM).unwrap().unwrap();
    h.run(
        &person(),
        Operation::RenameTeam {
            name: "Команда".into(),
        },
    )
    .unwrap();
    h.run(
        &person(),
        Operation::SetGoal {
            goal: "A goal\nwith a second line".into(),
        },
    )
    .unwrap();
    let now = h.store.team(TEAM).unwrap().unwrap();
    assert_eq!(now.id, old.id);
    assert_eq!(now.integration_branch, old.integration_branch);
    assert_eq!(now.bundle, old.bundle);
    assert_eq!(now.name, "Команда");
    assert_eq!(now.goal, "A goal\nwith a second line");
    assert_eq!(h.store.members(TEAM, 200, None).unwrap().rows, members);
    for name in ["", "  ", "bad\nname", "bad\u{202e}name"] {
        assert!(h
            .run(&person(), Operation::RenameTeam { name: name.into() })
            .is_err());
    }
    h.run(
        &person(),
        Operation::SetGoal {
            goal: String::new(),
        },
    )
    .unwrap();
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().goal, "");
}
#[test]
fn additions_enforce_role_agent_and_roster_limits_without_reusing_ids() {
    let mut h = Harness::new();
    let mut unknown = add();
    if let Operation::AddMember { role, .. } = &mut unknown {
        *role = "missing".into();
    }
    refused(h.run(&person(), unknown), code::UNKNOWN_ROLE);
    let mut unknown = add();
    if let Operation::AddMember { agent, .. } = &mut unknown {
        *agent = "opencode".into();
    }
    assert!(h.run(&person(), unknown).is_err());
    h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    assert_eq!(
        h.run(&person(), add()).unwrap().member.as_deref(),
        Some("m4")
    );
    h.settings(|s| s.caps.max_members = 3);
    refused(h.run(&person(), add()), code::OVER_CAP);
}

#[test]
fn unsupported_native_choices_refuse_before_creating_a_member() {
    let mut h = Harness::new();
    let before = h.store.members(TEAM, 200, None).unwrap().rows;
    for (agent, model, effort) in [
        ("dsh", "example-model", ""),
        ("dsh", "", "high"),
        ("kimi", "", "high"),
    ] {
        let result = h.run(
            &person(),
            Operation::AddMember {
                role: "developer".into(),
                agent: agent.into(),
                model: model.into(),
                effort: effort.into(),
                account: AccountChoice::Freest,
            },
        );
        assert_eq!(result.unwrap_err().code, code::INVALID_STATE);
        assert_eq!(h.store.members(TEAM, 200, None).unwrap().rows, before);
    }
}
