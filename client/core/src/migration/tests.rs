use super::*;
use serde_json::json;
use std::{
    io::Read,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

fn source(
    family: &str,
    values: impl Iterator<Item = SourceResult<Value>> + Send + 'static,
) -> SourceStream {
    SourceStream::new(
        SourceRecord {
            family: family.into(),
            schema: 1,
            format: "synthetic-json".into(),
            sha256: sha256(b"synthetic unchanged source bytes"),
            build: None,
        },
        values,
    )
}
fn bundle(sources: Vec<SourceStream>, budget: u64) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        "11111111-1111-4111-8111-111111111111".into(),
        1,
        sources,
        budget,
    )
    .unwrap()
}
fn read(mut reader: Cursor<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn source_reads_are_lazy_and_final_metadata_contains_no_private_body() {
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let values = [
        json!({"opaque_private_details":"private-marker-value"}),
        json!({"id":"second"}),
    ]
    .into_iter()
    .map(move |v| {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(v)
    });
    let mut producer = bundle(vec![source("qt-attention", values)], 1 << 20);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let mut snapshots = BTreeMap::new();
    let mut pages = Vec::new();
    match producer.next().unwrap().unwrap() {
        BundleItem::Snapshot { reference, reader } => {
            let data = read(reader);
            reference.verify(&data).unwrap();
            snapshots.insert(reference.sha256, data);
        }
        _ => panic!("snapshot must precede its metadata"),
    }
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    match producer.next().unwrap().unwrap() {
        BundleItem::Page { reference, reader } => {
            let data = read(reader);
            assert!(!String::from_utf8_lossy(&data).contains("private-marker-value"));
            pages.push((reference, data));
        }
        _ => panic!("expected metadata page"),
    }
    assert_eq!(
        reads.load(Ordering::SeqCst),
        1,
        "a metadata read cannot prefetch the next source item"
    );
    let mut final_record = None;
    for item in producer {
        assert!(final_record.is_none(), "manifest is always last");
        match item.unwrap() {
            BundleItem::Snapshot { reference, reader } => {
                let data = read(reader);
                reference.verify(&data).unwrap();
                snapshots.insert(reference.sha256, data);
            }
            BundleItem::Page { reference, reader } => {
                pages.push((reference, read(reader)));
            }
            BundleItem::Record(record) => final_record = Some(record),
        }
    }
    let record = final_record.unwrap();
    assert_eq!(record.evidence, Evidence::Staged);
    assert!(!record.automatic_approvals);
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("private-marker-value"));
    assert_eq!(record.pages.len(), 2);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    for (reference, data) in pages {
        let page = record.snapshot_page(&reference, &data).unwrap();
        page.snapshot
            .verify(&snapshots[&page.snapshot.sha256])
            .unwrap();
    }
}

#[test]
fn failed_changed_or_oversized_sources_never_produce_a_final_record() {
    for failure in [
        SourceError::SourceUnreadable,
        SourceError::UnsupportedSource,
        SourceError::SourceChanged,
    ] {
        let input = vec![
            Ok(json!({"before":"failure"})),
            Err(failure),
            Ok(json!({"unreachable":true})),
        ];
        let mut producer = bundle(vec![source("next-reads", input.into_iter())], 1 << 20);
        let mut seen_failure = false;
        for item in producer.by_ref() {
            match item {
                Ok(BundleItem::Record(_)) => panic!("partial source cannot finish evidence"),
                Err(e) => {
                    assert_eq!(e, failure);
                    seen_failure = true;
                }
                _ => {}
            }
        }
        assert!(seen_failure);
        assert!(producer.next().is_none());
    }
    let mut producer = bundle(
        vec![source(
            "oversized",
            std::iter::once(Ok(json!("x".repeat(MAX_SNAPSHOT_BYTES as usize)))),
        )],
        1 << 22,
    );
    assert!(matches!(producer.next(), Some(Err(SourceError::TooLarge))));
    assert!(producer.next().is_none());
}

#[test]
fn total_byte_budget_and_page_count_are_hard_refusals() {
    let mut tiny = bundle(vec![source("too-small", std::iter::once(Ok(json!({}))))], 1);
    assert!(matches!(tiny.next(), Some(Err(SourceError::TooLarge))));
    assert!(tiny.next().is_none());
    let mut many = bundle(
        vec![source("bounded", (0..1025).map(|_| Ok(Value::Null)))],
        4 << 20,
    );
    let mut count = 0;
    let mut snapshots = 0;
    let mut error = false;
    for item in &mut many {
        match item {
            Ok(BundleItem::Page { .. }) => count += 1,
            Ok(BundleItem::Snapshot { .. }) => snapshots += 1,
            Err(SourceError::TooLarge) => error = true,
            Ok(BundleItem::Record(_)) => panic!("over-cap source cannot finish"),
            Err(other) => panic!("{other}"),
        }
    }
    assert_eq!(count, 1024);
    assert_eq!(snapshots, 1, "identical bodies are emitted once");
    assert!(error);
}

#[test]
fn dev_replay_and_conflicting_source_identity_are_rejected_before_reading() {
    for (id, replay) in [
        ("dev.zerus.next", false),
        ("com.hgdev.hgs-tray.local", false),
        (SOURCE, true),
    ] {
        assert_eq!(
            Bundle::new(
                id,
                replay,
                "a".repeat(64),
                "11111111-1111-4111-8111-111111111111".into(),
                1,
                Vec::new(),
                1000
            )
            .err(),
            Some(SourceError::NotProduction)
        );
    }
    let same = || source("duplicate", std::iter::empty());
    assert_eq!(
        Bundle::new(
            SOURCE,
            false,
            "a".repeat(64),
            "11111111-1111-4111-8111-111111111111".into(),
            1,
            vec![same(), same()],
            1000
        )
        .err(),
        Some(SourceError::InvalidMetadata)
    );
}
