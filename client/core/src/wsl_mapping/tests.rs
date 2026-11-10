use super::*;
use serde_json::json;

const MACHINE: &str = "workstation";
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn list(transport: &str) -> Value {
    json!({"local": MACHINE, "machines": [
        {"alias": "ubuntu", "connection": {"transport": transport, "enabled": true,
            "wsl": {"distribution": "Ubuntu", "user": "dev", "hgs_path": "/home/test/.local/bin/hgs"}}},
        {"alias": "studio", "connection": {"transport": "ssh"}}]})
}
fn ok(value: &Value) -> Exit {
    Exit::Code {
        code: 0,
        stdout: value.to_string().into_bytes(),
        stderr: String::new(),
    }
}
fn refused(text: &str) -> Exit {
    Exit::Code {
        code: 1,
        stdout: Vec::new(),
        stderr: text.into(),
    }
}
fn fields() -> Fields {
    Fields {
        windows_root: r"C:\Projects".into(),
        mount_root: "/mnt".into(),
        workspace_id: "orbit".into(),
    }
}
fn presets(name: &str) -> Presets {
    let dir = std::env::temp_dir().join(format!("zerus-wsl-mapping-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Presets::new(dir.join("wsl-path-mappings.json"))
}
fn stdin(call: &Invocation) -> Value {
    serde_json::from_slice(call.stdin.as_ref().unwrap()).unwrap()
}

/// A fake hgs that answers path-ref as the real one would for this mapping.
fn path_ref(call: &Invocation) -> Exit {
    match call
        .args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["machine", "ls"] => ok(&list("wsl-local")),
        ["path-ref", "describe"] => {
            let input = stdin(call);
            let path = &input["path"];
            ok(
                &json!({"schema_version": 1, "machine_id": path["machine_id"], "environment_id": "windows-native",
                "path_flavor": "windows", "native_path": path["native_path"], "canonical_path": "c:/projects/app",
                "workspace_id": input["mapping"]["workspace_id"], "mapping_id": input["mapping"]["mapping_id"]}),
            )
        }
        ["path-ref", "map"] => {
            let input = stdin(call);
            let mut out = input["path"].clone();
            out["environment_id"] = input["mapping"]["wsl_environment_id"].clone();
            out["path_flavor"] = json!("posix");
            out["native_path"] = json!("/mnt/c/Projects/app");
            ok(&out)
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn only_an_explicit_local_wsl_connection_is_offered() {
    let found = connection(&list("wsl-local"), "ubuntu", true).unwrap();
    assert_eq!(found.environment(), "wsl:Ubuntu:dev");
    assert_eq!(found.machine_id, MACHINE);
    assert_eq!(
        connection(&list("ssh"), "ubuntu", false)
            .unwrap_err()
            .english(),
        "This action requires an explicitly configured local WSL connection."
    );
    assert!(connection(&list("wsl-local"), "studio", false).is_err());
    let mut disabled = list("wsl-local");
    disabled["machines"][0]["connection"]["enabled"] = json!(false);
    assert!(
        connection(&disabled, "ubuntu", false).is_ok(),
        "a mapping preview is lexical"
    );
    assert!(connection(&disabled, "ubuntu", true)
        .unwrap_err()
        .english()
        .starts_with("Choose an enabled"));
    let mut no_hgs = list("wsl-local");
    no_hgs["machines"][0]["connection"]["wsl"]["hgs_path"] = json!("");
    assert!(connection(&no_hgs, "ubuntu", true).is_err());
    let mut moved = list("wsl-local");
    assert!(unchanged(&moved, &found));
    moved["machines"][0]["connection"]["wsl"]["user"] = json!("other");
    assert!(!unchanged(&moved, &found));
    moved = list("wsl-local");
    moved["local"] = json!("another");
    assert!(!unchanged(&moved, &found));
    assert_eq!(found.key().len(), 64);
}

#[test]
fn a_mapping_is_previewed_through_describe_and_map_and_used_after_a_recheck() {
    let presets = presets("map");
    let calls = std::cell::RefCell::new(Vec::new());
    let mut run = |call: &Invocation, _cap: usize| {
        calls.borrow_mut().push(call.clone());
        path_ref(call)
    };
    let (mut dialog, target) = MappingDialog::open(&mut run, &presets, "ubuntu").unwrap();
    assert_eq!(target.environment, "wsl:Ubuntu:dev");
    assert_eq!(target.saved, Fields::default());
    let mapped = dialog
        .preview(&mut run, r"C:\Projects\app", &fields())
        .unwrap();
    assert_eq!(
        mapped,
        Mapped {
            windows: r"C:\Projects\app".into(),
            wsl: "/mnt/c/Projects/app".into(),
            canonical: "c:/projects/app".into()
        }
    );
    let describe = stdin(&calls.borrow()[1]);
    assert_eq!(describe["path"]["environment_id"], "windows-native");
    assert_eq!(describe["mapping"]["wsl_environment_id"], "wsl:Ubuntu:dev");
    assert_eq!(describe["mapping"]["windows_root"], r"C:\Projects");
    assert_eq!(calls.borrow()[2].args, ["path-ref", "map"]);
    // A changed source must be previewed again before use.
    assert!(dialog
        .accept(&mut run, &presets, r"C:\Projects\other", &fields(), true)
        .is_err());
    dialog
        .preview(&mut run, r"C:\Projects\app", &fields())
        .unwrap();
    assert_eq!(
        dialog
            .accept(&mut run, &presets, r"C:\Projects\app", &fields(), true)
            .unwrap(),
        "/mnt/c/Projects/app"
    );
    // Remembered: the next dialog opens with the fields and the same mapping id.
    let (again, target) = MappingDialog::open(&mut run, &presets, "ubuntu").unwrap();
    assert_eq!(target.saved, fields());
    assert_eq!(
        again.mapping_id,
        describe["mapping"]["mapping_id"].as_str().unwrap()
    );
}

#[test]
fn mismatched_replies_and_a_changed_connection_are_refused() {
    let presets = presets("refuse");
    let mut run = |call: &Invocation, _cap: usize| path_ref(call);
    let (mut dialog, _) = MappingDialog::open(&mut run, &presets, "ubuntu").unwrap();
    assert!(
        dialog.preview(&mut run, "", &fields()).is_err(),
        "every field is required"
    );
    // describe answers another mapping id
    let mut wrong_id = |call: &Invocation, _cap: usize| match call.args.get(1).map(String::as_str) {
        Some("describe") => {
            let Exit::Code { stdout, .. } = path_ref(call) else {
                unreachable!()
            };
            let mut value: Value = serde_json::from_slice(&stdout).unwrap();
            value["mapping_id"] = json!("other");
            ok(&value)
        }
        _ => path_ref(call),
    };
    assert_eq!(
        dialog
            .preview(&mut wrong_id, r"C:\Projects\app", &fields())
            .unwrap_err()
            .english(),
        "The source identity response does not match this mapping."
    );
    let mut relative = |call: &Invocation, _cap: usize| match call.args.get(1).map(String::as_str) {
        Some("map") => {
            let Exit::Code { stdout, .. } = path_ref(call) else {
                unreachable!()
            };
            let mut value: Value = serde_json::from_slice(&stdout).unwrap();
            value["native_path"] = json!("//server/share");
            ok(&value)
        }
        _ => path_ref(call),
    };
    assert_eq!(
        dialog
            .preview(&mut relative, r"C:\Projects\app", &fields())
            .unwrap_err()
            .english(),
        "The mapped response does not match the selected environment."
    );
    let mut hgs_refuses =
        |call: &Invocation, _cap: usize| match call.args.get(1).map(String::as_str) {
            Some("describe") => refused("path is outside windows_root"),
            _ => path_ref(call),
        };
    assert_eq!(
        dialog
            .preview(&mut hgs_refuses, r"D:\x", &fields())
            .unwrap_err()
            .english(),
        "path is outside windows_root"
    );
    dialog
        .preview(&mut run, r"C:\Projects\app", &fields())
        .unwrap();
    let mut moved = |call: &Invocation, _cap: usize| {
        if call.args == ["machine", "ls"] {
            let mut value = list("wsl-local");
            value["machines"][0]["connection"]["wsl"]["distribution"] = json!("Debian");
            ok(&value)
        } else {
            path_ref(call)
        }
    };
    assert_eq!(
        dialog
            .accept(&mut moved, &presets, r"C:\Projects\app", &fields(), false)
            .unwrap_err()
            .english(),
        "The WSL connection changed. Close this preview and select the machine again."
    );
    assert!(path_ref_call("validate", &json!({})).is_err());
    assert!(path_ref_call("describe", &json!({"x": "y".repeat(REQUEST_LIMIT)})).is_err());
}

fn scope() -> Scope {
    Scope {
        session: "claude/app/main".into(),
        run_id: "run-1".into(),
        conversation_id: "conv-1".into(),
        path: "/mnt/c/Projects/app/notes.md".into(),
        archive_id: None,
        agent_id: Some("child".into()),
    }
}

/// A fake hgs that proves the file as `hgs machine file-proof` would.
fn prover(token: &'static str) -> impl FnMut(&Invocation, usize) -> Exit {
    move |call: &Invocation, _cap: usize| match call
        .args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["machine", "ls"] => ok(&list("wsl-local")),
        ["--help"] => ok(&json!("hgs ... wsl-file-proof-v1 ...")),
        ["machine", "file-proof", "ubuntu", "--json"] => {
            let request = stdin(call);
            let selection = request["selection"].clone();
            ok(
                &json!({"schema_version": 1, "selection": selection, "alias": "ubuntu",
                "native_path": selection["path"], "bytes": 12, "sha256": DIGEST, "native_changed": false, "local_verified": false,
                "preview_identity": token,
                "target": {"schema_version": 1, "machine_id": MACHINE, "environment_id": "windows-native", "path_flavor": "windows",
                    "native_path": r"C:\Projects\app\notes.md", "canonical_path": "c:/projects/app/notes.md",
                    "workspace_id": selection["mapping"]["workspace_id"], "mapping_id": selection["mapping"]["mapping_id"]}}),
            )
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn a_file_is_proved_verified_locally_and_opened_only_with_the_same_proof() {
    let presets = presets("file");
    let mut run = prover(TOKEN);
    let verified = std::cell::RefCell::new(Vec::new());
    let verify = |path: &str, sha: &str, bytes: u64| -> Result<Guard, Text> {
        verified
            .borrow_mut()
            .push((path.to_owned(), sha.to_owned(), bytes));
        Ok(Guard::default())
    };
    let (mut dialog, target) = FileDialog::open(&mut run, &presets, "ubuntu", scope()).unwrap();
    assert_eq!(target.distribution, "Ubuntu");
    let preview = dialog.preview(&mut run, &verify, &fields()).unwrap();
    assert_eq!(preview.windows, r"C:\Projects\app\notes.md");
    assert_eq!(preview.wsl, "/mnt/c/Projects/app/notes.md");
    assert_eq!(
        verified.borrow()[0],
        (
            r"C:\Projects\app\notes.md".to_owned(),
            DIGEST.to_owned(),
            12
        )
    );
    let mut sent = Vec::new();
    let mut recording = |call: &Invocation, cap: usize| {
        if call.args.get(1).map(String::as_str) == Some("file-proof") {
            sent.push(stdin(call));
        }
        prover(TOKEN)(call, cap)
    };
    let (path, _guard) = dialog.recheck(&mut recording, &verify, &fields()).unwrap();
    assert_eq!(path, r"C:\Projects\app\notes.md");
    assert_eq!(sent[0]["expected_preview"], TOKEN);
    assert_eq!(sent[0]["selection"]["agent_id"], "child");
    assert_eq!(
        sent[0]["selection"]["connection"]["hgs_path"],
        "/home/test/.local/bin/hgs"
    );
    dialog.remember(&presets).unwrap();
    assert_eq!(
        FileDialog::open(&mut run, &presets, "ubuntu", scope())
            .unwrap()
            .1
            .saved,
        fields()
    );
}

#[test]
fn a_changed_proof_a_local_mismatch_or_an_old_cli_opens_nothing() {
    let presets = presets("file-refuse");
    let verify_ok = |_: &str, _: &str, _: u64| -> Result<Guard, Text> { Ok(Guard::default()) };
    let mut run = prover(TOKEN);
    let (mut dialog, _) = FileDialog::open(&mut run, &presets, "ubuntu", scope()).unwrap();
    dialog.preview(&mut run, &verify_ok, &fields()).unwrap();
    // A new preview identity on open.
    let other = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    assert_eq!(
        dialog
            .recheck(&mut prover(other), &verify_ok, &fields())
            .unwrap_err()
            .english(),
        "The file proof no longer matches this preview. Nothing was opened."
    );
    assert!(
        dialog.recheck(&mut run, &verify_ok, &fields()).is_err(),
        "the preview is spent"
    );
    // Different local contents.
    let verify_bad = |_: &str, _: &str, _: u64| -> Result<Guard, Text> {
        Err(crate::text!(
            "The Windows and WSL files have different contents. Nothing was opened."
        ))
    };
    assert!(dialog.preview(&mut run, &verify_bad, &fields()).is_err());
    // Changed fields need a new preview.
    dialog.preview(&mut run, &verify_ok, &fields()).unwrap();
    let mut changed = fields();
    changed.workspace_id = "other".into();
    assert!(dialog.recheck(&mut run, &verify_ok, &changed).is_err());
    // An older CLI.
    let mut old = |call: &Invocation, cap: usize| {
        if call.args == ["--help"] {
            ok(&json!("hgs"))
        } else {
            prover(TOKEN)(call, cap)
        }
    };
    assert_eq!(
        dialog
            .preview(&mut old, &verify_ok, &fields())
            .unwrap_err()
            .english(),
        "Update the matching local CLI to use scoped WSL file mapping."
    );
    // A refusal says why.
    let mut refusing = |call: &Invocation, cap: usize| {
        if call.args.get(1).map(String::as_str) == Some("file-proof") {
            refused("Session identity changed")
        } else {
            prover(TOKEN)(call, cap)
        }
    };
    assert_eq!(
        dialog
            .preview(&mut refusing, &verify_ok, &fields())
            .unwrap_err()
            .english(),
        "Session identity changed"
    );
    // No conversation or a relative path: never asked.
    let mut no_run = scope();
    no_run.run_id.clear();
    assert!(FileDialog::open(&mut run, &presets, "ubuntu", no_run).is_err());
    let mut relative = scope();
    relative.path = "notes.md".into();
    assert!(FileDialog::open(&mut run, &presets, "ubuntu", relative).is_err());
}

#[test]
fn only_plain_drive_paths_are_local_originals() {
    for good in [
        r"C:\Projects\app\notes.md",
        "c:/Projects/a b/x.txt",
        r"D:\x",
    ] {
        assert!(safe_drive_path(good), "{good}");
    }
    for bad in [
        r"\\server\share\x",
        r"\\?\C:\x",
        r"C:x",
        r"C:\a\..\b",
        r"C:\a\.\b",
        r"C:\a\\b",
        r"C:\a.",
        r"C:\a ",
        r"C:\CON",
        r"C:\nul.txt",
        r"C:\COM1",
        r"C:\LPT¹",
        r"C:\a:stream",
        r"C:\a?b",
        "C:\\a\u{1}b",
        "relative",
    ] {
        assert!(!safe_drive_path(bad), "{bad}");
    }
    assert!(
        safe_drive_path(r"C:\COM10"),
        "only COM0 to COM9 are reserved"
    );
    assert!(verify_local("relative", DIGEST, 1).is_err());
    assert!(verify_local(r"C:\x", "nope", 1).is_err());
    assert!(verify_local(r"C:\x", DIGEST, 0).is_err());
}

#[cfg(windows)]
#[test]
fn a_real_local_file_is_verified_by_size_and_digest() {
    let dir = std::env::temp_dir().join(format!("zerus-wsl-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("notes.md");
    std::fs::write(&file, b"hello world\n").unwrap();
    // The temp folder may sit behind a junction on some machines; use its final spelling.
    let canonical = std::fs::canonicalize(&file)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let real = canonical.trim_start_matches(r"\\?\").to_owned();
    let digest = format!("{:x}", Sha256::digest(b"hello world\n"));
    match verify_local(&real, &digest, 12) {
        Ok(_) => {}
        Err(error) => assert!(
            error.english().starts_with("Only a local fixed-drive"),
            "{}",
            error.english()
        ),
    }
    assert!(verify_local(&real, &digest, 13).is_err());
    assert!(verify_local(&real, DIGEST, 12).is_err());
}
