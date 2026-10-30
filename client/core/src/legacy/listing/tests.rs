use super::*;
use serde_json::json;
use std::{io::Cursor, time::Duration};

fn parse(bytes: &[u8]) -> Result<Listing, Error> {
    decode(
        bytes,
        Limits::default(),
        Instant::now() + Duration::from_secs(30),
        &AtomicBool::new(false),
        |_| Ok(()),
    )
}
struct Rows {
    count: usize,
    next: usize,
    current: Cursor<Vec<u8>>,
    done: bool,
    peak: usize,
}
impl Rows {
    fn new(count: usize) -> Self {
        Self {
            count,
            next: 0,
            current: Cursor::new(b"{\"sessions\":[".to_vec()),
            done: false,
            peak: 0,
        }
    }
}
impl Read for Rows {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            let n = self.current.read(out)?;
            if n > 0 || out.is_empty() {
                return Ok(n);
            }
            if self.done {
                return Ok(0);
            }
            let bytes = if self.next < self.count {
                let n = self.next;
                self.next += 1;
                let row = json!({"name":format!("codex/example/{n}"),"run_id":format!("run-{n}"),
                    "conversation_id":format!("conversation-{n}"),"process_work":{"count":7,"future":true}});
                let mut bytes = if n == 0 { Vec::new() } else { vec![b','] };
                bytes.extend(serde_json::to_vec(&row).unwrap());
                bytes
            } else {
                self.done = true;
                b"],\"host\":\"example-host\",\"machine_id\":\"example-machine\",\"future\":{\"kept\":true}}".to_vec()
            };
            self.peak = self.peak.max(bytes.len());
            self.current = Cursor::new(bytes);
        }
    }
}

