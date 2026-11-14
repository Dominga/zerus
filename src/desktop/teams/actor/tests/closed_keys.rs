//! zerus-63rru.32: a stopped or archived team clears its key from every
//! task it bound, closed ones included, in bounded background pages.
//! zerus-63rru.97: a key that comes back on an older merged document is
//! cleared where the guard meets it, through the caller's own store.
use super::*;
use crate::teams::ops::{self, Operation};
use crate::teams::tasks::{Caller, Meeting};
use zerus_replica::{Actor as TaskActor, Position, ProjectStore, Stage, Target};

const FOREIGN: &str = "00000000-0000-4000-8000-0000000000ff";

fn key_of(h: &Harness, uuid: &str) -> Option<zerus_replica::TaskTeam> {
    let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
    tasks
        .load(uuid)
        .unwrap()
        .view()
        .unwrap()
        .team
}
fn queue(h: &mut Harness, uuid: &str) -> u32 {
    let node = h
        .run(
            &person(),
            Operation::AddTask {
                task: TaskRef {
                    project: "example-project".into(),
                    uuid: uuid.to_string(),
                },
                role: None,
                pipeline: None,
            },
        )
        .unwrap()
        .node
        .unwrap();
    h.observe(node, crate::teams::ops::TaskStage::Todo);
    node
}
fn bind(h: &Harness, n: u32, team: &str, home: &str) -> String {
    let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
    let (mut doc, _) = tasks
        .new_named_task(
            &format!("Task {n}"),
            "Body",
            &[],
            Stage::Todo,
            Target::Any,
            TaskActor::Person,
            &Position::End,
            "example",
            1,
            None,
            false,
        )
        .unwrap();
    doc.set_team_named(
        Some(&zerus_replica::TaskTeam {
            team: team.into(),
            home: home.into(),
        }),
        "Example team",
        &TaskActor::Person,
        1,
    )
    .unwrap();
    let uuid = doc.uuid().to_string();
    tasks.save(&mut doc).unwrap();
    uuid
}
/// The real writer pair for a closed node: the cancel decision, the recorded
/// answer and the observation that ends the node as Cancelled.
fn close_cancelled(h: &mut Harness, uuid: &str, node: u32) {
    {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = tasks.load(uuid).unwrap();
        let home = h.store.team(TEAM).unwrap().unwrap().home;
        doc.record_send(
            &zerus_replica::SessionKey {
                machine: home,
                session: format!("codex/task-{node}"),
                session_id: String::new(),
            },
            "request-1",
            &TaskActor::Person,
            2,
        )
        .unwrap();
        doc.record_cancel("Superseded", &TaskActor::Person, 3).unwrap();
        tasks.save(&mut doc).unwrap();
    }
    let lead = if h.store.member(TEAM, "lead").unwrap().unwrap().session.is_none() {
        h.bind("lead")
    } else {
        let m = h.store.member(TEAM, "lead").unwrap().unwrap();
        ops::Actor::Session {
            person: "local".into(),
            team: TEAM.into(),
            member: "lead".into(),
            session: m.session.unwrap(),
        }
    };
    h.run(
        &lead,
        Operation::Cancel {
            node,
            reason: "Superseded".into(),
        },
    )
    .unwrap();
    // In a live team the task's end is recorded by its observation; the
    // pending cancel settles with the node.
    crate::teams::tasks::observe(
        h.temp.path(),
        &TaskRef {
            project: "example-project".into(),
            uuid: uuid.to_string(),
        },
    )
    .unwrap();
}
fn passes(h: &mut Harness, a: &mut super::super::Runtime, n: usize) {
    for i in 0..n {
        a.begin_pass();
        let teams = h.store.teams(200, None).unwrap().rows;
        let mut ports = super::core::Fake::new();
        a.page(&mut h.store, &mut ports, &teams, 10_000 + i as i64, false)
            .unwrap();
    }
}
fn keyed(h: &Harness) -> usize {
    let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
    let (docs, _) = tasks.listing().unwrap();
    docs.iter()
        .filter(|doc| doc.view().unwrap().team.is_some())
        .count()
}
fn agent(machine: &str) -> TaskActor {
    TaskActor::Agent(zerus_replica::SessionKey {
        machine: machine.into(),
        session: "codex/example/review97".into(),
        session_id: "run_id:review97-fixture".into(),
    })
}
/// The helper meeting through one caller-owned store. The store is dropped
/// before any caller reopens, so these direct controls never nest handles.
fn clear_at(h: &Harness, reference: &TaskRef, team: &str, machine: &str) -> bool {
    let projects = ProjectStore::open(&h.temp.path().join("replica"), &reference.project).unwrap();
    let mut meeting = Meeting::Projects(&projects);
    crate::teams::tasks::clear_returned_key(&h.store, &mut meeting, reference, team, machine)
        .unwrap()
}
/// The actual guard meeting with the caller's store retained for the call.
fn guard_at(
    h: &Harness,
    uuid: &str,
    machine: &str,
    action: &str,
) -> std::result::Result<(), zerus_team_plan::Problem> {
    let projects = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
    let doc = projects.load(uuid).unwrap();
    let caller = Caller {
        state: h.temp.path(),
        machine,
        actor: agent(machine),
    };
    let mut meeting = Meeting::Projects(&projects);
    crate::teams::tasks::guard(&caller, &mut meeting, &doc, action)
}

