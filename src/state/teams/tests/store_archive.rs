use super::*;
use crate::state::teams::model::{Attempt, Dependency, PhaseAcceptance, Report, TaskStatus, TeamStatus};

fn legacy(closed: u32) -> Team {
    let phases = closed / 50;
    Team {
        id: "archive-fixture".into(), name: "Archive fixture".into(), lead: "lead".into(),
        status: TeamStatus::Running,
        phases: (1..=phases + 1).map(|n| Phase {
            n, title: format!("Phase {n}"), state: PhaseState::Approved,
            tasks: ((n - 1) * 50 + 1..=if n <= phases { n * 50 } else { closed + 20 }).collect(),
            ..Default::default()
        }).collect(),
        tasks: (1..=closed + 20).map(|id| Task {
            id, phase: (id - 1) / 50 + 1, title: format!("Task {id}"),
            status: if id <= closed { TaskStatus::Accepted } else { TaskStatus::Pending },
            deps: if id == closed + 1 { vec![Dependency { task: 1, kind: Default::default() }] } else { vec![] },
            attempts: if id <= closed { vec![Attempt {
                n: 1, id: format!("attempt-{id}"), sent_at: 1.0, started_at: Some(2.0), decided_at: Some(4.0),
                report: Some(Report { text: "r".repeat(10_240), at: 3.0, ..Default::default() }),
                ..Default::default()
            }] } else { vec![] },
            ..Default::default()
        }).collect(),
        ..Default::default()
    }
}

fn insert(store: &Store, team: &Team) {
    store.db.execute("INSERT INTO teams(id,revision,status,updated,payload) VALUES(?1,0,'running',0,?2)",
        params![team.id, serde_json::to_string(team).unwrap()]).unwrap();
}

