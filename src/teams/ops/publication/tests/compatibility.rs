use super::*;

fn run(store: &mut Store, team: &str, actor: &Actor, op: Operation) {
    let rev = store.team(team).unwrap().map_or(0, |t| t.revision);
    let mut req = request(rev, op);
    req.team = team.into();
    apply(store, actor, &req, 1234).unwrap();
}

pub(super) fn export() {
    let root =
        std::path::PathBuf::from(std::env::var_os("ZERUS_PHASE_PUBLICATION_COMPAT_ROOT").unwrap())
            .canonicalize()
            .unwrap();
    let owned = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/wp161")
        .canonicalize()
        .unwrap();
    assert!(
        root.starts_with(owned)
            && root
                .join(".phase-publication-compatibility-fixture")
                .is_file()
    );
    let state = root.join("state");
    crate::platform::private_dir(&state.join("teams")).unwrap();
    std::fs::write(state.join("teams/preview"), "on\n").unwrap();
    let config = crate::config::Config {
        home: root.display().to_string(),
        dir: root.join("config"),
        state: state.clone(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    let mode = std::env::var("ZERUS_PHASE_PUBLICATION_COMPAT_MODE").unwrap();
    let mut store = Store::open(&state).unwrap();
    match mode.as_str() {
        "base" => {
            let home = fixture().team.home;
            crate::swarm::fixture_project(&config, &home, "example-project");
            let tasks =
                zerus_replica::ProjectStore::open(&state.join("replica"), "example-project")
                    .unwrap();
            tasks
                .save_project(&zerus_replica::ProjectDoc::new())
                .unwrap();
            drop(tasks);
            let mut base = create();
            if let Operation::Create {
                integration_branch, ..
            } = &mut base
            {
                integration_branch.push_str("-publication-base");
            }
            run(&mut store, TEAM_B, &person(), base);
            let mut planning = serde_json::to_value(create()).unwrap();
            planning["op"] = "start_planning".into();
            planning["goal"] = "Prepare the approved phase.".into();
            run(
                &mut store,
                TEAM,
                &person(),
                serde_json::from_value(planning).unwrap(),
            );
            let lead = fixture()
                .members
                .into_iter()
                .find(|m| m.id == "lead")
                .unwrap();
            run(
                &mut store,
                TEAM,
                &service(),
                Operation::SetMemberSession {
                    member: lead.id,
                    session: lead.session.unwrap(),
                    account: lead.account,
                    worktree: lead.worktree,
                    ownership_nonce: lead.ownership_nonce,
                },
            );
            run(
                &mut store,
                TEAM,
                &person(),
                Operation::SubmitDraftPhase {
                    phase: 1,
                    draft: input(&[1, 2]),
                },
            );
        }
        "reserve" => run(
            &mut store,
            TEAM,
            &person(),
            Operation::ApprovePhase {
                phase: 1,
                generation: 1,
                confirm_initial: true,
            },
        ),
        "publish" => {
            for r in store.publication_work(TEAM).unwrap() {
                assert_eq!(
                    crate::tasks::create_planned_task_for_test(
                        &config,
                        TEAM,
                        r.node.id,
                        &r.task.uuid,
                        false
                    )
                    .unwrap(),
                    "created"
                );
            }
            let p = store.phase_draft(TEAM, 1).unwrap().publication.unwrap();
            run(
                &mut store,
                TEAM,
                &service(),
                Operation::ReconcilePublication {
                    phase: 1,
                    publication: p.id,
                },
            );
            assert_eq!(
                store.team(TEAM).unwrap().unwrap().status,
                TeamStatus::Running
            );
        }
        _ => panic!("Unknown private fixture mode"),
    }
    let version: u32 = store
        .db
        .as_ref()
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        version,
        if mode == "base" {
            store::layout::NATIVE_PERSON
        } else {
            store::layout::PUBLICATION
        }
    );
    println!("slice-3 writer {mode}: schema {version}");
}