/// A genuine concurrent branch: a second real ProjectStore writes the key
/// before the stop, the real stop sweep clears the local copy, then the real
/// `ProjectStore::import_updates` returns the key. Every task and team row
/// comes from product writers; no direct SQL rows are inserted.
fn returned_from_sync() -> (Harness, String, String) {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let uuid = bind(&h, 97, TEAM, &home);
    let node = queue(&mut h, &uuid);
    close_cancelled(&mut h, &uuid, node);
    let old = {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        tasks.load(&uuid).unwrap().snapshot().unwrap()
    };
    let remote_root = tempfile::tempdir().unwrap();
    let incoming = {
        let remote =
            ProjectStore::open(&remote_root.path().join("replica"), "example-project").unwrap();
        let mut doc = zerus_replica::TaskDoc::from_bytes(&old).unwrap();
        // Advance the remote branch with the real key writers. The final key
        // write is concurrent with and later in Loro order than the clear.
        for n in 0..8 {
            doc.clear_team_silent().unwrap();
            doc.set_team_named(
                Some(&zerus_replica::TaskTeam {
                    team: TEAM.into(),
                    home: home.clone(),
                }),
                "Remote team",
                &TaskActor::Person,
                100 + n,
            )
            .unwrap();
        }
        remote.save(&mut doc).unwrap();
        doc.snapshot().unwrap()
    };
    let mut runtime = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    h.run(&person(), Operation::Stop).unwrap();
    passes(&mut h, &mut runtime, 6);
    assert_eq!(key_of(&h, &uuid), None);
    assert!(h.store.sweep_finished(TEAM).unwrap());
    {
        let local = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let imported = local
            .import_updates(vec![zerus_replica::Update {
                document: zerus_replica::Document::Task(uuid.clone()),
                bytes: incoming,
            }])
            .unwrap();
        assert!(imported.refused.is_empty());
        assert_eq!(imported.changed, 1);
        assert!(local.load(&uuid).unwrap().team().unwrap().is_some());
    }
    (h, uuid, home)
}

/// A closed task whose key names another computer, while the stopped team's
/// stored home is this computer: a foreign key, never local authority.
fn foreign_key_task() -> (Harness, String, String) {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let uuid = bind(&h, 98, TEAM, FOREIGN);
    {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = tasks.load(&uuid).unwrap();
        doc.record_send(
            &zerus_replica::SessionKey {
                machine: FOREIGN.into(),
                session: "codex/example/remote".into(),
                session_id: String::new(),
            },
            "remote-send",
            &TaskActor::Person,
            2,
        )
        .unwrap();
        doc.record_cancel("Closed remote fixture", &TaskActor::Person, 3).unwrap();
        tasks.save(&mut doc).unwrap();
    }
    h.run(&person(), Operation::Stop).unwrap();
    (h, uuid, home)
}

