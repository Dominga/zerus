//! Valid events come from the pinned core projection of product-writer rows.
use super::*;
use serde_json::json;
fn payload() -> Value {
    let pages: Value = serde_json::from_str(include_str!("../fixtures/drawers.json")).unwrap();
    let page = &pages["feed"];
    json!({"schema":1,"kind":"recorded_team_events","source":"local_hgs_displayed_page","scope":"journal_page","computer":null,"phase":null,"history_complete":false,
      "team":page["team"],"after":page["after"],"next":page["next"],"timeline":page["timeline"],"timings":null,
      "rows":page["rows"].as_array().unwrap().iter().map(|r|json!({"seq":r["seq"],"at_ms":r["at_ms"],"kind":r["kind"],"by":r["by"],"text":r["text"],"text_truncated":r["text_truncated"],"timing":null})).collect::<Vec<_>>()})
}
#[test]
fn captured_product_page_writes_exact_pretty_bytes_through_the_maintained_writer() {
    let value = payload();
    let bytes = encode(value.clone()).unwrap();
    assert!(bytes.windows(2).any(|v| v == b"\n "));
    let dir = std::env::temp_dir().join(format!("zerus-recorded-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    struct Scratch(std::path::PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let dir = Scratch(dir);
    let path = dir.0.join("recorded.json");
    crate::fsx::write_private_atomic(&path, &bytes, true).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), value);
}
#[test]
fn handwritten_foreign_scope_coverage_unknown_version_and_extra_file_paths_are_refused() {
    for (key, new) in [
        ("schema", json!(2)),
        ("computer", json!("not-established")),
        ("phase", json!(1)),
        ("history_complete", json!(true)),
        ("scope", json!("whole_history")),
        ("path", json!("unexpected")),
    ] {
        let mut value = payload();
        value[key] = new;
        assert!(encode(value).is_err(), "{key}");
    }
    let mut value = payload();
    value["timeline"]["count"] = json!(0);
    assert!(encode(value).is_err());
    let mut value = payload();
    value["rows"] = json!(vec![value["rows"][0].clone(); 201]);
    assert!(encode(value).is_err());
}
#[test]
fn handwritten_escaping_expansion_counts_the_actual_stored_pretty_bytes() {
    let mut value = payload();
    let row = value["rows"][0].clone();
    value["rows"] = json!((1..=200)
        .map(|seq| {
            let mut r = row.clone();
            r["seq"] = json!(seq);
            r["text"] = json!("\u{0000}".repeat(TEXT_CHARS));
            r
        })
        .collect::<Vec<_>>());
    value["timeline"]["count"] = json!(200);
    value["timeline"]["last_seq"] = json!(200);
    let error = encode(value).unwrap_err();
    assert!(error.contains("byte limit"), "{error}");
    // The bounded sink refuses before retaining more than the advertised cap.
    let mut sink = Limited(vec![0; EXPORT_BYTES - 1]);
    assert!(sink.write_all(b"xx").is_err());
    assert_eq!(sink.0.len(), EXPORT_BYTES - 1);
}
