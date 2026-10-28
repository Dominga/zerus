//! The settings store on a real disk, and Qt's stores read the way Qt reads
//! them. `fixtures/settings/qt-6.12.ini` was written by Qt 6.12's QSettings
//! and `qt-6.12-read.json` holds what Qt's readers return for each key, both
//! by `fixtures/settings/record.cpp` (write and read in separate processes).
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zerus_client_core::settings::qt::{qt_key, read_ini, read_plist, QtValue};
use zerus_client_core::settings::{
    effective, find, from_qt, seed_values, Kind, QtStore, Settings, CLIENT, KEYS,
    QT_AUTOMATIC_APPROVALS,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "zerus-client-core-settings-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/settings")
        .join(name)
}

const FLUSH: Duration = Duration::from_secs(5);

#[test]
fn qt_ini_values_read_as_qt_reads_them() {
    let values = read_ini(&fs::read(fixture("qt-6.12.ini")).unwrap());
    let expected: BTreeMap<String, Value> =
        serde_json::from_slice(&fs::read(fixture("qt-6.12-read.json")).unwrap()).unwrap();
    assert_eq!(
        values.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    for (key, read) in &expected {
        let value = &values[key];
        assert_eq!(value.to_bool(), read["bool"], "{key} toBool");
        let (double, double_ok) = value.to_double();
        assert_eq!(double_ok, read["double_ok"], "{key} toDouble ok");
        assert_eq!(double, read["double"].as_f64().unwrap(), "{key} toDouble");
        let (long, long_ok) = value.to_long();
        assert_eq!(long_ok, read["long_ok"], "{key} toLongLong ok");
        assert_eq!(long.to_string(), read["long"], "{key} toLongLong");
        assert_eq!(value.to_text(), read["string"], "{key} toString");
        assert_eq!(
            json!(value.to_text_list()),
            read["list"],
            "{key} toStringList"
        );
        assert_eq!(*value != QtValue::Invalid, read["valid"], "{key} isValid");
    }
}

#[test]
fn ini_escapes_follow_qt() {
    let values = read_ini(
        b"\xef\xbb\xbf; comment\n[General]\ntop=1\n[a%20b]\nc%U0416d\\e=\"x\" , y\\x41\n\
          [%General]\nk=@String(z)\n[s]\nk=1\nk=2\nq=\"a;b\" ;tail\n",
    );
    assert_eq!(values["top"], QtValue::Text("1".into()));
    assert_eq!(
        values["a b/c\u{416}d/e"],
        QtValue::List(vec![QtValue::Text("x".into()), QtValue::Text("yA".into())])
    );
    assert_eq!(values["General/k"], QtValue::Text("z".into()));
    // A later line wins; a quoted ';' stays in the value.
    assert_eq!(values["s/k"], QtValue::Text("2".into()));
    assert_eq!(values["s/q"], QtValue::Text("a;b".into()));
}

#[test]
fn mac_keys_rotate_slashes_dots_and_middle_dots() {
    assert_eq!(qt_key("workspace.theme"), "workspace/theme");
    assert_eq!(
        qt_key("shortcuts.queue\u{b7}sendNow"),
        "shortcuts/queue.sendNow"
    );
    assert_eq!(qt_key("4\u{b7}0.Browser/x"), "4.0/Browser\u{b7}x");
}

#[test]
fn mac_plist_values_read_as_qt_reads_them() {
    let values = read_plist(&fs::read(fixture("mac.plist")).unwrap()).unwrap();
    assert_eq!(values["workspace/theme"], QtValue::Text("light".into()));
    assert_eq!(values["workspace/contentScale"], QtValue::Double(1.5));
    assert_eq!(values["workspace/activityWidth"], QtValue::Int(1000));
    assert_eq!(values["processes/enabled"], QtValue::Bool(true));
    assert_eq!(values["speech/profile"], QtValue::Text("@x".into()));
    assert_eq!(values["speech/microphone"], QtValue::Bytes(b"mic".to_vec()));
    // An encoded variant the client does not read.
    assert_eq!(values["recovery/sharedPolicy"], QtValue::Invalid);
    assert_eq!(
        values["shortcuts/queue.sendNow"],
        QtValue::List(vec![QtValue::Text("Ctrl+Return".into())])
    );
    assert_eq!(values["shortcuts/message.push"], QtValue::List(vec![]));
    assert!(values.contains_key("updates/notified/zerus-update:v2:dev:1"));
}

#[test]
fn every_key_has_qt_s_default_and_range() {
    let default = |name: &str| effective(find(name).unwrap().kind, None);
    assert_eq!(default("workspace/theme"), "system");
    assert_eq!(default("workspace/language"), "en");
    assert_eq!(default("workspace/usageIndicator"), "rails");
    assert_eq!(default("workspace/contentScale"), 1.0);
    assert_eq!(default("workspace/activityWidth"), 900);
    assert_eq!(default("workspace/compact"), false);
    assert_eq!(default("workspace/expandSessionsOnHover"), true);
    assert_eq!(default("workspace/hideEmptyProjects"), true);
    assert_eq!(default("workspace/animateActivity"), true);
    assert_eq!(default("notifications/optionalQuestions"), false);
    assert_eq!(default("processes/enabled"), false);
    assert_eq!(default("processes/activeSeconds"), 2.5);
    assert_eq!(default("processes/backgroundSeconds"), 30.0);
    assert_eq!(default("processes/unknownSeconds"), 10.0);
    assert_eq!(default("processes/loadingSeconds"), 0.5);
    assert_eq!(default("updates/channel"), "stable");
    assert_eq!(default("updates/automatic"), true);
    assert_eq!(default("shortcuts/queue.sendNow"), json!([]));
    // The opt-in itself is not a client setting.
    assert!(find("automaticApprovals/enabled").is_none());
    assert!(find("shortcuts/").is_none() && find("shortcuts/a/b").is_none());
    assert_eq!(default("client/darkSkin"), "zerus-dark");
    assert_eq!(default("client/lightSkin"), "zerus-light");
    assert!(find("client/skin").is_none());
    assert_eq!(default("client/overviewWidth"), 1200);
    assert_eq!(default("client/overviewFullWidth"), false);
    // The client's own keys are never seeded from Qt.
    for key in KEYS.iter().filter(|key| key.name.starts_with(CLIENT)) {
        assert!(!key.seeded, "{}", key.name);
    }
    let clamp = |name: &str, value: Value| effective(find(name).unwrap().kind, Some(&value));
    assert_eq!(clamp("workspace/contentScale", json!(5)), 2.0);
    assert_eq!(clamp("workspace/activityWidth", json!(100)), 480);
    assert_eq!(clamp("processes/loadingSeconds", json!(-1)), 0.0);
    assert_eq!(clamp("workspace/theme", json!("sepia")), "system");
    assert_eq!(clamp("workspace/language", json!("de")), "en");
    // Names are unique.
    let mut names: Vec<_> = KEYS.iter().map(|key| key.name).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), KEYS.len());
}