#[test]
fn a_stopped_team_clears_its_key_from_closed_tasks_in_bounded_pages() {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    let uuids: Vec<String> = (1..=3)
        .map(|n| {
            let uuid = bind(&h, n, TEAM, &home);
            let node = queue(&mut h, &uuid);
            close_cancelled(&mut h, &uuid, node);
            uuid
        })
        .collect();
    h.run(&person(), Operation::Stop).unwrap();
    passes(&mut h, &mut a, 6);
    for uuid in uuids {
        assert_eq!(key_of(&h, &uuid), None, "the key stayed on a closed task");
    }
}

#[test]
fn more_than_two_hundred_closed_tasks_need_more_than_one_pass() {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    for n in 1..=220u32 {
        let uuid = bind(&h, n, TEAM, &home);
        let node = queue(&mut h, &uuid);
        close_cancelled(&mut h, &uuid, node);
    }
    h.run(&person(), Operation::Stop).unwrap();
    passes(&mut h, &mut a, 1);
    // One pass clears at most one page (200), and the cursor is stored for
    // the restart.
    let remaining = keyed(&h);
    assert!(
        (1..=20).contains(&remaining),
        "one pass cleared nothing or everything: {remaining}"
    );
    let cursor: Option<(String, String, bool)> = a.load(TEAM, "key_clear_cursor").unwrap();
    assert!(cursor.is_some(), "the cursor is not stored");
    passes(&mut h, &mut a, 3);
    assert_eq!(keyed(&h), 0, "a key stayed after enough passes");
}

#[test]
fn a_restart_in_the_middle_resumes_the_sweep() {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    for n in 1..=60u32 {
        let uuid = bind(&h, n, TEAM, &home);
        let node = queue(&mut h, &uuid);
        close_cancelled(&mut h, &uuid, node);
    }
    h.run(&person(), Operation::Stop).unwrap();
    passes(&mut h, &mut a, 1);
    drop(a);
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    passes(&mut h, &mut a, 2);
    assert_eq!(keyed(&h), 0, "the resumed sweep left a key");
}

#[test]
fn a_task_whose_key_names_another_team_is_untouched() {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    let uuid = bind(&h, 1, TEAM, &home);
    let node = queue(&mut h, &uuid);
    close_cancelled(&mut h, &uuid, node);
    {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = tasks.load(&uuid).unwrap();
        doc.set_team_named(
            Some(&zerus_replica::TaskTeam {
                team: "00000000-0000-4000-8000-0000000000cc".into(),
                home: "00000000-0000-4000-8000-0000000000cc".into(),
            }),
            "Other team",
            &TaskActor::Person,
            3,
        )
        .unwrap();
        tasks.save(&mut doc).unwrap();
    }
    h.run(&person(), Operation::Stop).unwrap();
    passes(&mut h, &mut a, 6);
    let key = key_of(&h, &uuid).unwrap();
    assert_eq!(key.team, "00000000-0000-4000-8000-0000000000cc", "{key:?}");
}

#[test]
fn the_other_computers_guard_no_longer_refuses_a_done_task_after_the_clear() {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    let uuid = bind(&h, 1, TEAM, &home);
    let node = queue(&mut h, &uuid);
    close_cancelled(&mut h, &uuid, node);
    h.run(&person(), Operation::Stop).unwrap();
    passes(&mut h, &mut a, 6);
    let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
    let doc = tasks.load(&uuid).unwrap();
    assert!(
        !crate::teams::tasks::guarded(h.temp.path(), &home, &doc).unwrap(),
        "the cleared task is still guarded"
    );
}

/// Round 2, P2: clearing the key of a closed task appends no history entry.
/// The history entries are byte-identical before and after the clear; only
/// the revision advances.
#[test]
fn clearing_a_closed_tasks_key_appends_no_history_entry() {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    let uuid = bind(&h, 1, TEAM, &home);
    let node = queue(&mut h, &uuid);
    close_cancelled(&mut h, &uuid, node);
    let (before, revision_before) = {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let doc = tasks.load(&uuid).unwrap();
        (
            serde_json::to_string(&doc.view().unwrap().log).unwrap(),
            doc.revision(),
        )
    };
    h.run(&person(), Operation::Stop).unwrap();
    passes(&mut h, &mut a, 6);
    let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
    let doc = tasks.load(&uuid).unwrap();
    let view = doc.view().unwrap();
    assert_eq!(view.team, None, "the key stayed");
    let after = serde_json::to_string(&view.log).unwrap();
    assert_eq!(before, after, "the history changed across the clear");
    assert_ne!(doc.revision(), revision_before, "the revision did not advance");
}