#[test]
fn archive_migrates_large_records_with_a_bounded_stub_window_and_pages() {
    for closed in [500, 5000, 20_000] {
        let dir = tempfile::tempdir().unwrap();
        crate::state::with_test_state_root(dir.path(), || {
            let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
            let team = legacy(closed);
            let payload_before = serde_json::to_vec(&team).unwrap().len();
            let status_before = crate::state::teams::commands::view_with_cancelled(&team, &[], Some(&[]), true).to_string().len();
            let first = serde_json::to_value(&team.tasks[0]).unwrap();
            insert(&store, &team);
            assert!(store.mutate(None, &team.id, None, |_, _| bail!("refused")).is_err());
            let count: i64 = store.db.query_row("SELECT count(*) FROM team_task_archive", [], |row| row.get(0)).unwrap();
            assert_eq!(count, 0);
            assert_eq!(store.task(&team.id, 1).unwrap(), first);
            store.mutate(None, &team.id, None, |_, _| Ok(json!({}))).unwrap();
            let migrated = store.load(&team.id).unwrap().unwrap();
            let payload_after = serde_json::to_vec(&migrated).unwrap().len();
            let status = crate::state::teams::commands::view_with_cancelled(&migrated, &[], Some(&[]), true);
            let status_after = status.to_string().len();
            let stubs: Vec<_> = migrated.tasks.iter().filter(|task| task.archived.is_some()).map(super::super::model::task_stub).collect();
            let stub_bytes = serde_json::to_vec(&stubs).unwrap().len();
            use std::io::Write;
            writeln!(std::io::stderr(), "closed={closed}, open=20: payload {payload_before} -> {payload_after} B; status {status_before} -> {status_after} B; {} stubs = {stub_bytes} B", stubs.len()).unwrap();
            assert_eq!(stubs.len(), (closed as usize).min(1000));
            assert!(stub_bytes < 500_000);
            assert!(status_after < 2 * 1024 * 1024);
            assert_eq!(store.task(&team.id, 1).unwrap(), first);
            assert_eq!(store.task(&team.id, closed + 1).unwrap()["id"], closed + 1);
            assert!(store.task(&team.id, closed + 21).is_err());
            assert!(!status["phase_summary"].as_array().unwrap().iter().any(|phase| phase["total"] == 0));
            let archived: u32 = store.db.query_row("SELECT count(*) FROM team_task_archive WHERE team=?1", [&team.id], |row| row.get(0)).unwrap();
            assert_eq!(archived, closed);
            assert_eq!(migrated.last_archived_task, closed);
            assert!(super::super::plan::satisfied(&migrated, 1, Default::default()));
            assert_eq!(migrated.archive_totals.phases, closed as usize / 50);
            assert_eq!(migrated.archive_totals.accepted, closed as usize);
            assert!(migrated.phases.iter().filter(|phase| phase.archived.is_some()).count() <= 200);
            let headers = store.archived_phases(&team.id, 0, 50).unwrap();
            assert_eq!(headers["items"][0]["n"], 1);
            assert_eq!(headers["items"][0]["archived"]["tasks"], 50);
            if closed > 10_000 { assert!(migrated.phases.iter().all(|phase| phase.n != 1)); }
            let page = store.archived_tasks(&team.id, 1, 0, 17).unwrap();
            assert_eq!(page["items"].as_array().unwrap().len(), 17);
            assert_eq!(page["next_cursor"], 17);
            let next = store.archived_tasks(&team.id, 1, 17, 50).unwrap();
            assert_eq!(next["items"][0]["id"], 18);
            assert_eq!(next["items"].as_array().unwrap().len(), 33);
            assert!(next["next_cursor"].is_null());
            let (heads, _) = store.heads(1).unwrap();
            assert_eq!(heads[0]["name"], team.name);
            assert_eq!(heads[0]["lead"], "lead");
            store.mutate(None, &team.id, None, |_, _| Ok(json!({}))).unwrap();
            assert_eq!(store.task(&team.id, 1).unwrap(), first);
            assert_eq!(store.archived_tasks(&team.id, 1, 17, 50).unwrap(), next);
            assert_eq!(store.load(&team.id).unwrap().unwrap().archive_totals.accepted, closed as usize);
            if closed == 20_000 {
                let request = json!({"task":1});
                store.mutate(Some(("22222222-2222-4222-8222-222222222222", &request)), &team.id, None, |team, _| {
                    team.task_mut(1).unwrap().status = TaskStatus::Pending;
                    Ok(json!({}))
                }).unwrap();
                let restored = store.load(&team.id).unwrap().unwrap();
                assert_eq!(restored.archive_totals.accepted, closed as usize - 50);
                assert_eq!(store.task(&team.id, 1).unwrap()["attempts"], first["attempts"]);
                assert_eq!(store.task(&team.id, 1).unwrap()["status"], "pending");
            }
        });
    }
}

#[test]
fn archive_rolls_back_and_waits_for_owner_acceptance() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let mut team = legacy(50);
    team.phases[0].acceptance = PhaseAcceptance::Owner;
    team.phases[0].decided_at = Some(0.5);
    insert(&store, &team);
    store.mutate(None, &team.id, None, |_, _| Ok(json!({}))).unwrap();
    assert!(store.load(&team.id).unwrap().unwrap().tasks[0].archived.is_none());
    let error = store.mutate(None, &team.id, None, |team, _| {
        team.phases[0].state = PhaseState::Done;
        bail!("refused")
    });
    assert!(error.is_err());
    assert!(store.load(&team.id).unwrap().unwrap().tasks[0].archived.is_none());
    store.mutate(None, &team.id, None, |team, events| {
        team.phases[0].state = PhaseState::Done;
        events.push(Event::new("phase_owner_accepted", json!({"phase":1})));
        Ok(json!({}))
    }).unwrap();
    let archived = store.load(&team.id).unwrap().unwrap();
    assert!(archived.tasks[0].archived.is_some());
    assert!(archived.phases[0].archived.as_ref().unwrap().closed_at.unwrap() > 4.0);
    let request = json!({"task":1});
    store.mutate(Some(("11111111-1111-4111-8111-111111111111", &request)), &team.id, None, |team, _| {
        let task = team.task_mut(1).unwrap();
        assert_eq!(task.attempts[0].report.as_ref().unwrap().text, "r".repeat(10_240));
        task.status = TaskStatus::Pending;
        Ok(json!({}))
    }).unwrap();
    assert_eq!(store.task(&team.id, 1).unwrap()["status"], "pending");
    assert_eq!(store.task(&team.id, 2).unwrap()["attempts"], serde_json::to_value(&team.tasks[1].attempts).unwrap());
}

