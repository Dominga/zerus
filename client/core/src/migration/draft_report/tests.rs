// The drafts capture's committed originals as a report source, on synthetic
// stores under the system temporary folder.
use super::*;
use crate::drafts;
use crate::fsx::preload as disk;
use crate::migration::creation_report::{Group, Record, Source};
use std::{
    fs,
    path::{Path, PathBuf},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zerus-draft-report-{}", uuid::Uuid::new_v4()));
        drop(disk::Parents::open(&root, true).unwrap());
        let scratch = Self(root.canonicalize().unwrap());
        drop(disk::Parents::open(&scratch.data(), true).unwrap());
        scratch
    }
    fn data(&self) -> PathBuf {
        self.0.join("data")
    }
    fn next(&self) -> PathBuf {
        self.0.join("next-drafts")
    }
    fn qt(&self) -> PathBuf {
        self.0.join("qt-drafts")
    }
    fn dev(&self) -> PathBuf {
        self.0.join("dev-drafts")
    }
    fn capture(&self) -> Capture {
        Capture::new(self.data(), self.next(), Some(self.qt()), vec![self.dev()])
    }
    /// The capture after its one attempt.
    fn captured(&self) -> Capture {
        let capture = self.capture();
        capture.prepare();
        capture
    }
    fn slot(&self) -> PathBuf {
        self.data().join("drafts-preload-v1")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A valid draft of `key` under `root` with one attachment reference.
fn draft(root: &Path, key: &str, text: &str) -> Vec<u8> {
    let folder = root.join(sha256(key.as_bytes()));
    fs::create_dir_all(&folder).unwrap();
    let manifest = json!({"version": 1, "key": key, "text": text, "attachments": [
        {"name": "notes.txt", "mime": "text/plain", "reference": "[notes.txt]",
         "sha256": sha256(b"abc"), "bytes": 3}
    ]})
    .to_string()
    .into_bytes();
    fs::write(folder.join("draft.json"), &manifest).unwrap();
    manifest
}

/// Every chunk of a stream, its recheck included.
fn drain(stream: SourceStream) -> SourceResult<Vec<Value>> {
    stream.into_chunks().collect()
}

/// Each published entry of a store family, in order.
fn entries(stream: SourceStream) -> Vec<Value> {
    drain(stream)
        .unwrap()
        .into_iter()
        .flat_map(|page| page["entries"].as_array().unwrap().clone())
        .collect()
}

#[test]
fn every_store_publishes_its_originals_and_the_provenance_binds_the_receipt() {
    let s = Scratch::new();
    let next = draft(&s.next(), "codex/sample/a", "next text");
    // A manifest the decoder refuses, and a folder without a manifest.
    let odd = s.next().join(sha256(b"codex/sample/odd"));
    fs::create_dir_all(&odd).unwrap();
    fs::write(odd.join("draft.json"), b"not json").unwrap();
    fs::create_dir_all(s.next().join(sha256(b"codex/sample/empty"))).unwrap();
    draft(&s.dev(), "codex/sample/a", "dev text");
    let qt = draft(&s.qt(), "codex/sample/q", "qt text");
    let capture = s.captured();
    assert_eq!(capture.status().code, "draft_capture_captured");
    let held = retained(&capture).unwrap().unwrap();
    let binding = held.reference().unwrap();
    assert!(binding.valid());
    assert!(!binding.earlier_run);
    assert_eq!(binding.capture_id, capture.status().capture_id.unwrap());
    assert_eq!(
        binding.receipt_sha256,
        sha256(&fs::read(s.slot().join("receipt.json")).unwrap())
    );
    let streams = held.clone().report_sources().unwrap();
    let families: Vec<&str> = streams
        .iter()
        .map(|stream| stream.identity().family.as_str())
        .collect();
    assert_eq!(
        families,
        [
            "next-drafts-preload",
            "next-dev-drafts-preload",
            "qt-drafts-preload",
            PROVENANCE
        ]
    );
    let mut streams = streams.into_iter();
    let next_entries = entries(streams.next().unwrap());
    assert_eq!(next_entries.len(), 3);
    // Entries follow the container order: folder names within a root.
    let supported: Vec<&Value> = next_entries
        .iter()
        .filter(|entry| entry["class"] == "supported")
        .collect();
    assert_eq!(supported.len(), 1);
    let entry = supported[0];
    assert_eq!(entry["sha256"], sha256(&next));
    assert_eq!(entry["blobs"][0]["class"], "missing");
    assert_eq!(entry["blob_bytes_included"], false);
    // The body is exactly the unchanged checkpoint decoder's.
    let body = sources::draft_checkpoint(Origin::Next, "codex/sample/a", &next)
        .unwrap()
        .into_chunks()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(entry["checkpoint"], body);
    for class in ["unsupported", "no_manifest"] {
        let entry = next_entries.iter().find(|e| e["class"] == class).unwrap();
        assert!(entry.get("checkpoint").is_none(), "{class}");
    }
    assert_eq!(entries(streams.next().unwrap()).len(), 1);
    let qt_entries = entries(streams.next().unwrap());
    let body = sources::draft_checkpoint(Origin::Qt, "codex/sample/q", &qt)
        .unwrap()
        .into_chunks()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(qt_entries[0]["checkpoint"], body);
    let provenance = drain(streams.next().unwrap()).unwrap();
    let provenance = &provenance[0];
    assert_eq!(provenance["reference"], json!(binding));
    assert_eq!(provenance["complete"], true);
    assert_eq!(provenance["drained"], false);
    assert_eq!(provenance["entries"], 5);
    let boundaries: Vec<&str> = provenance["roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|root| root["boundary"].as_str().unwrap())
        .collect();
    assert_eq!(boundaries, [BOUNDARY, DEV_BOUNDARY, QT_BOUNDARY]);
}

#[test]
fn no_draft_anywhere_publishes_nothing() {
    let s = Scratch::new();
    let capture = s.captured();
    assert_eq!(capture.status().code, "draft_capture_not_applicable");
    assert!(retained(&capture).unwrap().is_none());
}

/// Not captured, permanently: unavailable, never "nothing to publish".
#[test]
fn a_not_captured_outcome_is_unavailable_never_no_source() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "kept");
    let big = s.qt().join(sha256(b"codex/sample/big"));
    fs::create_dir_all(&big).unwrap();
    fs::write(
        big.join("draft.json"),
        vec![b' '; drafts::MAX_MANIFEST as usize + 1],
    )
    .unwrap();
    let capture = s.captured();
    assert_eq!(capture.status().code, "draft_capture_too_large");
    assert!(s.slot().join("failed.json").exists());
    assert!(retained(&capture).is_err());
}

