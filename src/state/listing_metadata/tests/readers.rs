use super::*;

#[test]
fn a_pinned_read_view_does_not_hold_the_publisher_gate() {
    let root = fixture();
    let selected = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&selected, &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    let reader = pages::Reader::open(root.path()).unwrap();
    assert!(reader.page(None).unwrap().fresh);
    value["phase"] = json!("working");
    write_at(root.path(), &selected, &value, |publication| {
        storage::atomic_observed(&selected, &value.to_string(), publication)
    })
    .unwrap();
    assert!(
        !root
            .path()
            .join("listing-metadata/coverage-lost.json")
            .exists(),
        "an ordinary page reader contended with its publisher"
    );
    let writer = Store::open(root.path()).unwrap();
    let pending: i64 = writer
        .db
        .query_row("SELECT count(*) FROM intents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        pending, 0,
        "a page forced the canonical publisher to lose coverage"
    );
    assert!(
        !reader.page(None).unwrap().fresh,
        "an old pinned seed borrowed a later native version"
    );
}

#[test]
fn an_in_place_change_cannot_reuse_a_fresh_snapshot() {
    let root = fixture();
    let selected = path(root.path());
    let mut value = record();
    value.as_object_mut().unwrap().remove("shell_jobs");
    storage::atomic(&selected, &value.to_string()).unwrap();
    assert!(repair_to_end(root.path()).complete);
    let store = Store::open(root.path()).unwrap();
    assert!(store.page(None).unwrap().fresh);
    value["phase"] = json!("working");
    // Keep the directory generation intact: only the original file changes.
    fs::write(&selected, value.to_string()).unwrap();
    assert!(
        !store.page(None).unwrap().fresh,
        "a directory witness cannot certify a changed snapshot row"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn ordinary_writes_between_slices_do_not_require_a_globally_quiet_pass() {
    let root = fixture();
    let mut current = record();
    current.as_object_mut().unwrap().remove("shell_jobs");
    for n in 0..384 {
        let mut value = current.clone();
        value["name"] = json!(format!("codex/example/member-{n}"));
        let selected = root.path().join(format!(
            "{}.json",
            storage::label_key(string(&value, "name"))
        ));
        storage::atomic(&selected, &value.to_string()).unwrap();
    }
    let selected = path(root.path());
    let publish = |value: &Value| {
        write_at(root.path(), &selected, value, |publication| {
            storage::atomic_observed(&selected, &value.to_string(), publication)
        })
        .unwrap();
    };
    publish(&current);
    assert!(repair_to_end(root.path()).complete);
    current["last_event_at"] = json!(1);
    publish(&current);
    let mut work = None;
    for tick in 0..128 {
        let stats = background_at(root.path(), &mut work).unwrap();
        assert!(stats.entries <= 256 && stats.rows <= 128 && stats.bytes <= 16 << 20);
        if stats.done && stats.complete {
            return;
        }
        current["last_event_at"] = json!(tick + 2);
        publish(&current);
    }
    panic!("continuous observed writes starved every complete display cut");
}
