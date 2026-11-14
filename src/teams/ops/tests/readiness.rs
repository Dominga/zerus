use super::*;

#[test]
fn public_readiness_returns_phase_and_node_data_without_writes() {
    let mut h = Harness::new();
    let node = h.add(&person(), 1);
    let before = h.store.team(TEAM).unwrap();
    let events = h.store.journal(TEAM, 200, None).unwrap();
    let changes: u64 = h
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT total_changes()", [], |r| r.get(0))
        .unwrap();
    let view = super::super::readiness(&h.store, TEAM).unwrap();
    assert_eq!(view.nodes, vec![h.node(node)]);
    assert_eq!(view.phases, h.store.phases(TEAM, 200, None).unwrap().rows);
    assert_eq!(h.store.team(TEAM).unwrap(), before);
    assert_eq!(h.store.journal(TEAM, 200, None).unwrap(), events);
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("SELECT total_changes()", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        changes
    );
}

#[test]
fn public_readiness_uses_archived_dependencies_and_stage_targets_as_leaves() {
    let mut h = Harness::new();
    h.settings(|s| s.integration = Integration::None);
    for id in 1..=3 {
        let node = h.add(&person(), id);
        let mut op = route(node);
        if id > 1 {
            if let Operation::Route { deps, .. } = &mut op {
                deps.push(Dep {
                    node: id - 1,
                    kind: DepKind::Hard,
                });
            }
        }
        h.run(&person(), op).unwrap();
        h.observe(node, TaskStage::Done);
    }
    let live = h.add(&person(), 4);
    let stage = h.add(&person(), 5);
    let mut op = route(live);
    if let Operation::Route { deps, .. } = &mut op {
        deps.push(Dep {
            node: 3,
            kind: DepKind::Hard,
        });
    }
    h.run(&person(), op).unwrap();
    let mut n = h.node(stage);
    n.kind = NodeKind::Stage;
    n.stage_of = Some(2);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE nodes SET data=?3 WHERE team=?1 AND id=?2",
            params![TEAM, stage, rows::node_payload(&n).unwrap()],
        )
        .unwrap();
    let view = super::super::readiness(&h.store, TEAM).unwrap();
    assert_eq!(
        view.nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
        vec![2, 3, 4, 5]
    );
    assert!(view.nodes[0].deps.is_empty() && view.nodes[1].deps.is_empty());
    assert_eq!(
        view.nodes[2].deps,
        vec![Dep {
            node: 3,
            kind: DepKind::Hard
        }]
    );
    assert_eq!(view.nodes[3].stage_of, Some(2));
}

#[test]
fn public_readiness_refuses_unknown_teams_and_preview_off_without_initializing() {
    let h = Harness::empty();
    refused(super::super::readiness(&h.store, TEAM), code::NOT_FOUND);
    assert!(!h.temp.path().join("teams/teams-v1.sqlite3").exists());
    std::fs::remove_file(h.temp.path().join("teams/preview")).unwrap();
    refused(
        super::super::readiness(&h.store, TEAM),
        code::TEAMS_INACTIVE,
    );
    let h = Harness::new();
    refused(super::super::readiness(&h.store, TEAM_B), code::NOT_FOUND);
}

#[test]
fn public_readiness_refuses_oversized_phase_node_and_byte_views() {
    let mut h = Harness::new();
    let phase = h.store.phases(TEAM, 1, None).unwrap().rows[0].clone();
    for n in 1..=200 {
        let mut p = phase.clone();
        p.n = n;
        h.store
            .db
            .as_ref()
            .unwrap()
            .execute(
                "INSERT INTO phases(team,n,data) VALUES(?1,?2,?3)",
                params![TEAM, n, encode(&p).unwrap()],
            )
            .unwrap();
    }
    refused(super::super::readiness(&h.store, TEAM), code::OVER_CAP);
    h = Harness::new();
    seed_nodes(&mut h, 601, 0);
    refused(super::super::readiness(&h.store, TEAM), code::OVER_CAP);
    h = Harness::new();
    seed_nodes(&mut h, 130, 65_536);
    refused(super::super::readiness(&h.store, TEAM), code::OVER_CAP);
}
fn seed_nodes(h: &mut Harness, count: u32, text_bytes: usize) {
    let first = h.add(&person(), 1);
    let mut n = h.node(first);
    let tx = h.store.db.as_mut().unwrap().transaction().unwrap();
    n.text = "x".repeat(text_bytes);
    for id in 2..=count {
        n.id = id;
        n.task = Some(task(id));
        tx.execute("INSERT INTO node_identity(team,id,task_project,task_uuid) VALUES(?1,?2,'example-project',?3)",params![TEAM,id,task(id).uuid]).unwrap();
        tx.execute("INSERT INTO nodes(team,id,data,rank,admitted,routed,task_revision,task_stage,task_cancelled) VALUES(?1,?2,?3,'',1,1,'fixture','todo',0)",params![TEAM,id,rows::node_payload(&n).unwrap()]).unwrap();
    }
    tx.commit().unwrap();
}