#[test]
fn qt_values_convert_with_qt_s_readers() {
    let kind = |name: &str| find(name).unwrap().kind;
    // toDouble fails to 0, which ContentScale::clamp lifts to 75%.
    assert_eq!(
        from_qt(kind("workspace/contentScale"), &QtValue::Text("big".into())),
        0.75
    );
    // toInt(&ok) failing keeps ActivityWidth's default.
    assert_eq!(
        from_qt(
            kind("workspace/activityWidth"),
            &QtValue::Text("9.5".into())
        ),
        900
    );
    assert_eq!(
        from_qt(kind("workspace/activityWidth"), &QtValue::Double(1000.6)),
        1001
    );
    assert_eq!(
        from_qt(kind("workspace/compact"), &QtValue::Text("FALSE".into())),
        false
    );
    assert_eq!(
        from_qt(kind("speech/profile_revision"), &QtValue::Double(7.0)),
        7
    );
    assert_eq!(from_qt(Kind::Chords, &QtValue::Invalid), json!([]));
}

#[test]
fn the_ini_seed_takes_qt_s_values_once_and_never_writes_qt_s_store() {
    let scratch = Scratch::new("seed-ini");
    let qt = scratch.0.join("hgs-tray.conf");
    fs::copy(fixture("qt-6.12.ini"), &qt).unwrap();
    let before = (
        fs::read(&qt).unwrap(),
        fs::metadata(&qt).unwrap().modified().unwrap(),
    );
    let path = scratch.0.join("data/settings.json");
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    let get = |name: &str| settings.get(name);
    assert_eq!(get("workspace/theme"), "dark");
    assert_eq!(get("workspace/language"), "ru");
    assert_eq!(get("workspace/usageIndicator"), "marker-countdown");
    assert_eq!(get("workspace/contentScale"), 1.25);
    assert_eq!(get("workspace/activityWidth"), 1300);
    assert_eq!(get("workspace/activityFullWidth"), true);
    assert_eq!(get("workspace/compact"), true);
    assert_eq!(get("workspace/hideEmptyProjects"), false);
    // Not stored in Qt: its default.
    assert_eq!(get("workspace/expandSessionsOnHover"), true);
    assert_eq!(get("notifications/optionalQuestions"), true);
    assert_eq!(get("processes/enabled"), true);
    assert_eq!(get("processes/activeSeconds"), 60.0);
    assert_eq!(get("processes/backgroundSeconds"), 5.0);
    assert_eq!(get("processes/loadingSeconds"), 0.0);
    assert_eq!(get("speech/manage_host"), "studio");
    assert_eq!(get("speech/host"), "studio");
    assert_eq!(get("speech/profile"), "@profile");
    assert_eq!(
        get("speech/machine_id"),
        "  spaced, \"quoted\"\tand\nnew line  "
    );
    assert_eq!(get("speech/preview_before_send"), false);
    assert_eq!(get("speech/video_mode"), true);
    assert_eq!(get("speech/profile_revision"), 7);
    // A Qt device id names no device of the client's.
    assert_eq!(get("speech/microphone"), "");
    assert_eq!(get("downloads/cacheBytes"), 5_368_709_120_i64);
    assert_eq!(get("downloads/folder"), "/srv/Загрузки, общие");
    assert_eq!(get("updates/channel"), "dev");
    assert_eq!(get("updates/automatic"), false);
    assert_eq!(get(QT_AUTOMATIC_APPROVALS), true);
    assert_eq!(
        get("shortcuts/queue.sendNow"),
        json!(["Ctrl+Return", "Meta+Return"])
    );
    let all = settings.all();
    assert_eq!(all["shortcuts/message.push"], json!([]));
    assert_eq!(all["shortcuts/app.settings"], json!(["Ctrl+,"]));
    assert_eq!(all["shortcuts/app.find"], json!([""]));
    let record = settings.seed_record().unwrap();
    assert_eq!(record["source"], "qt-ini");
    assert_eq!(record["path"], qt.to_string_lossy().as_ref());
    assert!(record["keys"]
        .as_array()
        .unwrap()
        .contains(&json!("workspace/theme")));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    assert_eq!(
        (
            fs::read(&qt).unwrap(),
            fs::metadata(&qt).unwrap().modified().unwrap()
        ),
        before,
        "Qt's store stays untouched"
    );
    // Once: a later Qt edit is not imported.
    fs::write(&qt, b"[workspace]\ntheme=light\n").unwrap();
    let again = Settings::open(path, Some(QtStore::Ini(qt)));
    assert_eq!(again.get("workspace/theme"), "dark");
    assert_eq!(again.seed_record(), Some(record));
}

