use super::*;
#[test]
fn member_lifecycle_and_observed_state_have_guards() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.run(
        &lead,
        Operation::PauseMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    h.run(
        &lead,
        Operation::ResumeMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    h.run(&service(), observed_state(MemberState::Paused))
        .unwrap();
    refused(
        h.run(&service(), observed_state(MemberState::Paused)),
        code::INVALID_STATE,
    );
    h.run(&service(), observed_state(MemberState::Active))
        .unwrap();
    h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    refused(
        h.run(
            &lead,
            Operation::ResumeMember {
                member: "m1".into(),
            },
        ),
        code::INVALID_STATE,
    );
}
#[test]
fn service_and_lead_cannot_reactivate_a_person_stopped_member() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    h.run(
        &person(),
        Operation::StopMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    refused(
        h.run(&service(), observed_state(MemberState::Active)),
        code::INVALID_STATE,
    );
    refused(
        h.run(
            &lead,
            Operation::ResumeMember {
                member: "m1".into(),
            },
        ),
        code::INVALID_STATE,
    );
    h.run(
        &person(),
        Operation::ResumeMember {
            member: "m1".into(),
        },
    )
    .unwrap();
}
#[test]
fn bindings_are_unique_immutable_and_use_identity_not_names() {
    let mut h = Harness::new();
    h.bind("m1");
    let m = fixture().members[1].clone();
    refused(
        h.run(
            &service(),
            Operation::SetMemberSession {
                member: "m1".into(),
                session: m.session.unwrap(),
                account: m.account,
                worktree: m.worktree,
                ownership_nonce: m.ownership_nonce,
            },
        ),
        code::INVALID_STATE,
    );
    let m = fixture().members[2].clone();
    let mut session = m.session.unwrap();
    session.session_id = "run_id:legacy-example".into();
    h.run(
        &service(),
        Operation::SetMemberSession {
            member: "m2".into(),
            session,
            account: m.account,
            worktree: m.worktree,
            ownership_nonce: m.ownership_nonce,
        },
    )
    .unwrap();
}

fn observed_state(state: MemberState) -> Operation {
    Operation::SetMemberState {
        member: "m1".into(),
        state,
    }
}

const HANDLE: &str = "00000000-0000-4000-8000-00000000c0de";
const HINT: &str = "codex/example/team-member";

fn handle(member: &str, session_id: &str) -> Operation {
    Operation::SetMemberHandle {
        member: member.into(),
        session_id: session_id.into(),
        label_hint: HINT.into(),
    }
}

/// Binds `member` of `team` to launch `launch` on the fixture's home.
fn bind_in(h: &mut Harness, team: &str, member: &str, launch: u32) {
    let m = fixture()
        .members
        .into_iter()
        .find(|m| m.id == member)
        .unwrap();
    let mut session = m.session.unwrap();
    session.session_id = format!("launch_id:00000000-0000-4000-8000-{launch:012}");
    h.run_for(
        team,
        &Actor::Service {
            person: "local".into(),
            team: team.into(),
        },
        Operation::SetMemberSession {
            member: member.into(),
            session,
            account: m.account,
            worktree: m.worktree,
            ownership_nonce: m.ownership_nonce,
        },
    )
    .unwrap();
}

fn second_team(h: &mut Harness) {
    let mut op = create();
    if let Operation::Create {
        integration_branch,
        bundle,
        ..
    } = &mut op
    {
        integration_branch.push_str("-second");
        bundle.pipelines[0].stages.clear();
    }
    h.run_for(TEAM_B, &person(), op).unwrap();
}

fn service_of(team: &str) -> Actor {
    Actor::Service {
        person: "local".into(),
        team: team.into(),
    }
}

#[test]
fn a_handle_is_written_once_by_the_service_for_a_member_bound_to_a_launch() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    // Only the service writes a handle.
    for actor in [&lead, &person()] {
        refused(h.run(actor, handle("m1", HANDLE)), code::NOT_ALLOWED);
    }
    // An unbound member, a member bound by run and a malformed ID get none.
    refused(h.run(&service(), handle("m3", HANDLE)), code::INVALID_STATE);
    let m = fixture().members[2].clone();
    let mut session = m.session.unwrap();
    session.session_id = "run_id:attached-example".into();
    h.run(
        &service(),
        Operation::SetMemberSession {
            member: "m2".into(),
            session,
            account: m.account,
            worktree: m.worktree,
            ownership_nonce: m.ownership_nonce,
        },
    )
    .unwrap();
    refused(h.run(&service(), handle("m2", HANDLE)), code::INVALID_STATE);
    refused(
        h.run(&service(), handle("m1", "not-a-session-id")),
        code::INVALID_STATE,
    );
    assert_eq!(h.store.member_handle(TEAM, "m1").unwrap(), None);

    h.run(&service(), handle("m1", HANDLE)).unwrap();
    let written = h.store.member_handle(TEAM, "m1").unwrap().unwrap();
    assert_eq!(written.session_id, HANDLE);
    assert_eq!(written.launch, "00000000-0000-4000-8000-000000000031");
    assert_eq!(written.machine, "00000000-0000-4000-8000-0000000000aa");
    assert_eq!(written.label_hint, HINT);
    // Written once: neither the same nor another handle replaces it.
    for again in [HANDLE, "00000000-0000-4000-8000-00000000c0df"] {
        refused(h.run(&service(), handle("m1", again)), code::DUPLICATE);
    }
    assert_eq!(h.store.member_handle(TEAM, "m1").unwrap(), Some(written));
    // Another member of the team cannot take the same session.
    refused(
        h.run(&service(), handle("lead", HANDLE)),
        code::INVALID_STATE,
    );
    // A paused team keeps it.
    h.run(&person(), Operation::Pause).unwrap();
    assert!(h.store.member_handle(TEAM, "m1").unwrap().is_some());
}

