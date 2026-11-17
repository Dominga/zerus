#![cfg(feature = "test-support")]
//! Temporary proof exhaustion cannot mutate or diagnose canonical data.
use std::{collections::BTreeMap, path::Path, time::Duration};
use zerus_replica::{
    with_proof_budget_for_test, Actor, Document, ErrorKind, NewTask, ProjectStore, Stage, Target,
    TaskDoc, Update,
};
fn files(path: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                visit(root, &p, out)
            } else if p.is_file() {
                out.insert(
                    p.strip_prefix(root).unwrap().into(),
                    std::fs::read(p).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(path, path, &mut out);
    out
}
fn fixture() -> (tempfile::TempDir, ProjectStore, TaskDoc) {
    let root = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(root.path(), "project-1").unwrap();
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: "T-a1b2c3",
        text: "Original task",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    store.save(&mut task).unwrap();
    (root, store, task)
}
fn busy<T>(result: zerus_replica::Result<T>) {
    let error = result.err().expect("incomplete proof cannot succeed");
    assert_eq!(error.kind(), ErrorKind::Busy);
}
#[test]
fn proof_busy_never_becomes_damaged_data_or_a_replacement_import() {
    let (_root, store, mut task) = fixture();
    let before = files(store.dir());
    let mut incoming = TaskDoc::from_bytes(&task.snapshot().unwrap()).unwrap();
    incoming
        .comment("Current descendant", &Actor::Person, 2)
        .unwrap();
    let bytes = incoming.snapshot().unwrap();
    with_proof_budget_for_test(Duration::from_secs(31), 131_072, || {
        busy(store.load(task.uuid()));
        busy(store.tasks());
        busy(store.inventory());
        busy(store.import_updates(vec![Update {
            document: Document::Task(task.uuid().into()),
            bytes: bytes.clone(),
        }]));
        busy(store.save(&mut task));
        let old = task.snapshot().unwrap();
        busy(task.import(&bytes));
        assert_eq!(old, task.snapshot().unwrap());
    });
    assert_eq!(
        files(store.dir()),
        before,
        "temporary exhaustion must not diagnose, fold, replace, pin or cache the copy"
    );
    assert_eq!(store.inventory().unwrap().1.len(), 0);
    assert_eq!(store.tasks().unwrap().1.len(), 0);
    assert_eq!(
        store
            .import_updates(vec![Update {
                document: Document::Task(task.uuid().into()),
                bytes
            }])
            .unwrap()
            .changed,
        1
    );
    assert_eq!(
        store
            .load(task.uuid())
            .unwrap()
            .view()
            .unwrap()
            .log
            .last()
            .unwrap()
            .text,
        "Current descendant"
    );
}
#[test]
fn proof_busy_save_keeps_the_original_and_a_fresh_retry_can_succeed() {
    let (_root, store, mut task) = fixture();
    task.comment("New comment", &Actor::Person, 2).unwrap();
    let before = files(store.dir());
    with_proof_budget_for_test(Duration::ZERO, 0, || busy(store.save(&mut task)));
    assert_eq!(files(store.dir()), before);
    store.save(&mut task).unwrap();
    assert_eq!(
        store
            .load(task.uuid())
            .unwrap()
            .view()
            .unwrap()
            .log
            .last()
            .unwrap()
            .text,
        "New comment"
    );
}