#[test]
fn the_plist_seed_reads_qt_s_mac_store() {
    let scratch = Scratch::new("seed-plist");
    let qt = scratch.0.join("com.hgdev.hgs-tray.plist");
    fs::copy(fixture("mac.plist"), &qt).unwrap();
    let store = QtStore::Plist {
        domain: None,
        path: qt,
    };
    let settings = Settings::open(scratch.0.join("settings.json"), Some(store));
    assert_eq!(settings.get("workspace/theme"), "light");
    assert_eq!(settings.get("workspace/contentScale"), 1.5);
    assert_eq!(settings.get("workspace/activityWidth"), 1000);
    assert_eq!(settings.get("processes/unknownSeconds"), 600.0);
    assert_eq!(settings.get("speech/manage_host"), "laptop");
    assert_eq!(settings.get("speech/profile"), "@x");
    assert_eq!(settings.get(QT_AUTOMATIC_APPROVALS), true);
    assert_eq!(
        settings.get("shortcuts/queue.sendNow"),
        json!(["Ctrl+Return"])
    );
    assert_eq!(settings.seed_record().unwrap()["source"], "qt-plist");
}

#[test]
fn the_plist_file_is_the_fallback_when_cfprefsd_cannot_be_asked() {
    if Path::new("/usr/bin/defaults").exists() {
        return; // macOS: covered by the export test below.
    }
    let store = QtStore::Plist {
        domain: Some("com.example.zerus-settings-test".into()),
        path: fixture("mac.plist"),
    };
    let (values, source) = store.read().unwrap().unwrap();
    assert_eq!(source, "qt-plist");
    assert_eq!(values["workspace/theme"], QtValue::Text("light".into()));
}