/// A handle never outlives the membership (zerus-63rru.53 a): removing the
/// member, or stopping, finishing or archiving its team, frees the session
/// for a binding in another team.
#[test]
fn a_session_can_be_bound_in_another_team_once_its_membership_ended() {
    let mut h = Harness::new();
    second_team(&mut h);
    h.bind("m1");
    h.bind("m2");
    bind_in(&mut h, TEAM_B, "m1", 41);
    bind_in(&mut h, TEAM_B, "m2", 42);
    bind_in(&mut h, TEAM_B, "m3", 43);
    let other = "00000000-0000-4000-8000-00000000c0e0";
    h.run(&service(), handle("m1", HANDLE)).unwrap();
    h.run(&service(), handle("m2", other)).unwrap();
    // While both memberships last, the sessions are taken.
    for (member, id) in [("m1", HANDLE), ("m2", other)] {
        refused(
            h.run_for(TEAM_B, &service_of(TEAM_B), handle(member, id)),
            code::INVALID_STATE,
        );
    }
    // The member's removal ends its membership; the other member keeps its.
    h.run(
        &person(),
        Operation::ArchiveMember {
            member: "m1".into(),
        },
    )
    .unwrap();
    assert_eq!(h.store.member_handle(TEAM, "m1").unwrap(), None);
    assert!(h.store.member_handle(TEAM, "m2").unwrap().is_some());
    h.run_for(TEAM_B, &service_of(TEAM_B), handle("m1", HANDLE))
        .unwrap();
    // The team's stop ends the rest.
    h.run(&person(), Operation::Stop).unwrap();
    assert_eq!(h.store.member_handle(TEAM, "m2").unwrap(), None);
    h.run_for(TEAM_B, &service_of(TEAM_B), handle("m2", other))
        .unwrap();
    // A finished team, whichever program finishes it, keeps none either; and
    // an archive after the stop or the finish finds nothing left.
    let db = rusqlite::Connection::open(h.temp.path().join("teams/teams-v1.sqlite3")).unwrap();
    db.execute(
        "UPDATE teams SET status='\"finished\"' WHERE id=?1",
        [TEAM_B],
    )
    .unwrap();
    drop(db);
    for member in ["m1", "m2"] {
        assert_eq!(h.store.member_handle(TEAM_B, member).unwrap(), None);
    }
    h.run(&person(), Operation::Archive).unwrap();
    let rows: u32 = rusqlite::Connection::open(h.temp.path().join("teams/teams-v1.sqlite3"))
        .unwrap()
        .query_row("SELECT count(*) FROM member_handles", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

/// zerus-63rru.85 round 3: the service records a binding its own observation
/// read as ended, so the store — not a settlement request's reason — shows
/// the member unbound. The state stays Active, the binding's account,
/// worktree and ownership nonce go with the session, and the durable handle
/// is cleared so a later binding can write its own. Only the service writes
/// it, and a second call is refused rather than a second write.
#[test]
fn the_service_records_an_ended_binding_as_unbound_once() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.bind("m1");
    let bound = h.store.member(TEAM, "m1").unwrap().unwrap();
    assert!(bound.session.is_some());
    assert!(!bound.account.is_empty());
    assert!(!bound.worktree.is_empty());
    assert!(!bound.ownership_nonce.is_empty());
    h.run(&service(), handle("m1", HANDLE)).unwrap();
    // Only the service records the end.
    for actor in [&lead, &person()] {
        refused(
            h.run(actor, Operation::UnbindMember { member: "m1".into() }),
            code::NOT_ALLOWED,
        );
    }
    h.run(&service(), Operation::UnbindMember { member: "m1".into() })
        .unwrap();
    let m = h.store.member(TEAM, "m1").unwrap().unwrap();
    assert_eq!(m.state, MemberState::Active);
    assert!(m.session.is_none());
    assert_eq!(m.account, "");
    assert_eq!(m.worktree, "");
    assert_eq!(m.ownership_nonce, "");
    assert_eq!(h.store.member_handle(TEAM, "m1").unwrap(), None);
    assert_eq!(h.store.member_session(TEAM, "m1").unwrap(), None);
    // The end is recorded once: a repeated call changes nothing.
    refused(
        h.run(&service(), Operation::UnbindMember { member: "m1".into() }),
        code::INVALID_STATE,
    );
    // The refusal wrote nothing.
    assert!(h.store.member(TEAM, "m1").unwrap().unwrap().session.is_none());
}
