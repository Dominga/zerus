use super::*;

#[test]
fn terminal_provider_and_id_publishers_are_covered_without_native_machine_identity() {
    let root = crate::host::staging::private_tempdir();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child.args([
        "--exact",
        "state::listing_metadata::tests::writers::writer_fixture",
        "--ignored",
        "--test-threads=1",
    ]);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("HGS_") {
            child.env_remove(key);
        }
    }
    let result = child
        .env("HOME", root.path())
        .env("HGS_STATE_DIR", root.path().join("state"))
        .env("HGS_CONFIG_DIR", root.path().join("config"))
        .env("TMUX_TMPDIR", root.path().join("tmux"))
        .env("ZERUS_LISTING_WRITER_FIXTURE", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
#[ignore = "private child with isolated canonical roots"]
fn writer_fixture() {
    if std::env::var("ZERUS_LISTING_WRITER_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let root = root();
    private_dir(&root).unwrap();
    enable(&root);
    let mut expected = 0;
    let mut providers = Vec::new();
    for folder in storage::BINDING_DIRS {
        let mut value = record();
        let by_id = folder.ends_with("sessions");
        value["name"] = json!(if folder.starts_with("dsh/") {
            "dsh/example/one"
        } else if folder.starts_with("opencode/") {
            "opencode/example/one"
        } else {
            "codex/example/one"
        });
        value["layout"] = if by_id { json!("id") } else { Value::Null };
        let id = if by_id {
            string(&value, "session_id").to_owned()
        } else {
            storage::label_key(string(&value, "name"))
        };
        let path = root.join(folder).join(format!("{id}.json"));
        let provider = if folder.starts_with("dsh/") {
            Some(provider_binding::Provider::DeepSeek)
        } else if folder.starts_with("opencode/") {
            Some(provider_binding::Provider::OpenCode)
        } else {
            None
        };
        if let Some(provider) = provider {
            value["agent"] = json!(if folder.starts_with("dsh/") {
                "dsh"
            } else {
                "opencode"
            });
            value["backend"] = value["agent"].clone();
            value["conversation_id"] = json!(if folder.starts_with("dsh/") {
                "session-fixture"
            } else {
                "ses_fixture"
            });
            if by_id {
                storage::atomic(&path, &value.to_string()).unwrap();
            }
            provider.save(&mut value).unwrap();
        } else {
            storage::atomic_record(&path, &mut value).unwrap();
        }
        expected += 1;
        let store = Store::open(&root).unwrap();
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM sources WHERE tombstone=0", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            expected,
            "missing publisher: {folder}"
        );
        let actual: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if let Some(provider) = provider {
            providers.push((provider, actual.clone()));
        }
        let key = Key::at(&root, &path).unwrap();
        let (seed, stamp): (String, String) = store
            .db
            .query_row(
                "SELECT seed,stamp FROM sources WHERE source=?1",
                [&key.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            seed,
            Seed::encode(&key, &actual).unwrap(),
            "publisher parity: {folder}"
        );
        assert_eq!(Some(stamp), store.source_stamp(&key).unwrap());
        drop(store);
    }
    for (provider, value) in providers.into_iter().rev() {
        provider.remove(&value).unwrap();
        expected -= 1;
        let store = Store::open(&root).unwrap();
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM sources WHERE tombstone=0", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap(),
            expected,
            "provider deletion publisher missing"
        );
    }
}