/// Round 2, P3: a finished sweep is recorded finished and does no work on
/// later passes; only a new stop or archive reopens it. A task closed after
/// the finish keeps its key until the archive's re-sweep clears it.
#[test]
fn a_finished_sweep_does_no_work_until_a_new_archive() {
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let mut a = super::super::Runtime::open(h.temp.path(), &home).unwrap();
    let first = bind(&h, 1, TEAM, &home);
    let node = queue(&mut h, &first);
    close_cancelled(&mut h, &first, node);
    h.run(&person(), Operation::Stop).unwrap();
    passes(&mut h, &mut a, 6);
    assert_eq!(key_of(&h, &first), None, "the first key stayed");
    assert!(
        h.store.sweep_finished(TEAM).unwrap(),
        "the finished sweep is not recorded"
    );
    // A task closed after the finish (its node's rows written the way a
    // cross-computer merge leaves them; the stopped team admits no new node
    // through the operations): the finished sweep does no work on later
    // passes, so its key stays until a new archive reopens the sweep.
    let later = bind(&h, 2, TEAM, &home);
    let db = h.store.db.as_ref().unwrap();
    let data = json!({"id": 2, "task": {"project": "example-project", "uuid": later}});
    db.execute(
        "INSERT INTO node_identity(team,id,task_project,task_uuid) VALUES(?1,?2,?3,?4)",
        rusqlite::params![TEAM, 2u32, "example-project", later],
    )
    .unwrap();
    db.execute(
        "INSERT INTO nodes(team,id,data,rank,admitted,routed,task_revision,task_stage,task_cancelled,integration_required) VALUES(?1,?2,?3,'',0,0,'','done',1,0)",
        rusqlite::params![TEAM, 2u32, data.to_string()],
    )
    .unwrap();
    passes(&mut h, &mut a, 6);
    assert!(
        key_of(&h, &later).is_some(),
        "the finished sweep ran without a new archive"
    );
    h.run(&person(), Operation::Archive).unwrap();
    passes(&mut h, &mut a, 6);
    assert_eq!(key_of(&h, &later), None, "the reopened sweep left the key");
    assert!(
        h.store.sweep_finished(TEAM).unwrap(),
        "the reopened sweep is not recorded finished"
    );
}

/// zerus-63rru.97 (F1/F2): an older replica document merged after the sweep
/// brings the key back; the real guard meeting clears it through the
/// caller's own store, and the task is usable again.
#[test]
fn an_older_merged_document_returns_the_key_and_the_guard_clears_it() {
    let (h, uuid, home) = returned_from_sync();
    let result = {
        let projects =
            ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let doc = projects.load(&uuid).unwrap();
        let caller = Caller {
            state: h.temp.path(),
            machine: &home,
            actor: agent(&home),
        };
        let mut meeting = Meeting::Projects(&projects);
        crate::teams::tasks::guard(&caller, &mut meeting, &doc, "rename")
    };
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(key_of(&h, &uuid), None, "the guard meeting did not clear");
}

/// The reviewer's local meeting control, through the actual guard.
#[test]
fn review97_guard_meeting_clears_a_returned_local_key() {
    let (h, uuid, home) = returned_from_sync();
    let result = {
        let projects =
            ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let doc = projects.load(&uuid).unwrap();
        let caller = Caller {
            state: h.temp.path(),
            machine: &home,
            actor: agent(&home),
        };
        let mut meeting = Meeting::Projects(&projects);
        crate::teams::tasks::guard(&caller, &mut meeting, &doc, "rename")
    };
    result.unwrap();
    assert_eq!(key_of(&h, &uuid), None, "the actual guard meeting must clear");
}