/// A capture that has not run (or is still inside its first job) has no
/// receipt: unavailable, never "nothing to publish", and nothing is created.
#[test]
fn a_capture_without_a_receipt_is_unavailable_and_creates_nothing() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "kept");
    assert!(retained(&s.capture()).is_err());
    assert!(!s.slot().exists());
    // A seed with no outcome: still inside the first job.
    let capture = s.captured();
    fs::remove_file(s.slot().join("receipt.json")).unwrap();
    assert!(retained(&capture).is_err());
}

/// The binding promises the receipt's exact bytes: a held snapshot whose
/// receipt changed, even to the same parsed receipt, is changed, and its
/// streams' rechecks refuse publication.
#[test]
fn the_held_reference_rechecks_the_exact_receipt_bytes() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "kept");
    let capture = s.captured();
    let held = retained(&capture).unwrap().unwrap();
    let streams = held.clone().report_sources().unwrap();
    let receipt = s.slot().join("receipt.json");
    let mut bytes = fs::read(&receipt).unwrap();
    bytes.push(b'\n');
    fs::write(&receipt, bytes).unwrap();
    assert_eq!(held.reference(), Err(SourceError::SourceChanged));
    for stream in streams {
        assert_eq!(drain(stream), Err(SourceError::SourceChanged));
    }
}

/// A container part replaced with the same length is never published: the
/// held reference sees its stamp change, and a fresh read refuses it.
#[test]
fn a_damaged_part_is_never_published() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "kept");
    let capture = s.captured();
    let held = retained(&capture).unwrap().unwrap();
    let part = s.slot().join("original.bin");
    let length = fs::metadata(&part).unwrap().len() as usize;
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::remove_file(&part).unwrap();
    disk::create(&part, &vec![b' '; length]).unwrap();
    assert_eq!(held.reference(), Err(SourceError::SourceChanged));
    assert!(retained(&capture).is_err());
}

/// Next's store already written by an earlier uncaptured run: Qt's and the
/// dev app's originals publish, Next's family is absent, and the report
/// stays incomplete.
#[test]
fn an_earlier_run_publishes_the_other_stores_and_stays_incomplete() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "typed after an import");
    fs::write(s.next().join(drafts::IMPORT_MARKER), b"").unwrap();
    draft(&s.qt(), "codex/sample/q", "qt text");
    let capture = s.captured();
    assert_eq!(capture.status().code, "draft_capture_next_earlier_run");
    let held = retained(&capture).unwrap().unwrap();
    let binding = held.reference().unwrap();
    assert!(binding.earlier_run);
    assert!(!Source::from(binding.clone()).complete());
    let streams = held.report_sources().unwrap();
    let families: Vec<&str> = streams
        .iter()
        .map(|stream| stream.identity().family.as_str())
        .collect();
    assert_eq!(families, ["qt-drafts-preload", PROVENANCE]);
    let provenance = drain(streams.into_iter().last().unwrap()).unwrap();
    assert_eq!(provenance[0]["complete"], false);
    let next = &provenance[0]["roots"][0];
    assert_eq!(next["store"], "next");
    assert_eq!(next["earlier_run"], true);
    assert_eq!(next["complete"], false);
    // With no other draft, the incomplete store still publishes provenance.
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "typed after an import");
    fs::write(s.next().join(drafts::IMPORT_MARKER), b"").unwrap();
    let held = retained(&s.captured()).unwrap().unwrap();
    let streams = held.report_sources().unwrap();
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].identity().family, PROVENANCE);
}

