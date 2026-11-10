//! These controls exercise real retained files and refusal entry points. Native
//! model confirmation remains the separate actual CLI / official-agent layer.
use super::*;
use fs2::FileExt as _;
use std::{
    io::{Read, Seek},
    os::unix::net::UnixStream,
    process::{Command, Stdio},
};
const FIXTURE: &str = "state::effort::picker_scope::driver::settings::tests::fixture";

fn run(case: &str) {
    for kind in ["fresh", "moved"] {
        let home = tempfile::tempdir().unwrap();
        let mut output = tempfile::tempfile().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", home.path())
            .env("TMPDIR", home.path())
            .env("HGS_CONFIG_DIR", home.path().join("config"))
            .env("HGS_STATE_DIR", home.path().join("state"))
            .env("ZERUS_SETTINGS_CONTROL", case)
            .env("ZERUS_SETTINGS_KIND", kind)
            .args(["--exact", FIXTURE, "--ignored", "--test-threads=1"])
            .stdin(Stdio::null())
            .stdout(output.try_clone().unwrap())
            .stderr(output.try_clone().unwrap())
            .spawn()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= end {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Settings refusal watchdog: {kind}/{case}");
            }
            std::thread::sleep(Duration::from_millis(3));
        };
        output.rewind().unwrap();
        let mut text = String::new();
        output.read_to_string(&mut text).unwrap();
        assert!(status.success(), "{kind}/{case}: {text}");
    }
}
#[test]
fn keeper_legacy_io_and_entry_refusals_are_sticky_and_bounded() {
    for case in [
        "guard-lock",
        "guard-lock-explicit",
        "guard-write",
        "guard-atomic",
        "guard-driving",
        "guard-pending",
        "guard-apply",
        "guard-mark",
        "guard-dispatch",
        "guard-dispatch-settings",
    ] {
        run(case);
    }
}
#[test]
fn settings_schedule_commits_only_its_own_receipt_and_pending_fields() {
    run("schedule");
}
#[test]
fn schedule_fence_refuses_native_markers_applied_foreign_receipts_and_keys() {
    for case in [
        "bad-applied",
        "bad-uuid",
        "bad-marker",
        "bad-model",
        "bad-effort",
        "bad-foreign-fields",
        "bad-target",
        "bad-key",
        "bad-record-fence",
        "bad-recovery",
        "bad-liveness",
    ] {
        run(case);
    }
}
#[test]
fn settings_and_effort_receipt_namespaces_remain_distinct() {
    run("receipt-directory");
}
#[test]
fn settings_keys_allow_list_is_unchanged_and_never_contains_escape() {
    let source = include_str!("../read.rs");
    let list = source
        .split("Call::Keys => {")
        .nth(1)
        .unwrap()
        .split("Call::Unsupported")
        .next()
        .unwrap();
    assert_eq!(list.trim(), r#"native
                && matches!(args,[verb,t,selected,key]
            if verb=="send-keys" && t=="-t" && pane(selected)
            && matches!(key.as_str(),"M-."|"M-,"|"Enter"|"Right"|"Left"|"M-s"|"Down"|"Up"|"Tab"|"s"))
                || native
                    && matches!(args,[verb,t,selected,literal,text]
                if verb=="send-keys" && t=="-t" && pane(selected) && literal=="-l"
                && matches!(text.as_str(),"/effort"|"/model"))
        }"#.trim());
}
#[test]
fn schedule_commit_rechecks_actual_child_liveness_after_staging() {
    run("liveness-change");
}

struct OwnChild(std::process::Child);
impl Drop for OwnChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn pending_original_receipt_is_required_before_any_key() {
    run("pending-replaced");
}

