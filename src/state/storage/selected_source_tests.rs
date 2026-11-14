mod cases;
use super::*;
use std::{
    io::Read,
    process::Command,
    time::{Duration, Instant},
};

fn fixture(case: &str) {
    let temp = tempfile::tempdir().unwrap();
    for name in ["home", "tmp", "state", "config"] {
        fs::create_dir(temp.path().join(name)).unwrap();
    }
    let output = tempfile::tempfile().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "state::storage::selected_source_tests::fixture_entry",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.path().join("home"))
        .env("USERPROFILE", temp.path().join("home"))
        .env("TMPDIR", temp.path().join("tmp"))
        .env("TEMP", temp.path().join("tmp"))
        .env("TMP", temp.path().join("tmp"))
        .env("HGS_STATE_DIR", temp.path().join("state"))
        .env("HGS_CONFIG_DIR", temp.path().join("config"))
        .env("ZERUS_SELECTED_SOURCE_CASE", case)
        .stdout(output.try_clone().unwrap())
        .stderr(output.try_clone().unwrap());
    for key in ["SYSTEMROOT", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    use std::io::{Seek, SeekFrom};
    let mut output = output;
    output.seek(SeekFrom::Start(0)).unwrap();
    let mut text = String::new();
    output.take(64 << 10).read_to_string(&mut text).unwrap();
    assert!(
        status.is_some_and(|s| s.success()),
        "{case}: {status:?}\n{text}"
    );
}

#[test]
fn a_selected_snapshot_must_not_enrich_the_stored_record() {
    fixture("raw");
}

fn raw() -> Value {
    json!({"version":1,"name":"claude/example/one","agent":"claude",
        "session_id":"11111111-1111-4111-8111-111111111111",
        "run_id":"run","conversation_id":"conversation",
        "base":["claude"],"launch_dir":"fixture",
        "subagents":{"child":{"external":true,"state":"working"}},
        "layout_transition":null,"future":{"opaque":[false,null,"retain"]}})
}

#[test]
#[ignore = "private environment fixture, invoked by its parent test"]
fn fixture_entry() {
    let Ok(case) = std::env::var("ZERUS_SELECTED_SOURCE_CASE") else {
        return;
    };
    match case.as_str() {
        "raw" => raw_case(),
        "changes" => cases::changes(),
        "slot" => cases::slot(),
        "bounds" => cases::bounds(),
        "observations" => cases::observations(),
        "retained" => cases::retained(),
        "hold" => std::thread::sleep(Duration::from_secs(60)),
        _ => panic!("unknown private case"),
    }
}

fn raw_case() {
    let mut raw = raw();
    raw["agent_home"] = json!(home().join(".claude"));
    let name = string(&raw, "name");
    let path = label_record_path(name);
    fs::write(&path, raw.to_string()).unwrap();
    // Keep the old reader as a negative control: its derived view is not the
    // complete original metadata the new snapshot must retain.
    assert_ne!(read(name).unwrap(), raw);
    let captured = selected_source::Snapshot::capture(&path, name).unwrap();
    assert_eq!(
        captured.record(),
        &raw,
        "a source frame must not contain derived observations"
    );
    captured.recheck().unwrap();
}
