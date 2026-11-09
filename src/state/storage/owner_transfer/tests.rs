use super::*;
use fs2::FileExt;
use std::{
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    time::Duration,
};

mod input_domain_tests;

fn control(case: &str) {
    let temp = tempfile::tempdir().unwrap();
    for name in ["home", "state", "config", "tmp"] {
        fs::create_dir(temp.path().join(name)).unwrap();
        fs::set_permissions(temp.path().join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "state::storage::owner_transfer::tests::fixture",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.path().join("home"))
        .env("TMPDIR", temp.path().join("tmp"))
        .env("HGS_CONFIG_DIR", temp.path().join("config"))
        .env("HGS_STATE_DIR", temp.path().join("state"))
        .env("ZERUS_OWNER_TRANSFER_CASE", case)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let until = Instant::now()
        + Duration::from_secs(if case == "review-record-headroom" {
            60
        } else {
            20
        });
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("bounded original fixture did not finish: {case}");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut out = String::new();
    let mut err = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(
        status.success() && out.contains("test result: ok. 1 passed"),
        "{case}: {out}{err}"
    );
}

#[test]
fn exact_original_record_jobs_settings_and_input_domain_move_together() {
    control("complete");
}
#[test]
fn the_existing_stopped_runtime_with_a_null_ping_counter_is_carried_unchanged() {
    control("stopped-runtime");
}
#[test]
fn every_derived_record_is_bounded_before_the_original_is_rewritten() {
    control("review-record-bound");
}
#[test]
fn a_record_near_the_bound_with_headroom_keeps_every_original_field() {
    control("review-record-headroom");
}
#[test]
fn a_busy_original_input_or_worker_refuses_before_the_pending_marker() {
    for case in ["busy-input", "busy-worker"] {
        control(case);
    }
}
#[test]
fn an_auxiliary_guard_never_substitutes_for_the_original_state_writer() {
    control("wrong-writer");
}
#[test]
fn durable_cuts_never_reissue_unknown_jobs_and_recovery_is_per_sid() {
    for n in 0..=20 {
        control(&format!("cut:{n}"));
    }
}
#[test]
fn conflicting_and_irregular_job_data_never_becomes_an_empty_before_image() {
    for case in [
        "foreign-job",
        "fifo-job",
        "foreign-target",
        "malformed-request",
        "missing-unknown-request",
        "malformed-attempt",
        "malformed-runtime",
    ] {
        control(case);
    }
}
#[test]
fn completed_recovery_never_recreates_a_later_archived_record() {
    control("archived");
}
#[test]
fn recovery_preserves_later_unrelated_keep_warm_choices() {
    control("unrelated-setting");
}
#[test]
fn missing_or_replaced_original_exclusions_are_never_reconstructed() {
    for case in [
        "missing-round",
        "replaced-round",
        "missing-setting-lock",
        "replaced-writer",
        "missing-input-namespace",
        "missing-input-file",
    ] {
        control(case);
    }
}

struct Fixture {
    original: Value,
    job: Vec<u8>,
    runtime: Vec<u8>,
    settings: Value,
    other: String,
    old: String,
    sid: String,
}
impl Fixture {
    fn create() -> Self {
        let sid = uuid::Uuid::new_v4().to_string();
        let run = uuid::Uuid::new_v4().to_string();
        let original = json!({"version":1,"agent":"claude","name":"claude/example/shared","session_id":sid,
            "run_id":run,"conversation_id":"conversation-original","created":123,
            "launch_dir":"/srv/example/work","cwd":"/srv/example/work","terminal_name":"claude/example/shared",
            "parent":{"session_id":uuid::Uuid::new_v4().to_string(),"machine":uuid::Uuid::new_v4().to_string()},
            "recovery_history":[{"request_id":"original-history","state":"uncertain"}]});
        let old = owner_key(&original);
        let other = uuid::Uuid::new_v4().to_string();
        let job=encoded(&json!({"id":uuid::Uuid::new_v4().to_string(),"name":original["name"],
            "identity":[run,original["conversation_id"],null,null,null],"state":"uncertain",
            "request_id":uuid::Uuid::new_v4().to_string(),"attempt":4,"due_at":4567.0,"acknowledged":false,
            "history":[{"state":"dispatching","attempt":4,"at":123.0}]})).unwrap();
        let runtime = encoded(
            &json!({"state":"uncertain","request_id":uuid::Uuid::new_v4().to_string(),
            "revision":42,"since":345.0,"last_ping_at":123.0,"pings":3}),
        )
        .unwrap();
        let settings = json!({"version":1,"sessions":{
            old.clone():{"name":original["name"],"enabled":true,"until":1234.0,"revision":42,"set_at":12.0},
            other.clone():{"name":"claude/example/other","enabled":false,"until":null,"revision":7,"set_at":5.0}
        }});
        atomic(
            &legacy_record_path(string(&original, "name")),
            &original.to_string(),
        )
        .unwrap();
        atomic(
            &root().join("recovery/jobs").join(format!("{old}.json")),
            std::str::from_utf8(&job).unwrap(),
        )
        .unwrap();
        atomic(
            &root()
                .join("recovery/keep-warm")
                .join(format!("{old}.json")),
            std::str::from_utf8(&runtime).unwrap(),
        )
        .unwrap();
        atomic(&root().join("keep-warm.json"), &settings.to_string()).unwrap();
        Self {
            original,
            job,
            runtime,
            settings,
            other,
            old,
            sid,
        }
    }
    fn check(&self, after: &Value) {
        let mut stripped = after.clone();
        stripped.as_object_mut().unwrap().remove("layout");
        stripped.as_object_mut().unwrap().remove(MARKER);
        assert!(
            stripped == self.original,
            "original history/parent/native identity must not change"
        );
        assert!(is_id_layout_record(after, &self.sid));
        assert!(!legacy_record_path(string(&self.original, "name")).exists());
        assert_eq!(
            fs::read(
                root()
                    .join("recovery/jobs")
                    .join(format!("{}.json", self.sid))
            )
            .unwrap(),
            self.job
        );
        assert_eq!(
            fs::read(
                root()
                    .join("recovery/keep-warm")
                    .join(format!("{}.json", self.sid))
            )
            .unwrap(),
            self.runtime
        );
        let settings = value(&fs::read(root().join("keep-warm.json")).unwrap()).unwrap();
        assert_eq!(
            settings["sessions"][&self.sid],
            self.settings["sessions"][&self.old]
        );
        assert!(settings["sessions"].get(&self.old).is_none());
        let inputs = root().join("native-input");
        let a = fs::metadata(inputs.join(format!("{}.lock", self.old))).unwrap();
        let b = fs::metadata(inputs.join(format!("{}.lock", self.sid))).unwrap();
        assert_eq!((a.dev(), a.ino(), a.nlink()), (b.dev(), b.ino(), 2));
        assert!(!blocked(after));
    }
}

#[test]
#[ignore = "run by the bounded original-process controls"]
fn fixture() {
    let Ok(case) = std::env::var("ZERUS_OWNER_TRANSFER_CASE") else {
        return;
    };
    let mut data = Fixture::create();
    let writer = lock(None).unwrap();
    let until = Instant::now() + Duration::from_secs(15);
    let legacy = legacy_record_path(string(&data.original, "name"));
    if let Some(case) = case.strip_prefix("input:") {
        input_domain_tests::fixture(&data, &writer, until, case);
        return;
    }
    if ["review-record-bound", "review-record-headroom"].contains(&case.as_str()) {
        // Original independent zerus-xrm5q counter, retained at the exact
        // existing cap; no fixture cap increase or weaker assertion.
        data.original["future_record_data"] = json!("");
        let size = ID_RECORD_BYTES as usize
            - if case == "review-record-headroom" {
                1024
            } else {
                0
            };
        let padding = size - encoded(&data.original).unwrap().len();
        data.original["future_record_data"] = json!("x".repeat(padding));
        assert_eq!(encoded(&data.original).unwrap().len(), size);
        valid_record(&data.original).unwrap();
        atomic(&legacy, &data.original.to_string()).unwrap();
        let until = Instant::now() + Duration::from_secs(120);
        let result = begin(&writer, &data.original, until);
        if case == "review-record-headroom" {
            let after =
                result.expect("sufficient headroom must not reject an otherwise valid original");
            data.check(&after);
            assert!(
                fs::metadata(id_record_path(&data.sid).unwrap())
                    .unwrap()
                    .len()
                    <= ID_RECORD_BYTES
            );
            return;
        }
        let current = fs::read(&legacy).unwrap();
        assert!(result.is_err());
        assert_eq!(
            current.len(),
            ID_RECORD_BYTES as usize,
            "refusal must keep the accepted original readable at its cap"
        );
        assert!(
            value(&current).unwrap() == data.original,
            "refusal must not rewrite the original"
        );
        assert!(recover(&writer, &data.sid, until).is_err());
        assert_eq!(fs::read(&legacy).unwrap(), current);
        return;
    }
    if case == "stopped-runtime" {
        // This is the actual off_runtime shape before any ping, not repaired
        // or defaulted into a new automatic delivery.
        data.runtime = encoded(
            &json!({"state":"stopped","reason":"setting disabled","since":123.0,"pings":null}),
        )
        .unwrap();
        data.settings["sessions"][&data.old]["enabled"] = json!(false);
        atomic(
            &root()
                .join("recovery/keep-warm")
                .join(format!("{}.json", data.old)),
            std::str::from_utf8(&data.runtime).unwrap(),
        )
        .unwrap();
        atomic(&root().join("keep-warm.json"), &data.settings.to_string()).unwrap();
    }
    if case == "wrong-writer" {
        let auxiliary = lock(Some(&root().join("auxiliary.lock"))).unwrap();
        let result = begin(&auxiliary, &data.original, until);
        drop(auxiliary);
        assert!(result.is_err());
        assert!(!root().join("native-input").exists());
        assert!(!root().join(DIRECTORY).exists());
        assert_eq!(value(&fs::read(&legacy).unwrap()).unwrap(), data.original);
        return;
    }
    if case == "busy-input" {
        let state = Directory::open(&crate::platform::canonical(root()).unwrap()).unwrap();
        let inputs = state.ensure_child("native-input").unwrap();
        let file = inputs
            .open_private_lock(&format!("{}.lock", data.old))
            .unwrap();
        file.try_lock_exclusive().unwrap();
        let result = begin(&writer, &data.original, until);
        fs2::FileExt::unlock(&file).unwrap();
        drop(file);
        assert!(result.is_err());
        assert_eq!(value(&fs::read(&legacy).unwrap()).unwrap(), data.original);
        assert!(!root().join(DIRECTORY).exists());
        data.check(&begin(&writer, &data.original, until).unwrap());
        return;
    }
    if case == "busy-worker" {
        let round = job_round::try_acquire(job_round::Kind::Recovery)
            .unwrap()
            .unwrap();
        let result = begin(&writer, &data.original, until);
        drop(round);
        assert!(result.is_err());
        assert!(!root().join(DIRECTORY).exists());
        assert_eq!(value(&fs::read(&legacy).unwrap()).unwrap(), data.original);
        data.check(&begin(&writer, &data.original, until).unwrap());
        return;
    }
    if [
        "foreign-job",
        "fifo-job",
        "foreign-target",
        "malformed-request",
        "missing-unknown-request",
        "malformed-attempt",
        "malformed-runtime",
    ]
    .contains(&case.as_str())
    {
        let old = root()
            .join("recovery/jobs")
            .join(format!("{}.json", data.old));
        if case == "foreign-job" {
            atomic(&old, "{\"state\":\"uncertain\"}").unwrap();
        }
        if [
            "malformed-request",
            "missing-unknown-request",
            "malformed-attempt",
        ]
        .contains(&case.as_str())
        {
            let mut job = value(&data.job).unwrap();
            match case.as_str() {
                "malformed-request" => job["request_id"] = json!({"unknown":true}),
                "missing-unknown-request" => job["request_id"] = Value::Null,
                "malformed-attempt" => job["attempt"] = json!("4"),
                _ => unreachable!(),
            }
            atomic(&old, &job.to_string()).unwrap();
        }
        if case == "malformed-runtime" {
            let mut runtime = value(&data.runtime).unwrap();
            runtime["request_id"] = json!(["unknown"]);
            atomic(
                &root()
                    .join("recovery/keep-warm")
                    .join(format!("{}.json", data.old)),
                &runtime.to_string(),
            )
            .unwrap();
        }
        if case == "fifo-job" {
            fs::remove_file(&old).unwrap();
            let name = std::ffi::CString::new(old.as_os_str().as_encoded_bytes()).unwrap();
            // SAFETY: the fixture owns this absent NUL-free pathname.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        }
        if case == "foreign-target" {
            atomic(
                &root()
                    .join("recovery/jobs")
                    .join(format!("{}.json", data.sid)),
                "{\"foreign\":true}",
            )
            .unwrap();
        }
        assert!(begin(&writer, &data.original, until).is_err());
        assert_eq!(value(&fs::read(&legacy).unwrap()).unwrap(), data.original);
        assert!(!id_record_path(&data.sid).unwrap().exists());
        return;
    }
    if let Some(n) = case.strip_prefix("cut:") {
        cut::arm(n.parse().unwrap());
        let result = begin(&writer, &data.original, until);
        cut::disarm();
        if let Ok(after) = result {
            data.check(&after);
            return;
        }
        let current = value(
            &fs::read(&legacy)
                .or_else(|_| fs::read(id_record_path(&data.sid).unwrap()))
                .unwrap(),
        )
        .unwrap();
        if current.get(MARKER).is_some() {
            assert!(blocked(&current));
        }
        let unrelated =
            json!({"name":data.original["name"],"session_id":uuid::Uuid::new_v4().to_string()});
        assert!(
            !blocked(&unrelated),
            "one pending transition cannot fence other sessions"
        );
        if let Ok(after) = recover(&writer, &data.sid, until) {
            data.check(&after);
        }
        for namespace in ["recovery/jobs", "recovery/keep-warm"] {
            for (key, expected) in [
                (
                    &data.old,
                    if namespace.ends_with("jobs") {
                        &data.job
                    } else {
                        &data.runtime
                    },
                ),
                (
                    &data.sid,
                    if namespace.ends_with("jobs") {
                        &data.job
                    } else {
                        &data.runtime
                    },
                ),
            ] {
                if let Ok(bytes) = fs::read(root().join(namespace).join(format!("{key}.json"))) {
                    assert_eq!(&bytes, expected);
                }
            }
        }
        return;
    }
    if [
        "unrelated-setting",
        "missing-round",
        "replaced-round",
        "missing-setting-lock",
        "replaced-writer",
        "missing-input-namespace",
        "missing-input-file",
    ]
    .contains(&case.as_str())
    {
        // All original job/runtime files have moved, but the setting's own
        // durable write refuses. The original entry is still at its old key.
        cut::arm(9);
        let result = begin(&writer, &data.original, until);
        cut::disarm();
        assert!(result.is_err());
        let mut settings = value(&fs::read(root().join("keep-warm.json")).unwrap()).unwrap();
        assert_eq!(
            settings["sessions"][&data.old],
            data.settings["sessions"][&data.old]
        );
        assert!(settings["sessions"].get(&data.sid).is_none());
        assert!(root()
            .join("recovery/jobs")
            .join(format!("{}.json", data.sid))
            .exists());
        if case != "unrelated-setting" {
            let path = match case.as_str() {
                "missing-round" | "replaced-round" => root().join("recovery/round.lock"),
                "missing-setting-lock" => root().join("keep-warm.lock"),
                "replaced-writer" => root().join(".lock"),
                "missing-input-namespace" => root().join("native-input"),
                "missing-input-file" => root()
                    .join("native-input")
                    .join(format!("{}.lock", data.sid)),
                _ => unreachable!(),
            };
            let retained = path.with_extension("original-retained");
            fs::rename(&path, &retained).unwrap();
            if case.starts_with("replaced") {
                atomic(&path, "").unwrap();
            }
            assert!(recover(&writer, &data.sid, until).is_err());
            if case.starts_with("missing") {
                assert!(!path.exists(), "recovery rebuilt the missing exclusion");
            } else {
                assert_ne!(
                    crate::platform::file_identity(&path).unwrap().inode,
                    crate::platform::file_identity(&retained).unwrap().inode
                );
            }
            let current = value(&fs::read(&legacy).unwrap()).unwrap();
            assert!(blocked(&current));
            assert_eq!(
                fs::read(
                    root()
                        .join("recovery/jobs")
                        .join(format!("{}.json", data.sid))
                )
                .unwrap(),
                data.job
            );
            return;
        }
        settings["sessions"][&data.other]["revision"] = json!(8);
        atomic(&root().join("keep-warm.json"), &settings.to_string()).unwrap();
        let after = recover(&writer, &data.sid, until).unwrap();
        data.check(&after);
        let actual = value(&fs::read(root().join("keep-warm.json")).unwrap()).unwrap();
        assert_eq!(
            actual["sessions"][&data.other],
            settings["sessions"][&data.other]
        );
        return;
    }

    let after = begin(&writer, &data.original, until).unwrap();
    data.check(&after);
    if case == "archived" {
        fs::remove_file(id_record_path(&data.sid).unwrap()).unwrap();
        assert!(recover(&writer, &data.sid, until).is_err());
        assert!(!id_record_path(&data.sid).unwrap().exists());
    }
}
