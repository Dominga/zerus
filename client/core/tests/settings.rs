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
    effective, find, from_qt, seed_values, Kind, QtPlatform, QtStore, Settings, CLIENT, KEYS,
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
fn qt_sources_resolve_without_opening_any_user_store() {
    let home = Path::new("/home/example");
    assert_eq!(
        QtStore::locate(QtPlatform::Linux, Some(home), None),
        Some(QtStore::Ini(home.join(".config/hgdev/hgs-tray.conf")))
    );
    assert_eq!(
        QtStore::locate(
            QtPlatform::Linux,
            Some(home),
            Some(Path::new("/config/example"))
        ),
        Some(QtStore::Ini(PathBuf::from(
            "/config/example/hgdev/hgs-tray.conf"
        )))
    );
    for relative in ["", "relative-config"] {
        assert_eq!(
            QtStore::locate(QtPlatform::Linux, Some(home), Some(Path::new(relative))),
            QtStore::locate(QtPlatform::Linux, Some(home), None)
        );
    }
    let appdata = Path::new("C:/Users/example/AppData/Roaming");
    assert_eq!(
        QtStore::locate(QtPlatform::Windows, None, Some(appdata)),
        Some(QtStore::Ini(appdata.join("hgdev/hgs-tray.ini")))
    );
    assert_eq!(QtStore::locate(QtPlatform::Windows, Some(home), None), None);
    assert_eq!(QtStore::locate(QtPlatform::Linux, None, None), None);
    let mac = Path::new("/Users/example");
    assert_eq!(
        QtStore::locate(QtPlatform::Mac, Some(mac), None),
        Some(QtStore::Plist {
            domain: Some("com.hgdev.hgs-tray".into()),
            path: mac.join("Library/Preferences/com.hgdev.hgs-tray.plist"),
        })
    );
}

