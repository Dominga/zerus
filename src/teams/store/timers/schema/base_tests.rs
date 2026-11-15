//! Simulated future layouts exercise the real migration transaction on product-written stores.
use super::*;
use crate::teams::ops::tests::{base_advance::GitFixture, person};

fn next_layout(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE INDEX fixture_next_layout ON base_origins(base_commit)")
        .map_err(storage)
}

fn next_index(db: &Connection) -> u32 {
    db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name='fixture_next_layout'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn advance_base_private_fence_does_not_skip_the_next_layout_step() {
    let f = GitFixture::new();
    for pinned in [true, false] {
        let mut h = f.harness();
        if pinned {
            let request = f.request(&h, &f.next);
            f.apply(&mut h, &person(), &request).unwrap();
        }
        let db = h.store.db.as_ref().unwrap();
        let before = version(db).unwrap().unwrap();
        let target = EXTENSION + 1;
        let mut steps = STEPS.to_vec();
        steps.push((EXTENSION, next_layout));
        assert_eq!(next_index(db), 0);
        extend_layout(db, target, &steps).unwrap();
        assert_eq!(next_index(db), 1, "the next ordinary layout must run");
        assert_eq!(version(db).unwrap(), Some(before + 1));
        assert_eq!(marked_layout(db, target, &steps).unwrap(), Mark::Current);
        extend_layout(db, target, &steps).unwrap();
        assert_eq!(next_index(db), 1, "the step runs exactly once");
        assert_eq!(version(db).unwrap(), Some(before + 1));
    }
}

fn failed_next_layout(db: &Connection) -> Result<()> {
    next_layout(db)?;
    Err(storage("fixture next-layout failure"))
}

#[test]
fn advance_base_private_fence_survives_a_failed_next_layout_step() {
    let f = GitFixture::new();
    let mut h = f.harness();
    let request = f.request(&h, &f.next);
    f.apply(&mut h, &person(), &request).unwrap();
    let db = h.store.db.as_ref().unwrap();
    let before = version(db).unwrap();
    let mut steps = STEPS.to_vec();
    steps.push((EXTENSION, failed_next_layout));
    let problem = extend_layout(db, EXTENSION + 1, &steps).unwrap_err();
    assert_eq!(problem.code, code::STORAGE_FAILED);
    assert_eq!(version(db).unwrap(), before);
    assert_eq!(next_index(db), 0, "a partial migration is rolled back");
    steps.pop();
    steps.push((EXTENSION, next_layout));
    extend_layout(db, EXTENSION + 1, &steps).unwrap();
    assert_eq!(next_index(db), 1);
    assert_eq!(version(db).unwrap(), before.map(|v| v + 1));
}
