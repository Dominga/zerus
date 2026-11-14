use super::*;
#[test]
fn lead_pause_resume_and_person_stop_archive_have_state_guards() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    let node = h.add(&person(), 1);
    refused(h.run(&person(), Operation::Archive), code::INVALID_STATE);
    h.run(&lead, Operation::Pause).unwrap();
    refused(h.run(&lead, Operation::Pause), code::INVALID_STATE);
    h.run(&lead, Operation::Resume).unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    assert!(h.node(node).hold);
    assert_eq!(h.node(node).state, NodeState::Pending);
    refused(h.run(&person(), Operation::Stop), code::INVALID_STATE);
    h.run(&person(), Operation::Archive).unwrap();
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Archived
    );
}
#[test]
fn lead_cannot_resume_a_team_the_person_paused() {
    let mut h = Harness::new();
    let lead = h.bind("lead");
    h.run(&person(), Operation::Pause).unwrap();
    refused(h.run(&lead, Operation::Resume), code::INVALID_STATE);
    h.run(&person(), Operation::Resume).unwrap();
}
#[test]
fn branch_must_be_nonempty_and_not_used_by_another_live_team() {
    let mut h = Harness::empty();
    let mut op = create();
    if let Operation::Create {
        integration_branch, ..
    } = &mut op
    {
        integration_branch.clear();
    }
    refused(h.run(&person(), op), code::INVALID_STATE);
    assert!(h.store.db.is_none());
    h.run(&person(), create()).unwrap();
    refused(h.run_for(TEAM_B, &person(), create()), code::INVALID_STATE);
    h.run(&person(), Operation::Stop).unwrap();
    h.run_for(TEAM_B, &person(), create()).unwrap();
}
#[test]
fn settings_validate_phase_acceptance_and_admitted_caps() {
    let mut h = Harness::new();
    h.add(&person(), 1);
    let settings = Settings {
        phase_acceptance: Acceptance::Pipeline,
        ..Settings::default()
    };
    refused(
        h.run(&person(), Operation::ChangeSettings { settings }),
        code::INVALID_STATE,
    );
    h.settings(|s| s.caps.max_open_tasks = 1);
    refused(
        h.run(
            &person(),
            Operation::AddTask {
                task: task(2),
                role: None,
                pipeline: None,
            },
        ),
        code::OVER_CAP,
    );
}
#[test]
fn preview_revokes_existing_store_handles() {
    let mut h = Harness::new();
    std::fs::remove_file(h.temp.path().join("teams/preview")).unwrap();
    refused(h.store.team(TEAM), code::TEAMS_INACTIVE);
    refused(h.run(&person(), Operation::Pause), code::TEAMS_INACTIVE);
}

/// Records every UPDATE of a node row from now on, by table.
fn watch_node_writes(h: &Harness) {
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch(
            "CREATE TEMP TABLE node_writes(tbl TEXT);
             CREATE TEMP TRIGGER nodes_write AFTER UPDATE ON nodes BEGIN
                 INSERT INTO node_writes VALUES('nodes');
             END;
             CREATE TEMP TRIGGER archive_write AFTER UPDATE ON node_archive BEGIN
                 INSERT INTO node_writes VALUES('node_archive');
             END;",
        )
        .unwrap();
}

fn node_writes(h: &Harness) -> (u32, u32) {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT (SELECT count(*) FROM node_writes WHERE tbl='nodes'),
                    (SELECT count(*) FROM node_writes WHERE tbl='node_archive')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

/// An ended node, as the archive keeps it: only its requirement matters here.
fn archived_row(h: &Harness, id: u32, integration_required: bool) {
    let uuid = format!("00000000-0000-4000-8000-{id:012x}");
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "INSERT OR IGNORE INTO node_identity(team,id,task_project,task_uuid)
             VALUES(?1,?2,'example-project',?3)",
            params![TEAM, id, uuid],
        )
        .unwrap();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "INSERT INTO node_archive(team,id,data,rank,admitted,routed,task_revision,
                 task_stage,task_cancelled,integration_required)
             VALUES(?1,?2,'{}','',1,1,'','done',0,?3)",
            params![TEAM, id, integration_required],
        )
        .unwrap();
}

fn requirement(h: &Harness, table: &str, id: u32) -> bool {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            &format!("SELECT integration_required FROM {table} WHERE team=?1 AND id=?2"),
            params![TEAM, id],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn a_settings_change_that_keeps_integration_writes_no_node_row() {
    let mut h = Harness::new();
    h.add(&person(), 1);
    watch_node_writes(&h);
    let before = h.store.team(TEAM).unwrap().unwrap().settings.land_allowed;
    h.settings(|s| s.land_allowed = !before);
    let after = h.store.team(TEAM).unwrap().unwrap().settings.land_allowed;
    assert_eq!(after, !before);
    assert_eq!(node_writes(&h), (0, 0));
}

#[test]
fn a_settings_change_updates_live_nodes_and_leaves_archived_nodes_as_they_were() {
    let mut h = Harness::new();
    let live = h.add(&person(), 1);
    archived_row(&h, 900, true);
    archived_row(&h, 901, false);
    watch_node_writes(&h);
    h.settings(|s| s.integration = Integration::None);
    assert!(!requirement(&h, "nodes", live));
    assert!(requirement(&h, "node_archive", 900));
    assert!(!requirement(&h, "node_archive", 901));
    assert_eq!(node_writes(&h), (1, 0));
}

#[test]
fn an_archive_of_many_nodes_is_not_touched_by_a_settings_change() {
    let mut h = Harness::new();
    for id in 1000..1300 {
        archived_row(&h, id, id % 2 == 0);
    }
    watch_node_writes(&h);
    h.settings(|s| s.integration = Integration::None);
    assert_eq!(node_writes(&h).1, 0);
    for id in [1000, 1001, 1299] {
        assert_eq!(requirement(&h, "node_archive", id), id % 2 == 0);
    }
}
