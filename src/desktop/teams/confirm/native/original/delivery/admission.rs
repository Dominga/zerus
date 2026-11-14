//! Concurrent writers exercise complete Native::deliver and the real spawn.
use super::*;
use std::sync::{atomic::AtomicUsize, mpsc, Mutex};
use std::thread::JoinHandle;

#[derive(Clone, Copy, Debug)]
enum Change {
    Trust,
    Resume,
    TeamPause,
    MemberPause,
    MemberStop,
}
impl Change {
    fn kind(self) -> Kind {
        if matches!(self, Self::Trust) {
            Kind::Trust
        } else {
            Kind::Resume
        }
    }
    fn apply(self, db: &rusqlite::Connection) -> rusqlite::Result<usize> {
        match self {
            Self::Trust => db.execute("UPDATE teams SET settings=json_set(settings,'$.delegations.confirm_trust',json('false'))", []),
            Self::Resume => db.execute("UPDATE teams SET settings=json_set(settings,'$.delegations.resume_exited',json('false'))", []),
            Self::TeamPause => db.execute("UPDATE teams SET status='\"paused\"'", []),
            Self::MemberStop => db.execute("UPDATE members SET person_stopped=1,data=json_set(data,'$.state','paused') WHERE id='m1'", []),
            Self::MemberPause => db.execute("UPDATE members SET data=json_set(data,'$.state',?1) WHERE id='m1'", ["paused"]),
        }
    }
}
fn changes() -> [Change; 5] {
    [
        Change::Trust,
        Change::Resume,
        Change::TeamPause,
        Change::MemberPause,
        Change::MemberStop,
    ]
}
#[test]
fn a_revocation_committed_before_admission_starts_nothing() {
    for change in changes() {
        let mut f = Fixture::new(change.kind());
        let state = f.own.config.state.clone();
        let result = f.deliver(move || {
            change.apply(&team_db(&state)).unwrap();
        });
        assert!(matches!(result, Outcome::NotStarted(_)), "{change:?}");
        assert!(!f.called(), "{change:?}");
    }
}
thread_local! { static WAITING:RefCell<Option<mpsc::Sender<()>>>=const{RefCell::new(None)}; }
fn busy(count: i32) -> bool {
    WAITING.with(|s| {
        if let Some(tx) = s.borrow_mut().take() {
            let _ = tx.send(());
        }
    });
    std::thread::sleep(Duration::from_millis(1));
    count < 3000
}
fn writer(
    state: PathBuf,
    change: Change,
    committed: Arc<AtomicUsize>,
) -> (mpsc::Receiver<()>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        WAITING.with(|s| *s.borrow_mut() = Some(tx));
        let db = team_db(&state);
        db.busy_handler(Some(busy)).unwrap();
        change.apply(&db).unwrap();
        committed.store(1, Ordering::Release);
    });
    (rx, handle)
}
fn assert_reservation(state: &Path, row: &Record) {
    let db = crate::approval_log::open(&state.join("approval-audit")).unwrap();
    let status: String = db
        .query_row(
            "SELECT status FROM attempts WHERE request_id=?1",
            [&row.audit.as_ref().unwrap().request],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "prepared");
}
#[test]
fn writers_behind_admission_commit_after_spawn_and_before_the_child_finishes() {
    for change in changes() {
        let mut f = Fixture::new(change.kind());
        let state = f.own.config.state.clone();
        let committed = Arc::new(AtomicUsize::new(0));
        let task = Arc::new(Mutex::new(None::<JoinHandle<()>>));
        let (hold, done) = (task.clone(), committed.clone());
        let row = f.row.clone();
        LOCKED_FACTS.with(|s| {
            *s.borrow_mut() = Some(Box::new(move || {
                assert_reservation(&state, &row);
                let (blocked, thread) = writer(state, change, done.clone());
                *hold.lock().unwrap() = Some(thread);
                blocked
                    .recv_timeout(Duration::from_secs(4))
                    .expect("writer must meet the admission lock");
                assert_eq!(done.load(Ordering::Acquire), 0);
            }))
        });
        let done = committed.clone();
        original_child::AFTER_FINAL_CHECK.with(|s| {
            *s.borrow_mut() = Some(Box::new(move || {
                std::thread::sleep(Duration::from_millis(30));
                assert_eq!(
                    done.load(Ordering::Acquire),
                    0,
                    "transaction released before spawn"
                );
            }))
        });
        let (hold, done) = (task.clone(), committed.clone());
        original_child::AFTER_SPAWN.with(|s| {
            *s.borrow_mut() = Some(Box::new(move || {
                assert!(original_child::SPAWNED.with(|s| s.get()).is_some());
                hold.lock().unwrap().take().unwrap().join().unwrap();
                assert_eq!(done.load(Ordering::Acquire), 1);
            }))
        });
        let result = f.deliver(|| {});
        assert!(matches!(result, Outcome::Approved(_)), "{change:?}");
        assert!(f.called());
        assert_eq!(committed.load(Ordering::Acquire), 1);
        assert!(task.lock().unwrap().is_none());
    }
}
#[test]
fn committed_after_facts_controls_cannot_cross_the_spawn() {
    for change in [Change::Trust, Change::TeamPause] {
        let mut f = Fixture::new(change.kind());
        let state = f.own.config.state.clone();
        AFTER_FACTS.with(|s| {
            *s.borrow_mut() = Some(Box::new(move || {
                change.apply(&team_db(&state)).unwrap();
            }))
        });
        let result = f.deliver(|| {});
        assert!(
            matches!(result, Outcome::NotStarted(_)),
            "committed {change:?} crossed admission"
        );
        assert!(!f.called());
    }
}

#[test]
fn a_busy_admission_has_a_bounded_wait_and_starts_nothing() {
    let mut f = Fixture::new(Kind::Trust);
    let db = team_db(&f.own.config.state);
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = Instant::now();
    let result = f.deliver(|| {});
    assert!(!f.called(), "another writer still owns the admission lock");
    assert!(matches!(result, Outcome::Retry));
    assert!(started.elapsed() < Duration::from_secs(3));
    db.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn failed_spawn_releases_the_admission_without_running_a_child() {
    let mut f = Fixture::new(Kind::Trust);
    original_child::REFUSE_PRE_EXEC.with(|flag| flag.set(true));
    let result = f.deliver(|| {});
    assert!(matches!(result, Outcome::NotStarted(_)));
    assert!(!f.called());
    team_db(&f.own.config.state)
        .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
        .unwrap();
}

#[test]
fn the_store_and_native_capture_cannot_admit_a_members_binding_on_another_computer() {
    let mut f = Fixture::new(Kind::Trust);
    let other = uuid::Uuid::new_v4().to_string();
    let path = f.own.config.dir.join("swarm/catalog.json");
    let mut catalog: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    catalog["node_id"] = json!(&other);
    private_write(&path, catalog.to_string().as_bytes());
    let result = f.deliver_on(&other, || {});
    assert!(matches!(result, Outcome::NotStarted(_)));
    assert!(!f.called());
}
