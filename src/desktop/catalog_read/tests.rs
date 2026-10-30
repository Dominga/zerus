use super::*;
use crate::desktop::store::Location;
use zerus_watch_protocol::CatalogItem;

struct Fixture {
    _temp: tempfile::TempDir,
    store: Store,
    feed: feed::Broker,
    producer: std::sync::Arc<feed::Producer>,
    epoch: String,
}
impl Fixture {
    fn new(rows: usize) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: temp.path().canonicalize().unwrap().join("state/desktop"),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        let store = Store::open(&location, &epoch).unwrap();
        let mut feed = feed::Broker::new(&epoch);
        let producer = feed.open(Some(&json!({"abi":feed::ABI,"producer":uuid::Uuid::new_v4().to_string(),"mode":"display_only"}))).unwrap();
        let mut fixture = Self {
            _temp: temp,
            store,
            feed,
            producer,
            epoch,
        };
        fixture.replace(rows, 1);
        fixture
    }
    fn replace(&mut self, rows: usize, generation: u64) {
        let pages = rows.div_ceil(128).max(1);
        self.apply(feed::Packet::LegacyBegin {
            computer: "peer".into(),
            generation,
            metadata: json!({"machine_id":"example-machine"}),
            rows,
            pages,
        });
        for index in 0..pages {
            let items = (index * 128..((index + 1) * 128).min(rows))
                .map(item)
                .collect();
            self.apply(feed::Packet::LegacyPage {
                computer: "peer".into(),
                generation,
                index,
                items,
            });
        }
        self.apply(feed::Packet::LegacyComplete {
            computer: "peer".into(),
            generation,
        });
    }
    fn apply(&mut self, packet: feed::Packet) {
        self.feed
            .apply(&mut self.store, &self.producer, &packet)
            .unwrap();
    }
    fn read(&self, reader: &mut Reader, cursor: Option<&str>, now: Instant) -> Value {
        reader.at(
            &self.store,
            &self.feed,
            &self.epoch,
            &query("catalog_page", "peer", cursor),
            now,
        )
    }
}
fn item(n: usize) -> CatalogItem {
    CatalogItem {
        key: format!("run:run-{n}"),
        row: json!({"run_id":format!("run-{n}"),"name":format!("codex/example/{n}"),"process_work":{"count":3,"future":true},"unknown":{"kept":"exact"}}),
    }
}
fn query(op: &str, computer: &str, cursor: Option<&str>) -> Value {
    json!({"request_id":uuid::Uuid::new_v4().to_string(),"op":op,"computer":computer,"cursor":cursor})
}
fn token(value: &Value) -> String {
    assert_eq!(value["accepted"], true, "{value}");
    value["next_cursor"].as_str().unwrap().to_owned()
}