fn contended(file: &File) -> bool {
    match file.try_lock_exclusive() {
        Ok(()) => {
            fs2::FileExt::unlock(file).unwrap();
            false
        }
        Err(error) => {
            assert!(crate::platform::lock_contended(&error));
            true
        }
    }
}
#[test]
#[ignore = "bounded private Settings original-object fixture; invoked by parent controls"]
fn fixture() {
    let case = std::env::var("ZERUS_SETTINGS_CONTROL").unwrap();
    let kind = std::env::var("ZERUS_SETTINGS_KIND").unwrap();
    let (writer, mut record) = super::super::super::tests::setup(&kind);
    record["base"] = json!(["claude"]);
    let mut native = if case == "liveness-change" {
        let child = Command::new("/bin/sleep")
            .arg("8")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let child = OwnChild(child);
        record["pid"] = json!(child.0.id());
        record["process_start"] = json!(crate::state::storage::process_start(child.0.id()));
        let bin = PathBuf::from(crate::platform::home().unwrap()).join("fixture-bin");
        fs::create_dir(&bin).unwrap();
        let path = bin.join("tmux");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\t$7\\t%%7\\t0\\t%s\\n' '{}' '{}'\n",
                string(&record, "terminal_name"),
                string(&record, "run_id")
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        Some(child)
    } else {
        None
    };
    crate::state::storage::write(&mut record).unwrap();
    let raw=serde_json::to_vec(&json!({"request_id":uuid::Uuid::new_v4().to_string(),"expected_run_id":record["run_id"],
        "expected_conversation_id":record["conversation_id"],"model":"fixture-model","effort":"high"})).unwrap();
    let frame = host::frame::Frame::new(
        host::frame::Operation::Settings,
        raw,
        &record,
        host::frame::Deadline::after(Duration::from_secs(8)).unwrap(),
    )
    .unwrap();
    let (peer, stream) = UnixStream::pair().unwrap();
    let control = host::Control::fixture(frame, stream).unwrap();
    let (owner, done) = owner::Owner::new(writer, control).unwrap();
    let entered = context::Entered::install(owner.clone()).unwrap();
    owner.open_scope(&record).unwrap();
    let root_file = File::options()
        .read(true)
        .write(true)
        .open(root().join(".lock"))
        .unwrap();
    let sid_file = super::super::super::tests::input(&record);
    assert!(contended(&root_file) && contended(&sid_file));
    let frame = owner.control.frame();
    let receipt = json!({"request_id":frame.request_id,"name":record["name"],"run_id":record["run_id"],
        "conversation_id":record["conversation_id"],"digest":format!("{:x}",Sha256::digest(&frame.raw)),
        "status":"scheduled","model":"fixture-model","effort":"high","scope":"session","at":now(),"apply_when":"resume"});
    if case.starts_with("guard-") {
        let native = request_guard(frame.operation, &frame.raw).unwrap();
        let path = root().join("settings_receipts").join("ordinary.json");
        let mut value = record.clone();
        let error = match case.as_str() {
            "guard-lock" => crate::state::storage::lock(None).err().unwrap(),
            "guard-lock-explicit" => crate::state::storage::lock(Some(
                &root()
                    .join("native-input")
                    .join(format!("{}.lock", record["session_id"].as_str().unwrap())),
            ))
            .err()
            .unwrap(),
            "guard-write" => {
                let error = crate::state::storage::write(&mut value).unwrap_err();
                assert_eq!(value, record);
                error
            }
            "guard-atomic" => {
                let error = crate::state::storage::atomic(&path, "{}").unwrap_err();
                assert!(!path.exists());
                error
            }
            "guard-driving" => startup_choice::driving(&record).err().unwrap(),
            "guard-pending" => apply_pending(string(&record, "name"), record.clone()).unwrap_err(),
            "guard-apply" => {
                let model = ModelOption {
                    id: "fixture-model".into(),
                    label: "Fixture".into(),
                    efforts: vec!["high".into()],
                    default_effort: "high".into(),
                };
                apply_settings(
                    string(&record, "name"),
                    &native,
                    &model,
                    &path,
                    &mut json!({}),
                )
                .unwrap_err()
            }
            "guard-mark" => {
                mark_unconfirmed(&mut value);
                assert_eq!(value, record);
                owner.actor().unwrap_err()
            }
            "guard-dispatch" => dispatch(&[]).unwrap_err(),
            "guard-dispatch-settings" => dispatch_settings(&[]).unwrap_err(),
            _ => unreachable!(),
        };
        let expected = match case.as_str() {
            "guard-driving" => "cannot reacquire",
            "guard-pending" => "Legacy pending",
            "guard-apply" => "Legacy Settings",
            "guard-mark" => "Legacy failure",
            "guard-dispatch" | "guard-dispatch-settings" => "another public controller",
            _ => "Ordinary storage",
        };
        assert!(error.contains(expected), "{case}: {error}");
        assert!(owner.actor().is_err());
    } else if case == "schedule" || case == "receipt-directory" {
        let bytes = scheduled(&owner, receipt, record.clone(), Liveness::Stopped, true).unwrap();
        let applied: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(applied["status"], "scheduled");
        assert!(
            root()
                .join("settings_receipts")
                .join(format!("{}.json", frame.request_id))
                .exists()
        );
        assert!(
            !root()
                .join("effort_receipts")
                .join(format!("{}.json", frame.request_id))
                .exists()
        );
        let next = owner.snapshot().unwrap().0;
        assert_eq!(next["pending_settings"]["request_id"], frame.request_id);
        assert_eq!(next["model"], record["model"]);
        assert!(next.get("settings_unconfirmed_at").is_none());
        assert!(owner.reservation.lock().unwrap().is_none());
    } else if case == "pending-replaced" {
        let id = uuid::Uuid::new_v4().to_string();
        let mut marked = owner.snapshot().unwrap().0;
        let marker = json!(now());
        marked["settings_unconfirmed_at"] = marker.clone();
        marked["pending_settings"] = json!({"request_id":id,"model":"fixture-model","effort":"high",
            "run_id":record["run_id"],"conversation_id":record["conversation_id"],"receipt_name":record["name"],"receipt_run_id":record["run_id"]});
        let mut progress = receipt.clone();
        progress["status"] = json!("in_progress");
        let mut inner = progress.clone();
        inner["request_id"] = json!(id);
        inner["digest"] = json!(format!(
            "{:x}",
            Sha256::digest(b"original scheduled request")
        ));
        let (outer, original) = owner
            .with_scope(|scope| {
                scope.receipts.write_new(
                    &format!("{}.json", frame.request_id),
                    progress.to_string().as_bytes(),
                )?;
                scope
                    .receipts
                    .write_new(&format!("{id}.json"), inner.to_string().as_bytes())?;
                Ok((
                    Original::open(&scope.receipts, format!("{}.json", frame.request_id))?,
                    Original::open(&scope.receipts, format!("{id}.json"))?,
                ))
            })
            .unwrap();
        let staged = stage_record(&owner, &marked).unwrap();
        // Only a synthetic unit observation: exercise actual retained commit
        // and reservation checks, without claiming a native key permission.
        let observation = context::Observation::begin().unwrap().unwrap();
        observation.complete(&record).unwrap();
        let fence = publication::CommitFence::native(&owner, &staged).unwrap();
        owner.adopt_record(staged.commit(&fence).unwrap()).unwrap();
        let (_, record_version) = owner.snapshot().unwrap();
        let kind = owner.with_scope(|scope| Ok(scope.proof.kind())).unwrap();
        *owner.reservation.lock().unwrap() = Some(transaction::Reservation {
            id: frame.request_id.clone(),
            digest: format!("{:x}", Sha256::digest(&frame.raw)),
            run: string(&record, "run_id").into(),
            conversation: string(&record, "conversation_id").into(),
            kind,
            marker,
            prior: None,
            record_version,
            receipt: outer,
            pending: Some(transaction::Pending {
                id: id.clone(),
                receipt: original,
                value: inner.clone(),
                pending: marked["pending_settings"].clone(),
            }),
        });
        assert!(
            transaction::key_ready(&owner).is_ok(),
            "positive original W/U reservation is required"
        );
        let path = root().join("settings_receipts").join(format!("{id}.json"));
        fs::rename(&path, path.with_extension("kept")).unwrap();
        owner
            .with_scope(|scope| {
                scope
                    .receipts
                    .write_new(&format!("{id}.json"), inner.to_string().as_bytes())
            })
            .unwrap();
        assert!(
            transaction::key_ready(&owner).is_err(),
            "same JSON in a new U inode must not grant input"
        );
    } else if case == "liveness-change" {
        let mut value = receipt.clone();
        value["apply_when"] = json!("ready");
        let witness = Schedule::new(Liveness::Running, None, value.clone());
        let staged = stage_receipt(&owner, &frame.request_id, &value, None).unwrap();
        let fence = publication::CommitFence::schedule(&owner, &staged, &witness).unwrap();
        native.as_mut().unwrap().0.kill().unwrap();
        native.as_mut().unwrap().0.wait().unwrap();
        assert!(
            staged.commit(&fence).is_err(),
            "an exited original cannot commit its old running class"
        );
        assert!(owner.actor().is_err());
        assert!(
            !root()
                .join("settings_receipts")
                .join(format!("{}.json", frame.request_id))
                .exists()
        );
    } else {
        let mut witness = Schedule::new(Liveness::Stopped, None, receipt.clone());
        let mut value = receipt.clone();
        if case == "bad-applied" {
            value["status"] = json!("applied");
        }
        if case == "bad-uuid" {
            value["request_id"] = json!(uuid::Uuid::new_v4().to_string());
        }
        if case == "bad-target" {
            value["model"] = json!("another-model");
        }
        let staged = if matches!(
            case.as_str(),
            "bad-marker" | "bad-model" | "bad-effort" | "bad-foreign-fields"
        ) {
            // All other schedule predicates are genuinely satisfied. The
            // field-mask check alone must reject this extra native/foreign
            // mutation, even if an erroneous caller proposes these same bytes.
            let mut changed = record.clone();
            changed["pending_settings"] = json!({"request_id":frame.request_id,"model":"fixture-model","effort":"high",
                "run_id":record["run_id"],"conversation_id":record["conversation_id"],"receipt_name":record["name"],"receipt_run_id":record["run_id"]});
            changed["resume_settings"] = json!({"model":"fixture-model","effort":"high"});
            changed["updated"] = json!(now());
            changed[match case.as_str() {
                "bad-marker" => "settings_unconfirmed_at",
                "bad-model" => "model",
                "bad-effort" => "effort",
                _ => "foreign",
            }] = json!("changed");
            witness.record = Some(changed.clone());
            stage_record(&owner, &changed).unwrap()
        } else {
            stage_receipt(&owner, &frame.request_id, &value, None).unwrap()
        };
        if case == "bad-key" {
            assert!(transaction::key_ready(&owner).is_err());
        } else if case == "bad-recovery" {
            assert!(publication::CommitFence::recovery(&owner, &staged, &[]).is_err());
        } else if case == "bad-record-fence" {
            let fence = publication::CommitFence::schedule(&owner, &staged, &witness).unwrap();
            assert!(fence.record(&record).is_err());
        } else if case == "bad-liveness" {
            let wrong = Schedule::new(Liveness::Running, None, receipt);
            assert!(publication::CommitFence::schedule(&owner, &staged, &wrong).is_err());
        } else {
            assert!(publication::CommitFence::schedule(&owner, &staged, &witness).is_err());
        }
        assert!(
            !root()
                .join("settings_receipts")
                .join(format!("{}.json", frame.request_id))
                .exists()
        );
    }
    assert!(contended(&root_file) && contended(&sid_file));
    drop(entered);
    drop(owner);
    done.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(!contended(&root_file) && !contended(&sid_file));
    drop(peer);
}
