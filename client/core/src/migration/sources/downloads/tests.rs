use super::*;
use crate::migration::{Bundle, BundleItem};
use std::{fs, path::PathBuf};
use zerus_desktop_migration::{Evidence, ImportRecord, MAX_SNAPSHOT_BYTES, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
const FOLDER: &str = "/srv/example/downloads";

fn entry(name: &str) -> Value {
    json!({"name":name,"bytes":4,"saved":1_800_000_000_000i64,
        "sha256":sha256(b"keep"),"private_extension":{"original":true}})
}
fn document(files: Vec<Value>) -> Value {
    json!({"version":1,"files":files,"future_header":{"private":"retain"}})
}
fn encoded(document: &Value) -> Vec<u8> {
    serde_json::to_vec(document).unwrap()
}
fn bundle(stream: SourceStream) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        vec![stream],
        64 << 20,
    )
    .unwrap()
}
fn bodies(stream: SourceStream) -> (Vec<Value>, ImportRecord) {
    let mut snapshots = Vec::new();
    let mut record = None;
    for item in bundle(stream) {
        match item.unwrap() {
            BundleItem::Snapshot { reference, reader } => {
                reference.verify(reader.get_ref()).unwrap();
                snapshots.push(serde_json::from_slice(reader.get_ref()).unwrap());
            }
            BundleItem::Record(value) => record = Some(value),
            _ => (),
        }
    }
    let record = record.unwrap();
    assert_eq!(record.evidence, Evidence::Staged);
    assert!(!record.automatic_approvals);
    (snapshots, record)
}
fn refused(result: SourceResult<SourceStream>) -> SourceError {
    let stream = match result {
        Ok(value) => value,
        Err(error) => return error,
    };
    let mut error = None;
    for item in bundle(stream) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("refused source cannot finish"),
            Err(value) => error = Some(value),
            _ => (),
        }
    }
    error.expect("expected refusal")
}

#[test]
fn shared_decoder_preserves_qt_source_identity_and_original_private_fields() {
    let mut attributed = entry("report.pdf");
    attributed["source"] = json!({"host":"mac","path":"/srv/projects/report.pdf"});
    // Qt 6.12 QDataStream (Qt_6_0) << host << path, then SHA-256.
    attributed["source_sha256"] =
        json!("ba716e2d4f5b8e750832e7051d4aecf26c57f2580c96b004e15edb9c08f940c0");
    let original = document(vec![attributed.clone(), entry("unattributed.bin")]);
    let bytes = encoded(&original);
    assert_eq!(
        download_cache::decode_index(&bytes).unwrap(),
        original.as_object().unwrap().clone()
    );
    let (snapshots, record) = bodies(download_index(FOLDER, &bytes).unwrap());
    assert_eq!(
        snapshots[0],
        json!({"kind":"download-index-header","folder":FOLDER,
        "content":{"version":1,"future_header":{"private":"retain"}}})
    );
    assert_eq!(snapshots[1]["entries"], original["files"]);
    assert_eq!(
        record.sources[0].family,
        format!("download-index/{}", sha256(FOLDER.as_bytes()))
    );
    assert_eq!(record.sources[0].schema, 1);
    assert_eq!(record.sources[0].format, "download-cache-json-v1");
    assert_eq!(record.sources[0].sha256, sha256(&bytes));
    let manifest = serde_json::to_string(&record).unwrap();
    for private in [FOLDER, "report.pdf", "/srv/projects", "future_header"] {
        assert!(!manifest.contains(private));
    }
    assert_eq!(bytes, encoded(&original));
}

#[test]
fn one_shared_folder_has_one_family_and_empty_index_is_preserved() {
    let bytes = encoded(&document(Vec::new()));
    for folder in [
        FOLDER,
        "C:/Users/example/Downloads",
        "//server/share/downloads",
    ] {
        let first = download_index(folder, &bytes).unwrap();
        let second = download_index(folder, &bytes).unwrap();
        assert_eq!(first.identity(), second.identity());
        let (values, _) = bodies(first);
        assert_eq!(values.len(), 1);
        assert_eq!(values[0]["folder"], folder);
        let duplicate = Bundle::new(
            SOURCE,
            false,
            "a".repeat(64),
            ID.into(),
            1,
            vec![download_index(folder, &bytes).unwrap(), second],
            64 << 20,
        );
        assert_eq!(duplicate.err(), Some(SourceError::InvalidMetadata));
    }
    assert_ne!(
        download_index(FOLDER, &bytes).unwrap().identity().family,
        download_index("/srv/other", &bytes)
            .unwrap()
            .identity()
            .family
    );
    for folder in [
        "",
        "relative",
        "C:relative",
        "\\\\?\\C:\\downloads",
        "/srv/example\nother",
    ] {
        assert_eq!(
            refused(download_index(folder, &bytes)),
            SourceError::UnsupportedSource
        );
    }
}