/// A drafts binding is its own untagged shape, only in the drafts scope.
#[test]
fn a_drafts_binding_fits_only_the_drafts_scope() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "kept");
    let binding = retained(&s.captured())
        .unwrap()
        .unwrap()
        .reference()
        .unwrap();
    let configuration = "a".repeat(64);
    let record =
        Record::new_group(Group::Drafts, configuration.clone(), binding.clone(), 1).unwrap();
    assert_eq!(record.scope, "next-drafts-preload-v1");
    let bytes = serde_json::to_vec(&record).unwrap();
    let again: Record = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(again.source, Source::Drafts(binding.clone()));
    assert!(again.source.complete());
    for group in [
        Group::Creations,
        Group::LocalReads,
        Group::WorktreeCache,
        Group::Downloads,
    ] {
        assert!(
            Record::new_group(group, configuration.clone(), binding.clone(), 1).is_err(),
            "{group:?}"
        );
    }
}

/// A slot that also holds a failure record is not one recorded outcome:
/// never published, by a fresh read or by the held reference.
#[test]
fn a_failure_record_beside_the_receipt_is_never_published() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "kept");
    let capture = s.captured();
    let held = retained(&capture).unwrap().unwrap();
    let failed = json!({"version": 2, "capture_id": capture.status().capture_id.unwrap(), "reason": "changed"});
    disk::create(
        &s.slot().join("failed.json"),
        &serde_json::to_vec(&failed).unwrap(),
    )
    .unwrap();
    assert_eq!(held.reference(), Err(SourceError::SourceChanged));
    assert!(retained(&capture).is_err());
}

/// A further container part the receipt does not list is never published,
/// by the held reference or by a fresh read.
#[test]
fn an_extra_container_part_is_never_published() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "kept");
    let capture = s.captured();
    let held = retained(&capture).unwrap().unwrap();
    assert!(!s.slot().join("original-1.bin").exists());
    disk::create(&s.slot().join("original-1.bin"), b"more").unwrap();
    assert_eq!(held.reference(), Err(SourceError::SourceChanged));
    assert!(retained(&capture).is_err());
}

/// An index replaced with the same bytes is another leaf: the receipt's
/// stamp no longer matches, so it is never published.
#[test]
fn a_replaced_index_is_never_published() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "kept");
    let capture = s.captured();
    let held = retained(&capture).unwrap().unwrap();
    let index = s.slot().join("index.json");
    let bytes = fs::read(&index).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::remove_file(&index).unwrap();
    disk::create(&index, &bytes).unwrap();
    assert_eq!(held.reference(), Err(SourceError::SourceChanged));
    assert!(retained(&capture).is_err());
}

/// A manifest whose checkpoint body is `body` bytes long once encoded.
fn manifest_with_body(key: &str, body: usize) -> Vec<u8> {
    let manifest = |text: usize| {
        json!({"version": 1, "key": key, "text": "x".repeat(text)})
            .to_string()
            .into_bytes()
    };
    let encoded = |bytes: &[u8]| {
        let body = sources::draft_checkpoint(Origin::Next, key, bytes)
            .unwrap()
            .into_chunks()
            .next()
            .unwrap()
            .unwrap();
        serde_json::to_vec(&body).unwrap().len()
    };
    let overhead = encoded(&manifest(0));
    let bytes = manifest(body - overhead);
    assert_eq!(encoded(&bytes), body);
    bytes
}

/// The size edge is intended: a supported manifest whose body fits the
/// checkpoint decoder's 1 MiB bound but whose entry cannot fit one report
/// page refuses the whole family as too large, never a prefix.
#[test]
fn an_entry_that_cannot_fit_one_page_refuses_the_family_never_a_prefix() {
    let s = Scratch::new();
    draft(&s.next(), "codex/sample/a", "small");
    let key = "codex/sample/big";
    let folder = s.next().join(sha256(key.as_bytes()));
    fs::create_dir_all(&folder).unwrap();
    let body = zerus_desktop_migration::MAX_SNAPSHOT_BYTES as usize - 100;
    fs::write(folder.join("draft.json"), manifest_with_body(key, body)).unwrap();
    let capture = s.captured();
    assert_eq!(capture.status().code, "draft_capture_captured");
    let held = retained(&capture).unwrap().unwrap();
    let classes: Vec<Class> = held.entries().iter().map(|entry| entry.class).collect();
    assert_eq!(classes, [Class::Supported, Class::Supported]);
    let next = held.report_sources().unwrap().into_iter().next().unwrap();
    assert_eq!(next.identity().family, "next-drafts-preload");
    assert_eq!(drain(next), Err(SourceError::TooLarge));
}
