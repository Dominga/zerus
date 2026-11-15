//! Protocol migration of literal pre-upgrade tasks; no hand-written rows.
use super::*;
use zerus_replica::{Actor, Position, Stage, Target};

fn legacy(m: &Members, schema: i64) -> TaskDoc {
    let store = ProjectStore::open(&m.configs["a"].state.join("replica"), "p").unwrap();
    let (mut task, _) = store
        .new_named_task(
            "Historical task",
            "No conversation yet",
            &[],
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::End,
            "example",
            1,
            None,
            false,
        )
        .unwrap();
    // Schema 4 is the literal writer above. The other cases vary only the
    // raw old-schema metadata with the replica's public fixture helper.
    if schema != 4 {
        task.lower_stored_schema_for_test(schema);
    }
    store.save_legacy_for_test(&mut task).unwrap();
    assert!(!task.execution_managed());
    task
}
#[test]
fn all_legacy_schema_writers_reach_a_historical_root_without_starting() {
    for schema in 4..=7 {
        let m = members(&["a"], &[]);
        let task = legacy(&m, schema);
        let config = &m.configs["a"];
        let net = InProcess::new(&m, Faults::default());
        let mut worker = crate::swarm::replica::CutoverScan::default();
        assert!(worker.advance_with(config, &net).unwrap());
        let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
        let mut current = store.load(task.uuid()).unwrap();
        assert!(current.floor_record().is_some());
        assert!(current.cutover_seal().unwrap().is_some());
        assert_eq!(current.schema(), zerus_replica::TASK_SCHEMA);
        assert!(current.view().unwrap().execution.is_none());
        assert_eq!(current.stage().unwrap(), Stage::Todo);
        current
            .comment("Ordinary person edit", &Actor::Person, 2)
            .unwrap();
        store.save(&mut current).unwrap();
        drop(store);
        assert!(
            !record_path(&m, task.uuid()).exists(),
            "migration creates no dispatch certificate"
        );
    }
}
#[test]
fn a_hold_keeps_an_unsealed_task_and_its_original_files() {
    let m = members(&["a"], &[]);
    let task = legacy(&m, 4);
    let config = &m.configs["a"];
    {
        let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
        let mut project = store.project().unwrap();
        project.set_cutover_hold(true, &Actor::Person, 2).unwrap();
        store.save_project(&project).unwrap();
    }
    let net = InProcess::new(&m, Faults::default());
    assert!(catch_up_writer(config, &net, "p", task.uuid(), soon()).is_err());
    let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
    assert!(
        store
            .pending_cutover_capture(task.uuid())
            .unwrap()
            .is_none(),
        "a refused floor must not freeze the task"
    );
    drop(store);
    let current = held(&m, "a", task.uuid()).unwrap();
    assert_eq!(current.revision(), task.revision());
    assert!(current.floor_record().is_none() && current.cutover_seal().unwrap().is_none());
}
#[test]
fn returning_prefloor_writers_adopt_the_original_and_refuse_old_rule_edits() {
    for dirty in [false, true] {
        let m = members(&["a", "b"], &[("a", "b")]);
        let task = legacy(&m, 4);
        {
            let store = ProjectStore::open(&m.configs["b"].state.join("replica"), "p").unwrap();
            assert!(store
                .import_updates(vec![Update {
                    document: Document::Task(task.uuid().into()),
                    bytes: task.snapshot().unwrap()
                }])
                .unwrap()
                .refused
                .is_empty());
        }
        synced(&m, "a");
        let net = InProcess::new(
            &m,
            Faults {
                offline: BTreeSet::from([m.ids["b"].clone()]),
                ..Faults::default()
            },
        );
        catch_up_writer(&m.configs["a"], &net, "p", task.uuid(), soon()).unwrap();
        let first = held(&m, "a", task.uuid()).unwrap();
        let before = {
            let store = ProjectStore::open(&m.configs["b"].state.join("replica"), "p").unwrap();
            let mut old = store.load(task.uuid()).unwrap();
            if dirty {
                old.comment("Old-rule edit after disconnection", &Actor::Person, 3)
                    .unwrap();
                store.save_legacy_for_test(&mut old).unwrap();
            }
            old.revision()
        };
        synced(&m, "b");
        let net = InProcess::new(&m, Faults::default());
        let answer = catch_up_writer(&m.configs["b"], &net, "p", task.uuid(), soon());
        let current = held(&m, "b", task.uuid()).unwrap();
        if dirty {
            assert!(answer.is_err());
            assert_eq!(current.revision(), before, "refusal keeps original data");
            assert!(
                current.floor_record().is_none(),
                "a stale branch must not mint F2"
            );
        } else {
            answer.unwrap();
            assert_eq!(current.floor_record(), first.floor_record());
            assert_eq!(
                current.cutover_seal().unwrap().unwrap().identity(),
                first.cutover_seal().unwrap().unwrap().identity()
            );
        }
        assert!(!record_path(&m, task.uuid()).exists());
        assert!(current.view().unwrap().execution.is_none());
    }
}