#[test]
fn an_exported_xml_plist_reads_like_the_binary_file() {
    let binary = read_plist(&fs::read(fixture("mac.plist")).unwrap()).unwrap();
    let xml = read_plist(&fs::read(fixture("mac-export.plist")).unwrap()).unwrap();
    assert_eq!(xml, binary);
}

/// cfprefsd answers for a domain, read-only (the global domain always
/// exists).
#[cfg(target_os = "macos")]
#[test]
fn a_domain_reads_through_defaults_export() {
    let store = QtStore::Plist {
        domain: Some("NSGlobalDomain".into()),
        path: PathBuf::from("/nonexistent/NSGlobalDomain.plist"),
    };
    let (_, source) = store.read().unwrap().unwrap();
    assert_eq!(source, "qt-defaults");
}

#[test]
fn a_missing_qt_store_seeds_nothing_and_is_not_read_later() {
    let scratch = Scratch::new("seed-missing");
    let qt = scratch.0.join("hgs-tray.conf");
    let path = scratch.0.join("settings.json");
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert_eq!(settings.get("workspace/theme"), "system");
    let record = settings.seed_record().unwrap();
    assert_eq!(record["source"], "none");
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    fs::write(&qt, b"[workspace]\ntheme=dark\n").unwrap();
    let again = Settings::open(path, Some(QtStore::Ini(qt)));
    assert_eq!(again.get("workspace/theme"), "system");
}

#[cfg(unix)]
#[test]
fn an_unreadable_qt_store_is_seeded_at_the_next_start() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("seed-unreadable");
    let qt = scratch.0.join("hgs-tray.conf");
    fs::write(&qt, b"[workspace]\ntheme=dark\n").unwrap();
    fs::set_permissions(&qt, fs::Permissions::from_mode(0o000)).unwrap();
    let path = scratch.0.join("settings.json");
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    let unreadable = settings.get("workspace/theme");
    fs::set_permissions(&qt, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(unreadable, "system");
    assert_eq!(settings.seed_record(), None);
    drop(settings);
    let again = Settings::open(path, Some(QtStore::Ini(qt)));
    assert_eq!(again.get("workspace/theme"), "dark");
    assert_eq!(again.seed_record().unwrap()["source"], "qt-ini");
}

#[test]
fn without_a_seed_source_nothing_is_seeded() {
    let scratch = Scratch::new("no-seed");
    let settings = Settings::open(scratch.0.join("settings.json"), None);
    assert_eq!(settings.get("workspace/theme"), "system");
    assert_eq!(settings.seed_record(), None);
}