#[test]
fn archive_full_database_refuses_without_losing_the_live_record() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let mut team = legacy(50);
    team.phases[0].decided_at = Some(0.5);
    insert(&store, &team);
    let before: String = store.db.query_row("SELECT payload FROM teams WHERE id=?1", [&team.id], |row| row.get(0)).unwrap();
    let pages: i64 = store.db.pragma_query_value(None, "page_count", |row| row.get(0)).unwrap();
    store.db.pragma_update(None, "max_page_count", pages).unwrap();
    let error = store.mutate(None, &team.id, None, |_, _| Ok(json!({}))).unwrap_err().to_string();
    assert!(error.contains("teams.sqlite3") && error.contains("262144") && error.contains("no changes were saved"), "{error}");
    let after: String = store.db.query_row("SELECT payload FROM teams WHERE id=?1", [&team.id], |row| row.get(0)).unwrap();
    assert_eq!(before, after);
    let tasks: i64 = store.db.query_row("SELECT count(*) FROM team_task_archive", [], |row| row.get(0)).unwrap();
    let phases: i64 = store.db.query_row("SELECT count(*) FROM team_phase_archive", [], |row| row.get(0)).unwrap();
    assert_eq!((tasks, phases), (0, 0));
    assert_eq!(store.task(&team.id, 1).unwrap()["attempts"][0]["report"]["text"], "r".repeat(10_240));
    store.db.pragma_update(None, "max_page_count", 262144).unwrap();
    store.mutate(None, &team.id, None, |_, _| Ok(json!({}))).unwrap();
    assert_eq!(store.load(&team.id).unwrap().unwrap().phases[0].archived.as_ref().unwrap().closed_at, Some(4.0));
}

#[test]
fn archive_waits_until_merge_ranges_and_queue_entries_close() {
    use crate::state::teams::{merge_queue, model::{MergeEntry, MergeRange}};
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let mut team = legacy(50);
    let range = MergeRange { task: 1, prev: "before".into(), merged: "after".into() };
    team.tasks[0].untested_merge = Some(range.clone());
    insert(&store, &team);
    store.mutate(None, &team.id, None, |_, _| Ok(json!({}))).unwrap();
    let live = store.load(&team.id).unwrap().unwrap();
    assert!(live.tasks[0].archived.is_none());
    assert_eq!(merge_queue::open_merge_ranges(&live), vec![range.clone()]);
    store.mutate(None, &team.id, None, |team, _| {
        merge_queue::close_merge_ranges(team, std::slice::from_ref(&range));
        team.task_mut(1).unwrap().merge = Some(MergeEntry { state: "queued".into(), ..Default::default() });
        Ok(json!({}))
    }).unwrap();
    assert!(store.load(&team.id).unwrap().unwrap().tasks[0].archived.is_none());
    store.mutate(None, &team.id, None, |team, _| {
        team.task_mut(1).unwrap().merge.as_mut().unwrap().state = "testing".into();
        Ok(json!({}))
    }).unwrap();
    assert!(store.load(&team.id).unwrap().unwrap().tasks[0].archived.is_none());
    store.mutate(None, &team.id, None, |team, _| {
        team.task_mut(1).unwrap().merge = None;
        Ok(json!({}))
    }).unwrap();
    assert!(store.load(&team.id).unwrap().unwrap().tasks[0].archived.is_some());
    assert_eq!(store.task(&team.id, 1).unwrap()["attempts"], serde_json::to_value(&team.tasks[0].attempts).unwrap());
}