/// F2: the caller retains its own ProjectStore while the actual guard meets
/// a local returned key. A second ProjectStore under that lock would wait
/// forever; the bounded wait proves the meeting writes through the caller's.
#[test]
fn review97_guard_under_the_callers_project_lock_is_bounded() {
    let (h, uuid, home) = returned_from_sync();
    let path = h.temp.path().to_path_buf();
    let uuid_in = uuid.clone();
    let home_in = home.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let _running = std::thread::spawn(move || {
        let projects = ProjectStore::open(&path.join("replica"), "example-project").unwrap();
        let doc = projects.load(&uuid_in).unwrap();
        let caller = Caller {
            state: &path,
            machine: &home_in,
            actor: agent(&home_in),
        };
        let mut meeting = Meeting::Projects(&projects);
        let result = crate::teams::tasks::guard(&caller, &mut meeting, &doc, "rename");
        let key = projects.load(&uuid_in).unwrap().team().unwrap();
        drop(projects);
        let _ = tx.send((result.is_ok(), key));
    });
    let (ok, key) = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("the guard did not finish under the caller's project lock");
    assert!(ok, "the meeting did not clear");
    assert!(key.is_none(), "the returned key stayed under the lock");
}

/// The reviewer's foreign-key control: a key whose home is another computer
/// naming a locally stopped team's ID is neither cleared nor authorized.
#[test]
fn review97_foreign_key_is_not_cleared_or_authorized() {
    let (h, uuid, home) = foreign_key_task();
    let result = guard_at(&h, &uuid, &home, "rename");
    assert!(result.is_err(), "foreign task authority must stay refused");
    assert_eq!(key_of(&h, &uuid).unwrap().home, FOREIGN);
}

/// The reviewer's other-computer control: the stored team's home is another
/// computer, so this caller's guard gives it no clear authority.
#[test]
fn review97_a_team_stopped_on_another_computer_is_not_local_authority() {
    use crate::teams::ops::tests::{create, fixture};
    let mut h = Harness::empty();
    let local_home = fixture().team.home;
    let mut operation = create();
    if let Operation::Create { home, .. } = &mut operation {
        *home = FOREIGN.into();
    }
    h.run(&person(), operation).unwrap();
    let uuid = bind(&h, 99, TEAM, FOREIGN);
    {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = tasks.load(&uuid).unwrap();
        doc.record_send(
            &zerus_replica::SessionKey {
                machine: FOREIGN.into(),
                session: "codex/example/remote-owner".into(),
                session_id: String::new(),
            },
            "remote-owner-send",
            &TaskActor::Person,
            2,
        )
        .unwrap();
        doc.record_cancel("Closed remote owner fixture", &TaskActor::Person, 3)
            .unwrap();
        tasks.save(&mut doc).unwrap();
    }
    h.run(&person(), Operation::Stop).unwrap();
    assert_eq!(h.store.team(TEAM).unwrap().unwrap().home, FOREIGN);
    assert_ne!(local_home, FOREIGN);
    let result = guard_at(&h, &uuid, &local_home, "rename");
    assert!(
        result.is_err(),
        "a stop on the other computer gives this caller no clear authority"
    );
    assert_eq!(key_of(&h, &uuid).unwrap().home, FOREIGN);
}

/// F3: a key that claims this computer while the stored team's home is
/// another computer is still not this computer's to clear.
#[test]
fn a_team_whose_home_is_another_computer_is_not_cleared_here() {
    use crate::teams::ops::tests::{create, fixture};
    let mut h = Harness::empty();
    let local_home = fixture().team.home;
    let mut operation = create();
    if let Operation::Create { home, .. } = &mut operation {
        *home = FOREIGN.into();
    }
    h.run(&person(), operation).unwrap();
    let uuid = bind(&h, 98, TEAM, &local_home);
    h.run(&person(), Operation::Stop).unwrap();
    let reference = TaskRef {
        project: "example-project".into(),
        uuid: uuid.clone(),
    };
    assert!(
        !clear_at(&h, &reference, TEAM, &local_home),
        "cleared a team whose home is another computer"
    );
    assert_eq!(key_of(&h, &uuid).unwrap().home, local_home);
}

