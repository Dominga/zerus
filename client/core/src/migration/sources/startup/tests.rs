use super::*;
use crate::migration::{Bundle, BundleItem};
use std::{fs, path::PathBuf};
use zerus_desktop_migration::{Evidence, SOURCE};

const ID: &str = "11111111-1111-4111-8111-111111111111";
fn bundle(source: SourceStream) -> Bundle {
    Bundle::new(
        SOURCE,
        false,
        "a".repeat(64),
        ID.into(),
        1,
        vec![source],
        8 << 20,
    )
    .unwrap()
}
fn body(source: SourceStream) -> Value {
    let mut body = None;
    let mut record = None;
    for item in bundle(source) {
        match item.unwrap() {
            BundleItem::Snapshot { reference, reader } => {
                reference.verify(reader.get_ref()).unwrap();
                body = Some(serde_json::from_slice::<Value>(reader.get_ref()).unwrap());
            }
            BundleItem::Record(value) => record = Some(value),
            _ => (),
        }
    }
    let record = record.unwrap();
    assert_eq!(record.evidence, Evidence::Staged);
    assert!(!record.automatic_approvals);
    assert_eq!(record.sources[0].family, "tray-config");
    assert_eq!(record.sources[0].schema, 1);
    assert_eq!(record.sources[0].format, "tray-conf-v1");
    assert!(!serde_json::to_string(&record)
        .unwrap()
        .contains("/opt/example"));
    body.unwrap()
}

#[test]
fn existing_parser_wins_last_without_dropping_private_unknown_lines_or_changing_values() {
    let text="# original comments\r\nhgs=/opt/example/hgs\r\nlocal-poll = 3000\r\npeer-poll=invalid\r\nterminal=example-terminal -e {cmd}\r\nicon-color=#abcdef\r\nopen-mode=tab\r\nopen-mode = clipboard\r\nfuture-key=retain privately\r\nnot an assignment\r\n";
    let source = tray_config(text.as_bytes()).unwrap();
    assert_eq!(source.identity().sha256, sha256(text.as_bytes()));
    let value = body(source);
    assert_eq!(value["text"], text);
    assert_eq!(
        value["parsed"],
        json!({"hgs":"/opt/example/hgs","local-poll":"3000","peer-poll":"invalid",
        "terminal":"example-terminal -e {cmd}","icon-color":"#abcdef","open-mode":"clipboard"})
    );
    // Parsing is evidence only, even for a malformed known value.
    for key in KEYS {
        assert_eq!(value["parsed"][key], tray_conf_value(text, key).unwrap());
    }
}

#[test]
fn empty_missing_and_future_fields_do_not_manufacture_defaults() {
    let value = body(tray_config(b"hgs=\n# terminal=ignored\nfuture-key=value\n").unwrap());
    assert_eq!(value["parsed"], json!({"hgs":""}));
    let empty = body(tray_config(b"").unwrap());
    assert_eq!(empty["text"], "");
    assert_eq!(empty["parsed"], json!({}));
}

#[test]
fn malformed_encoding_size_and_fresh_source_changes_prevent_final_evidence() {
    assert_eq!(
        tray_config(&[0xff]).err(),
        Some(SourceError::UnsupportedSource)
    );
    assert_eq!(
        tray_config(&vec![b'x'; MAX_CONFIG + 1]).err(),
        Some(SourceError::TooLarge)
    );
    let exact = vec![b'x'; MAX_CONFIG];
    assert_eq!(
        body(tray_config(&exact).unwrap())["text"]
            .as_str()
            .unwrap()
            .len(),
        MAX_CONFIG
    );
    let source = tray_config(b"hgs=/opt/example/hgs\n")
        .unwrap()
        .recheck(|| Ok(sha256(b"changed config")));
    let mut failed = false;
    for item in bundle(source) {
        match item {
            Ok(BundleItem::Record(_)) => panic!("changed config cannot finish"),
            Err(SourceError::SourceChanged) => failed = true,
            Err(error) => panic!("unexpected {error}"),
            _ => (),
        }
    }
    assert!(failed);
}

struct Folder(PathBuf);
impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn isolated_config_bytes_time_and_directory_stay_unchanged() {
    let folder =
        Folder(std::env::temp_dir().join(format!("zerus-g3-tray-config-{}", std::process::id())));
    fs::create_dir(&folder.0).unwrap();
    let path = folder.0.join("synthetic-tray.conf");
    let bytes = b"# retained\nhgs=/opt/example/hgs\nopen-mode=terminal\nunknown=original\n";
    fs::write(&path, bytes).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let check = path.clone();
    let source = tray_config(&fs::read(&path).unwrap())
        .unwrap()
        .recheck(move || {
            fs::read(check)
                .map(|b| sha256(&b))
                .map_err(|_| SourceError::SourceUnreadable)
        });
    assert_eq!(body(source)["text"], std::str::from_utf8(bytes).unwrap());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
}
