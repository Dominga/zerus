//! Placing tasks from a source that answers without reading every task
//! document (zerus-cg5h.24): hgs's task index. Its answers give the same
//! ranks as the documents, a declined or mismatched answer reads the
//! documents for the whole write, and an add reads only its neighbours.
use crate::{
    load_count_for_test, Actor, Placed, Placement, Position, ProjectStore, Result, Stage, Target,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn store(root: &Path) -> ProjectStore {
    ProjectStore::open(root, "project-1").unwrap()
}

fn file(store: &ProjectStore, text: &str, stage: Stage) -> String {
    let mut task = store
        .new_task(
            text,
            stage,
            Target::Any,
            Actor::Person,
            &Position::End,
            "orchard",
            1,
        )
        .unwrap();
    store.save(&mut task).unwrap();
    task.uuid().to_owned()
}

/// The documents' own order and short IDs, as an exact source answers.
#[derive(Default)]
struct Snapshot {
    order: BTreeMap<String, Vec<Placed>>,
    ids: BTreeSet<String>,
    declines: bool,
}

impl Snapshot {
    fn of(store: &ProjectStore) -> Self {
        let mut snapshot = Snapshot::default();
        for stage in [Stage::Todo, Stage::Backlog] {
            let placed = store
                .ordered(stage)
                .unwrap()
                .iter()
                .map(|t| Placed {
                    uuid: t.uuid().to_owned(),
                    id: t.id(),
                    rank: t.rank(),
                })
                .collect();
            snapshot.order.insert(stage.as_str().to_owned(), placed);
        }
        let (tasks, _) = store.tasks().unwrap();
        snapshot.ids = tasks.iter().map(|t| t.id()).collect();
        snapshot
    }
}

impl Placement for Snapshot {
    fn order(&self, stage: Stage) -> Result<Option<Vec<Placed>>> {
        Ok((!self.declines).then(|| self.order.get(stage.as_str()).cloned().unwrap_or_default()))
    }

    fn short_id_used(&self, id: &str) -> Result<Option<bool>> {
        Ok((!self.declines).then(|| self.ids.contains(id)))
    }
}

fn positions(store: &ProjectStore) -> Vec<Position> {
    let list = store.ordered(Stage::Todo).unwrap();
    vec![
        Position::Top,
        Position::End,
        Position::Index(0),
        Position::Index(2),
        Position::Index(99),
        Position::Before(list[1].id()),
        Position::After(list[2].uuid().to_owned()),
        Position::After(list.last().unwrap().id()),
    ]
}

#[test]
fn a_placed_task_gets_the_rank_the_documents_give() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    for n in 0..6 {
        file(&store, &format!("Task {n}"), Stage::Todo);
    }
    let snapshot = Snapshot::of(&store);
    for position in positions(&store) {
        let expected = store.rank_for(Stage::Todo, &position, None).unwrap();
        let placed = store
            .new_task_placed(
                Some(&snapshot),
                "New",
                Stage::Todo,
                Target::Any,
                Actor::Person,
                &position,
                "orchard",
                1,
            )
            .unwrap();
        assert_eq!(placed.rank(), expected, "{position:?}");
        assert!(!snapshot.ids.contains(&placed.id()));
    }
    // A move within the stage and a change of stage, as the documents place them.
    let list = store.ordered(Stage::Todo).unwrap();
    let mut moving = store.load(list[4].uuid()).unwrap();
    let expected = store
        .rank_for(Stage::Todo, &Position::Index(1), Some(moving.uuid()))
        .unwrap();
    store
        .reorder_placed(Some(&snapshot), &mut moving, &Position::Index(1))
        .unwrap();
    assert_eq!(moving.rank(), expected);
    let mut staged = store.load(list[0].uuid()).unwrap();
    let expected = store
        .rank_for(Stage::Backlog, &Position::Top, Some(staged.uuid()))
        .unwrap();
    store
        .change_stage_placed(
            Some(&snapshot),
            &mut staged,
            Stage::Backlog,
            &Actor::Person,
            2,
        )
        .unwrap();
    assert_eq!(staged.rank(), expected);
}

#[test]
fn an_add_reads_only_its_neighbours() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    for n in 0..40 {
        file(&store, &format!("Task {n}"), Stage::Todo);
    }
    let snapshot = Snapshot::of(&store);
    let before = load_count_for_test();
    store
        .new_task_placed(
            Some(&snapshot),
            "New",
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::Index(20),
            "orchard",
            1,
        )
        .unwrap();
    assert!(
        load_count_for_test() - before <= 2,
        "{} loads",
        load_count_for_test() - before
    );
    // Without a source every task is read, as before.
    let before = load_count_for_test();
    store
        .new_task_placed(
            None,
            "New",
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::Index(20),
            "orchard",
            1,
        )
        .unwrap();
    assert!(load_count_for_test() - before >= 40);
}

#[test]
fn a_declined_or_mismatched_answer_reads_the_documents_for_the_whole_write() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    for n in 0..5 {
        file(&store, &format!("Task {n}"), Stage::Todo);
    }
    // Declined.
    let mut declined = Snapshot::of(&store);
    declined.declines = true;
    let expected = store
        .rank_for(Stage::Todo, &Position::Index(2), None)
        .unwrap();
    let placed = store
        .new_task_placed(
            Some(&declined),
            "New",
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::Index(2),
            "orchard",
            1,
        )
        .unwrap();
    assert_eq!(placed.rank(), expected);
    // A neighbour whose rank the source has wrong (a writer it has not seen).
    let mut stale = Snapshot::of(&store);
    stale.order.get_mut("todo").unwrap()[1].rank = "Zz".into();
    let before = load_count_for_test();
    let placed = store
        .new_task_placed(
            Some(&stale),
            "New",
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::Index(2),
            "orchard",
            1,
        )
        .unwrap();
    assert_eq!(placed.rank(), expected);
    assert!(
        load_count_for_test() - before >= 5,
        "the whole write read the documents"
    );
    // A reference the source does not know is looked up in the documents too.
    let mut missing = Snapshot::of(&store);
    let gone = missing.order.get_mut("todo").unwrap().remove(3);
    let expected = store
        .rank_for(Stage::Todo, &Position::Before(gone.id.clone()), None)
        .unwrap();
    let placed = store
        .new_task_placed(
            Some(&missing),
            "New",
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::Before(gone.id),
            "orchard",
            1,
        )
        .unwrap();
    assert_eq!(placed.rank(), expected);
}

#[test]
fn a_short_id_the_source_knows_is_never_reused() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    file(&store, "Task", Stage::Todo);
    let mut snapshot = Snapshot::of(&store);
    // Every short ID but a handful is taken: the add keeps drawing until one is free.
    struct Taken<'a>(&'a Snapshot, std::cell::Cell<usize>);
    impl Placement for Taken<'_> {
        fn order(&self, stage: Stage) -> Result<Option<Vec<Placed>>> {
            self.0.order(stage)
        }
        fn short_id_used(&self, _id: &str) -> Result<Option<bool>> {
            self.1.set(self.1.get() + 1);
            Ok(Some(self.1.get() < 5))
        }
    }
    snapshot.ids.clear();
    let taken = Taken(&snapshot, std::cell::Cell::new(0));
    store
        .new_task_placed(
            Some(&taken),
            "New",
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::End,
            "orchard",
            1,
        )
        .unwrap();
    assert_eq!(taken.1.get(), 5);
}
