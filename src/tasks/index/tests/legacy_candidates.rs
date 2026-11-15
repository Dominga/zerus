use super::*;
#[test]
fn the_background_cursor_includes_every_supported_legacy_schema() {
    for schema in 1..zerus_replica::TASK_SCHEMA {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let mut task = new_task(&store, "T-a1b2c3", Stage::Todo, "V", "Literal old writer");
        task.lower_stored_schema_for_test(schema);
        store.save_legacy_for_test(&mut task).unwrap();
        age(&store, task.uuid());
        let mut index = Index::open(&store).unwrap();
        assert_eq!(
            index.cutover_candidate().unwrap().as_deref(),
            Some(task.uuid()),
            "schema {schema}"
        );
    }
}
#[test]
fn the_background_cursor_does_not_migrate_current_or_future_schema_data() {
    for schema in [zerus_replica::TASK_SCHEMA, zerus_replica::TASK_SCHEMA + 1] {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let mut task = new_task(&store, "T-a1b2c3", Stage::Todo, "V", "Current writer");
        store.save(&mut task).unwrap();
        if schema > zerus_replica::TASK_SCHEMA {
            task.lower_stored_schema_for_test(schema);
            store.save_legacy_for_test(&mut task).unwrap();
        }
        age(&store, task.uuid());
        let mut index = Index::open(&store).unwrap();
        assert!(
            index.cutover_candidate().unwrap().is_none(),
            "schema {schema}"
        );
    }
}
