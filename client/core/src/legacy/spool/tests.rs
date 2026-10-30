use super::*;
use serde_json::json;
use std::{
    io::Cursor,
    sync::{Barrier, Mutex},
    thread,
    time::Duration,
};

fn until() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
fn empty() -> &'static [u8] {
    br#"{"sessions":[],"future":{"kept":true}}"#
}
fn stage(pool: &Pool, bytes: &[u8]) -> Result<Staged, Error> {
    pool.reserve().unwrap().stage(
        Box::new(Cursor::new(Vec::new())),
        bytes,
        Limits::default(),
        Instant::now(),
        until(),
        &AtomicBool::new(false),
    )
}
fn error<T>(result: Result<T, Error>) -> Error {
    result.err().unwrap()
}

#[test]
fn reservations_cover_staging_retention_and_replay_without_double_release() {
    let pool = Pool::default();
    let first = stage(&pool, empty()).unwrap();
    let second = stage(&pool, empty()).unwrap();
    assert_eq!(pool.active(), 2);
    assert_eq!(pool.reserved_bytes(), 64 << 20);
    assert!(pool.reserve().is_none());
    let mut pages = 0;
    let listing = first
        .replay(until(), &AtomicBool::new(false), |rows| {
            assert!(rows.is_empty());
            assert_eq!(pool.active(), 2, "replay callback still owns its file");
            pages += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(pages, 1);
    assert_eq!(listing.rows, 0);
    assert_eq!(listing.metadata["future"]["kept"], true);
    assert_eq!(pool.active(), 1);
    drop(second);
    assert_eq!(pool.active(), 0);
    assert_eq!(pool.reserved_bytes(), 0);
    assert!(pool.reserve().is_some());
}

#[test]
fn a_large_legal_singleton_replays_unchanged() {
    let pool = Pool::default();
    let row = json!({"name":"codex/example/large","future":"x".repeat(700_000)});
    let input = serde_json::to_vec(&json!({"sessions":[row.clone()]})).unwrap();
    let staged = stage(&pool, &input).unwrap();
    assert_eq!(staged.listing().pages, 1);
    let mut pages = 0;
    let listing = staged
        .replay(until(), &AtomicBool::new(false), |items| {
            assert_eq!(items[0].row, row);
            pages += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!((pages, listing.rows), (1, 1));
    assert_eq!(pool.active(), 0);
}

#[test]
fn shared_pool_admits_only_two_concurrent_jobs() {
    let pool = Pool::default();
    let entered = Arc::new(Barrier::new(17));
    let release = Arc::new(Barrier::new(17));
    let accepted = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::new();
    for _ in 0..16 {
        let (p, a, b, c) = (
            pool.clone(),
            entered.clone(),
            release.clone(),
            accepted.clone(),
        );
        threads.push(thread::spawn(move || {
            let reservation = p.reserve();
            if reservation.is_some() {
                c.fetch_add(1, Ordering::Relaxed);
            }
            a.wait();
            b.wait();
            drop(reservation);
        }));
    }
    entered.wait();
    assert_eq!(accepted.load(Ordering::Relaxed), 2);
    assert_eq!(pool.active(), 2);
    release.wait();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(pool.active(), 0);
}

struct Rows {
    next: usize,
    count: usize,
    extra: usize,
    current: Cursor<Vec<u8>>,
    done: bool,
}
impl Rows {
    fn new(count: usize, extra: usize) -> Self {
        Self {
            next: 0,
            count,
            extra,
            current: Cursor::new(b"{\"sessions\":[".to_vec()),
            done: false,
        }
    }
}
impl Read for Rows {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            let n = self.current.read(out)?;
            if n != 0 || out.is_empty() {
                return Ok(n);
            }
            if self.done {
                return Ok(0);
            }
            let bytes = if self.next < self.count {
                let n = self.next;
                self.next += 1;
                let row = json!({"name":format!("codex/example/{}{n}", "x".repeat(self.extra)),
                    "run_id":format!("r{n}"), "process_work":{"count":7,"future":true}});
                let mut bytes = if n == 0 { Vec::new() } else { vec![b','] };
                bytes.extend(serde_json::to_vec(&row).unwrap());
                bytes
            } else {
                self.done = true;
                b"],\"machine_id\":\"example-machine\",\"tail\":{\"kept\":true}}".to_vec()
            };
            self.current = Cursor::new(bytes);
        }
    }
}

#[test]
fn large_listing_replays_bounded_pages_and_keeps_its_original_age() {
    let pool = Pool::default();
    let started = Instant::now();
    thread::sleep(Duration::from_millis(5));
    let staged = pool
        .reserve()
        .unwrap()
        .stage(
            Box::new(Cursor::new(Vec::new())),
            Rows::new(100_000, 0),
            Limits::default(),
            started,
            until(),
            &AtomicBool::new(false),
        )
        .unwrap();
    let observed = staged.observed_at();
    assert_eq!(observed, started);
    let bytes = staged.bytes();
    assert!(bytes < SPOOL_BYTES);
    assert_eq!(staged.listing().metadata["tail"]["kept"], true);
    thread::sleep(Duration::from_millis(5));
    let (mut rows, mut pages, mut high) = (0, 0, 0);
    let listing = staged
        .replay(until(), &AtomicBool::new(false), |page| {
            assert!(page.len() <= 128);
            assert!(page.iter().all(|i| i.row["process_work"]["count"] == 7));
            let size = serde_json::to_vec(page).unwrap().len();
            high = high.max(size);
            rows += page.len();
            pages += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(rows, 100_000);
    assert_eq!(listing.rows, rows);
    assert_eq!(listing.pages, pages);
    assert!(observed.elapsed() >= Duration::from_millis(5));
    assert_eq!(pool.active(), 0);
    eprintln!("spooled rows={rows}, pages={pages}, private bytes={bytes}, replay page highwater={high}bytes, reserved maximum={}bytes",SPOOL_BYTES*SPOOL_COUNT);
}

struct Counted(Arc<AtomicUsize>);
impl Read for Counted {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}
impl Seek for Counted {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Ok(self.0.load(Ordering::Relaxed) as u64)
    }
}
impl Write for Counted {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.fetch_add(b.len(), Ordering::Relaxed);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn encoded_spool_bound_refuses_before_input_bound_without_a_large_cache() {
    let pool = Pool::default();
    let written = Arc::new(AtomicUsize::new(0));
    let result = pool.reserve().unwrap().stage(
        Box::new(Counted(written.clone())),
        Rows::new(200_000, 160),
        Limits::default(),
        Instant::now(),
        until(),
        &AtomicBool::new(false),
    );
    assert_eq!(
        error(result),
        Error::TooLarge,
        "encoded identities add bytes independently of input"
    );
    assert!(written.load(Ordering::Relaxed) <= SPOOL_BYTES);
    assert!(written.load(Ordering::Relaxed) > 31 << 20);
    assert_eq!(pool.active(), 0);
}

#[derive(Clone)]
struct Shared(Arc<Mutex<Cursor<Vec<u8>>>>);
impl Read for Shared {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.0.lock().unwrap().read(b)
    }
}
impl Write for Shared {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Seek for Shared {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        self.0.lock().unwrap().seek(p)
    }
}
fn shared_stage(count: usize) -> (Pool, Staged, Shared) {
    let pool = Pool::default();
    let storage = Shared(Arc::new(Mutex::new(Cursor::new(Vec::new()))));
    let staged = pool
        .reserve()
        .unwrap()
        .stage(
            Box::new(storage.clone()),
            Rows::new(count, 0),
            Limits::default(),
            Instant::now(),
            until(),
            &AtomicBool::new(false),
        )
        .unwrap();
    (pool, staged, storage)
}
#[test]
fn corrupt_headers_payloads_trailing_and_truncated_storage_never_complete() {
    for mutation in 0..5 {
        let (pool, staged, storage) = shared_stage(1);
        {
            let mut cursor = storage.0.lock().unwrap();
            let bytes = cursor.get_mut();
            match mutation {
                0 => bytes[0] ^= 1,
                1 => bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes()),
                2 => bytes[8 + HEADER] ^= 1,
                3 => bytes.push(0),
                _ => {
                    bytes.pop();
                }
            }
        }
        let mut pages = 0;
        let result = staged.replay(until(), &AtomicBool::new(false), |_| {
            pages += 1;
            Ok(())
        });
        assert!(matches!(error(result), Error::Corrupt | Error::Storage));
        if mutation != 3 {
            assert_eq!(pages, 0);
        }
        assert_eq!(pool.active(), 0);
    }
}
#[test]
fn a_late_replay_error_abandons_the_whole_listing_after_provisional_pages() {
    let (pool, staged, storage) = shared_stage(300);
    {
        let mut cursor = storage.0.lock().unwrap();
        let bytes = cursor.get_mut();
        let size = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        bytes[8 + HEADER + size + HEADER] ^= 1;
    }
    let mut pages = 0;
    let result = staged.replay(until(), &AtomicBool::new(false), |_| {
        pages += 1;
        Ok(())
    });
    assert_eq!(error(result), Error::Corrupt);
    assert_eq!(pages, 1);
    assert_eq!(pool.active(), 0);
}

#[test]
fn cancelled_expired_failed_input_and_sink_release_their_reservations() {
    let pool = Pool::default();
    for (cancel, deadline) in [(true, until()), (false, Instant::now())] {
        let result = pool.reserve().unwrap().stage(
            Box::new(Cursor::new(Vec::new())),
            empty(),
            Limits::default(),
            Instant::now(),
            deadline,
            &AtomicBool::new(cancel),
        );
        assert_eq!(
            error(result),
            if cancel {
                Error::Cancelled
            } else {
                Error::TimedOut
            }
        );
        assert_eq!(pool.active(), 0);
    }
    assert_eq!(
        error(stage(&pool, b"{\"sessions\":[")),
        Error::Decode(listing::Error::Invalid)
    );
    for cancel in [true, false] {
        let staged = stage(&pool, empty()).unwrap();
        let result = staged.replay(
            if cancel { until() } else { Instant::now() },
            &AtomicBool::new(cancel),
            |_| panic!("no replay"),
        );
        assert_eq!(
            error(result),
            if cancel {
                Error::Cancelled
            } else {
                Error::TimedOut
            }
        );
        assert_eq!(pool.active(), 0);
    }
    let staged = stage(&pool, empty()).unwrap();
    let result = staged.replay(until(), &AtomicBool::new(false), |_| {
        Err(io::Error::other("private sink error"))
    });
    assert_eq!(error(result), Error::Sink);
    assert_eq!(pool.active(), 0);
    let (pool, staged, _) = shared_stage(300);
    let stop = AtomicBool::new(false);
    let mut pages = 0;
    let result = staged.replay(until(), &stop, |_| {
        pages += 1;
        stop.store(true, Ordering::Relaxed);
        Ok(())
    });
    assert_eq!(error(result), Error::Cancelled);
    assert_eq!(pages, 1);
    assert_eq!(pool.active(), 0);
}

struct Fault {
    cursor: Cursor<Vec<u8>>,
    at: &'static str,
}
impl Read for Fault {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if self.at == "read" {
            Err(io::Error::other("private read"))
        } else {
            self.cursor.read(b)
        }
    }
}
impl Write for Fault {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if self.at == "write" {
            Err(io::Error::other("private write"))
        } else {
            self.cursor.write(b)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.at == "flush" {
            Err(io::Error::other("private flush"))
        } else {
            Ok(())
        }
    }
}
impl Seek for Fault {
    fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
        if self.at == "seek" {
            Err(io::Error::other("private seek"))
        } else {
            self.cursor.seek(p)
        }
    }
}
#[test]
fn storage_failures_and_nonempty_objects_fail_without_truncating() {
    let pool = Pool::default();
    for at in ["seek", "write", "flush", "read"] {
        let result = pool.reserve().unwrap().stage(
            Box::new(Fault {
                cursor: Cursor::new(Vec::new()),
                at,
            }),
            empty(),
            Limits::default(),
            Instant::now(),
            until(),
            &AtomicBool::new(false),
        );
        let err = if at == "read" {
            error(
                result
                    .unwrap()
                    .replay(until(), &AtomicBool::new(false), |_| Ok(())),
            )
        } else {
            error(result)
        };
        assert_eq!(err, Error::Storage);
        assert!(!err.to_string().contains("private"));
        assert_eq!(pool.active(), 0);
    }
    let storage = Shared(Arc::new(Mutex::new(Cursor::new(b"keep".to_vec()))));
    let result = pool.reserve().unwrap().stage(
        Box::new(storage.clone()),
        empty(),
        Limits::default(),
        Instant::now(),
        until(),
        &AtomicBool::new(false),
    );
    assert_eq!(error(result), Error::Storage);
    assert_eq!(storage.0.lock().unwrap().get_ref(), b"keep");
    assert_eq!(pool.active(), 0);
}