#[test]
fn archive_small_phases_evict_stubs_together_with_their_headers() {
    for per_phase in [1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        let mut team = legacy(0);
        team.tasks = (1..=1200).map(|id| Task { id, phase: (id - 1) / per_phase + 1,
            title: format!("Task {id}"), status: TaskStatus::Accepted, ..Default::default() }).collect();
        team.phases = (1..=1200/per_phase).map(|n| Phase { n, state: PhaseState::Approved,
            tasks: ((n-1)*per_phase+1..=n*per_phase).collect(), ..Default::default() }).collect();
        insert(&store, &team);
        store.mutate(None, &team.id, None, |_, _| Ok(json!({}))).unwrap();
        let archived = store.load(&team.id).unwrap().unwrap();
        assert_eq!(archived.phases.len(), ARCHIVED_PHASE_WINDOW);
        assert_eq!(archived.tasks.len(), ARCHIVED_PHASE_WINDOW * per_phase as usize);
        for phase in crate::state::teams::plan::phase_summary(&archived) {
            assert_eq!(phase["state"], "done", "{phase}");
            assert!(archived.phases.iter().any(|header| json!(header.n) == phase["n"]));
        }
        assert_eq!(archived.archive_totals.accepted, 1200);
        assert_eq!(store.task(&team.id, 1).unwrap()["title"], "Task 1");
    }
}

#[test]
fn archive_prerequisites_resolve_evicted_tasks_in_live_and_proposed_requirements() {
    use crate::state::teams::{model::{Member, Prerequisite}, ops::{self, Actor}};
    use std::collections::BTreeSet;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        let mut team = legacy(1050);
        team.settings.phase_checks.unresolved_prerequisites = true;
        team.members = vec![
            Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() },
            Member { id: "dev".into(), role: "Developer".into(), agent: "claude".into(), ..Default::default() },
        ];
        for task in &mut team.tasks {
            task.assignee = "dev".into();
            task.deps.clear(); // Only requires may bring these evicted tasks back.
        }
        for (n, id, requires) in [(23, 1071, vec![Prerequisite::Task { task: 1 }]), (24, 1072, vec![])] {
            team.tasks.push(Task { id, phase: n, title: format!("Phase {n}"), assignee: "dev".into(), requires, ..Default::default() });
            team.phases.push(Phase { n, tasks: vec![id], state: PhaseState::Draft, ..Default::default() });
        }
        insert(&store, &team);
        store.mutate(None, &team.id, None, |_, _| Ok(json!({}))).unwrap();
        let loaded = store.load(&team.id).unwrap().unwrap();
        assert!(loaded.tasks.iter().all(|task| task.id != 1 && task.id != 2), "old tasks must leave the live window");
        assert_eq!(loaded.task(1).unwrap().status, TaskStatus::Accepted);
        assert!(loaded.archive_deps.contains_key(&1));
        let approve = json!({"phase":23});
        let result = store.mutate(Some(("11111111-1111-4111-8111-111111111111", &approve)), &team.id, None,
            |team, events| ops::apply(team, "approve-phase", &approve, &Actor::Human, &BTreeSet::new(), events)).unwrap();
        assert_eq!(result["status"], "phase_approved");
        let edit = json!({"phase":24,"tasks_update":[{"id":1072,"requires":[{"task":2},{"task":99999}]}]});
        store.mutate(Some(("22222222-2222-4222-8222-222222222222", &edit)), &team.id, None, |team, events| {
            assert_eq!(team.task(2).unwrap().status, TaskStatus::Accepted, "the proposed input must resolve its archived requirement");
            ops::apply(team, "edit-draft", &edit, &Actor::Human, &BTreeSet::new(), events)
        }).unwrap();
        let loaded = store.load(&team.id).unwrap().unwrap();
        let checks = loaded.phases.iter().find(|phase| phase.n == 24).unwrap().checks.as_ref().unwrap();
        assert_eq!(checks.unresolved, vec!["Task #1072: task #99999 does not exist"]);
        let approve = json!({"phase":24});
        let error = store.mutate(Some(("33333333-3333-4333-8333-333333333333", &approve)), &team.id, None,
            |team, events| ops::apply(team, "approve-phase", &approve, &Actor::Human, &BTreeSet::new(), events)).unwrap_err().to_string();
        assert!(error.contains("Unresolved prerequisites") && error.contains("#99999"), "{error}");
    });
}