#[test]
fn set_normalizes_persists_and_tells_listeners() {
    let scratch = Scratch::new("set");
    let path = scratch.0.join("data/settings.json");
    let settings = Settings::open(path.clone(), None);
    let heard = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&heard);
    let revisions = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&revisions);
    settings.subscribe(Box::new(move |name, value, revision| {
        sink.lock().unwrap().push((name.to_owned(), value.clone()));
        seen.lock().unwrap().push(revision);
    }));
    assert_eq!(
        settings.set("workspace/theme", json!("dark")),
        Ok(json!("dark"))
    );
    assert_eq!(
        settings.set("processes/activeSeconds", json!(0.2)),
        Ok(json!(1.0))
    );
    assert_eq!(
        settings.set("workspace/activityWidth", json!(1300)),
        Ok(json!(1300))
    );
    assert!(settings
        .set("workspace/activityWidth", json!(13.5))
        .is_err());
    assert!(settings.set("workspace/compact", json!("yes")).is_err());
    assert!(settings.set("no/such", json!(true)).is_err());
    assert!(settings.set(QT_AUTOMATIC_APPROVALS, json!(true)).is_err());
    assert_eq!(
        settings.set("shortcuts/queue.sendNow", json!(["Ctrl+K"])),
        Ok(json!(["Ctrl+K"]))
    );
    assert_eq!(
        settings.set("shortcuts/message.push", json!([])),
        Ok(json!([]))
    );
    // Null removes an override: the defaults apply again, told as null.
    assert_eq!(
        settings.set("shortcuts/message.push", Value::Null),
        Ok(Value::Null)
    );
    assert!(!settings.all().contains_key("shortcuts/message.push"));
    assert_eq!(heard.lock().unwrap().len(), 6);
    // The load reads the revision under the same lock.
    assert_eq!(settings.all_tracked().1, 6);
    // Each change takes the next revision; the answer carries it too.
    assert_eq!(*revisions.lock().unwrap(), vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(
        settings.set_tracked("workspace/language", json!("ru")),
        Ok((json!("ru"), 7))
    );
    assert_eq!(
        heard.lock().unwrap()[1],
        ("processes/activeSeconds".to_owned(), json!(1.0))
    );
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["version"], 1);
    assert_eq!(stored["values"]["workspace/theme"], "dark");
    assert!(stored["values"].get("shortcuts/message.push").is_none());
    let names: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(names, vec!["settings.json"], "no temporary files left");
    let again = Settings::open(path, None);
    assert_eq!(again.get("workspace/theme"), "dark");
    assert_eq!(again.get("processes/activeSeconds"), 1.0);
    assert_eq!(again.get("shortcuts/queue.sendNow"), json!(["Ctrl+K"]));
}

#[test]
fn an_unreadable_store_keeps_its_file_and_runs_at_defaults() {
    let scratch = Scratch::new("damaged");
    let path = scratch.0.join("settings.json");
    fs::write(&path, b"{not json").unwrap();
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(fixture("qt-6.12.ini"))));
    assert_eq!(settings.get("workspace/theme"), "system");
    assert!(settings.error().is_some());
    assert!(settings.set("workspace/theme", json!("dark")).is_err());
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    assert_eq!(fs::read(&path).unwrap(), b"{not json");
}

#[test]
fn seed_values_take_only_what_qt_stored() {
    let values: BTreeMap<String, QtValue> = [
        ("workspace/theme".to_owned(), QtValue::Text("dark".into())),
        ("speech/host".to_owned(), QtValue::Text("old".into())),
        (
            "automaticApprovals/enabled".to_owned(),
            QtValue::Bool(false),
        ),
        // SettingsJson stores the unconfirmed provider change as bytes.
        (
            "speech/providerOperation".to_owned(),
            QtValue::Bytes(br#"{"request_id":"r"}"#.to_vec()),
        ),
    ]
    .into_iter()
    .collect();
    let seeded = seed_values(
        &values,
        &[
            "workspace/theme",
            "workspace/compact",
            "speech/manage_host",
            "speech/host",
            "speech/providerOperation",
            QT_AUTOMATIC_APPROVALS,
            "speech/microphone",
        ],
    );
    assert_eq!(
        seeded,
        [
            (QT_AUTOMATIC_APPROVALS.to_owned(), json!(false)),
            ("speech/host".to_owned(), json!("old")),
            ("speech/manage_host".to_owned(), json!("old")),
            (
                "speech/providerOperation".to_owned(),
                json!(r#"{"request_id":"r"}"#)
            ),
            ("workspace/theme".to_owned(), json!("dark")),
        ]
        .into_iter()
        .collect()
    );
}

/// The web client imports every setting's default from
/// `client/src/store/settings-defaults.json`; this keeps that file equal to
/// the table. `ZERUS_UPDATE_SETTINGS_DEFAULTS=1` rewrites it.
#[test]
fn the_web_client_defaults_equal_the_table() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/store/settings-defaults.json");
    let table: serde_json::Map<String, Value> = KEYS
        .iter()
        .map(|key| (key.name.to_owned(), effective(key.kind, None)))
        .collect();
    let expected = serde_json::to_string_pretty(&table).unwrap() + "\n";
    if std::env::var_os("ZERUS_UPDATE_SETTINGS_DEFAULTS").is_some() {
        fs::write(&path, &expected).unwrap();
    }
    assert_eq!(
        fs::read_to_string(&path).unwrap_or_default(),
        expected,
        "regenerate with ZERUS_UPDATE_SETTINGS_DEFAULTS=1 cargo test --test settings"
    );
}