#[test]
fn partial_future_invalid_names_and_mismatched_source_records_refuse() {
    for bytes in [
        b"{".as_slice(),
        b"null",
        br#"{"version":2,"files":[]}"#,
        br#"{"version":1,"files":{}}"#,
    ] {
        assert_eq!(
            refused(download_index(FOLDER, bytes)),
            SourceError::UnsupportedSource
        );
    }
    for name in ["", "..", "../outside", "C:outside", ".ZERUS-files.json"] {
        assert_eq!(
            refused(download_index(
                FOLDER,
                &encoded(&document(vec![entry(name)]))
            )),
            SourceError::UnsupportedSource
        );
    }
    let repeated = document(vec![entry("same.bin"), entry("same.bin")]);
    assert_eq!(
        refused(download_index(FOLDER, &encoded(&repeated))),
        SourceError::UnsupportedSource
    );
    for (field, value) in [
        ("bytes", json!(-1)),
        ("saved", json!(null)),
        ("sha256", json!("ABC")),
        ("source", json!({"host":"peer","path":"/srv/source"})),
        ("source_sha256", json!("a".repeat(64))),
    ] {
        let mut bad = entry("bad.bin");
        bad[field] = value;
        assert_eq!(
            refused(download_index(FOLDER, &encoded(&document(vec![bad])))),
            SourceError::UnsupportedSource
        );
    }
    let mut bad = entry("bad.bin");
    bad["source"] = json!({"host":"mac","path":"/srv/projects/report.pdf","unknown":true});
    bad["source_sha256"] =
        json!("ba716e2d4f5b8e750832e7051d4aecf26c57f2580c96b004e15edb9c08f940c0");
    assert_eq!(
        refused(download_index(FOLDER, &encoded(&document(vec![bad])))),
        SourceError::UnsupportedSource
    );
}

#[test]
fn bounded_pages_keep_all_entries_and_never_truncate_oversized_items_or_headers() {
    let original = document((0..65).map(|n| entry(&format!("file-{n}.bin"))).collect());
    let (snapshots, _) = bodies(download_index(FOLDER, &encoded(&original)).unwrap());
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[1]["entries"].as_array().unwrap().len(), 64);
    assert_eq!(snapshots[2]["entries"].as_array().unwrap().len(), 1);
    let restored: Vec<Value> = snapshots[1..]
        .iter()
        .flat_map(|v| v["entries"].as_array().unwrap().clone())
        .collect();
    assert_eq!(restored, original["files"].as_array().unwrap().clone());
    let mut huge = entry("huge.bin");
    huge["unknown"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    assert_eq!(
        refused(download_index(
            FOLDER,
            &encoded(&document(vec![entry("first.bin"), huge]))
        )),
        SourceError::TooLarge
    );
    let mut huge_header = document(Vec::new());
    huge_header["unknown"] = json!("x".repeat(MAX_SNAPSHOT_BYTES as usize));
    assert_eq!(
        refused(download_index(FOLDER, &encoded(&huge_header))),
        SourceError::TooLarge
    );
}

#[test]
fn source_size_entry_count_and_fresh_recheck_gate_the_final_record() {
    let bytes = vec![b' '; download_cache::MAX_INDEX_BYTES as usize + 1];
    assert_eq!(
        refused(download_index(FOLDER, &bytes)),
        SourceError::TooLarge
    );
    let too_many = document(
        (0..=READ_ENTRIES)
            .map(|n| entry(&format!("file-{n}")))
            .collect(),
    );
    assert_eq!(
        refused(download_index(FOLDER, &encoded(&too_many))),
        SourceError::TooLarge
    );
    let bytes = encoded(&document(vec![entry("keep.bin")]));
    let changed = download_index(FOLDER, &bytes)
        .unwrap()
        .recheck(|| Ok(sha256(b"replaced index")));
    assert_eq!(refused(Ok(changed)), SourceError::SourceChanged);
    let replaced = download_index(FOLDER, &bytes)
        .unwrap()
        .recheck(|| Err(SourceError::SourceChanged));
    assert_eq!(refused(Ok(replaced)), SourceError::SourceChanged);
    let hash = sha256(&bytes);
    bodies(
        download_index(FOLDER, &bytes)
            .unwrap()
            .recheck(move || Ok(hash)),
    );
}

struct Folder(PathBuf);
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn index_import_never_acquires_lock_or_opens_changes_and_unknown_cache_files() {
    let scratch = Folder(
        std::env::temp_dir().join(format!("zerus-g3-download-evidence-{}", std::process::id())),
    );
    fs::create_dir(&scratch.0).unwrap();
    let original = encoded(&document(vec![entry("edited.bin")]));
    let files = [
        (".zerus-files.json", original.as_slice()),
        (
            ".zerus-files.lock",
            b"synthetic lock must stay byte-for-byte".as_slice(),
        ),
        (
            "edited.bin",
            b"user edits differ from the saved digest".as_slice(),
        ),
        ("unknown.txt", b"local data".as_slice()),
        (
            ".zerus-part-in-progress",
            b"unknown in-flight download".as_slice(),
        ),
    ];
    for (name, bytes) in files {
        fs::write(scratch.0.join(name), bytes).unwrap();
    }
    let before = files.map(|(name, bytes)| {
        (
            name,
            bytes.to_vec(),
            fs::metadata(scratch.0.join(name))
                .unwrap()
                .modified()
                .unwrap(),
        )
    });
    let folder = fs::canonicalize(&scratch.0).unwrap();
    let path = folder.join(".zerus-files.json");
    let stream = download_index(folder.to_str().unwrap(), &fs::read(&path).unwrap())
        .unwrap()
        .recheck(move || {
            Ok(sha256(
                &fs::read(path).map_err(|_| SourceError::SourceUnreadable)?,
            ))
        });
    bodies(stream);
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), files.len());
    for (name, bytes, modified) in before {
        let path = scratch.0.join(name);
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    }
}
