use super::*;
use std::cell::Cell;
use std::rc::Rc;

struct Entries {
    total: usize,
    position: usize,
    bytes: usize,
    rows: bool,
    calls: usize,
    clock: Rc<Cell<u64>>,
    admission: Vec<u64>,
    delay: u64,
    yield_after: Option<usize>,
    session: String,
}
impl Source for Entries {
    fn next(&mut self, _store: &mut Store, remaining: usize, _until: Instant) -> Result<Read> {
        self.calls += 1;
        if self.yield_after == Some(self.position) {
            self.yield_after = None;
            return Ok(Read::Yield);
        }
        if self.position == self.total {
            return Ok(Read::End);
        }
        if self.bytes > remaining {
            return Ok(Read::Yield);
        }
        self.admission.push(self.clock.get());
        self.clock.set(self.clock.get() + self.delay);
        let n = self.position as u32 + 1;
        self.position += 1;
        let source = super::super::Source {
            kind: Kind::Native,
            key: format!("source-{n}"),
        };
        let process = Process {
            pid: n,
            birth: Birth::Linux(u64::from(n) + 100),
        };
        let publication = Publication {
            anchors: vec![Anchor {
                process: process.clone(),
                role: Role::Agent,
                source: source.clone(),
                revision: 1,
                fingerprint: "a".repeat(64),
                published_at: 1,
            }],
            edges: vec![Edge {
                process,
                role: Role::Agent,
                session_id: self.session.clone(),
                run: "original-run".into(),
                conversation: None,
                source: source.clone(),
                revision: 1,
                fingerprint: "a".repeat(64),
                published_at: 1,
            }],
        };
        Ok(Read::Entry {
            bytes: self.bytes,
            row: self.rows.then_some(Snapshot {
                source,
                revision: 1,
                publication,
            }),
        })
    }
}
fn fixture(total: usize, bytes: usize, rows: bool) -> (Store, Entries) {
    let store = Store::create(
        Connection::open_in_memory().unwrap(),
        Scope {
            machine: "scan-machine".into(),
            configuration: "scan-state".into(),
            boot: "scan-boot".into(),
        },
    )
    .unwrap();
    let source = Entries {
        total,
        position: 0,
        bytes,
        rows,
        calls: 0,
        clock: Rc::new(Cell::new(0)),
        admission: Vec::new(),
        delay: 0,
        yield_after: None,
        session: uuid::Uuid::new_v4().to_string(),
    };
    (store, source)
}
#[test]
fn sparse_large_directories_never_exceed_256_entries_per_step() {
    let (mut store, source) = fixture(100_000, 0, false);
    let mut scan = Scan::new(&mut store, source).unwrap();
    let step = scan.step(&mut store, Instant::now).unwrap();
    assert_eq!(step.entries, 256);
    assert_eq!(scan.source.calls, 256);
    assert!(!step.done);
}
#[test]
fn source_io_never_exceeds_two_mib_and_defers_the_next_entry() {
    let (mut store, source) = fixture(100, 128 * 1024, false);
    let mut scan = Scan::new(&mut store, source).unwrap();
    let first = scan.step(&mut store, Instant::now).unwrap();
    assert_eq!(first.bytes, STEP_BYTES);
    assert_eq!(scan.source.position, 16);
    let second = scan.step(&mut store, Instant::now).unwrap();
    assert_eq!(second.bytes, STEP_BYTES);
    assert_eq!(scan.source.position, 32);
}
#[test]
fn deadline_admits_no_new_entry_after_50_ms() {
    let (mut store, mut source) = fixture(1000, 10, false);
    source.delay = 3;
    let clock = source.clock.clone();
    let start = Instant::now();
    let mut scan = Scan::new(&mut store, source).unwrap();
    let step = scan
        .step(&mut store, || start + Duration::from_millis(clock.get()))
        .unwrap();
    assert!(step.entries > 0 && step.entries <= 17);
    assert!(!step.done);
    assert!(scan.source.admission.iter().all(|at| *at < 50));
}
#[test]
fn finished_native_scan_never_claims_complete_writer_coverage() {
    let (mut store, source) = fixture(8, 100, true);
    let mut scan = Scan::new(&mut store, source).unwrap();
    for _ in 0..10 {
        if scan.step(&mut store, Instant::now).unwrap().done {
            break;
        }
    }
    assert!(scan.done);
    assert!(store.progress().unwrap().scanned);
    assert!(
        !store.stamp().unwrap().complete,
        "uncovered writer families must remain unavailable"
    );
    assert!(
        store
            .current_source(&super::super::Source {
                kind: Kind::Native,
                key: "source-8".into()
            })
            .unwrap()
            .is_some()
    );
}
#[test]
fn yielded_source_is_not_spun_or_declared_finished() {
    let (mut store, mut source) = fixture(8, 100, true);
    source.yield_after = Some(0);
    let mut scan = Scan::new(&mut store, source).unwrap();
    let first = scan.step(&mut store, Instant::now).unwrap();
    assert!(first.yielded);
    assert!(!first.done);
    assert_eq!(scan.source.calls, 1);
    assert!(scan.step(&mut store, Instant::now).unwrap().done);
}
