use super::*;

fn isolated(case: &str) {
    let private = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "state::archive_catalog::refresh::writer::tests::probe",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", private.path())
        .env("HGS_STATE_DIR", private.path().join("state"))
        .env("HGS_CONFIG_DIR", private.path().join("config"))
        .env("ZERUS_ARCHIVE_WRITER_CASE", case)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn consecutive_native_archives_remain_observable_without_a_worker() {
    isolated("batch");
}
#[test]
fn an_unbuilt_catalog_is_explicitly_incomplete_until_its_worker_finishes() {
    isolated("cold");
}
#[test]
fn run_lookup_uses_native_identity_coverage_instead_of_search_rows() {
    isolated("search");
}
#[test]
fn a_shared_run_with_two_native_archives_remains_ambiguous() {
    isolated("ambiguous");
}
#[test]
fn a_known_write_cannot_seal_an_unrelated_unindexed_archive() {
    isolated("interleaved");
}
#[test]
fn run_lookup_cannot_turn_an_in_place_replacement_into_absence() {
    isolated("replacement");
}
#[test]
fn an_archive_writer_excludes_other_cache_writers_before_native_publication() {
    isolated("writer-exclusion");
}
#[test]
fn a_run_lookup_cannot_adopt_another_configured_state_root() {
    isolated("foreign-root");
}
#[test]
fn a_native_forget_keeps_the_next_native_archive_observable() {
    isolated("forget");
}

// Input values are synthetic; every canonical archive row is published by the
// real archive writer, not by handwritten SQLite/cache rows.
fn input(n: u128) -> Value {
    json!({"version":1,"name":"codex/example/member","agent":"codex",
        "run_id":format!("run-{n}"),"conversation_id":format!("conversation-{n}"),
        "session_id":uuid::Uuid::from_u128(n).to_string(),"created":1,
        "cwd":root().join("work").display().to_string()})
}
fn save(record: &Value) -> String {
    let _guard = lock(None).unwrap();
    archive::save(record).unwrap()
}
fn lookup(record: &Value) -> Result<Option<String>> {
    super::super::super::find_run(&root(), string(record, "name"), string(record, "run_id"))
}

#[test]
#[ignore = "private process and state root"]
fn probe() {
    let Ok(case) = std::env::var("ZERUS_ARCHIVE_WRITER_CASE") else {
        return;
    };
    private_dir(&root()).unwrap();
    let record = input(1);
    if case == "interleaved" {
        BEFORE_PROOF.with(|slot| {
            slot.set(Some(|| {
                let mut other = input(77);
                let id = uuid::Uuid::from_u128(77).to_string();
                other["archive_id"] = json!(id);
                other["archived_at"] = json!(1);
                other["state"] = json!("archived");
                // Product publication primitive, simulating a concurrent import
                // whose valid native archive was not this writer's selected ID.
                atomic_record(
                    &root().join("archives").join(format!("{id}.json")),
                    &mut other,
                )
                .unwrap();
            }))
        });
        let id = save(&record);
        assert!(archive::read_archive(string(&record, "name"), &id).is_ok());
        assert!(
            super::super::catalog().is_err(),
            "known write advertised incomplete native coverage as ready"
        );
        assert!(lookup(&record).unwrap_err().starts_with(INCOMPLETE));
        return;
    }
    let id = save(&record);
    assert_eq!(lookup(&record).unwrap(), Some(id.clone()));
    match case.as_str() {
        "batch" => {
            for n in 2..=6 {
                let record = input(n);
                let id = save(&record);
                assert_eq!(lookup(&record).unwrap(), Some(id));
            }
            assert_eq!(lookup(&input(99)).unwrap(), None);
        }
        "cold" => {
            fs::remove_file(root().join(DATABASE)).unwrap();
            archive_references::METADATA_WORK.with(|work| work.set((0, 0)));
            let error = lookup(&record).unwrap_err();
            assert!(error.starts_with(INCOMPLETE), "{error}");
            assert_eq!(
                archive_references::METADATA_WORK.with(std::cell::Cell::get),
                (0, 0)
            );
            assert!(
                !root().join(DATABASE).exists(),
                "foreground lookup rebuilt the catalog"
            );
            let error = {
                let _guard = lock(None).unwrap();
                archive::save(&input(2)).unwrap_err()
            };
            assert!(error.starts_with(INCOMPLETE), "{error}");
            assert!(!root().join(DATABASE).exists());
            assert_eq!(fs::read_dir(root().join("archives")).unwrap().count(), 1);
            assert_eq!(worker().unwrap(), 0);
            assert_eq!(lookup(&record).unwrap(), Some(id));
        }
        "search" => {
            let db = Connection::open(root().join(DATABASE)).unwrap();
            db.execute("DELETE FROM archive_rows", []).unwrap();
            drop(db);
            assert_eq!(lookup(&record).unwrap(), Some(id));
        }
        "ambiguous" => {
            let mut other = record.clone();
            other["conversation_id"] = json!("another-native-conversation");
            assert_ne!(save(&other), id);
            assert!(lookup(&record).unwrap_err().contains("unique"));
        }
        "replacement" => {
            let mut replacement = record.clone();
            replacement["run_id"] = json!("run-2");
            replacement["archive_id"] = json!(id);
            replacement["archived_at"] = json!(1);
            replacement["state"] = json!("archived");
            // Deliberate handwritten damage: replace a native body's bytes
            // in place, leaving the directory's stamp unchanged.
            fs::write(archive::path(&id).unwrap(), replacement.to_string()).unwrap();
            assert!(lookup(&replacement).unwrap_err().starts_with(INCOMPLETE));
        }
        "writer-exclusion" => {
            let _guard = lock(None).unwrap();
            let _writer = super::super::super::writer_catalog().unwrap();
            let other = Connection::open(root().join(DATABASE)).unwrap();
            other.busy_timeout(Duration::ZERO).unwrap();
            assert!(
                other.execute_batch("BEGIN IMMEDIATE").is_err(),
                "another cache writer was admitted before native publication"
            );
        }
        "foreign-root" => {
            let foreign = root().join("other-state");
            private_dir(&foreign).unwrap();
            assert!(super::super::super::find_run(
                &foreign,
                string(&record, "name"),
                string(&record, "run_id")
            )
            .unwrap_err()
            .contains("another state root"));
            assert!(!foreign.join(DATABASE).exists());
        }
        "forget" => {
            let second = input(2);
            let second_id = save(&second);
            {
                let _guard = lock(None).unwrap();
                archive::remove_synced(&archive::path(&id).unwrap()).unwrap();
            }
            assert_eq!(lookup(&record).unwrap(), None);
            assert_eq!(lookup(&second).unwrap(), Some(second_id));
            assert!(save(&input(3)).len() > 1);
        }
        _ => panic!("unknown case"),
    }
}
