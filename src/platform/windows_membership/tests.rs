use super::*;
use std::{cell::RefCell, time::Duration};

fn until() -> Instant {
    Instant::now() + Duration::from_secs(2)
}
fn set(bytes: &mut [u8], at: usize, width: usize, value: u64) {
    bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
}
fn row(pid: u32, created: u64, next: bool) -> Vec<u8> {
    let mut bytes = vec![0; 336];
    set(&mut bytes, 0, 4, if next { 336 } else { 0 });
    set(&mut bytes, 4, 4, 1);
    set(&mut bytes, 32, 8, created);
    set(&mut bytes, 80, 8, pid.into());
    bytes
}
#[test]
fn census_checks_every_record_and_never_turns_a_truncated_tail_into_clear() {
    let bytes = [row(0, 0, true), row(40, 100, true), row(80, 200, false)].concat();
    let valid = census(&bytes, until()).unwrap();
    assert_eq!(valid.len(), 2);
    assert_eq!(
        valid[&40],
        Process {
            pid: 40,
            created: 100
        }
    );
    for length in [0, 255, 335, 336, 337, 670, 1007] {
        assert_eq!(
            census(&bytes[..length], until()),
            Err(Error::Incomplete),
            "{length}"
        );
    }
    for (at, width, value) in [
        (0, 4, 8),
        (0, 4, 337),
        (0, 4, 4096),
        (4, 4, u32::MAX as u64),
        (336 + 32, 8, 0),
        (336 + 80, 8, u32::MAX as u64 + 1),
        (672 + 80, 8, 40),
    ] {
        let mut bad = bytes.clone();
        set(&mut bad, at, width, value);
        assert_eq!(census(&bad, until()), Err(Error::Incomplete), "offset {at}");
    }
    assert!(census(&[row(0, 0, true), row(0, 0, false)].concat(), until()).is_err());
    assert!(census(&row(0, 0, false), until()).is_err());
    assert!(census(&bytes, Instant::now()).is_err());
}
#[test]
fn idle_pseudo_process_birth_is_not_an_openable_process_identity() {
    for idle_birth in [0, 1, 134_000_000_000_000_000, u64::MAX] {
        let bytes = [row(0, idle_birth, true), row(40, 100, false)].concat();
        let rows = checked_census(&bytes, until()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[&40],
            Process {
                pid: 40,
                created: 100
            }
        );
        assert!(!rows.contains_key(&0));
    }
    for real_birth in [0, i64::MAX as u64 + 1, u64::MAX] {
        let bytes = [row(0, 1, true), row(40, real_birth, false)].concat();
        assert_eq!(checked_census(&bytes, until()), Err(CensusFault::Creation));
    }
    assert!(checked_census(&row(0, 1, false), until()).is_err());
    assert!(checked_census(&[row(0, 1, true), row(0, 1, false)].concat(), until()).is_err());
}
#[test]
fn only_a_next_header_needs_alignment_not_the_final_names_tail() {
    for extra in [2, 4, 6] {
        let mut bytes = row(40, 100, false);
        bytes.extend(vec![b'x'; extra]);
        assert_eq!(
            checked_census(&bytes, until()).unwrap()[&40],
            Process {
                pid: 40,
                created: 100
            }
        );
        // The identical offset cannot introduce a following header unaligned.
        let next = bytes.len() as u64;
        set(&mut bytes, 0, 4, next);
        bytes.extend(row(80, 200, false));
        assert_eq!(
            checked_census(&bytes, until()),
            Err(CensusFault::NextAlignment)
        );
    }
    for extra in [1, 3, 5, 7] {
        let mut bytes = row(40, 100, false);
        bytes.extend(vec![b'x'; extra]);
        assert_eq!(
            checked_census(&bytes, until()),
            Err(CensusFault::RecordSize)
        );
    }
}
#[test]
fn a_census_over_the_process_limit_is_not_a_complete_prefix() {
    let mut bytes = Vec::new();
    for pid in 1..=MAX_PROCESSES as u32 {
        bytes.extend(row(pid, u64::from(pid), true));
    }
    let last = bytes.len() - 336;
    set(&mut bytes[last..], 0, 4, 0);
    assert_eq!(census(&bytes, until()).unwrap().len(), MAX_PROCESSES);
    set(&mut bytes[last..], 0, 4, 336);
    bytes.extend(row(MAX_PROCESSES as u32 + 1, 1, false));
    assert_eq!(census(&bytes, until()), Err(Error::Incomplete));
}
#[test]
fn table_growth_has_one_deadline_exact_bounds_and_no_partial_success() {
    let mut calls = 0;
    let bytes = table(until(), |buffer| {
        calls += 1;
        if calls == 1 {
            (LENGTH_MISMATCH, 2 << 20)
        } else {
            buffer[0] = 7;
            (0, 8)
        }
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(bytes, 7u64.to_le_bytes());
    for (status, size) in [
        (ACCESS_DENIED, 0),
        (0, 0),
        (0, MAX_TABLE),
        (BUFFER_TOO_SMALL, MAX_TABLE + 1),
        (-1, 8),
    ] {
        assert_eq!(table(until(), |_| (status, size)), Err(Error::Incomplete));
    }
    let mut calls = 0;
    assert_eq!(
        table(until(), |_| {
            calls += 1;
            (LENGTH_MISMATCH, 0)
        }),
        Err(Error::Incomplete)
    );
    assert_eq!(calls, MAX_QUERIES);
    let mut calls = 0;
    assert!(table(Instant::now(), |_| {
        calls += 1;
        (0, 8)
    })
    .is_err());
    assert_eq!(calls, 0);
    let deadline = Instant::now() + Duration::from_millis(5);
    assert_eq!(
        table(deadline, |_| {
            std::thread::sleep(Duration::from_millis(10));
            (0, 8)
        }),
        Err(Error::Incomplete)
    );
}

struct Image {
    bytes: BTreeMap<u64, Vec<u8>>,
    calls: RefCell<BTreeMap<u64, usize>>,
    fail: Option<(u64, Error)>,
    change: Option<u64>,
    short: bool,
}
impl Memory for Image {
    fn read(&self, address: u64, length: usize, _: Instant) -> Result<Vec<u8>> {
        let mut calls = self.calls.borrow_mut();
        let count = calls.entry(address).or_default();
        *count += 1;
        if let Some((at, error)) = self.fail {
            if at == address {
                return Err(error);
            }
        }
        let mut bytes = self.bytes.get(&address).ok_or(Error::Incomplete)?.clone();
        assert_eq!(
            bytes.len(),
            length,
            "fixture must model the exact remote read"
        );
        if self.change == Some(address) && *count > 1 {
            bytes[0] ^= 2;
        }
        if self.short {
            bytes.pop();
        }
        Ok(bytes)
    }
}
fn image(layout: Layout) -> Image {
    let width = layout.width();
    let dir = layout.directory();
    let mut header = vec![0; dir + 3 * width];
    let size = header.len() as u64;
    set(&mut header, 0, 4, size);
    set(&mut header, 4, 4, size);
    set(&mut header, 8, 4, 1);
    let text: Vec<u8> = "C:\\private\\checkout\\"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    set(&mut header, dir, 2, text.len() as u64);
    set(&mut header, dir + 2, 2, text.len() as u64 + 2);
    set(&mut header, dir + width, width, 0x4000);
    set(&mut header, dir + 2 * width, width, 0x88);
    Image {
        bytes: BTreeMap::from([
            (
                0x1000 + layout.parameters() as u64,
                0x2000u64.to_le_bytes()[..width].to_vec(),
            ),
            (0x2000, header),
            (0x4000, text),
        ]),
        calls: RefCell::default(),
        fail: None,
        change: None,
        short: false,
    }
}
#[test]
fn native64_and_wow32_read_full_cwd_and_bracket_the_header_and_text() {
    for layout in [Layout::Native64, Layout::Wow32] {
        let fixture = image(layout);
        let cwd = current_directory(&fixture, 0x1000, layout, until()).unwrap();
        assert_eq!(
            String::from_utf16(&cwd.utf16).unwrap(),
            "C:\\private\\checkout\\"
        );
        assert_eq!(cwd.handle, 0x88);
        assert!(fixture.calls.borrow().values().all(|calls| *calls == 2));
        for changed in fixture.bytes.keys() {
            let mut fixture = image(layout);
            fixture.change = Some(*changed);
            assert_eq!(
                current_directory(&fixture, 0x1000, layout, until()),
                Err(Error::Incomplete)
            );
        }
    }
}
#[test]
fn unsupported_pointers_lengths_partial_reads_and_inconsistent_layouts_refuse() {
    for layout in [Layout::Native64, Layout::Wow32] {
        let dir = layout.directory();
        let width = layout.width();
        for (at, width, value) in [
            (0, 4, 0),
            (4, 4, 0),
            (8, 4, 0),
            (dir, 2, 1),
            (dir, 2, 0),
            (dir + 2, 2, 2),
            (dir + width, width, 0),
            (dir + width, width, 0x4001),
            (dir + width, width, u64::MAX),
            (dir + 2 * width, width, 0),
        ] {
            let mut fixture = image(layout);
            set(fixture.bytes.get_mut(&0x2000).unwrap(), at, width, value);
            assert_eq!(
                current_directory(&fixture, 0x1000, layout, until()),
                Err(Error::Incomplete)
            );
        }
        let mut fixture = image(layout);
        fixture.short = true;
        assert_eq!(
            current_directory(&fixture, 0x1000, layout, until()),
            Err(Error::Incomplete)
        );
        assert_eq!(
            current_directory(&image(layout), 0x1000, layout, Instant::now()),
            Err(Error::Incomplete)
        );
        let mut fixture = image(layout);
        fixture.bytes.get_mut(&0x4000).unwrap()[0..2].copy_from_slice(&[0, 0]);
        assert!(current_directory(&fixture, 0x1000, layout, until()).is_err());
    }
}
#[test]
fn a_real_per_read_denial_is_distinct_from_all_other_read_failures() {
    for layout in [Layout::Native64, Layout::Wow32] {
        for at in image(layout).bytes.keys() {
            for error in [Error::Denied, Error::Incomplete] {
                let mut fixture = image(layout);
                fixture.fail = Some((*at, error));
                assert_eq!(
                    current_directory(&fixture, 0x1000, layout, until()),
                    Err(error)
                );
            }
        }
    }
}
