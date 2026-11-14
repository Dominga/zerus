use super::*;
use crate::desktop::teams::tests::plan_fixture;
#[test]
fn native_arguments_preserve_launch_identity_account_and_role_choices() {
    for agent in ["claude", "codex", "kimi", "dsh", "opencode"] {
        let mut p = plan_fixture();
        p.agent = agent.into();
        p.model = if agent == "dsh" { "" } else { "fixture-model" }.into();
        p.effort = if matches!(agent, "claude" | "codex" | "opencode") {
            "high"
        } else {
            ""
        }
        .into();
        let tree = Worktree {
            path: p.placement.destination.clone(),
            ownership_nonce: "synthetic-nonce".into(),
        };
        let args = arguments::launch(&p, &tree).unwrap();
        assert!(args.windows(2).any(|v| v == ["--launch-id", &p.launch_id]));
        assert!(args.windows(2).any(|v| v == ["--account", &p.account]));
        assert_eq!(
            args.contains(&"-d".into()),
            !matches!(agent, "dsh" | "opencode")
        );
        if agent == "codex" {
            assert!(args.contains(&"model_reasoning_effort=\"high\"".into()));
        }
        if agent == "claude" {
            assert!(args.windows(2).any(|v| v == ["--effort", "high"]));
        }
        if agent == "opencode" {
            assert!(!args.contains(&"--".into()));
        }
        if agent == "kimi" {
            assert!(args.windows(2).any(|v| v == ["--model", "fixture-model"]));
        }
    }
}
#[test]
fn unsupported_native_choices_are_refused_instead_of_ignored() {
    let mut p = plan_fixture();
    let tree = Worktree {
        path: p.placement.destination.clone(),
        ownership_nonce: "nonce".into(),
    };
    p.agent = "kimi".into();
    p.effort = "high".into();
    assert!(arguments::launch(&p, &tree).is_err());
    p.agent = "dsh".into();
    p.effort.clear();
    p.model = "fixture-model".into();
    assert!(arguments::launch(&p, &tree).is_err());
}
#[test]
fn usage_inspection_checks_identity_authentication_and_reset_units() {
    let v = json!({"id":"a","provider":"claude","auth_status":"signed_in","status":"rate_limited","windows":[{"used_percent":100,"resets_at":"2030-01-01T00:00:00Z"}]});
    let a = arguments::usage(&v, "claude", "a").unwrap();
    assert!(a.signed_in);
    assert_eq!(a.windows[0].resets_at, Some(1893456000));
    assert!(arguments::usage(&v, "codex", "a").is_err());
    assert!(arguments::usage(&v, "claude", "b").is_err());
    let mut v = v;
    v["auth_status"] = json!("signed_out");
    v["status"] = json!("signed_out");
    assert!(!arguments::usage(&v, "claude", "a").unwrap().signed_in);
    v["windows"][0]["used_percent"] = Value::Null;
    assert!(arguments::usage(&v, "claude", "a").is_err());
}
#[test]
fn unknown_authentication_is_not_an_unmeasured_signed_in_account() {
    let v = json!({"id":"a","provider":"codex","status":"unavailable","windows":[]});
    assert!(!arguments::usage(&v, "codex", "a").unwrap().signed_in);
}
#[test]
fn worktree_arguments_use_only_the_saved_generated_placement() {
    let p = plan_fixture();
    let args = arguments::worktree(&p);
    for (key, value) in [
        ("--path", &p.placement.source),
        ("--branch", &p.placement.branch),
        ("--destination", &p.placement.destination),
        ("--base", &p.base_commit),
    ] {
        assert!(args.windows(2).any(|v| v == [key, value]));
    }
    assert!(!args
        .iter()
        .any(|v| v == "reset" || v == "push" || v == "fetch" || v == "init"));
}
#[test]
fn repository_observation_does_not_initialize_an_ordinary_folder() {
    let temp = tempfile::tempdir().unwrap();
    assert!(observation::common_dir(temp.path()).is_err());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn a_verified_api_key_without_quota_windows_is_available_as_an_unmeasured_account() {
    let v = json!({"id":"a","provider":"codex","status":"unavailable","identity":{"auth_method":"apiKey"},"windows":[]});
    let account = arguments::usage(&v, "codex", "a").unwrap();
    assert!(account.signed_in);
    assert!(account.windows.is_empty());
}

struct Records {
    h: crate::teams::ops::tests::Harness,
    p: Plan,
    native: Native,
    record: Value,
}
impl Records {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let h = crate::teams::ops::tests::Harness::new();
        let state = h.temp.path().canonicalize().unwrap();
        let mut p = plan_fixture();
        let source = state.as_path().join("repository");
        std::fs::create_dir(&source).unwrap();
        let empty = state.as_path().join("empty-git-config");
        std::fs::write(&empty, b"").unwrap();
        let git = |args: &[&str]| {
            let mut cmd = std::process::Command::new("git");
            for (key, _) in std::env::vars_os() {
                if key.to_string_lossy().starts_with("GIT_") {
                    cmd.env_remove(key);
                }
            }
            let out = cmd
                .current_dir(&source)
                .env("GIT_CONFIG_GLOBAL", &empty)
                .env("GIT_CONFIG_SYSTEM", &empty)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.test",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "-q"]);
        git(&["commit", "--allow-empty", "-qm", "Fixture base"]);
        let destination = state.as_path().join(format!("zerus-team-{}", p.launch_id));
        git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &p.placement.branch,
            destination.to_str().unwrap(),
        ]);
        let metadata = {
            let out = std::process::Command::new("git")
                .args([
                    "-C",
                    destination.to_str().unwrap(),
                    "rev-parse",
                    "--absolute-git-dir",
                ])
                .output()
                .unwrap();
            assert!(out.status.success());
            PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
        };
        let common = source.join(".git");
        p.placement.source = source.to_str().unwrap().into();
        p.placement.git_dir = common.to_str().unwrap().into();
        p.placement.destination = destination.to_str().unwrap().into();
        let nonce = uuid::Uuid::new_v4().to_string();
        let owner = json!({"version":1,"id":nonce,"common_dir":common,"path":destination,"metadata":metadata,"branch":p.placement.branch,"head":"fixture"});
        let cache = state.as_path().join("worktree-cache");
        std::fs::create_dir(&cache).unwrap();
        std::fs::write(
            crate::state::team_worktree_receipt_path(state.as_path(), &common, &destination),
            owner.to_string(),
        )
        .unwrap();
        std::fs::write(metadata.join("zerus-owner.json"), owner.to_string()).unwrap();
        for dir in ["launches", "sessions"] {
            let d = state.as_path().join(dir);
            std::fs::create_dir(&d).unwrap();
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let sid = uuid::Uuid::new_v4().to_string();
        let record = json!({"version":1,"layout":"id","session_id":sid,"launch_id":p.launch_id,"name":"claude/example/renamed","agent":p.agent,"account_id":p.account,"cwd":p.placement.destination,"run_id":uuid::Uuid::new_v4().to_string(),"conversation_id":""});
        let start = json!({"launch_id":p.launch_id,"name":"claude/example/original","session_id":sid,"started_at":1.0});
        let start_path = state
            .as_path()
            .join("launches")
            .join(format!("{}.json", p.launch_id));
        std::fs::write(&start_path, start.to_string()).unwrap();
        std::fs::set_permissions(start_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let config = Config {
            home: String::new(),
            dir: state.as_path().join("config"),
            state: state.as_path().into(),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        let native = Native {
            config,
            machine: p.machine.clone(),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let f = Self {
            h,
            p,
            native,
            record,
        };
        f.write();
        f
    }
    fn write(&self) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.h.temp.path().join("sessions").join(format!(
            "{}.json",
            self.record["session_id"].as_str().unwrap()
        ));
        std::fs::write(&path, self.record.to_string()).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn read(&self) -> Result<LaunchRead> {
        observation::session(
            &self.native,
            &self.p,
            Instant::now() + Duration::from_secs(3),
        )
    }
}
#[test]
fn native_observation_binds_an_actual_renamed_record_without_conversation() {
    let f = Records::new();
    let LaunchRead::Session(s) = f.read().unwrap() else {
        panic!("trust record not found")
    };
    assert!(s.conversation_id.is_empty());
    assert_eq!(s.name, "claude/example/renamed");
}
#[test]
fn native_observation_refuses_a_different_launch_id_and_cwd_in_actual_records() {
    for field in ["launch_id", "cwd"] {
        let mut f = Records::new();
        f.record[field] = json!(if field == "launch_id" {
            uuid::Uuid::new_v4().to_string()
        } else {
            "/workspace/another-checkout".into()
        });
        f.write();
        assert_eq!(f.read().unwrap_err().code, code::DELIVERY_UNCERTAIN);
    }
}
#[test]
fn native_observation_distinguishes_an_ended_launch_from_missing_evidence() {
    use std::os::unix::fs::PermissionsExt;
    let f = Records::new();
    let path =
        f.h.temp
            .path()
            .join("launches")
            .join(format!("{}.end.json", f.p.launch_id));
    std::fs::write(
        &path,
        json!({"launch_id":f.p.launch_id,"code":2,"ended_at":3.0}).to_string(),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(f.read().unwrap(), LaunchRead::Ended { code: 2 }));
}

#[test]
fn ownership_read_failure_defers_binding_and_a_launch_before_any_child_starts() {
    let mut f = Records::new();
    let metadata =
        std::fs::read_to_string(Path::new(&f.p.placement.destination).join(".git")).unwrap();
    let metadata = Path::new(metadata.trim().strip_prefix("gitdir: ").unwrap());
    // Both receipts and the checkout still match; only Git's verification fails.
    let link = metadata.join("commondir");
    let original = std::fs::read(&link).unwrap();
    std::fs::remove_file(&link).unwrap();
    assert_eq!(f.read().unwrap_err().code, code::INVALID_STATE);
    let tree = Worktree {
        path: f.p.placement.destination.clone(),
        ownership_nonce: "fixture".into(),
    };
    let outcome = f
        .native
        .launch(&f.p, &tree, &uuid::Uuid::new_v4().to_string());
    assert!(matches!(outcome, Effect::Deferred(p) if p.code == code::INVALID_STATE));
    std::fs::write(&link, original).unwrap();
    assert!(matches!(f.read().unwrap(), LaunchRead::Session(_)));
}

#[test]
fn native_provider_without_an_account_id_cannot_bind_as_another_profile() {
    use std::os::unix::fs::PermissionsExt;
    for agent in ["dsh", "opencode"] {
        let mut f = Records::new();
        f.p.agent = agent.into();
        f.p.account = format!("another-{agent}");
        f.record["agent"] = json!(agent);
        f.record["name"] = json!(format!("{agent}/example/member"));
        f.record.as_object_mut().unwrap().remove("account_id");
        let folder = f.h.temp.path().join(agent).join("sessions");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = folder.join(format!("{}.json", f.record["session_id"].as_str().unwrap()));
        std::fs::write(&path, f.record.to_string()).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(f.h.temp.path().join("launches").join(format!("{}.json",f.p.launch_id)), json!({"launch_id":f.p.launch_id,"name":format!("{agent}/example/original"),"session_id":f.record["session_id"],"started_at":1.0}).to_string()).unwrap();
        assert_eq!(f.read().unwrap_err().code, code::DELIVERY_UNCERTAIN);
        f.p.account = format!("native-{agent}");
        if agent == "dsh" {
            // Identity alone cannot turn an unreachable hosted agent into a
            // live one. The reviewed-host contract covers the positive case.
            assert_eq!(f.read().unwrap_err().code, code::INVALID_STATE);
            continue;
        }
        let LaunchRead::Session(session) = f.read().unwrap() else {
            panic!("native account was hidden")
        };
        assert_eq!(session.account, f.p.account);
    }
}

#[test]
fn a_configured_deepseek_api_key_is_authenticated_without_quota_windows() {
    let mut data = json!({"id":"native-dsh","provider":"dsh","status":"configured","windows":[]});
    assert!(arguments::usage(&data, "dsh", "native-dsh").unwrap().signed_in);
    data["auth_status"] = json!("signed_out");
    assert!(!arguments::usage(&data, "dsh", "native-dsh").unwrap().signed_in);
}

#[test]
fn native_created_worktree_with_unavailable_ownership_defers_only_its_read() {
    let mut f = Records::new();
    let line = std::fs::read_to_string(Path::new(&f.p.placement.destination).join(".git")).unwrap();
    let metadata = Path::new(line.trim().strip_prefix("gitdir: ").unwrap());
    std::fs::remove_file(metadata.join("commondir")).unwrap();
    let output = json!({"status":"created","path":f.p.placement.destination,"branch":f.p.placement.branch,"managed_cleanup":true});
    child::FIXTURE_EXIT
        .with(|slot| *slot.borrow_mut() = Some(serde_json::to_vec(&output).unwrap()));
    let outcome = f.native.worktree(&f.p, &uuid::Uuid::new_v4().to_string());
    assert!(matches!(outcome, Effect::Submitted(None)));
    assert!(child::FIXTURE_EXIT.with(|slot| slot.borrow().is_none()));
}

mod parent;