/// R2: a document whose key names another team than the meeting's team is
/// not cleared, even when the caller's stale read still named this team.
#[test]
fn a_key_rebound_before_the_meeting_is_not_cleared() {
    use crate::teams::ops::tests::{create, TEAM_B};
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let uuid = bind(&h, 95, TEAM, &home);
    h.run(&person(), Operation::Stop).unwrap();
    h.run_for(TEAM_B, &person(), create()).unwrap();
    {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = tasks.load(&uuid).unwrap();
        doc.set_team_named(
            Some(&zerus_replica::TaskTeam {
                team: TEAM_B.into(),
                home: home.clone(),
            }),
            "Second live team",
            &TaskActor::Person,
            500,
        )
        .unwrap();
        tasks.save(&mut doc).unwrap();
    }
    let reference = TaskRef {
        project: "example-project".into(),
        uuid: uuid.clone(),
    };
    assert!(
        !clear_at(&h, &reference, TEAM, &home),
        "cleared a key that names another team"
    );
    assert_eq!(key_of(&h, &uuid).unwrap().team, TEAM_B);
}

/// R3: a task bound again to a live second team after the stop keeps its
/// key even when an older document still names the stopped team.
#[test]
fn a_task_bound_to_a_live_team_after_the_stop_keeps_its_key() {
    use crate::teams::ops::tests::{create, TEAM_B};
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let uuid = bind(&h, 96, TEAM, &home);
    let reference = TaskRef {
        project: "example-project".into(),
        uuid: uuid.clone(),
    };
    // The stop first frees the fixture's integration branch for TEAM_B; the
    // stopped team's key stays on the document.
    h.run(&person(), Operation::Stop).unwrap();
    h.run_for(TEAM_B, &person(), create()).unwrap();
    h.run_for(
        TEAM_B,
        &person(),
        Operation::AddTask {
            task: reference.clone(),
            role: None,
            pipeline: None,
        },
    )
    .unwrap();
    assert_eq!(
        crate::teams::tasks::binding(&h.store, &reference)
            .unwrap()
            .unwrap()
            .0,
        TEAM_B
    );
    assert!(
        !clear_at(&h, &reference, TEAM, &home),
        "cleared a key of a task bound to a live team"
    );
    assert_eq!(key_of(&h, &uuid).unwrap().team, TEAM);
}

/// A live team's key and a real post-stop rebound through AddTask are
/// untouched by the meeting (zerus-63rru.97).
#[test]
fn a_live_teams_key_and_a_post_stop_rebound_are_untouched() {
    use crate::teams::ops::tests::{create, TEAM_B};
    let mut h = Harness::new();
    let home = h.store.team(TEAM).unwrap().unwrap().home;
    let uuid = bind(&h, 1, TEAM, &home);
    let reference = TaskRef {
        project: "example-project".into(),
        uuid: uuid.clone(),
    };
    // The live team: nothing clears.
    assert!(
        !clear_at(&h, &reference, TEAM, &home),
        "a live team's key was cleared"
    );
    assert!(key_of(&h, &uuid).is_some());
    // A real stop, then a real rebound to a live second team through AddTask.
    h.run(&person(), Operation::Stop).unwrap();
    h.run_for(TEAM_B, &person(), create()).unwrap();
    {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
        let mut doc = tasks.load(&uuid).unwrap();
        doc.set_team_named(
            Some(&zerus_replica::TaskTeam {
                team: TEAM_B.into(),
                home: home.clone(),
            }),
            "Second live team",
            &TaskActor::Person,
            500,
        )
        .unwrap();
        tasks.save(&mut doc).unwrap();
    }
    h.run_for(
        TEAM_B,
        &person(),
        Operation::AddTask {
            task: reference.clone(),
            role: None,
            pipeline: None,
        },
    )
    .unwrap();
    assert_eq!(
        crate::teams::tasks::binding(&h.store, &reference)
            .unwrap()
            .unwrap()
            .0,
        TEAM_B
    );
    assert!(
        !clear_at(&h, &reference, TEAM, &home),
        "the post-stop rebound was cleared"
    );
    assert_eq!(key_of(&h, &uuid).unwrap().team, TEAM_B);
}