#[test]
fn archived_task_stubs_and_the_full_task_keep_the_origin() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        let mut team = legacy(50);
        team.tasks[1].origin = Some(1);
        insert(&store, &team);
        store.mutate(None, &team.id, None, |_, _| Ok(Value::Null)).unwrap();
        let saved = store.load(&team.id).unwrap().unwrap();
        assert!(saved.task(2).unwrap().archived.is_some());
        assert_eq!(saved.task(2).unwrap().origin, Some(1));
        let page = store.archived_tasks(&team.id, 1, 0, 50).unwrap();
        let stub = page["items"].as_array().unwrap().iter().find(|task| task["id"] == 2).unwrap();
        assert_eq!(stub["origin"], 1);
        assert_eq!(store.task(&team.id, 2).unwrap()["origin"], 1);
    });
}

#[test]
fn evicted_phase_modes_are_pruned_while_live_modes_and_free_planning_survive() {
    use crate::state::teams::model::PlanMode;
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        let mut team = legacy(0);
        team.phases = (1..=202).map(|n| Phase {
            n, state: PhaseState::Approved, tasks: vec![n], ..Default::default()
        }).collect();
        team.tasks = (1..=202).map(|id| Task {
            id, phase: id, status: if id <= 201 { TaskStatus::Accepted } else { TaskStatus::Pending },
            ..Default::default()
        }).collect();
        team.phase_modes = [(1, PlanMode::Free), (2, PlanMode::Free), (202, PlanMode::Standard)].into();
        insert(&store, &team);
        store.mutate(None, &team.id, None, |_, _| Ok(Value::Null)).unwrap();
        let mut saved = store.load(&team.id).unwrap().unwrap();
        assert!(!saved.phase_modes.contains_key(&1));
        assert_eq!(saved.phase_modes.get(&2), Some(&PlanMode::Free));
        assert_eq!(saved.phase_modes.get(&202), Some(&PlanMode::Standard));
        assert_eq!(super::super::ops::free_phase(&saved), None, "archived free phases stay closed");
        saved.phase_modes.insert(202, PlanMode::Free);
        assert_eq!(super::super::ops::open_phase(&saved), 202);
        assert_eq!(super::super::ops::free_phase(&saved), Some(202));
    });
}

#[test]
fn a_follow_up_resolves_an_evicted_origin_without_restoring_it_into_the_plan() {
    use crate::state::teams::{model::Member, ops::{self, Actor}, plan};
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
        let mut team = legacy(0);
        team.members = vec![
            Member { id: "lead".into(), role: "Lead".into(), agent: "claude".into(), ..Default::default() },
            Member { id: "dev".into(), role: "Backend".into(), agent: "codex".into(), ..Default::default() },
        ];
        team.phases = (1..=202).map(|n| Phase {
            n, state: PhaseState::Approved, tasks: vec![n], ..Default::default()
        }).collect();
        team.tasks = (1..=202).map(|id| Task {
            id, title: format!("Task {id}"), assignee: "dev".into(), phase: id,
            status: if id <= 201 { TaskStatus::Accepted } else { TaskStatus::Pending },
            ..Default::default()
        }).collect();
        insert(&store, &team);
        store.mutate(None, &team.id, None, |_, _| Ok(Value::Null)).unwrap();
        assert!(store.load(&team.id).unwrap().unwrap().tasks.iter().all(|task| task.id != 1));
        let input = json!({"tasks":[{"id":203,"title":"Follow up","assignee":"dev","origin":1,"deps":[{"task":202}]}]});
        store.mutate(Some(("11111111-1111-4111-8111-111111111112", &input)), &team.id, None, |team, events| {
            ops::apply(team, "extend", &input, &Actor::Human, &Default::default(), events)
        }).unwrap();
        let saved = store.load(&team.id).unwrap().unwrap();
        assert_eq!(saved.task(203).unwrap().origin, Some(1));
        assert!(saved.task(1).unwrap().archived.is_some(), "resolved through the indexed archive helper");
        assert!(saved.tasks.iter().all(|task| task.id != 1), "the origin stays out of the live plan");
        let layers = plan::layers(&saved.tasks);
        assert_eq!(layers.get(&203), Some(&1), "the hard dependency still places the follow-up");
        assert_eq!(store.task(&team.id, 203).unwrap()["origin"], 1);
    });
}
