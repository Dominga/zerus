use super::*;
use std::{io::Write, time::Duration};

fn until() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
fn original() -> Binding {
    Binding {
        request: uuid::Uuid::parse_str("a1823760-bf43-4bb3-a921-092c8c1b935f").unwrap(),
        checkout: Object {
            volume: 14,
            file: [7; 16],
        },
        deadline_ticks: 123_456,
    }
}
fn data() -> Data {
    Data {
        inventoried: 3,
        queried: 2,
        processes: vec![ProcessReads {
            pid: 25,
            created: 85,
            image: Read::Use {
                object: original().checkout,
            },
            cwd: Read::Denied {},
            files: Read::Incomplete {},
        }],
        unbound_uses: vec![UnboundUse {
            pid: 27,
            object: Object {
                volume: 14,
                file: [8; 16],
            },
        }],
        faults: vec![Fault::Census, Fault::ObjectQuery],
    }
}
fn spelling() -> Vec<u16> {
    r"C:\ZerusFixture\checkout".encode_utf16().collect()
}
fn frame(magic: &[u8; 8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = magic.to_vec();
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes
}
fn raw_report(data: Data) -> Vec<u8> {
    frame(
        REPORT,
        &serde_json::to_vec(&Report {
            scope: Scope::from(original()).unwrap(),
            data,
        })
        .unwrap(),
    )
}
fn mutate(bytes: &[u8], edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut value = serde_json::from_slice(&bytes[HEADER..]).unwrap();
    edit(&mut value);
    frame(
        bytes[..8].try_into().unwrap(),
        &serde_json::to_vec(&value).unwrap(),
    )
}

#[test]
fn original_request_and_unqualified_data_round_trip_without_changing_scope() {
    let input = request(original(), &spelling(), until()).unwrap();
    assert_eq!(
        read_request(&input, until()).unwrap(),
        (original(), spelling())
    );
    let observations = data();
    let bytes = report(original(), observations.clone(), until()).unwrap();
    assert_eq!(
        read_report(&bytes, original(), until()).unwrap(),
        observations
    );
    // Even empty data/equal counters have no Complete/Clear representation.
    let empty = Data {
        inventoried: 0,
        queried: 0,
        processes: vec![],
        unbound_uses: vec![],
        faults: vec![],
    };
    let bytes = report(original(), empty.clone(), until()).unwrap();
    assert_eq!(read_report(&bytes, original(), until()).unwrap(), empty);
    for label in ["complete", "clear", "consent", "stop", "force"] {
        let changed = mutate(&bytes, |v| v["data"][label] = true.into());
        assert!(read_report(&changed, original(), until()).is_err());
    }
}

#[test]
fn every_original_binding_field_is_required_and_is_never_renewed_from_output() {
    let bytes = report(original(), data(), until()).unwrap();
    let mut variants = [original(); 4];
    variants[0].request = uuid::Uuid::new_v4();
    variants[1].checkout.volume += 1;
    variants[2].checkout.file[15] ^= 1;
    variants[3].deadline_ticks += 1;
    for changed in variants {
        assert!(read_report(&bytes, changed, until()).is_err());
        let changed = report(changed, data(), until()).unwrap();
        assert!(read_report(&changed, original(), until()).is_err());
    }
    for changed in [
        Binding {
            request: uuid::Uuid::nil(),
            ..original()
        },
        Binding {
            deadline_ticks: 0,
            ..original()
        },
    ] {
        assert!(request(changed, &spelling(), until()).is_err());
        assert!(report(changed, data(), until()).is_err());
        assert!(read_report(&bytes, changed, until()).is_err());
    }
    for id in [
        uuid::Uuid::nil().to_string(),
        original().request.to_string().to_uppercase(),
        original().request.simple().to_string(),
        "not-an-original-request".into(),
    ] {
        let changed = mutate(&bytes, |v| v["scope"]["request"] = id.into());
        assert!(read_report(&changed, original(), until()).is_err());
    }
}

#[test]
fn only_one_whole_exactly_versioned_frame_is_accepted() {
    let bytes = report(original(), data(), until()).unwrap();
    for n in 0..bytes.len() {
        assert!(read_report(&bytes[..n], original(), until()).is_err());
    }
    for suffix in [b"\n".as_slice(), b"\0", bytes.as_slice()] {
        assert!(read_report(&[bytes.as_slice(), suffix].concat(), original(), until()).is_err());
    }
    assert!(read_report(
        &[b"progress ".as_slice(), &bytes].concat(),
        original(),
        until()
    )
    .is_err());
    for index in 0..8 {
        let mut wrong = bytes.clone();
        wrong[index] ^= 1;
        assert!(read_report(&wrong, original(), until()).is_err());
    }
    for length in [0, 1, u32::MAX] {
        let mut wrong = bytes.clone();
        wrong[8..HEADER].copy_from_slice(&length.to_le_bytes());
        assert!(read_report(&wrong, original(), until()).is_err());
    }
    let mut oversized = bytes.clone();
    oversized.resize(MAX_FRAME + 1, b' ');
    assert!(read_report(&oversized, original(), until()).is_err());
    let input = request(original(), &spelling(), until()).unwrap();
    assert!(read_report(&input, original(), until()).is_err());
    assert!(read_request(&bytes, until()).is_err());
}

#[test]
fn unknown_duplicate_or_malformed_fields_cannot_be_a_report() {
    let bytes = report(original(), data(), until()).unwrap();
    let wrong = [
        mutate(&bytes, |v| v["extra"] = 1.into()),
        mutate(&bytes, |v| v["scope"]["denied"] = true.into()),
        mutate(&bytes, |v| v["data"]["processes"][0]["parent"] = 1.into()),
        mutate(&bytes, |v| {
            v["data"]["processes"][0]["cwd"]["kind"] = "clear".into()
        }),
        mutate(&bytes, |v| {
            v["data"]["processes"][0]["cwd"]["pid"] = 25.into()
        }),
        mutate(&bytes, |v| {
            v["data"]["processes"][0]["image"]["object"]["path"] = "decoy".into()
        }),
        mutate(&bytes, |v| {
            v["data"]["faults"][0] = "denied_all_processes".into()
        }),
        mutate(&bytes, |v| {
            v["scope"].as_object_mut().unwrap().remove("deadline_ticks");
        }),
    ];
    for (index, wrong) in wrong.into_iter().enumerate() {
        assert!(
            read_report(&wrong, original(), until()).is_err(),
            "case {index}"
        );
    }
    let scope = serde_json::to_string(&Scope::from(original()).unwrap()).unwrap();
    let observations = serde_json::to_string(&data()).unwrap();
    let duplicate = format!(r#"{{"scope":{scope},"scope":{scope},"data":{observations}}}"#);
    for body in [duplicate.as_bytes(), b"null", b"[]", b"{}", b"{} {}"] {
        assert!(read_report(&frame(REPORT, body), original(), until()).is_err());
    }
}

#[test]
fn per_read_denials_and_global_faults_never_erase_any_positive_class() {
    for index in 0..3 {
        let mut observations = data();
        let reads = [Read::Denied {}, Read::Incomplete {}, Read::NoUseObserved {}];
        let mut classes = reads;
        classes[index] = Read::Use {
            object: original().checkout,
        };
        let row = &mut observations.processes[0];
        [row.image, row.cwd, row.files] = classes;
        let bytes = report(original(), observations.clone(), until()).unwrap();
        assert_eq!(
            read_report(&bytes, original(), until()).unwrap(),
            observations
        );
    }
}

#[test]
fn invalid_or_reused_native_identity_refuses_the_whole_report() {
    for (pid, created) in [(0, 85), (25, 0), (25, i64::MAX as u64 + 1)] {
        let mut wrong = data();
        wrong.processes[0].pid = pid;
        wrong.processes[0].created = created;
        assert!(read_report(&raw_report(wrong.clone()), original(), until()).is_err());
        assert!(report(original(), wrong, until()).is_err());
    }
    for created in [85, 86] {
        let mut wrong = data();
        let mut duplicate = wrong.processes[0].clone();
        duplicate.created = created;
        wrong.processes.push(duplicate);
        assert!(read_report(&raw_report(wrong), original(), until()).is_err());
    }
    let mut wrong = data();
    wrong.unbound_uses.push(wrong.unbound_uses[0].clone());
    assert!(read_report(&raw_report(wrong), original(), until()).is_err());
    let mut wrong = data();
    wrong.unbound_uses[0].pid = 0;
    assert!(read_report(&raw_report(wrong), original(), until()).is_err());
    let mut wrong = data();
    wrong.faults.push(wrong.faults[0]);
    assert!(read_report(&raw_report(wrong), original(), until()).is_err());
}

#[test]
fn counts_are_bounded_and_never_publish_a_valid_prefix() {
    let mut observations = data();
    observations.inventoried = MAX_OBJECTS;
    observations.queried = MAX_OBJECTS;
    observations.processes = (1..=MAX_PROCESSES as u32)
        .map(|pid| ProcessReads {
            pid,
            created: 85,
            ..data().processes.remove(0)
        })
        .collect();
    let bytes = report(original(), observations.clone(), until()).unwrap();
    assert_eq!(
        read_report(&bytes, original(), until()).unwrap(),
        observations
    );
    let mut wrong = observations.clone();
    wrong.processes.push(ProcessReads {
        pid: MAX_PROCESSES as u32 + 1,
        ..data().processes.remove(0)
    });
    assert!(read_report(&raw_report(wrong.clone()), original(), until()).is_err());
    assert!(report(original(), wrong, until()).is_err());
    for (inventoried, queried) in [(MAX_OBJECTS + 1, 0), (1, 2)] {
        let mut wrong = data();
        wrong.inventoried = inventoried;
        wrong.queried = queried;
        assert!(read_report(&raw_report(wrong), original(), until()).is_err());
    }
    let mut wrong = data();
    wrong.unbound_uses = (1..=MAX_PROCESSES as u32 + 1)
        .map(|pid| UnboundUse {
            pid,
            object: original().checkout,
        })
        .collect();
    assert!(read_report(&raw_report(wrong), original(), until()).is_err());
}

#[test]
fn request_spelling_and_frame_are_bounded_data_not_membership_authority() {
    for wrong in [vec![], vec![0], vec![0xd800], vec![65; MAX_PATH / 2 + 1]] {
        assert!(request(original(), &wrong, until()).is_err());
        let wire = Request {
            scope: Scope::from(original()).unwrap(),
            path: wrong,
        };
        assert!(read_request(
            &frame(REQUEST, &serde_json::to_vec(&wire).unwrap()),
            until()
        )
        .is_err());
    }
    let maximum = vec![65; MAX_PATH / 2];
    let bytes = request(original(), &maximum, until()).unwrap();
    assert_eq!(read_request(&bytes, until()).unwrap().1, maximum);
    let bytes = request(original(), &spelling(), until()).unwrap();
    let unknown = mutate(&bytes, |v| v["pid"] = 25.into());
    assert!(read_request(&unknown, until()).is_err());
}

#[test]
fn encoding_stops_before_exceeding_its_cap_and_all_paths_keep_the_deadline() {
    let mut buffer = Buffer {
        bytes: vec![0; MAX_FRAME - 1],
        until: until(),
    };
    assert_eq!(
        buffer.write(&[1, 2]).unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
    assert_eq!(buffer.bytes.len(), MAX_FRAME - 1);
    assert_eq!(buffer.write(&[1]).unwrap(), 1);
    assert_eq!(buffer.bytes.len(), MAX_FRAME);
    assert!(buffer.write(&[2]).is_err());
    let past = Instant::now();
    buffer.until = past;
    assert_eq!(
        buffer.write(&[]).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    let input = request(original(), &spelling(), until()).unwrap();
    let output = report(original(), data(), until()).unwrap();
    assert!(request(original(), &spelling(), past).is_err());
    assert!(report(original(), data(), past).is_err());
    assert!(read_request(&input, past).is_err());
    assert!(read_report(&output, original(), past).is_err());
}