#[test]
fn many_pages_keep_one_cursor_and_preserve_raw_rows_without_writes() {
    let f = Fixture::new(10_000);
    let mut reader = Reader::default();
    let now = Instant::now();
    let changes = f.store.db.total_changes();
    let mut cursor = None;
    let mut count = 0;
    let mut pages = 0;
    loop {
        let value = f.read(&mut reader, cursor.as_deref(), now);
        assert_eq!(value["accepted"], true, "{value}");
        assert_eq!(value["complete"], true);
        assert_eq!(value["authority"], false);
        assert_eq!(value["durable"], false);
        let rows = value["items"].as_array().unwrap();
        assert!(rows.len() <= 128);
        for row in rows {
            assert_eq!(row["key"], item(count).key);
            assert_eq!(row["row"], item(count).row);
            count += 1;
        }
        protocol::encode(&crate::desktop::frame(
            &f.epoch,
            u64::MAX,
            "response",
            value.clone(),
        ))
        .unwrap();
        cursor = value["next_cursor"].as_str().map(str::to_owned);
        assert_eq!(reader.walks.len(), usize::from(cursor.is_some()));
        pages += 1;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(count, 10_000);
    assert_eq!(pages, 79);
    assert_eq!(f.store.db.total_changes(), changes);
    f.producer.close();
    assert_eq!(
        f.read(&mut reader, None, now)["complete"],
        false,
        "atomic closure must invalidate coverage before retirement"
    );
}

#[test]
fn cursors_are_one_use_scoped_private_and_closed_explicitly() {
    let f = Fixture::new(1000);
    let mut reader = Reader::default();
    let now = Instant::now();
    let first = token(&f.read(&mut reader, None, now));
    let wrong = reader.at(
        &f.store,
        &f.feed,
        &f.epoch,
        &query("catalog_page", "other", Some(&first)),
        now,
    );
    assert_eq!(wrong["code"], "catalog_cursor_scope");
    assert_eq!(reader.walks.len(), 1);
    let mut restarted = Reader::default();
    assert_eq!(
        f.read(&mut restarted, Some(&first), now)["code"],
        "catalog_cursor_expired"
    );
    let second = token(&f.read(&mut reader, Some(&first), now));
    assert_ne!(first, second);
    assert_eq!(
        f.read(&mut reader, Some(&first), now)["code"],
        "catalog_cursor_expired"
    );
    let close = query("catalog_close", "peer", Some(&second));
    assert_eq!(
        reader.at(&f.store, &f.feed, &f.epoch, &close, now)["closed"],
        true
    );
    assert_eq!(
        reader.at(&f.store, &f.feed, &f.epoch, &close, now)["accepted"],
        false
    );
    assert!(reader.walks.is_empty());
    for value in [
        query("catalog_page", "bad alias", None),
        query("catalog_close", "peer", None),
        query("catalog_page", "peer", Some("bad")),
        json!({"op":"catalog_page","computer":"peer"}),
        json!({"op":"catalog_page","request_id":uuid::Uuid::new_v4().to_string(),"computer":"peer","after":1}),
    ] {
        assert_eq!(
            reader.at(&f.store, &f.feed, &f.epoch, &value, now)["accepted"],
            false
        );
    }
}

#[test]
fn walk_capacity_idle_and_absolute_expiry_never_grow_metadata() {
    let f = Fixture::new(1000);
    let mut reader = Reader::default();
    let now = Instant::now();
    let mut tokens = Vec::new();
    for _ in 0..WALKS {
        tokens.push(token(&f.read(&mut reader, None, now)));
    }
    assert_eq!(reader.walks.len(), WALKS);
    assert_eq!(
        f.read(&mut reader, None, now)["code"],
        "catalog_walk_capacity"
    );
    let next = token(&f.read(&mut reader, Some(&tokens[0]), now + Duration::from_secs(1)));
    assert_eq!(reader.walks.len(), WALKS, "rotation needs no extra slot");
    assert_eq!(
        f.read(
            &mut reader,
            Some(&next),
            now + IDLE + Duration::from_secs(1)
        )["code"],
        "catalog_cursor_expired"
    );
    assert!(reader.walks.is_empty());
    let mut current = token(&f.read(&mut reader, None, now));
    for seconds in [100, 200, 300, 400, 500] {
        current = token(&f.read(
            &mut reader,
            Some(&current),
            now + Duration::from_secs(seconds),
        ));
    }
    assert_eq!(
        f.read(&mut reader, Some(&current), now + LIFETIME)["code"],
        "catalog_cursor_expired"
    );
    assert!(reader.walks.is_empty());
}

#[test]
fn replaced_generation_and_storage_failures_refuse_only_the_read() {
    let mut f = Fixture::new(300);
    let mut reader = Reader::default();
    let now = Instant::now();
    let old = token(&f.read(&mut reader, None, now));
    f.replace(300, 2);
    assert_eq!(
        f.read(&mut reader, Some(&old), now)["code"],
        "catalog_read_refused"
    );
    assert!(reader.walks.is_empty());
    f.store
        .db
        .execute_batch("ALTER TABLE desktop_catalog_sources RENAME TO broken_sources")
        .unwrap();
    assert_eq!(
        f.read(&mut reader, None, now)["code"],
        "catalog_read_refused"
    );
    f.store
        .db
        .execute_batch("ALTER TABLE broken_sources RENAME TO desktop_catalog_sources")
        .unwrap();
    assert_eq!(f.read(&mut reader, None, now)["complete"], true);
    assert_eq!(f.store.effect_status()["revoked"], true);
}

#[test]
fn encoded_output_and_corrupt_row_failures_release_the_walk() {
    let f = Fixture::new(300);
    let mut reader = Reader::default();
    let now = Instant::now();
    let old = token(&f.read(&mut reader, None, now));
    // Seed a bounded stored row whose escaped key pushes the IPC envelope over
    // its limit. Stored size is checked before allocation, encoded size after.
    let key = "\u{1}".repeat(200_000);
    f.store.db.execute("UPDATE desktop_catalog_rows SET key=?1,json='{}',bytes=200066 WHERE id=(SELECT min(id)+128 FROM desktop_catalog_rows)",[&key]).unwrap();
    assert_eq!(
        f.read(&mut reader, Some(&old), now)["code"],
        "catalog_read_refused"
    );
    assert!(reader.walks.is_empty());
    assert_eq!(
        f.read(&mut reader, Some(&old), now)["code"],
        "catalog_cursor_expired"
    );
    f.store.db.execute_batch("UPDATE desktop_catalog_rows SET key='repaired',json='not-json',bytes=80 WHERE length(key)>1000").unwrap();
    // The malformed private body is never returned as a diagnostic.
    let first = token(&f.read(&mut reader, None, now));
    let result = f.read(&mut reader, Some(&first), now);
    assert_eq!(result["code"], "catalog_read_refused");
    assert!(!result.to_string().contains("not-json"));
}