#[test]
fn large_listing_is_generated_and_consumed_one_bounded_page_at_a_time() {
    let mut reader = Rows::new(100_000);
    let (mut rows, mut pages, mut high) = (0, 0, 0);
    let listing = decode(
        &mut reader,
        Limits::default(),
        Instant::now() + Duration::from_secs(30),
        &AtomicBool::new(false),
        |items| {
            assert!(items.len() <= PAGE_ROWS);
            let bytes = items
                .iter()
                .map(|i| serde_json::to_vec(i).unwrap().len() + 1)
                .sum::<usize>();
            assert!(bytes <= PAGE_BYTES);
            high = high.max(bytes);
            pages += 1;
            rows += items.len();
            for item in items {
                assert_eq!(item.key, catalog_key(&item.row).unwrap());
                assert_eq!(item.row["process_work"], json!({"count":7,"future":true}));
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(listing.rows, 100_000);
    assert_eq!(rows, listing.rows);
    assert_eq!(pages, listing.pages);
    assert_eq!(
        listing.metadata,
        json!({"host":"example-host","machine_id":"example-machine","future":{"kept":true}})
    );
    assert!(reader.peak < 1024);
    println!("decoded rows={rows}, pages={pages}, page highwater={high}bytes, generated row buffer={}bytes, input={}bytes",reader.peak,listing.bytes);
}

#[test]
fn arbitrary_field_order_empty_listing_and_unknown_fields_are_preserved() {
    for text in [
        r#"{"sessions":[],"host":"example"}"#,
        r#"{"host":"example","sessions":[]}"#,
    ] {
        let mut pages = 0;
        let parsed = decode(
            text.as_bytes(),
            Limits::default(),
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
            |rows| {
                assert!(rows.is_empty());
                pages += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(pages, 1);
        assert_eq!(parsed.rows, 0);
        assert_eq!(parsed.metadata, json!({"host":"example"}));
    }
}

#[test]
fn malformed_partial_duplicate_and_trailing_bodies_never_finish() {
    for bytes in [
        &b"{}"[..],
        b"[]",
        b"null",
        b"{\"sessions\":null}",
        b"{\"sessions\":{}}",
        b"{\"sessions\":[]",
        b"{\"sessions\":[]}junk",
        b"{\"sessions\":[],\"sessions\":[]}",
        b"{\"sessions\":[],\"host\":1,\"host\":2}",
        b"{\"sessions\":[{\"name\":\"\"}]}",
        b"{\"sessions\":[{\"name\":\"example\",\"run_id\":true}]}",
        b"{\"sessions\":[{\"name\":\"example\",\"conversation_id\":123}]}",
    ] {
        assert_eq!(
            parse(bytes).err(),
            Some(Error::Invalid),
            "case length {}",
            bytes.len()
        );
    }
}

#[test]
fn exact_byte_boundary_requires_real_eof_and_caps_rows() {
    let good = b"{\"sessions\":[]}";
    let run = |bytes: &[u8]| {
        decode(
            bytes,
            Limits {
                bytes: good.len(),
                rows: 1,
            },
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
            |_| Ok(()),
        )
    };
    assert_eq!(run(good).unwrap().bytes, good.len());
    assert_eq!(
        run(&[&good[..], b"x"].concat()).err(),
        Some(Error::TooLarge)
    );
    assert_eq!(
        decode(
            Rows::new(2),
            Limits {
                bytes: INPUT_BYTES,
                rows: 1
            },
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
            |_| Ok(())
        )
        .err(),
        Some(Error::TooLarge)
    );
    assert_eq!(
        decode(
            &good[..],
            Limits { bytes: 0, rows: 1 },
            Instant::now() + Duration::from_secs(1),
            &AtomicBool::new(false),
            |_| Ok(())
        )
        .err(),
        Some(Error::TooLarge)
    );
}

#[test]
fn field_count_key_length_and_aggregate_metadata_are_bounded() {
    let mut fields = Map::new();
    fields.insert("sessions".into(), json!([]));
    for n in 0..FIELDS {
        fields.insert(format!("field-{n}"), json!(n));
    }
    assert_eq!(
        parse(&serde_json::to_vec(&fields).unwrap()).err(),
        Some(Error::TooLarge)
    );
    let long = json!({"sessions":[],"x".repeat(257):0});
    assert_eq!(
        parse(&serde_json::to_vec(&long).unwrap()).err(),
        Some(Error::TooLarge)
    );
    let large = "x".repeat(200_000);
    assert_eq!(
        parse(&serde_json::to_vec(&json!({"sessions":[],"a":large,"b":large,"c":large})).unwrap())
            .err(),
        Some(Error::TooLarge)
    );
    // Escaped metadata keys are charged by encoded size too.
    let text = json!({"sessions":[],"\n".repeat(200):"x".repeat(METADATA_BYTES-300)});
    assert_eq!(
        parse(&serde_json::to_vec(&text).unwrap()).err(),
        Some(Error::TooLarge)
    );
}

#[test]
fn oversized_single_value_stops_before_consuming_the_full_body() {
    use std::sync::atomic::AtomicUsize;
    struct Count<'a, R> {
        inner: R,
        bytes: &'a AtomicUsize,
    }
    impl<R: Read> Read for Count<'_, R> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(out)?;
            self.bytes.fetch_add(n, Ordering::Relaxed);
            Ok(n)
        }
    }
    let bytes = AtomicUsize::new(0);
    let input = Cursor::new(b"{\"sessions\":[{\"name\":\"example\",\"value\":\"")
        .chain(io::repeat(b'x').take((2 * MAX_FRAME_BYTES) as u64))
        .chain(Cursor::new(b"\"}]}"));
    let parsed = decode(
        Count {
            inner: input,
            bytes: &bytes,
        },
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        &AtomicBool::new(false),
        |_| panic!("oversized row cannot stage"),
    );
    assert_eq!(parsed.err(), Some(Error::TooLarge));
    assert!(bytes.load(Ordering::Relaxed) <= MAX_FRAME_BYTES + BUFFER_BYTES + 128);
}

#[test]
fn legal_large_singleton_is_bounded_and_values_are_unchanged() {
    let row = json!({"name":"codex/example/session","run_id":"run-1","future":"x".repeat(700_000),"process_work":{"count":0}});
    let text = serde_json::to_vec(&json!({"sessions":[row]})).unwrap();
    let mut pages = 0;
    let result = decode(
        &text[..],
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        &AtomicBool::new(false),
        |items| {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].row, row);
            assert!(serde_json::to_vec(&items[0]).unwrap().len() < ITEM_BYTES);
            pages += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(result.rows, 1);
    assert_eq!(pages, 1);
}

#[test]
fn cancellation_and_deadlines_stop_before_or_during_input() {
    struct MustNotRead;
    impl Read for MustNotRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("cancelled before I/O")
        }
    }
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        decode(
            MustNotRead,
            Limits::default(),
            Instant::now() + Duration::from_secs(1),
            &cancelled,
            |_| Ok(())
        )
        .err(),
        Some(Error::Cancelled)
    );
    assert_eq!(
        decode(
            MustNotRead,
            Limits::default(),
            Instant::now(),
            &AtomicBool::new(false),
            |_| Ok(())
        )
        .err(),
        Some(Error::TimedOut)
    );
    struct CancelOnRead<'a, R> {
        inner: R,
        flag: &'a AtomicBool,
    }
    impl<R: Read> Read for CancelOnRead<'_, R> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.flag.store(true, Ordering::Relaxed);
            self.inner.read(bytes)
        }
    }
    let flag = AtomicBool::new(false);
    let input = io::repeat(b' ')
        .take((2 * BUFFER_BYTES) as u64)
        .chain(Cursor::new(b"{\"sessions\":[]}"));
    assert_eq!(
        decode(
            CancelOnRead {
                inner: input,
                flag: &flag
            },
            Limits::default(),
            Instant::now() + Duration::from_secs(1),
            &flag,
            |_| Ok(())
        )
        .err(),
        Some(Error::Cancelled)
    );
}

#[test]
fn cancellation_or_failure_after_a_page_never_grants_completion() {
    let flag = AtomicBool::new(false);
    let mut pages = 0;
    let result = decode(
        Rows::new(300),
        Limits::default(),
        Instant::now() + Duration::from_secs(5),
        &flag,
        |_| {
            pages += 1;
            flag.store(true, Ordering::Relaxed);
            Ok(())
        },
    );
    assert_eq!(result.err(), Some(Error::Cancelled));
    assert_eq!(pages, 1);
    let result = decode(
        Rows::new(1),
        Limits::default(),
        Instant::now() + Duration::from_secs(1),
        &AtomicBool::new(false),
        |_| Err(io::Error::other("private staging details")),
    );
    let error = result.err().unwrap();
    assert_eq!(error, Error::Sink);
    assert!(!error.to_string().contains("private staging details"));
}