#[test]
fn linux_and_windows_switches_seed_once_and_keep_the_clients_choices() {
    let scratch = Scratch::new("switch-platforms");
    for platform in [QtPlatform::Linux, QtPlatform::Windows] {
        let base = scratch.0.join(format!("{platform:?}"));
        let config = base.join("config");
        let qt = QtStore::locate(
            platform,
            Some(&base),
            (platform == QtPlatform::Windows).then_some(config.as_path()),
        )
        .unwrap();
        fs::create_dir_all(qt.path().parent().unwrap()).unwrap();
        let source = b"[workspace]\ntheme=dark\nlanguage=ru\nskin=whitewh\ninspectorWidth=512\ninspectorOpen=true\ninspectorTab=details\n[automaticApprovals]\nenabled=true\n";
        fs::write(qt.path(), source).unwrap();
        let before = fs::metadata(qt.path()).unwrap().modified().unwrap();
        let path = base.join("com.hgdev.hgs-tray/settings.json");
        // An existing interim choice wins over Qt's old value.
        let interim = Settings::open(path.clone(), None);
        interim.set("workspace/theme", json!("light")).unwrap();
        assert!(interim.flush_within(FLUSH));
        drop(interim);
        let migrated = Settings::open(path.clone(), Some(qt.clone()));
        assert_eq!(migrated.get("workspace/theme"), "light");
        assert_eq!(migrated.get("workspace/language"), "ru");
        assert_eq!(migrated.get("workspace/skin"), "whitewh");
        assert_eq!(migrated.get("workspace/inspectorWidth"), 512);
        assert_eq!(migrated.get("workspace/inspectorOpen"), true);
        assert_eq!(migrated.get("workspace/inspectorTab"), "details");
        assert_eq!(migrated.get(QT_AUTOMATIC_APPROVALS), true);
        assert!(!migrated.flag("automaticApprovals/enabled"));
        assert_eq!(migrated.get("automaticApprovals/error"), "");
        let seed = migrated.seed_record().unwrap();
        let seeded = seed["keys"].as_array().unwrap();
        for name in ["automaticApprovals/enabled", "automaticApprovals/error"] {
            assert!(!seeded.contains(&json!(name)), "{name}");
        }
        assert!(migrated.flush_within(FLUSH));
        drop(migrated);
        assert_eq!(fs::read(qt.path()).unwrap(), source);
        assert_eq!(fs::metadata(qt.path()).unwrap().modified().unwrap(), before);
        // A rollback edit never becomes a new seed, even after client edits.
        fs::write(qt.path(), b"[workspace]\ntheme=system\nlanguage=en\n").unwrap();
        let restarted = Settings::open(path, Some(qt.clone()));
        assert_eq!(restarted.get("workspace/theme"), "light");
        assert_eq!(restarted.get("workspace/language"), "ru");
        assert_eq!(restarted.seed_record(), Some(seed));
    }
}

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
    // Qt's historical opt-in never arms the client's worker or imports errors.
    let automatic = find("automaticApprovals/enabled").unwrap();
    assert!(matches!(automatic.kind, Kind::Flag(false)));
    assert_eq!(default(automatic.name), false);
    assert!(!automatic.seeded);
    let error = find("automaticApprovals/error").unwrap();
    assert!(matches!(error.kind, Kind::Text("")));
    assert_eq!(default(error.name), "");
    assert!(!error.seeded);
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
fn update_families_seed_only_recognized_channels_and_tokens() {
    let scratch = Scratch::new("update-families");
    let qt = scratch.0.join("hgs-tray.conf");
    fs::write(
        &qt,
        b"[updates]\nlastAttempt/dev=20\nlastSuccess/dev=19\ncatalog/dev=@ByteArray(catalog)\nlastAttempt/other=99\nnotified/zerus-update:v2:dev:1=true\nnotified/other=true\n",
    )
    .unwrap();
    let settings = Settings::open(scratch.0.join("settings.json"), Some(QtStore::Ini(qt)));
    assert_eq!(settings.get("updates/lastAttempt/dev"), 20);
    assert_eq!(settings.get("updates/lastSuccess/dev"), 19);
    assert_eq!(settings.get("updates/catalog/dev"), "catalog");
    assert!(settings.flag("updates/notified/zerus-update:v2:dev:1"));
    assert!(settings.get("updates/lastAttempt/other").is_null());
    assert!(settings.get("updates/notified/other").is_null());
    assert!(!settings.contains("updates/channel"));
    settings.set("updates/channel", json!("dev")).unwrap();
    assert!(settings.contains("updates/channel"));
    settings
        .set("updates/lastSuccess/nightly", json!(50))
        .unwrap();
    let all = settings.all();
    assert_eq!(all["updates/lastSuccess/nightly"], 50);
    assert_eq!(all["updates/catalog/dev"], "catalog");
    assert!(settings.flush_within(FLUSH));
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
fn machine_appearance_editing_is_local_and_color_reset_preserves_style() {
    let scratch = Scratch::new("machine-appearance-edit");
    let path = scratch.0.join("settings.json");
    let settings = Settings::open(path.clone(), None);
    assert_eq!(
        settings.set("machines/colors/peer", json!("#abcdef")),
        Ok(json!("#abcdef"))
    );
    assert_eq!(
        settings.set("machines/vivid/peer", json!(true)),
        Ok(json!(true))
    );
    assert_eq!(
        settings.set("machines/colors/peer", Value::Null),
        Ok(Value::Null)
    );
    assert!(!settings.all().contains_key("machines/colors/peer"));
    assert!(settings.flag("machines/vivid/peer"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let reopened = Settings::open(path, None);
    assert!(!reopened.all().contains_key("machines/colors/peer"));
    assert!(reopened.flag("machines/vivid/peer"));
}

#[test]
fn account_marks_are_local_per_profile_and_removable() {
    let scratch = Scratch::new("account-marks");
    let path = scratch.0.join("settings.json");
    let settings = Settings::open(path.clone(), None);
    assert_eq!(settings.get("sessions/accountMarks"), "letter");
    assert_eq!(
        settings.set("sessions/accountMarks", json!("emoji")),
        Ok(json!("emoji"))
    );
    let color = "accounts/color/studio%1Fclaude-a1";
    let mark = "accounts/mark/studio%1Fclaude-a1";
    assert_eq!(settings.set(color, json!("#2b97ff")), Ok(json!("#2b97ff")));
    assert_eq!(settings.set(mark, json!("x")), Ok(json!("x")));
    for invalid in [
        "accounts/color/",
        "accounts/color/a/b",
        "accounts/mark/a\\b",
    ] {
        assert!(
            settings.set(invalid, json!("#2b97ff")).is_err(),
            "{invalid}"
        );
    }
    assert!(settings.all().contains_key(color));
    assert_eq!(settings.set(mark, Value::Null), Ok(Value::Null));
    assert!(!settings.all().contains_key(mark));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let reopened = Settings::open(path, None);
    assert_eq!(reopened.get(color), "#2b97ff");
    assert!(!reopened.all().contains_key(mark));
    assert_eq!(reopened.get("sessions/accountMarks"), "emoji");
}

#[test]
fn account_marks_can_be_hidden_per_account_and_everywhere() {
    // zerus-sa9qn.13: presentation only; labels and colors stay.
    let scratch = Scratch::new("account-marks-hidden");
    let settings = Settings::open(scratch.0.join("settings.json"), None);
    assert_eq!(settings.get("sessions/showAccountMarks"), true);
    assert_eq!(
        settings.set("sessions/showAccountMarks", json!(false)),
        Ok(json!(false))
    );
    let hidden = "accounts/hide/studio%1Fclaude-a1";
    assert_eq!(settings.set(hidden, json!(true)), Ok(json!(true)));
    assert!(settings.set(hidden, json!("yes")).is_err());
    assert!(settings.set("accounts/hide/a/b", json!(true)).is_err());
    assert_eq!(settings.set(hidden, Value::Null), Ok(Value::Null));
    assert!(!settings.all().contains_key(hidden));
}

#[test]
fn new_child_rows_start_compact_unless_turned_off() {
    // zerus-sa9qn.14: a client flag, on by default, and the device's record.
    let scratch = Scratch::new("compact-new-children");
    let settings = Settings::open(scratch.0.join("settings.json"), None);
    assert_eq!(settings.get("sessions/compactNewChildren"), true);
    assert_eq!(
        settings.set("sessions/compactNewChildren", json!(false)),
        Ok(json!(false))
    );
    assert_eq!(settings.get("sessions/childRowsSeen"), "");
    let seen = r#"{"11111111-1111-4111-8111-111111111111":[1,["a"]]}"#;
    assert_eq!(
        settings.set("sessions/childRowsSeen", json!(seen)),
        Ok(json!(seen))
    );
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

/// A write the disk refuses is not flushed: flush_within promises the data
/// is on disk (speech providers send nothing on a false answer).
#[cfg(unix)]
#[test]
fn flush_within_is_false_after_a_refused_write() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("refused");
    let dir = scratch.0.join("data");
    fs::create_dir_all(&dir).unwrap();
    let settings = Settings::open(dir.join("settings.json"), None);
    assert!(settings.set("workspace/theme", json!("dark")).is_ok());
    assert!(settings.flush_within(FLUSH));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
    // Root ignores the mode; the refusal cannot be shown then.
    if fs::write(dir.join("probe"), b"").is_ok() {
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    assert!(settings.set("workspace/theme", json!("light")).is_ok());
    assert!(!settings.flush_within(FLUSH));
    assert!(settings
        .error()
        .is_some_and(|error| error.starts_with("Settings not saved")));
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    // A later write that succeeds clears the failure.
    assert!(settings.set("workspace/theme", json!("system")).is_ok());
    assert!(settings.flush_within(FLUSH));
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
    // Task #33: compared as text. A Windows checkout with Git's autocrlf
    // holds the fixture with CRLF, and the bytes then differed although the
    // contents are equal.
    assert_eq!(
        fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n"),
        expected,
        "regenerate with ZERUS_UPDATE_SETTINGS_DEFAULTS=1 cargo test --test settings"
    );
}

fn legacy_switch(path: &Path, values: Value) {
    fs::write(
        path,
        serde_json::to_vec(&json!({
            "version": 1, "seed": {"source":"qt-ini", "at":1, "keys":["workspace/theme"]},
            "values": values, "future_extension":{"retained":true},
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn first_g3_families_import_once_after_the_switch_and_existing_next_values_win() {
    let scratch = Scratch::new("g3-families");
    let qt = scratch.0.join("qt.conf");
    let bytes = b"[workspace]\ntheme=dark\nlanguage=ru\nallSwarmProjects=true\n[machines]\ncolors\\peer=#123456\ncolors\\local=#abcdef\nvivid\\local=true\n";
    fs::write(&qt, bytes).unwrap();
    let modified = fs::metadata(&qt).unwrap().modified().unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(
        &path,
        json!({"workspace/theme":"light", "workspace/allSwarmProjects":false, "machines/colors/peer":"#654321"}),
    );
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert_eq!(
        settings.get("workspace/theme"),
        "light",
        "the old seed never repeats"
    );
    assert_eq!(
        settings.get("workspace/language"),
        "en",
        "old seed keys never newly enter"
    );
    assert!(
        !settings.flag("workspace/allSwarmProjects"),
        "existing Next false is still a value"
    );
    assert_eq!(settings.text("machines/colors/peer"), "#654321");
    assert_eq!(settings.all()["machines/colors/local"], "#abcdef");
    assert!(settings.flag("machines/vivid/local"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    assert_eq!(fs::read(&qt).unwrap(), bytes);
    assert_eq!(fs::metadata(&qt).unwrap().modified().unwrap(), modified);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["future_extension"], json!({"retained":true}));
    for family in ["machine-appearance", "project-view"] {
        let record = &saved["g3"]["families"][family];
        assert_eq!(record["family"], family);
        assert_eq!(record["schema"], 1);
        assert_eq!(record["source_path"], qt.to_string_lossy().as_ref());
        assert_eq!(record["sha256"], zerus_desktop_migration::sha256(bytes));
        assert_eq!(record["source_mtime_after_switch"], true);
        assert!(record["at"].as_u64().unwrap() > 1);
    }
    assert_eq!(
        saved["g3"]["families"]["project-view"]["keys"],
        json!(["workspace/allSwarmProjects"])
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // Handled families do not even need a readable source on another switch.
    fs::remove_file(&qt).unwrap();
    fs::create_dir(&qt).unwrap();
    let again = Settings::open(path.clone(), Some(QtStore::Ini(qt)));
    assert_eq!(again.text("machines/colors/local"), "#abcdef");
    assert!(again.flag("machines/vivid/local"));
    assert!(again.flush_within(FLUSH));
    drop(again);
    let reopened: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(reopened, saved);
}

#[test]
fn the_teams_view_preferences_import_once_and_existing_next_values_win() {
    let scratch = Scratch::new("g3-teams-view");
    let qt = scratch.0.join("qt.conf");
    let bytes = b"[teams]\nhideFinished=true\nheat\\team-one=time\nheat\\team-two=tokens\nplanMode\\team-one=free\n[ui]\ncatalogHideUnusedRoles=true\n";
    fs::write(&qt, bytes).unwrap();
    let path = scratch.0.join("settings.json");
    // The switch happened before this family existed: the old seed never
    // repeats, so only the family can import these keys now.
    legacy_switch(&path, json!({"teams/hideFinished": false}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(
        !settings.flag("teams/hideFinished"),
        "an existing Next value wins"
    );
    assert!(settings.flag("ui/catalogHideUnusedRoles"));
    assert_eq!(
        settings.text("teams/heatModes"),
        r#"{"team-one":"time","team-two":"tokens"}"#
    );
    assert_eq!(settings.text("teams/planModes"), r#"{"team-one":"free"}"#);
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    // Qt's store is only read.
    assert_eq!(fs::read(&qt).unwrap(), bytes);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let record = &saved["g3"]["families"]["teams-view-preferences"];
    assert_eq!(record["family"], "teams-view-preferences");
    assert_eq!(
        record["keys"],
        json!([
            "teams/heat/team-one",
            "teams/heat/team-two",
            "teams/hideFinished",
            "teams/planMode/team-one",
            "ui/catalogHideUnusedRoles"
        ])
    );
    // A handled family imports nothing more, even from a changed source.
    fs::write(&qt, b"[teams]\nhideFinished=false\nheat\\team-one=tokens\n").unwrap();
    let again = Settings::open(path.clone(), Some(QtStore::Ini(qt)));
    assert!(again.flag("ui/catalogHideUnusedRoles"));
    assert_eq!(
        again.text("teams/heatModes"),
        r#"{"team-one":"time","team-two":"tokens"}"#
    );
    assert_eq!(again.text("teams/planModes"), r#"{"team-one":"free"}"#);
    assert!(again.flush_within(FLUSH));
}

#[test]
fn a_bad_teams_view_family_is_quarantined_without_partial_values() {
    let scratch = Scratch::new("g3-teams-view-bad");
    let qt = scratch.0.join("qt.conf");
    let bytes =
        b"[teams]\nhideFinished=true\nheat\\team-one=purple\n[ui]\ncatalogHideUnusedRoles=true\n";
    fs::write(&qt, bytes).unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(
        !settings.flag("teams/hideFinished"),
        "no partial family is imported"
    );
    assert!(!settings.flag("ui/catalogHideUnusedRoles"));
    assert_eq!(settings.text("teams/heatModes"), "{}");
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(saved["g3"]["families"]
        .get("teams-view-preferences")
        .is_none());
    assert_eq!(
        saved["g3"]["quarantine"]["teams-view-preferences"]["preserved_in"],
        "source"
    );
    assert!(!saved.to_string().contains("purple"));
}

#[test]
fn a_failed_family_is_quarantined_as_metadata_and_retries_without_partial_values() {
    let scratch = Scratch::new("g3-retry");
    let qt = scratch.0.join("qt.conf");
    let bytes = b"[machines]\ncolors\\good=#123456\ncolors\\bad=not-a-color-private-value\n[workspace]\nallSwarmProjects=true\n";
    fs::write(&qt, bytes).unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(
        !settings.contains("machines/colors/good"),
        "no partial family is imported"
    );
    assert!(
        settings.flag("workspace/allSwarmProjects"),
        "independent family completes"
    );
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(saved["g3"]["families"].get("machine-appearance").is_none());
    assert_eq!(
        saved["g3"]["quarantine"]["machine-appearance"]["preserved_in"],
        "source"
    );
    assert!(!saved.to_string().contains("not-a-color-private-value"));
    assert_eq!(fs::read(&qt).unwrap(), bytes);
    fs::write(
        &qt,
        b"[machines]\ncolors\\good=#abcdef\n[workspace]\nallSwarmProjects=false\n",
    )
    .unwrap();
    let retried = Settings::open(path.clone(), Some(QtStore::Ini(qt)));
    assert_eq!(retried.text("machines/colors/good"), "#abcdef");
    assert!(
        retried.flag("workspace/allSwarmProjects"),
        "handled families stay handled"
    );
    assert!(retried.flush_within(FLUSH));
    drop(retried);
    let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["g3"]["families"]["machine-appearance"]["schema"], 1);
}

#[test]
fn newer_settings_and_family_records_survive_without_reseeding() {
    let scratch = Scratch::new("g3-future");
    let qt = scratch.0.join("qt.conf");
    fs::write(
        &qt,
        b"[workspace]\nallSwarmProjects=true\n[machines]\ncolors\\peer=#abcdef\n",
    )
    .unwrap();
    let path = scratch.0.join("settings.json");
    let future = br#"{"version":2,"values":{"workspace/theme":"dark"},"unknown":"retained"}"#;
    fs::write(&path, future).unwrap();
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(settings.error().is_some());
    assert!(settings.set("workspace/theme", json!("light")).is_err());
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    assert_eq!(fs::read(&path).unwrap(), future);
    let g3 = json!({"version":2,"families":{},"new_schema":"retained"});
    fs::write(
        &path,
        serde_json::to_vec(&json!({"version":1,"values":{},"g3":g3})).unwrap(),
    )
    .unwrap();
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt)));
    assert!(!settings.contains("machines/colors/peer"));
    assert!(!settings.flag("workspace/allSwarmProjects"));
    assert!(settings.seed_record().is_none());
    settings.set("workspace/theme", json!("light")).unwrap();
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["g3"], g3);
}

#[test]
fn family_reports_are_bounded_and_never_commit_a_partial_large_family() {
    let scratch = Scratch::new("g3-bound");
    let qt = scratch.0.join("qt.conf");
    let mut source = String::from("[machines]\n");
    for n in 0..4097 {
        source.push_str(&format!("colors\\peer-{n}=#123456\n"));
    }
    fs::write(&qt, &source).unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(!settings.contains("machines/colors/peer-0"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(saved["g3"]["families"].get("machine-appearance").is_none());
    assert_eq!(
        saved["g3"]["quarantine"]["machine-appearance"]["keys"]
            .as_array()
            .unwrap()
            .len(),
        4096
    );
    assert_eq!(fs::read_to_string(qt).unwrap(), source);
}

#[test]
fn an_overlapping_source_is_preserved_without_even_a_family_record_write() {
    let scratch = Scratch::new("g3-overlap");
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({"workspace/theme":"dark"}));
    let before = fs::read(&path).unwrap();
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(path.clone())));
    assert!(settings.error().is_some());
    assert!(settings.set("workspace/theme", json!("light")).is_err());
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn row_density_imports_once_preserves_empty_next_values_and_retries_bad_packing() {
    let scratch = Scratch::new("g3-row-density");
    let qt = scratch.0.join("qt.conf");
    let original = format!(
        "[workspace]\ncollapsedSessionRows=@ByteArray({})\n",
        "\\xab".repeat(32)
    );
    fs::write(&qt, &original).unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert_eq!(
        settings.all()["workspace/collapsedSessionRows"],
        json!(["ab".repeat(32)])
    );
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        saved["g3"]["families"]["row-density"]["sha256"],
        zerus_desktop_migration::sha256(original.as_bytes())
    );
    assert_eq!(fs::read_to_string(&qt).unwrap(), original);
    let reopened = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    reopened
        .set("workspace/collapsedSessionRows", json!([]))
        .unwrap();
    assert!(reopened.flush_within(FLUSH));
    drop(reopened);
    let again = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert_eq!(again.all()["workspace/collapsedSessionRows"], json!([]));
    drop(again);
    // Existing explicit empty values win even before this family's first import.
    let empty = scratch.0.join("empty.json");
    legacy_switch(&empty, json!({"workspace/collapsedSessionRows":[]}));
    let settings = Settings::open(empty, Some(QtStore::Ini(qt.clone())));
    assert_eq!(settings.all()["workspace/collapsedSessionRows"], json!([]));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let bad = scratch.0.join("retry.json");
    legacy_switch(&bad, json!({}));
    fs::write(
        &qt,
        b"[workspace]\ncollapsedSessionRows=@ByteArray(short)\n",
    )
    .unwrap();
    let settings = Settings::open(bad.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(!settings.contains("workspace/collapsedSessionRows"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&bad).unwrap()).unwrap();
    assert!(saved["g3"]["families"].get("row-density").is_none());
    assert_eq!(
        saved["g3"]["quarantine"]["row-density"]["preserved_in"],
        "source"
    );
    fs::write(&qt, original).unwrap();
    let retried = Settings::open(bad, Some(QtStore::Ini(qt)));
    assert_eq!(
        retried.all()["workspace/collapsedSessionRows"],
        json!(["ab".repeat(32)])
    );
    assert!(retried.flush_within(FLUSH));
}

#[test]
fn oversized_preferences_cannot_make_the_existing_store_unreadable() {
    let scratch = Scratch::new("g3-store-limit");
    let path = scratch.0.join("settings.json");
    let settings = Settings::open(path.clone(), None);
    settings.set("workspace/theme", json!("dark")).unwrap();
    assert!(settings.flush_within(FLUSH));
    let before = fs::read(&path).unwrap();
    assert!(settings
        .set("workspace/launchAccounts", json!("x".repeat(4 << 20)))
        .is_err());
    assert_eq!(settings.text("workspace/theme"), "dark");
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    assert_eq!(fs::read(&path).unwrap(), before);
    let reopened = Settings::open(path, None);
    assert!(reopened.error().is_none());
    assert_eq!(reopened.text("workspace/theme"), "dark");
}

#[test]
fn task_preferences_cover_existing_switches_but_never_reseed_an_old_handled_key() {
    let scratch = Scratch::new("g3-task-preferences");
    let qt = scratch.0.join("qt.conf");
    let original = b"[tasks]\nonlySession=true\n";
    fs::write(&qt, original).unwrap();
    for (name, values, expected) in [
        ("previous-switch", json!({}), true),
        ("next-false-wins", json!({"tasks/onlySession":false}), false),
    ] {
        let path = scratch.0.join(format!("{name}.json"));
        legacy_switch(&path, values);
        let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
        assert_eq!(settings.flag("tasks/onlySession"), expected);
        assert!(settings.flush_within(FLUSH));
        drop(settings);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            saved["g3"]["families"]["task-preferences"]["keys"],
            json!(["tasks/onlySession"])
        );
        assert_eq!(
            saved["g3"]["families"]["task-preferences"]["sha256"],
            zerus_desktop_migration::sha256(original)
        );
    }
    let path = scratch.0.join("old-handled.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"version":1,"values":{},
        "seed":{"at":1,"source":"qt-ini","keys":["tasks/onlySession"]}}))
        .unwrap(),
    )
    .unwrap();
    let settings = Settings::open(path, Some(QtStore::Ini(qt.clone())));
    assert!(!settings.contains("tasks/onlySession"));
    assert!(!settings.flag("tasks/onlySession"));
    assert!(settings.flush_within(FLUSH));
    assert_eq!(fs::read(&qt).unwrap(), original);
    drop(settings);
    // An unusable value that is not an import candidate cannot erase or
    // repeatedly quarantine the person's explicit Next choice.
    fs::write(&qt, b"[tasks]\nonlySession=@Invalid()\n").unwrap();
    let kept = scratch.0.join("keep-invalid-unused.json");
    legacy_switch(&kept, json!({"tasks/onlySession":false}));
    let settings = Settings::open(kept.clone(), Some(QtStore::Ini(qt)));
    assert!(!settings.flag("tasks/onlySession"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(kept).unwrap()).unwrap();
    assert!(saved["g3"]["families"].get("task-preferences").is_some());
}

#[test]
fn inspector_preferences_are_device_local_and_explicit_false_wins() {
    use zerus_client_core::settings::{default_value, effective, find};
    let open = find("workspace/inspectorOpen").unwrap();
    assert!(open.seeded);
    assert_eq!(default_value(open.kind), json!("auto"));
    assert_eq!(effective(open.kind, Some(&json!(false))), json!(false));
    assert_eq!(effective(open.kind, Some(&json!(true))), json!(true));
    assert_eq!(effective(open.kind, Some(&json!("bad"))), json!("auto"));
    let width = find("workspace/inspectorWidth").unwrap();
    assert!(width.seeded);
    assert_eq!(effective(width.kind, Some(&json!(100))), json!(240));
    assert_eq!(effective(width.kind, Some(&json!(2000))), json!(900));
    assert!(find("workspace/inspectorTab").unwrap().seeded);
    assert!(find("workspace/inspectorClosed").unwrap().seeded);
}

#[test]
fn termination_default_imports_once_and_explicit_next_false_always_wins() {
    let scratch = Scratch::new("g3-termination-default");
    let qt = scratch.0.join("qt.conf");
    let original = b"[workspace]\nremoveWorktreeAfterTermination=true\n";
    fs::write(&qt, original).unwrap();
    for (name, values, expected) in [
        ("previous-switch", json!({}), true),
        (
            "next-false-wins",
            json!({"workspace/removeWorktreeAfterTermination":false}),
            false,
        ),
    ] {
        let path = scratch.0.join(format!("{name}.json"));
        legacy_switch(&path, values);
        let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
        assert_eq!(
            settings.flag("workspace/removeWorktreeAfterTermination"),
            expected
        );
        assert!(settings.flush_within(FLUSH));
        drop(settings);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            saved["g3"]["families"]["termination-default"]["keys"],
            json!(["workspace/removeWorktreeAfterTermination"])
        );
        assert_eq!(
            saved["g3"]["families"]["termination-default"]["sha256"],
            zerus_desktop_migration::sha256(original)
        );
    }
    let path = scratch.0.join("old-handled.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"version":1,"values":{},
        "seed":{"at":1,"source":"qt-ini","keys":["workspace/removeWorktreeAfterTermination"]}}))
        .unwrap(),
    )
    .unwrap();
    let settings = Settings::open(path, Some(QtStore::Ini(qt.clone())));
    assert!(!settings.contains("workspace/removeWorktreeAfterTermination"));
    assert!(!settings.flag("workspace/removeWorktreeAfterTermination"));
    assert!(settings.flush_within(FLUSH));
    assert_eq!(fs::read(&qt).unwrap(), original);
    drop(settings);
    // An unusable value that is not an import candidate cannot erase or
    // repeatedly quarantine the person's explicit Next choice.
    fs::write(
        &qt,
        b"[workspace]\nremoveWorktreeAfterTermination=@Invalid()\n",
    )
    .unwrap();
    let kept = scratch.0.join("keep-invalid-unused.json");
    legacy_switch(
        &kept,
        json!({"workspace/removeWorktreeAfterTermination":false}),
    );
    let settings = Settings::open(kept.clone(), Some(QtStore::Ini(qt)));
    assert!(!settings.flag("workspace/removeWorktreeAfterTermination"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(kept).unwrap()).unwrap();
    assert!(saved["g3"]["families"].get("termination-default").is_some());
}

#[test]
fn project_vivid_is_exact_local_boolean_state_and_never_an_old_switch_seed() {
    let scratch = Scratch::new("project-vivid");
    let path = scratch.0.join("settings.json");
    let store = Settings::open(path.clone(), None);
    for id in ["alpha", "nested/project", "project\\id"] {
        let key = format!("projects/vivid/{id}");
        assert!(!find(&key).unwrap().seeded);
        assert!(!store.contains(&key));
        assert_eq!(store.set(&key, json!(true)).unwrap(), json!(true));
        assert_eq!(store.set(&key, json!(false)).unwrap(), json!(false));
    }
    for key in [
        "projects/vivid/".into(),
        "projects/vivid/bad\nidentity".into(),
        format!("projects/vivid/{}", "a".repeat(257)),
    ] {
        assert!(find(&key).is_none());
    }
    assert!(store.flush_within(FLUSH));
    drop(store);
    let store = Settings::open(path, None);
    assert!(store.contains("projects/vivid/alpha"));
    assert!(!store.flag("projects/vivid/alpha"));
    assert!(!store.contains("projects/vivid/other"));
}

fn qt_layout_ini() -> Vec<u8> {
    fn ints(values: &[i32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }
    fn packed(bytes: &[u8]) -> String {
        format!(
            "@ByteArray({})",
            bytes
                .iter()
                .map(|b| format!("\\x{b:02x}"))
                .collect::<String>()
        )
    }
    fn split(first: i32, orientation: i32) -> Vec<u8> {
        let mut bytes = ints(&[0xff, 1, 2, first, 800]);
        bytes.push(0);
        bytes.extend(ints(&[8]));
        bytes.push(1);
        bytes.extend(ints(&[orientation]));
        bytes.push(1);
        bytes
    }
    let mut geometry = ints(&[0x01d9d0cb]);
    geometry.extend([0, 3, 0, 0]);
    geometry.extend(ints(&[20, 30, 1219, 829, 20, 30, 1219, 829, 0]));
    geometry.extend([2, 0]);
    geometry.extend(ints(&[1920, 20, 30, 1219, 829]));
    format!(
        "[sessions]\ngeometry={}\n[workspace]\nprojectFolderSplit={}\nsplitter={}\n",
        packed(&geometry),
        packed(&split(320, 2)),
        packed(&split(380, 1))
    )
    .into_bytes()
}

#[test]
fn window_and_splitter_families_import_once_without_mutating_qt_or_replacing_next() {
    let scratch = Scratch::new("window-and-splitter");
    let qt = scratch.0.join("qt.ini");
    let original = qt_layout_ini();
    fs::write(&qt, &original).unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert_eq!(settings.number("workspace/projectFolderSplit"), 320.0);
    assert_eq!(settings.number("workspace/sessionsWidth"), 380.0);
    let geometry: Value = serde_json::from_str(&settings.text("client/windowGeometry")).unwrap();
    assert_eq!(geometry["width"], 1200);
    assert_eq!(geometry["maximized"], true);
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (family, key) in [
        ("window-geometry", "sessions/geometry"),
        ("session-splitter", "workspace/splitter"),
        ("project-folder-splitter", "workspace/projectFolderSplit"),
    ] {
        assert_eq!(saved["g3"]["families"][family]["keys"], json!([key]));
        assert_eq!(
            saved["g3"]["families"][family]["sha256"],
            zerus_desktop_migration::sha256(&original)
        );
    }
    assert_eq!(fs::read(&qt).unwrap(), original);
    fs::write(&qt, b"[workspace]\nprojectFolderSplit=@Invalid()\n").unwrap();
    let restarted = Settings::open(path, Some(QtStore::Ini(qt.clone())));
    assert_eq!(restarted.number("workspace/projectFolderSplit"), 320.0);
    assert_eq!(restarted.number("workspace/sessionsWidth"), 380.0);
    assert!(restarted.flush_within(FLUSH));
    drop(restarted);
    // Explicit Next null/empty/unsupported values also count as present.
    let own = scratch.0.join("own.json");
    legacy_switch(
        &own,
        json!({"workspace/projectFolderSplit":null,"workspace/sessionsWidth":600,"client/windowGeometry":""}),
    );
    fs::write(&qt, &original).unwrap();
    let kept = Settings::open(own, Some(QtStore::Ini(qt.clone())));
    assert_eq!(kept.number("workspace/projectFolderSplit"), 230.0);
    assert_eq!(kept.number("workspace/sessionsWidth"), 600.0);
    assert_eq!(kept.text("client/windowGeometry"), "");
    assert!(kept.flush_within(FLUSH));
    assert_eq!(fs::read(&qt).unwrap(), original);
}

#[test]
fn malformed_splitter_is_unhandled_and_retry_is_family_atomic() {
    let scratch = Scratch::new("splitter-retry");
    let qt = scratch.0.join("qt.ini");
    let path = scratch.0.join("settings.json");
    fs::write(&qt, b"[workspace]\nprojectFolderSplit=@ByteArray(bad)\n").unwrap();
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(!settings.contains("workspace/projectFolderSplit"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(saved["g3"]["families"]
        .get("project-folder-splitter")
        .is_none());
    assert!(saved["g3"]["quarantine"]
        .get("project-folder-splitter")
        .is_some());
    fs::write(&qt, qt_layout_ini()).unwrap();
    let settings = Settings::open(path, Some(QtStore::Ini(qt)));
    assert_eq!(settings.number("workspace/projectFolderSplit"), 320.0);
    assert!(settings.flush_within(FLUSH));
}

/// Task 102: the fork's mobile services, New session's folder mode and the
/// microphone come over once on a client that switched before them, from a
/// fixture INI as the fork's Qt writes it. Next's own values win, a Qt
/// device id is never stored (an id this computer does not know gives no
/// name either), a malformed port keeps its whole family out, and the INI
/// is never written.
#[test]
fn task_102_families_import_once_and_the_ini_is_never_written() {
    use zerus_client_core::settings::{MICROPHONE_HINT, NEW_SESSION_FOLDER_MODE};
    let scratch = Scratch::new("t102-families");
    let qt = scratch.0.join("hgs-tray.ini");
    let bytes: &[u8] = b"[mobile]\r\nlocalRelay=true\r\nlocalRelayPort=9443\r\nrunConnector=false\r\n\r\n[speech]\r\nmicrophone=\"{0.0.1.00000000}.{00000000-0000-0000-0000-000000000000}\"\r\n\r\n[workspace]\r\nlaunchWorktree=none\r\n";
    fs::write(&qt, bytes).unwrap();
    let modified = fs::metadata(&qt).unwrap().modified().unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({"workspace/theme":"light"}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(settings.flag("mobile/localRelay"));
    assert_eq!(settings.number("mobile/localRelayPort"), 9443.0);
    assert!(!settings.flag("mobile/runConnector"));
    assert_eq!(settings.text(NEW_SESSION_FOLDER_MODE), "none");
    assert_eq!(
        settings.text("speech/microphone"),
        "",
        "a Qt device id is never stored"
    );
    assert!(
        !settings.contains(MICROPHONE_HINT),
        "an unknown endpoint gives no name"
    );
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    assert_eq!(fs::read(&qt).unwrap(), bytes);
    assert_eq!(fs::metadata(&qt).unwrap().modified().unwrap(), modified);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for family in [
        "mobile-local-services",
        "new-session-folder-mode",
        "voice-device-hint",
    ] {
        assert_eq!(saved["g3"]["families"][family]["family"], family);
    }
    // Handled: Qt's later change does not come over again.
    fs::write(&qt, b"[workspace]\r\nlaunchWorktree=auto\r\n").unwrap();
    let again = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert_eq!(again.text(NEW_SESSION_FOLDER_MODE), "none");
    drop(again);
    // Next's own choices win; anything but "none" is Auto; a bad port keeps
    // the mobile family out (quarantined, its source untouched).
    let path = scratch.0.join("settings-own.json");
    legacy_switch(
        &path,
        json!({"mobile/runConnector": true, "newSession/folderMode": "none"}),
    );
    let qt = scratch.0.join("hgs-tray-other.ini");
    let bytes: &[u8] = b"[mobile]\r\nlocalRelay=true\r\nlocalRelayPort=99999\r\n\r\n[workspace]\r\nlaunchWorktree=auto\r\n";
    fs::write(&qt, bytes).unwrap();
    let own = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert_eq!(own.text(NEW_SESSION_FOLDER_MODE), "none");
    assert!(!own.flag("mobile/localRelay"));
    assert_eq!(own.number("mobile/localRelayPort"), 8787.0);
    assert!(own.flush_within(FLUSH));
    drop(own);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        saved["g3"]["quarantine"]["mobile-local-services"]["reason"],
        "invalid relay port"
    );
    assert_eq!(fs::read(&qt).unwrap(), bytes);
    let path = scratch.0.join("settings-auto.json");
    legacy_switch(&path, json!({}));
    let auto = Settings::open(path, Some(QtStore::Ini(qt)));
    assert_eq!(auto.text(NEW_SESSION_FOLDER_MODE), "auto");
}

/// zerus-5wmh.66.5.3: Qt's side panel (shown, width, tab) imports once into
/// Next's inspector keys; Next's own choices win and Qt is never written.
#[test]
fn inspector_panel_family_imports_once_and_next_choices_win() {
    let scratch = Scratch::new("inspector-panel");
    let qt = scratch.0.join("qt.ini");
    let original =
        b"[workspace]\ninspectorVisible=true\ninspectorWidth=600\ninspectorTab=2\n".to_vec();
    fs::write(&qt, &original).unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert_eq!(settings.get("workspace/inspectorOpen"), json!(true));
    assert_eq!(settings.number("workspace/inspectorWidth"), 600.0);
    assert_eq!(settings.text("workspace/inspectorTab"), "worktrees");
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let family = &saved["g3"]["families"]["inspector-panel"];
    assert_eq!(
        family["keys"],
        json!([
            "workspace/inspectorTab",
            "workspace/inspectorVisible",
            "workspace/inspectorWidth"
        ])
    );
    assert_eq!(family["sha256"], zerus_desktop_migration::sha256(&original));
    assert_eq!(fs::read(&qt).unwrap(), original);
    // Handled once: a later Qt change is never imported again.
    fs::write(
        &qt,
        b"[workspace]\ninspectorVisible=false\ninspectorTab=0\n",
    )
    .unwrap();
    let restarted = Settings::open(path, Some(QtStore::Ini(qt.clone())));
    assert_eq!(restarted.get("workspace/inspectorOpen"), json!(true));
    assert_eq!(restarted.text("workspace/inspectorTab"), "worktrees");
    assert!(restarted.flush_within(FLUSH));
    drop(restarted);
    // Next's own choices win, a hidden panel included; Qt bounds its tab
    // index and Next its width.
    fs::write(
        &qt,
        b"[workspace]\ninspectorVisible=true\ninspectorWidth=5000\ninspectorTab=9\n",
    )
    .unwrap();
    let own = scratch.0.join("own.json");
    legacy_switch(&own, json!({"workspace/inspectorOpen": false}));
    let kept = Settings::open(own, Some(QtStore::Ini(qt.clone())));
    assert_eq!(kept.get("workspace/inspectorOpen"), json!(false));
    assert_eq!(kept.number("workspace/inspectorWidth"), 900.0);
    assert_eq!(kept.text("workspace/inspectorTab"), "tasks");
    assert!(kept.flush_within(FLUSH));
}

/// #1239 review: Qt reads the tab with a 32-bit toInt(); a value outside it
/// waits (the whole family) instead of importing a tab Qt never showed.
#[test]
fn an_inspector_tab_outside_qts_32_bit_domain_waits() {
    let scratch = Scratch::new("inspector-tab-domain");
    let qt = scratch.0.join("qt.ini");
    let path = scratch.0.join("settings.json");
    fs::write(
        &qt,
        b"[workspace]\ninspectorVisible=true\ninspectorTab=2147483648\n",
    )
    .unwrap();
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(!settings.contains("workspace/inspectorTab"));
    assert!(!settings.contains("workspace/inspectorOpen"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        saved["g3"]["quarantine"]["inspector-panel"]["reason"],
        "invalid inspector tab"
    );
    // The largest 32-bit index is one Qt bounds to its last tab.
    fs::write(&qt, b"[workspace]\ninspectorTab=2147483647\n").unwrap();
    let settings = Settings::open(path, Some(QtStore::Ini(qt)));
    assert_eq!(settings.text("workspace/inspectorTab"), "tasks");
    assert!(settings.flush_within(FLUSH));
}

#[test]
fn a_malformed_inspector_value_leaves_the_whole_family_for_a_retry() {
    let scratch = Scratch::new("inspector-retry");
    let qt = scratch.0.join("qt.ini");
    let path = scratch.0.join("settings.json");
    fs::write(
        &qt,
        b"[workspace]\ninspectorVisible=true\ninspectorWidth=wide\n",
    )
    .unwrap();
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(!settings.contains("workspace/inspectorOpen"));
    assert!(!settings.contains("workspace/inspectorWidth"));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(saved["g3"]["families"].get("inspector-panel").is_none());
    assert_eq!(
        saved["g3"]["quarantine"]["inspector-panel"]["reason"],
        "invalid inspector width"
    );
    fs::write(
        &qt,
        b"[workspace]\ninspectorVisible=true\ninspectorWidth=520\n",
    )
    .unwrap();
    let settings = Settings::open(path, Some(QtStore::Ini(qt)));
    assert_eq!(settings.get("workspace/inspectorOpen"), json!(true));
    assert_eq!(settings.number("workspace/inspectorWidth"), 520.0);
    assert!(settings.flush_within(FLUSH));
}

/// zerus-5wmh.66.5.3: the Migration report's Qt preferences section lists
/// key names only: what imported, what kept a Next value, what waits to
/// retry and why, and the Qt keys Next has no use for. Never a value.
#[test]
fn the_preference_report_names_keys_and_never_values() {
    let scratch = Scratch::new("preference-report");
    let qt = scratch.0.join("qt.ini");
    fs::write(
        &qt,
        b"[workspace]\ninspectorVisible=true\ninspectorWidth=613\ninspectorTab=2\nsplitter=@ByteArray(bad)\n\
          expandedSessions=secret-row\nworktreeCleanupDeleteBranches=true\n",
    )
    .unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({"workspace/inspectorOpen": false}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(settings.flush_within(FLUSH));
    let report = settings.preference_report();
    let family = |name: &str| {
        report["families"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["family"] == name)
            .cloned()
            .unwrap_or(Value::Null)
    };
    let inspector = family("inspector-panel");
    assert_eq!(inspector["state"], "imported");
    assert_eq!(
        inspector["imported"],
        json!(["workspace/inspectorTab", "workspace/inspectorWidth"])
    );
    assert_eq!(inspector["kept"], json!(["workspace/inspectorVisible"]));
    let splitter = family("session-splitter");
    assert_eq!(splitter["state"], "waiting");
    assert_eq!(splitter["reason"], "invalid Qt splitter");
    assert_eq!(splitter["keys"], json!(["workspace/splitter"]));
    assert_eq!(
        report["unused"],
        json!([
            "workspace/expandedSessions",
            "workspace/worktreeCleanupDeleteBranches"
        ])
    );
    let text = report.to_string();
    for value in ["613", "secret-row", "bad"] {
        assert!(!text.contains(value), "{value} in {text}");
    }
    drop(settings);
    // The report-only family is never a family row (no "no-consumer" label).
    assert!(report["families"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["family"] != "no-consumer"));
    // A family that waits records no outcome; it is reported from its quarantine.
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(saved["g3_outcome"].get("session-splitter").is_none());
    assert!(saved["g3_outcome"].get("no-consumer").is_none());
    // The same after a restart, from the saved journal.
    let restarted = Settings::open(path, Some(QtStore::Ini(qt)));
    assert_eq!(
        restarted.preference_report()["families"],
        report["families"]
    );
    assert!(restarted.flush_within(FLUSH));
}

/// #1243 review Q4: an install whose earlier families were all handled
/// before the report existed still gains the "Not imported" keys, once,
/// through the report-only no-consumer family.
#[test]
fn unused_qt_keys_are_reported_after_every_other_family_was_handled() {
    let scratch = Scratch::new("preference-report-upgrade");
    let qt = scratch.0.join("qt.ini");
    fs::write(
        &qt,
        b"[workspace]\nexpandedSessions=row\nsessionsCollapsed=true\n",
    )
    .unwrap();
    let path = scratch.0.join("settings.json");
    legacy_switch(&path, json!({}));
    let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
    assert!(settings.flush_within(FLUSH));
    drop(settings);
    // As saved by a build without the report: every other family handled.
    let mut saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    saved["g3"]["families"]
        .as_object_mut()
        .unwrap()
        .remove("no-consumer");
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let upgraded = Settings::open(path, Some(QtStore::Ini(qt)));
    assert!(upgraded.flush_within(FLUSH));
    assert_eq!(
        upgraded.preference_report()["unused"],
        json!(["workspace/expandedSessions", "workspace/sessionsCollapsed"])
    );
}

/// zerus-5wmh.66.5.3.12: Qt's explicit sessions dock width imports once and
/// wins over the legacy splitter, without replacing a width chosen in Next.
#[test]
fn the_explicit_sessions_dock_width_wins_over_the_legacy_splitter() {
    let scratch = Scratch::new("sessions-width");
    let qt = scratch.0.join("qt.ini");
    let layout = String::from_utf8(qt_layout_ini()).unwrap();
    let with_width = |width: &str| {
        layout
            .replace(
                "[workspace]\n",
                &format!("[workspace]\nsessionsWidth={width}\n"),
            )
            .into_bytes()
    };
    let width_of = |values: Value, families: Value, outcome: Value, bytes: &[u8]| {
        fs::write(&qt, bytes).unwrap();
        let path = scratch.0.join("settings.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "version": 1, "seed": {"source":"qt-ini", "at":1, "keys":["workspace/theme"]},
                "values": values, "g3": {"version":1, "families": families, "quarantine": {}},
                "g3_outcome": outcome,
            }))
            .unwrap(),
        )
        .unwrap();
        let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
        assert!(settings.flush_within(FLUSH));
        let width = settings.number("workspace/sessionsWidth");
        let report = settings.preference_report();
        drop(settings);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::remove_file(&path).unwrap();
        (width, report, saved)
    };
    let splitter = |bytes: &[u8]| {
        json!({"session-splitter": {"version":1, "family":"session-splitter", "schema":1, "source":"qt-ini",
            "source_path": qt.to_string_lossy(), "at":1, "keys":["workspace/splitter"],
            "sha256": zerus_desktop_migration::sha256(bytes), "source_mtime_after_switch": null}})
    };
    let family = |report: &Value, name: &str| {
        report["families"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["family"] == name)
            .cloned()
            .unwrap_or(Value::Null)
    };
    let bytes = with_width("540");
    // A new install: the dock width imports. The splitter neither imported
    // nor kept a choice, since the width came from Qt in the same pass.
    let (width, report, _) = width_of(json!({}), json!({}), json!({}), &bytes);
    assert_eq!(width, 540.0);
    assert_eq!(
        family(&report, "sessions-width")["imported"],
        json!(["workspace/sessionsWidth"])
    );
    let legacy = family(&report, "session-splitter");
    assert_eq!(
        (&legacy["imported"], &legacy["kept"]),
        (&json!([]), &json!([]))
    );
    // A malformed splitter beside an imported dock width is neither kept nor waiting.
    let malformed = String::from_utf8(bytes.clone())
        .unwrap()
        .lines()
        .map(|line| {
            if line.starts_with("splitter=") {
                "splitter=@ByteArray(bad)".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let (width, report, saved) = width_of(json!({}), json!({}), json!({}), malformed.as_bytes());
    assert_eq!(width, 540.0);
    let legacy = family(&report, "session-splitter");
    assert_eq!(
        (&legacy["imported"], &legacy["kept"]),
        (&json!([]), &json!([]))
    );
    assert!(saved["g3"]["quarantine"].get("session-splitter").is_none());
    // The splitter import wrote the width from these same bytes: replaced,
    // and its import no longer holds the width. A record from before
    // outcomes has value equality as its only evidence.
    for outcome in [
        json!({"session-splitter": {"imported":["workspace/splitter"], "kept":[]}}),
        json!({}),
    ] {
        let (width, report, saved) = width_of(
            json!({"workspace/sessionsWidth": 380}),
            splitter(&bytes),
            outcome.clone(),
            &bytes,
        );
        assert_eq!(width, 540.0, "{outcome}");
        assert_eq!(
            family(&report, "sessions-width")["imported"],
            json!(["workspace/sessionsWidth"])
        );
        // No superseded "Imported earlier" splitter row, with or without an
        // earlier outcome (probe PE).
        let legacy = family(&report, "session-splitter");
        assert_eq!(
            (&legacy["imported"], &legacy["kept"]),
            (&json!([]), &json!([])),
            "{outcome}"
        );
        assert_eq!(
            saved["g3_outcome"]["session-splitter"],
            json!({"imported": [], "kept": []})
        );
    }
    // The splitter recorded the person's own width (kept): never replaced,
    // even when it equals the splitter's value (probe PB).
    let (width, _, _) = width_of(
        json!({"workspace/sessionsWidth": 380}),
        splitter(&bytes),
        json!({"session-splitter": {"imported":[], "kept":["workspace/splitter"]}}),
        &bytes,
    );
    assert_eq!(width, 380.0);
    // A width chosen in Next, or Qt bytes changed since that import: kept.
    let (width, report, _) = width_of(
        json!({"workspace/sessionsWidth": 600}),
        splitter(&bytes),
        json!({}),
        &bytes,
    );
    assert_eq!(width, 600.0);
    assert_eq!(
        family(&report, "sessions-width")["kept"],
        json!(["workspace/sessionsWidth"])
    );
    let (width, _, _) = width_of(
        json!({"workspace/sessionsWidth": 380}),
        splitter(b"[workspace]\n"),
        json!({}),
        &bytes,
    );
    assert_eq!(width, 380.0);
    // Qt's 0 is its default: nothing imports and the splitter applies.
    let (width, _, saved) = width_of(json!({}), json!({}), json!({}), &with_width("0"));
    assert_eq!(width, 380.0);
    assert!(saved["g3"]["families"].get("sessions-width").is_some());
    // Outside Qt's 32-bit toInt(): the family waits.
    let (_, _, saved) = width_of(json!({}), json!({}), json!({}), &with_width("2147483648"));
    assert_eq!(
        saved["g3"]["quarantine"]["sessions-width"]["reason"],
        "invalid sessions width"
    );
}

/// WhiteWh fork: Qt's Keep cache warm hours (`workspace/cacheKeepWarm`, its
/// compact JSON as a QByteArray) import once with only the sessions that are
/// on, under the same identities; a choice already made in Next wins.
#[test]
fn keep_cache_warm_hours_import_once_from_qt() {
    use zerus_client_core::cache_keepalive::{self, KEY};
    let (a, b) = ("a".repeat(64), "b".repeat(64));
    let ini = format!(
        "[workspace]\ncacheKeepWarm=\"@ByteArray({{\\\"{a}\\\":3,\\\"{b}\\\":0,\\\"short\\\":2}})\"\n"
    );
    let mut on = serde_json::Map::new();
    on.insert(a, json!(3));
    for (existing, expected) in [(None, on), (Some("{}"), serde_json::Map::new())] {
        let scratch = Scratch::new("cache-keep-warm");
        let qt = scratch.0.join("qt.ini");
        fs::write(&qt, &ini).unwrap();
        let path = scratch.0.join("settings.json");
        legacy_switch(
            &path,
            existing.map_or(json!({}), |value| json!({ KEY: value })),
        );
        let settings = Settings::open(path.clone(), Some(QtStore::Ini(qt.clone())));
        assert!(settings.flush_within(FLUSH));
        assert_eq!(cache_keepalive::parse(&settings.text(KEY)), expected);
        drop(settings);
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            saved["g3"]["families"]["cache-keep-warm"]["keys"],
            json!([KEY])
        );
        let outcome = &saved["g3_outcome"]["cache-keep-warm"];
        let (imported, kept) = if existing.is_none() {
            (json!([KEY]), json!([]))
        } else {
            (json!([]), json!([KEY]))
        };
        assert_eq!((&outcome["imported"], &outcome["kept"]), (&imported, &kept));
    }
}
