//! Product task document -> held service source -> independent writer.
use super::*;
use ops::observed_timing::{self, TimingInput};
use zerus_team_plan::observed_timing::{Category, Reason, DEFAULT_WINDOW_MS};

fn selected(h: &Harness, reference: &TaskRef, epoch: &str) -> super::super::timing::TimingRead {
    super::super::timing::observe(
        h.temp.path(),
        reference,
        epoch,
        Some("replica-fixture".into()),
        Some(100),
    )
    .unwrap()
    .unwrap()
}
fn append(h: &mut Harness, read: &super::super::timing::TimingRead, now: i64) -> ops::Reply {
    let revision = h.store.team(&read.team).unwrap().unwrap().revision;
    ops::apply(
        &mut h.store,
        &ops::tests::service(),
        &ops::tests::request(
            revision,
            Operation::RecordTimingFact {
                node: read.node,
                observation: Box::new(TimingInput::Task {
                    sample: Box::new(read.sample.clone()),
                }),
            },
        ),
        now,
    )
    .unwrap()
}

#[test]
fn held_product_document_coalesces_without_claiming_native_work_or_losing_its_lock() {
    let mut h = Harness::new();
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let epoch = uuid::Uuid::new_v4().to_string();
    let read = selected(&h, &reference, &epoch);
    let busy = ProjectStore::open_within(
        &h.temp.path().join("replica"),
        &reference.project,
        std::time::Duration::from_millis(20),
    )
    .unwrap();
    assert!(
        busy.is_none(),
        "the real source lock must survive through the timing writer"
    );
    append(&mut h, &read, 1_700_000_000_000);
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    assert!(!observed_timing::task_needed(
        &h.store,
        TEAM,
        read.node,
        &read.sample,
        1_700_000_000_001
    )
    .unwrap());
    let mut moved = read.sample.clone();
    moved.replica_offset = Some(101);
    assert!(
        !observed_timing::task_needed(&h.store, TEAM, read.node, &moved, 1_700_000_000_002)
            .unwrap(),
        "unrelated replica bytes do not produce per-tick facts"
    );
    assert_eq!(append(&mut h, &read, 1_700_000_000_002).revision, revision);
    assert!(
        observed_timing::task_needed(&h.store, TEAM, read.node, &moved, 1_700_000_060_001).unwrap()
    );
    let page = h
        .store
        .observed_timing(TEAM, 0, None, DEFAULT_WINDOW_MS, 200, None)
        .unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].category, Category::TaskTodo);
    assert!(page.rows[0].implementation_observed_ms.is_none());
    assert!(page.rows[0].source.event_sequence.is_none());
    assert!(page.rows[0].break_before);
    assert!(page.rows[0].reasons.contains(&Reason::SampledSource));
    drop(read);
    assert!(ProjectStore::open_within(
        &h.temp.path().join("replica"),
        &reference.project,
        std::time::Duration::from_millis(20)
    )
    .unwrap()
    .is_some());
}

#[test]
fn observer_restart_and_clock_reversal_are_immutable_points_not_intervals() {
    let mut h = Harness::new();
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let first = selected(&h, &reference, &uuid::Uuid::new_v4().to_string());
    append(&mut h, &first, 1_700_000_000_000);
    drop(first);
    let read = selected(&h, &reference, &uuid::Uuid::new_v4().to_string());
    append(&mut h, &read, 1_699_999_999_000);
    let page = h
        .store
        .observed_timing(TEAM, 0, None, DEFAULT_WINDOW_MS, 200, None)
        .unwrap();
    assert!(page.reasons.contains(&Reason::ClockDiscontinuity));
    assert_eq!(page.as_of_ms, Some(1_700_000_000_000));
    assert!(page
        .rows
        .iter()
        .all(|f| f.implementation_observed_ms.is_none() && f.break_before));
    assert!(page
        .rows
        .iter()
        .any(|f| f.reasons.contains(&Reason::ObserverRestart)));
}

#[test]
fn finite_sequence_lookahead_precedes_window_filter_even_for_a_long_unit_stream() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let mut h = Harness::new();
    let reference = document(&h);
    send(
        &caller(&h, &TaskActor::Person),
        &reference,
        TEAM,
        None,
        None,
    )
    .unwrap();
    let read = selected(&h, &reference, &uuid::Uuid::new_v4().to_string());
    for n in 0..1200 {
        append(&mut h, &read, 1_700_000_000_000 + n * 60_001);
    }
    let work = Arc::new(AtomicUsize::new(0));
    let count = work.clone();
    h.store.db.as_ref().unwrap().progress_handler(
        100,
        Some(move || count.fetch_add(100, Ordering::Relaxed) > 15_000),
    );
    let result = h
        .store
        .observed_timing(TEAM, 0, Some(read.node), 1, 200, None);
    h.store
        .db
        .as_ref()
        .unwrap()
        .progress_handler(0, None::<fn() -> bool>);
    let page = result.unwrap();
    eprintln!(
        "finite sequence window read: SQLite instructions approximately {}",
        work.load(Ordering::Relaxed)
    );
    assert!(work.load(Ordering::Relaxed) <= 15_100);
    assert_eq!(page.rows.len(), 1);
    assert!(page.more && page.truncated);
}
