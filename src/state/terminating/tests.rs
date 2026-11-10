use super::*;
use serde_json::json;

#[test]
fn any_marker_value_retains_the_unix_finalization_fence() {
    assert!(!pending(&json!({})));
    for marker in [Value::Null, json!(false), json!("damaged"), json!({})] {
        assert_eq!(
            pending(&json!({"terminating": marker})),
            cfg!(any(target_os = "linux", target_os = "macos"))
        );
    }
}

#[test]
fn mutation_admission_and_scoped_actions_refuse_every_marker_shape() {
    let mut record = json!({"name":"codex/termination-fence","run_id":"run-1",
        "conversation_id":"conversation-1"});
    let scope = crate::state::session_action::Scope::expecting(
        "codex/termination-fence",
        "run-1",
        "conversation-1",
    );
    assert!(check_mutation(&record).is_ok());
    assert!(scope.check(&record).is_ok());
    for marker in [Value::Null, json!(false), json!("damaged"), json!({})] {
        record["terminating"] = marker;
        let refused = cfg!(any(target_os = "linux", target_os = "macos"));
        assert_eq!(check_mutation(&record).is_err(), refused);
        assert_eq!(scope.check(&record).is_err(), refused);
        // Lookup/base correlation is still available; it creates no consent.
        assert!(scope.check_base(&record).is_ok());
    }
}

