//! Private original native owner; no installed factory or provider operation.
use super::*;
use std::{
    fs,
    process::{Command, Stdio},
    time::Duration,
};
const ENTRY: &str = "state::supervisor::owned::appearance::tests::private_producer";

struct Root(std::process::Child);
impl std::ops::Deref for Root {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Root {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if self.0.try_wait().unwrap().is_some() {
                return;
            }
            if Instant::now() >= until {
                // Only the SAME private fixture Child, including on unwind.
                let _ = self.0.kill();
                self.0.wait().unwrap();
                if !std::thread::panicking() {
                    panic!("original fixture root missed its drain bound");
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn probe(case: &str) {
    let temp = tempfile::tempdir().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("HGS_STATE_DIR", temp.path().join("state"))
        .env("HGS_CONFIG_DIR", temp.path().join("config"))
        .env("HGS_TEST_APPEARANCE_PRODUCER", case)
        .args([
            "--exact",
            ENTRY,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ]);
    let (status, out, err) = crate::platform::run_bounded(
        &mut command,
        [64 << 10; 2],
        Instant::now() + Duration::from_secs(15),
        &|| false,
    )
    .unwrap();
    assert!(
        status.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    assert!(String::from_utf8_lossy(&out).contains("1 passed; 0 failed;"));
}
#[test]
fn a_closed_original_producer_cannot_admit_or_be_recreated_from_its_observation() {
    probe("close");
    probe("closed-late");
}
#[test]
fn original_native_source_and_domain_changes_refuse_and_remain_sticky() {
    for case in ["config", "domain", "root", "marker", "layout"] {
        probe(case);
    }
}
#[test]
fn closed_helper_targets_same_writer_and_original_admission_budget_are_required() {
    for case in ["target", "writer", "busy"] {
        probe(case);
    }
}
#[test]
fn pre_spawn_drop_keeps_only_its_exact_pending_row_and_does_not_claim_global_coverage() {
    probe("row");
}

#[test]
#[ignore = "private reexecution, invoked by maintained original producer controls"]
fn private_producer() {
    let case = std::env::var("HGS_TEST_APPEARANCE_PRODUCER").unwrap();
    let root = storage::root();
    drop(storage::lock(None).unwrap());
    let config = crate::config::config_dir(&home().to_string_lossy());
    crate::platform::private_dir(&config).unwrap();
    let source = Arc::new(native::Source::capture(&root, &config).unwrap());
    let domain = Arc::new(terminating::OriginalDomain::fixture(&root));
    let scope = domain.observation();
    // The real fresh-owner constructor requires the executable's sole main
    // thread. Libtest runs this private entry on a worker, so this fixture
    // exercises only the retained producer/row adapter with its SAME live
    // native source and Child. Actual constructor coverage is the CLI suite.
    #[cfg(target_os = "linux")]
    assert_eq!(
        unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) },
        0
    );
    let mut child = Root(
        Command::new("python3")
            .args(["-c", "import sys;sys.stdin.buffer.read()"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let input = child.stdin.take();
    let native = crate::platform::process_identity(child.id()).unwrap();
    let binding = json!({"owner":std::process::id(),"root":child.id(),"root_birth":native.birth,
        "session":scope["session"],"run":scope["run"],"source":source.wire()});
    let record = json!({"version":1,"name":"claude/fixture/original","session_id":scope["session"],
        "run_id":scope["run"],"conversation_id":null,"expected_id":"","agent":"claude",
        "run_identity_version":1,"pid":child.id(),"process_start":"original-root",
        "supervisor":{"pid":std::process::id(),"start":"original-supervisor"},"pane":"%123",
        "terminal_name":"original-terminal","cwd":"/fixture/work","launch_dir":"/fixture/work",
        "hgs":"/fixture/hgs","original_owner":binding});
    storage::atomic(&storage::own_path(&record).unwrap(), &record.to_string()).unwrap();
    let producer = Arc::new(Producer {
        source,
        domain: domain.clone(),
        binding,
        record: record.clone(),
        request: uuid::Uuid::new_v4().to_string(),
        state: root.clone(),
        config: config.clone(),
        open: AtomicBool::new(true),
        failed: AtomicBool::new(false),
    });
    let life = Appearance::new(producer.clone());
    let producer = life.producer();
    producer.check(&record).unwrap();
    let pending_path = root
        .join("terminating-writers")
        .join(scope["session"].as_str().unwrap())
        .join(scope["run"].as_str().unwrap())
        .join("pending.json");
    let rows = || {
        serde_json::from_slice::<Value>(&fs::read(&pending_path).unwrap()).unwrap()["pending"]
            .as_array()
            .unwrap()
            .clone()
    };
    let mut changed = record.clone();
    let mut closed = false;
    if case == "closed-late" {
        let writer = producer
            .writer(Instant::now() + Duration::from_secs(2))
            .unwrap();
        let admitted = producer
            .admit(
                &writer,
                &record,
                &AppearanceCall::Capture("%123".into()),
                Arc::new(()),
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(rows().len(), 1);
        drop(life);
        closed = true;
        assert!(producer.check(&record).is_err());
        drop(admitted);
        assert_eq!(rows().len(), 1);
    } else {
        let mut life = Some(life);
        match case.as_str() {
            "close" => {
                drop(life.take());
                closed = true;
            }
            "config" => {
                fs::rename(&config, config.with_extension("old")).unwrap();
                crate::platform::private_dir(&config).unwrap();
                assert!(producer.check(&record).is_err());
                fs::remove_dir(&config).unwrap();
                fs::rename(config.with_extension("old"), &config).unwrap();
            }
            "domain" => {
                let gate = pending_path.with_file_name("gate");
                let bytes = fs::read(&gate).unwrap();
                fs::rename(&gate, gate.with_extension("old")).unwrap();
                use std::os::unix::fs::PermissionsExt;
                fs::write(&gate, bytes).unwrap();
                fs::set_permissions(gate, fs::Permissions::from_mode(0o600)).unwrap();
            }
            "root" => changed["original_owner"]["root_birth"] = json!("changed"),
            "marker" => changed["terminating"] = Value::Null,
            "layout" => changed["layout_transition"] = Value::Null,
            "target" | "writer" | "busy" | "row" => {}
            _ => panic!("unknown original producer control"),
        }
        if ["target", "writer", "busy", "row"].contains(&case.as_str()) {
            let writer = producer
                .writer(Instant::now() + Duration::from_secs(2))
                .unwrap();
            if case == "busy" {
                let start = Instant::now();
                assert!(producer.writer(start).is_err());
                assert!(producer.writer(start + Duration::from_secs(31)).is_err());
                assert!(producer.writer(start + Duration::from_millis(30)).is_err());
                assert!(start.elapsed() < Duration::from_secs(1));
            } else {
                let foreign = tempfile::tempfile().unwrap();
                let foreign = storage::Guard::exclusive(foreign).unwrap();
                let selected = if case == "writer" { &foreign } else { &writer };
                let call = AppearanceCall::Capture(
                    if case == "target" { "%foreign" } else { "%123" }.into(),
                );
                let admitted = producer.admit(
                    selected,
                    &record,
                    &call,
                    Arc::new(()),
                    Instant::now() + Duration::from_secs(2),
                );
                if case == "row" {
                    assert!(admitted.is_ok());
                    assert_eq!(rows().len(), 1);
                    drop(admitted);
                } else {
                    assert!(admitted.is_err());
                    assert!(rows().is_empty());
                }
            }
        } else {
            assert!(producer.check(&changed).is_err());
            assert!(
                producer.check(&record).is_err(),
                "source refusal must stay sticky"
            );
            assert!(rows().is_empty());
        }
        drop(life);
    }
    assert!(!producer.observation()["open"].as_bool().unwrap());
    if closed {
        assert!(producer.check(&record).is_err());
    }
    assert_eq!(domain.observation()["coverage"], "initialized_not_enrolled");
    assert_eq!(domain.observation()["effect_authority"], false);
    drop(input);
    assert!(child.wait().unwrap().success());
}
