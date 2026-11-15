use super::*;
use std::{fs, process::Command, time::Duration};
const SID: &str = "11111111-1111-4111-8111-111111111111";
const RUN: &str = "22222222-2222-4222-8222-222222222222";
const NAME: &str = "codex/domain/original";

#[test]
fn the_live_original_source_refuses_changed_bindings_routes_and_native_writers() {
    for case in [
        "fresh",
        "repeat",
        "lock",
        "config",
        "config-restore",
        "root",
        "record",
        "late-transition",
        "deadline",
        "alias",
        "alias-retarget",
        "id-alias",
        "invalid-run",
        "busy",
        "transitions",
        "duplicate",
        "id-parent-link",
    ] {
        let d = tempfile::tempdir().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "state::supervisor::owned::creation::tests::private_probe",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", d.path())
            .env("HGS_STATE_DIR", d.path().join("state"))
            .env("HGS_CONFIG_DIR", d.path().join("config"))
            .env("HGS_SESSION_ID", SID)
            .env("HGS_TEST_ORIGINAL_DOMAIN", case);
        let (status, stdout, stderr) = crate::platform::run_bounded(
            &mut command,
            [64 << 10; 2],
            Instant::now() + Duration::from_secs(15),
            &|| false,
        )
        .unwrap();
        assert!(
            status.success(),
            "{case}: {} {}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        assert!(String::from_utf8_lossy(&stdout).contains("test result: ok. 1 passed; 0 failed;"));
    }
}

#[test]
#[ignore = "private environment reexecution, invoked by maintained wrapper"]
fn private_probe() {
    let case = std::env::var("HGS_TEST_ORIGINAL_DOMAIN").unwrap();
    let configured = storage::root();
    crate::platform::private_dir(&configured).unwrap();
    let config = crate::config::config_dir(&home().to_string_lossy());
    crate::platform::private_dir(&config).unwrap();
    if case.contains("alias") {
        fs::rename(&configured, configured.with_extension("original")).unwrap();
        std::os::unix::fs::symlink(configured.with_extension("original"), &configured).unwrap();
    }
    let mut record = json!({"version":1,"name":NAME,"agent":"codex","session_id":SID,"run_id":RUN});
    let until = Instant::now() + Duration::from_secs(5);
    let writer = storage::lock(None).unwrap();
    if matches!(case.as_str(), "id-alias" | "id-parent-link") {
        record["layout"] = json!("id");
        crate::platform::private_dir(&configured.join("sessions")).unwrap();
        storage::atomic(&storage::id_record_path(SID).unwrap(), &record.to_string()).unwrap();
    }
    storage::write(&mut record).unwrap();
    drop(writer);
    if case == "id-parent-link" {
        fs::rename(
            configured.join("sessions"),
            configured.join("retained-sessions"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            configured.join("retained-sessions"),
            configured.join("sessions"),
        )
        .unwrap();
        assert!(begin(NAME, RUN, until).is_err());
        assert!(!configured.join("terminating-writers").exists());
        return;
    }
    if case == "busy" {
        let _held = storage::lock(None).unwrap();
        let began = Instant::now();
        assert!(begin(NAME, RUN, began + Duration::from_millis(100)).is_err());
        assert!(began.elapsed() < Duration::from_secs(2));
        assert!(!configured.join("terminating-writers").exists());
        return;
    }
    if case == "transitions" {
        for field in ["terminating", "pausing", "layout_transition"] {
            for value in [
                Value::Null,
                json!(false),
                json!(0),
                json!("opaque"),
                json!([]),
                json!({}),
            ] {
                let mut changed = record.clone();
                changed[field] = value;
                storage::write(&mut changed).unwrap();
                assert!(
                    begin(NAME, RUN, until).is_err(),
                    "{field}: {}",
                    changed[field]
                );
            }
        }
        assert!(!configured.join("terminating-writers").exists());
        return;
    }
    if case == "duplicate" {
        let text = record.to_string();
        storage::atomic(
            &storage::own_path(&record).unwrap(),
            &format!("{{\"run_id\":\"{RUN}\",{}", &text[1..]),
        )
        .unwrap();
        assert!(begin(NAME, RUN, until).is_err());
        assert!(!configured.join("terminating-writers").exists());
        return;
    }
    let (writer, source, mut record) = begin(NAME, RUN, until).unwrap();
    record["original_owner"] = json!({"owner":std::process::id(),"owner_birth":source.birth(),
        "source":source.wire(),"session":SID,"run":RUN,"nonce":uuid::Uuid::new_v4().to_string()});
    if case == "invalid-run" {
        record["run_id"] = json!("../invalid");
        record["original_owner"]["run"] = record["run_id"].clone();
    }
    storage::write(&mut record).unwrap();
    let original = CreationSource::capture(&source, &writer, &record, until).unwrap();
    let root = configured.join("terminating-writers").join(SID).join(RUN);
    match case.as_str() {
        "lock" => {
            fs::rename(configured.join(".lock"), configured.join(".lock.old")).unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::write(configured.join(".lock"), b"").unwrap();
            fs::set_permissions(configured.join(".lock"), fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        "config" | "config-restore" => {
            fs::rename(&config, config.with_extension("old")).unwrap();
            crate::platform::private_dir(&config).unwrap();
        }
        "root" => {
            fs::rename(&configured, configured.with_extension("old")).unwrap();
            crate::platform::private_dir(&configured).unwrap();
        }
        "record" => {
            record["conversation_id"] = json!("late conversation");
            storage::write(&mut record).unwrap();
        }
        "late-transition" => {
            record["layout_transition"] = Value::Null;
            storage::write(&mut record).unwrap();
        }
        "deadline" => {
            std::thread::sleep(until.saturating_duration_since(Instant::now()));
        }
        "alias-retarget" => {
            fs::remove_file(&configured).unwrap();
            crate::platform::private_dir(&configured.with_extension("replacement")).unwrap();
            std::os::unix::fs::symlink(configured.with_extension("replacement"), &configured)
                .unwrap();
        }
        _ => {}
    }
    let result = terminating::OriginalDomain::create(&original);
    if matches!(case.as_str(), "fresh" | "repeat" | "alias" | "id-alias") {
        let domain = result.unwrap();
        domain.current().unwrap();
        assert_eq!(domain.observation()["coverage"], "initialized_not_enrolled");
        assert_eq!(domain.observation()["effect_authority"], false);
        let pending = fs::read(root.join("pending.json")).unwrap();
        assert!(terminating::OriginalDomain::create(&original).is_err());
        drop(domain);
        assert_eq!(fs::read(root.join("pending.json")).unwrap(), pending);
        assert_eq!(record["original_owner"]["run"], RUN);
    } else {
        assert!(result.is_err(), "{case}");
        assert!(!root.exists(), "changed source created a domain: {case}");
        if case == "config-restore" {
            fs::remove_dir(&config).unwrap();
            fs::rename(config.with_extension("old"), &config).unwrap();
            original.check().unwrap();
            assert!(
                terminating::OriginalDomain::create(&original).is_err(),
                "original attempt replayed after restoration"
            );
            assert!(!root.exists());
        }
    }
}