#[test]
fn marked_hook_admission_is_completion_only_and_windows_is_unchanged() {
    let record = json!({"terminating":Value::Null});
    for kind in [
        "SessionEnd",
        "PostToolUse",
        "PostToolUseFailure",
        "PostCompact",
        "PostModelSwitch",
        "Stop",
        "StopFailure",
        "SubagentStop",
        "Interrupt",
        "PermissionResult",
        "SessionHeartbeat",
    ] {
        assert!(completion_hook(&record, kind), "{kind}");
    }
    for kind in [
        "SessionStart",
        "UserPromptSubmit",
        "UserPromptQueued",
        "PreToolUse",
        "TurnStarted",
        "TaskStarted",
        "PermissionRequest",
        "SubagentStart",
        "unknown",
    ] {
        assert_eq!(
            completion_hook(&record, kind),
            !cfg!(any(target_os = "linux", target_os = "macos")),
            "{kind}"
        );
        assert!(completion_hook(&json!({}), kind));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::*;
    use crate::state::{
        archive, storage, supervisor, worktree_cleanup, worktree_lifecycle, Snapshot,
    };
    use std::fs;
    use std::os::unix::{fs::PermissionsExt, process::ExitStatusExt};
    use std::process::{Command, ExitStatus};
    use std::time::{Duration, Instant};

    const ARCHIVE: &str = "11111111-1111-4111-8111-111111111111";
    const AGENTS: [(&str, &str); 3] = [
        ("claude", "prompt_input_exit"),
        ("codex", "other"),
        ("kimi", "exit"),
    ];

    fn marker(kind: &str) -> Value {
        match kind {
            "null" => Value::Null,
            "false" => json!(false),
            "damaged" => json!("not a termination claim"),
            "object" => json!({"request_id":"22222222-2222-4222-8222-222222222222"}),
            _ => panic!("unknown marker case"),
        }
    }

    fn probe(case: &str, agent: &str, kind: &str) {
        let private = tempfile::tempdir().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "state::terminating::tests::unix::finalization_probe",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", private.path())
            .env("HGS_CONFIG_DIR", private.path().join("config"))
            .env("HGS_STATE_DIR", private.path().join("state"))
            .env("HGS_TEST_TERMINATING_CASE", case)
            .env("HGS_TEST_TERMINATING_AGENT", agent)
            .env("HGS_TEST_TERMINATING_MARKER", kind);
        let (status, out, err) = crate::platform::run_bounded(
            &mut command,
            [64 * 1024; 2],
            Instant::now() + Duration::from_secs(10),
            &|| false,
        )
        .unwrap_or_else(|error| panic!("{case}/{agent}/{kind}: {error:?}"));
        assert!(
            status.success(),
            "{case}/{agent}/{kind}: {}\n{}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        );
    }

    #[test]
    fn supervisor_records_each_native_exit_without_archive_or_restore_cleanup() {
        for (agent, _) in AGENTS {
            for kind in ["object", "null", "false", "damaged"] {
                probe("exit", agent, kind);
            }
            probe("exit", agent, "absent");
        }
        for kind in ["object", "null", "false", "damaged"] {
            probe("restore_exit", "codex", kind);
        }
        probe("restore_exit", "codex", "absent");
    }

    #[test]
    fn direct_archive_restore_and_reconciliation_cannot_bypass_the_fence() {
        for case in [
            "save",
            "stale_save",
            "failed_restore",
            "confirm_restore",
            "reconcile",
            "reconcile_existing",
            "active",
        ] {
            probe(case, "codex", "object");
        }
    }

    #[test]
    fn after_archive_never_starts_git_for_a_pending_termination() {
        probe("after_archive", "codex", "object");
        probe("after_archive", "codex", "absent");
    }

    #[test]
    #[ignore = "private re-execution entry"]
    fn finalization_probe() {
        let case = std::env::var("HGS_TEST_TERMINATING_CASE").unwrap();
        let agent = std::env::var("HGS_TEST_TERMINATING_AGENT").unwrap();
        let kind = std::env::var("HGS_TEST_TERMINATING_MARKER").unwrap();
        let reason = AGENTS.iter().find(|row| row.0 == agent).unwrap().1;
        let name = format!("{agent}/termination-fence");
        let mut record = json!({
            "version":1,"name":name,"agent":agent,"run_id":"original-run",
            "conversation_id":"original-conversation","pid":42,
            "conversation_state":"ended","completion_source":"supervisor",
            "completion_reason":"clean_exit","exit_code":0,
            "session_end":{"run_id":"original-run","conversation_id":"original-conversation",
                "reason":reason}
        });
        record["launch_dir"] = json!(storage::root().join("checkout"));
        assert!(archive::known_exit(&record));
        if kind != "absent" {
            record["terminating"] = marker(&kind);
            assert!(!archive::known_exit(&record));
        }
        let restoring = matches!(
            case.as_str(),
            "restore_exit" | "failed_restore" | "confirm_restore"
        );
        if restoring {
            let mut source = record.clone();
            source.as_object_mut().unwrap().remove("terminating");
            source["run_id"] = json!("archived-run");
            source["archive_id"] = json!(ARCHIVE);
            storage::atomic_record(&archive::path(ARCHIVE).unwrap(), &mut source).unwrap();
            record["restore_archive_id"] = json!(ARCHIVE);
            if case != "confirm_restore" {
                record["expected_id"] = json!("original-conversation");
            }
        }
        if matches!(case.as_str(), "reconcile_existing" | "active") {
            // A crash-left equivalent archive must not retire the marked binding.
            let mut source = record.clone();
            source.as_object_mut().unwrap().remove("terminating");
            source["archive_id"] = json!(ARCHIVE);
            storage::atomic_record(&archive::path(ARCHIVE).unwrap(), &mut source).unwrap();
        }
        if case == "after_archive" {
            let folder = storage::root().join("checkout");
            fs::create_dir_all(&folder).unwrap();
            record["launch_dir"] = json!(folder);
            let bin = storage::root().join("bin");
            fs::create_dir_all(&bin).unwrap();
            let git = bin.join("git");
            fs::write(
                &git,
                "#!/bin/sh\nprintf '%s\\n' invoked >> \"$HGS_TEST_GIT_LOG\"\nexit 1\n",
            )
            .unwrap();
            fs::set_permissions(&git, fs::Permissions::from_mode(0o700)).unwrap();
            std::env::set_var("PATH", format!("{}:/usr/bin:/bin", bin.display()));
            std::env::set_var("HGS_TEST_GIT_LOG", storage::root().join("git-calls"));
        }
        storage::write(&mut record).unwrap();
        let before = fs::read(storage::own_path(&record).unwrap()).unwrap();
        let archive_before = fs::read(archive::path(ARCHIVE).unwrap()).ok();
        match case.as_str() {
            "exit" | "restore_exit" => {
                // Actual finalizer, synthetic exit evidence: no agent is run/signalled.
                supervisor::finish(&name, "original-run", 42, ExitStatus::from_raw(0)).unwrap();
                if kind == "absent" {
                    assert!(!storage::own_path(&record).unwrap().exists());
                    let archives = archive::records().unwrap();
                    assert_eq!(archives.len(), 1);
                    if restoring {
                        assert!(!archives[0]["last_restore_error"]
                            .as_str()
                            .unwrap()
                            .is_empty());
                    } else {
                        assert_eq!(archives[0]["completion_reason"], "clean_exit");
                    }
                    return;
                }
                let current = storage::read(&name).unwrap();
                assert_eq!(current["terminating"], record["terminating"]);
                assert_eq!(current["run_id"], record["run_id"]);
                assert_eq!(current["conversation_id"], record["conversation_id"]);
                assert_eq!(current["exit_code"], 0);
                assert!(current["termination_signal"].is_null());
                assert!(current["exited_at"].as_f64().is_some());
                assert_eq!(current["completion_source"], "supervisor");
                assert_eq!(current["completion_reason"], "unconfirmed_exit");
                // Root cleanup still sees this binding even after root exit.
                let bindings = worktree_cleanup::bindings().unwrap();
                assert_eq!(bindings.len(), 1);
                assert_eq!(bindings[0].name, name);
                assert!(bindings[0]
                    .folders
                    .contains(&storage::root().join("checkout")));
            }
            "save" => {
                assert!(archive::save(&record).is_err());
            }
            "stale_save" => {
                let mut stale = record.clone();
                stale.as_object_mut().unwrap().remove("terminating");
                assert!(archive::save(&stale).is_err());
            }
            "failed_restore" => assert!(!archive::failed_restore(&mut record).unwrap()),
            "confirm_restore" => archive::confirm_restore(&mut record).unwrap(),
            "reconcile" | "reconcile_existing" => {
                let (_, kept) = archive::reconcile_locked(&Snapshot::new()).unwrap();
                assert_eq!(kept.len(), 1);
                assert_eq!(kept[0]["terminating"], record["terminating"]);
            }
            "active" => {
                let kept = archive::read_active(&Snapshot::new()).unwrap();
                assert_eq!(kept.len(), 1);
                assert_eq!(kept[0]["terminating"], record["terminating"]);
            }
            "after_archive" => {
                worktree_lifecycle::after_archive(&record, ARCHIVE);
                assert_eq!(storage::root().join("git-calls").exists(), kind == "absent");
            }
            _ => panic!("unknown finalizer case"),
        }
        if case != "exit" && case != "restore_exit" {
            assert_eq!(
                fs::read(storage::own_path(&record).unwrap()).unwrap(),
                before
            );
        }
        assert_eq!(
            fs::read(archive::path(ARCHIVE).unwrap()).ok(),
            archive_before
        );
        assert_eq!(
            archive::records().unwrap().len(),
            usize::from(archive_before.is_some())
        );
    }
}
