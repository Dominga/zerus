//! Display-only, bounded discovery for unindexed search and its fallback.
//! No canonical inventory, recovery, process proof or mutation uses this view.
use super::*;
use std::collections::VecDeque;

pub(in crate::state) struct Sources {
    pub records: Vec<Value>,
    pub bytes: usize,
    pub entries: usize,
    pub metadata_io: usize,
    pub complete: bool,
}

pub(in crate::state) fn legacy_sources(state: &Path) -> Result<Sources> {
    let budget = Budget {
        files: 500,
        directory_entries: 2000,
        records: 500,
        bytes: 4 << 20,
        deadline: Instant::now() + Duration::from_millis(500),
    };
    discover(state, &budget)
}

// OpenCode has no content adapter yet. Its live namespace cannot disappear
// from a supposedly complete answer. Inspect at most one entry per namespace.
pub(super) fn unsupported_native(state: &Path) -> bool {
    ["opencode/bindings", "opencode/sessions"].iter().any(|name| match fs::read_dir(state.join(name)) {
        Ok(mut entries) => entries.next().is_some(),
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    })
}

fn discover(state: &Path, budget: &Budget) -> Result<Sources> {
    let mut streams = VecDeque::new();
    let mut complete = !unsupported_native(state);
    for directory in [
        "",
        "bindings",
        "sessions",
        "archives",
        "dsh/bindings",
        "dsh/sessions",
    ] {
        match fs::read_dir(state.join(directory)) {
            Ok(entries) => streams.push_back((directory, entries)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(_) => complete = false,
        }
    }
    let mut progress = Progress::default();
    let mut reader = metadata::Reader::foreground(state);
    let mut records = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    // Round-robin prevents a large current directory from hiding archives or
    // native providers. Never collect/sort an entire directory before a bound.
    while let Some((directory, mut entries)) = streams.pop_front() {
        if progress.files >= budget.files
            || progress.directory_entries >= budget.directory_entries
            || Instant::now() >= budget.deadline
        {
            complete = false;
            break;
        }
        let Some(entry) = entries.next() else {
            continue;
        };
        streams.push_back((directory, entries));
        progress.directory_entries += 1;
        let mut path = match entry {
            Ok(entry) => entry.path(),
            Err(_) => {
                complete = false;
                continue;
            }
        };
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let by_id = directory.ends_with("sessions");
        let provider = directory.starts_with("dsh/");
        let archived = directory == "archives";
        if path.extension().is_none_or(|s| s != "json")
            || if by_id {
                !session_id::canonical(&stem)
            } else if archived {
                uuid::Uuid::parse_str(&stem).is_err()
            } else if provider {
                false // Native provider bindings also have non-hash legacy names.
            } else {
                stem.len() != 64
                    || !stem
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }
        {
            continue;
        }
        let label = matches!(directory, "" | "bindings");
        if label {
            let target = label_path(state, &stem);
            if directory == "bindings" && path != target {
                continue; // Unpublished/orphan provider-independent binding.
            }
            path = target;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        let mut record = match reader.read(&path, &mut progress, budget, 0) {
            Ok(metadata::Record::Ready(record)) => record,
            Ok(metadata::Record::Deferred) => {
                complete = false;
                continue;
            }
            Ok(metadata::Record::OverBudget) | Err(_) => {
                complete = false;
                continue;
            }
        };
        if archived && rename::normalize_archive(&mut record).is_err() {
            complete = false;
            continue;
        }
        if label && label_path(state, &stem) != path {
            complete = false;
            continue;
        }
        let name = string(&record, "name");
        let native = provider || record["backend"] == "dsh";
        let valid = if native {
            record["backend"] == "dsh"
                && name.starts_with("dsh/")
                && string(&record, "conversation_id").starts_with("session-")
                && string(&record, "conversation_id").len() <= 2048
        } else {
            record["version"] == 1 && AGENTS.contains(&string(&record, "agent"))
        };
        if !valid
            || name.is_empty()
            || if archived {
                record["archive_id"] != stem
            } else if by_id {
                !storage::is_id_layout_record(&record, &stem)
            } else {
                storage::in_id_layout(&record)
                    || (!native && format!("{:x}", Sha256::digest(name.as_bytes())) != stem)
            }
        {
            complete = false;
            continue;
        }
        if !archived
            && (record["rename_shadow"] == true
                || (native && !string(&record, "archive_id").is_empty()))
        {
            continue;
        }
        if native {
            record["agent"] = json!("dsh");
            record["updated"] = record["created"].clone();
        }
        records.push(record);
    }
    // A crash-left binding and its archive are one search target. This set is
    // bounded by discovery, unlike the previous records × archives scan.
    let archived: std::collections::BTreeSet<_> = records
        .iter()
        .filter(|r| !string(r, "archive_id").is_empty())
        .map(identity)
        .collect();
    records.retain(|r| !string(r, "archive_id").is_empty() || !archived.contains(&identity(r)));
    records.sort_by(|a, b| number(b, "updated").total_cmp(&number(a, "updated")));
    complete &= Instant::now() < budget.deadline;
    Ok(Sources {
        records,
        bytes: progress.bytes,
        metadata_io: progress.metadata_io,
        entries: progress.directory_entries,
        complete,
    })
}

fn number(record: &Value, key: &str) -> f64 {
    record[key].as_f64().filter(|n| n.is_finite()).unwrap_or(0.)
}

fn identity(record: &Value) -> (String, String, String) {
    // Preserve archive::equivalent's Value equality, including absent versus
    // empty identities in old records. Strings alone would collapse them.
    (
        record["name"].to_string(),
        record["run_id"].to_string(),
        record["conversation_id"].to_string(),
    )
}

fn label_path(state: &Path, stem: &str) -> PathBuf {
    let file = format!("{stem}.json");
    if state.join("name_routes").join(&file).exists() {
        state.join("bindings").join(file)
    } else {
        state.join(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget() -> Budget {
        Budget {
            files: 500,
            directory_entries: 2000,
            records: 500,
            bytes: 4 << 20,
            deadline: Instant::now() + Duration::from_secs(2),
        }
    }

    fn save(state: &Path, directory: &str, stem: &str, record: &Value) {
        fs::create_dir_all(state.join(directory)).unwrap();
        fs::write(
            state.join(directory).join(format!("{stem}.json")),
            serde_json::to_vec(record).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn realistic_two_hundred_session_discovery_covers_large_private_histories() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..200 {
            let name = format!("codex/example/search-{n}");
            let key = format!("{:x}", Sha256::digest(name.as_bytes()));
            let record = json!({"version":1,"name":name,"agent":"codex","run_id":format!("run-{n}"),
                "conversation_id":format!("conversation-{n}"),"prompt":"public needle",
                "shell_jobs":{"job":{"output":"private output ".repeat(18_000)}}});
            save(dir.path(), "", &key, &record);
        }
        let mut store = Store::open(&dir.path().join("content-search-v1")).unwrap();
        for _ in 0..128 {
            store
                .discover(
                    dir.path(),
                    &DiscoveryBudget::interactive(),
                    &AtomicBool::new(false),
                )
                .unwrap();
            store
                .consume(
                    dir.path(),
                    &Budget {
                        deadline: Instant::now() + Duration::from_secs(1),
                        ..Budget::interactive()
                    },
                    &AtomicBool::new(false),
                )
                .unwrap();
            if store.catalog_status().unwrap()["pending_paths"] == 0 {
                break;
            }
        }
        let started = Instant::now();
        let sources = legacy_sources(dir.path()).unwrap();
        eprintln!(
            "200-session discovery: {:?}, records={}, bytes={}, complete={}",
            started.elapsed(),
            sources.records.len(),
            sources.bytes,
            sources.complete
        );
        assert!(
            sources.complete,
            "ordinary sized private histories must not hide later sessions"
        );
        assert_eq!(sources.records.len(), 200);
        assert!(sources
            .records
            .iter()
            .all(|r| r.get("shell_jobs").is_none()));
    }

    #[test]
    fn unrelated_entries_are_charged_before_filtering() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..20 {
            fs::write(dir.path().join(format!("unrelated-{n}")), "").unwrap();
        }
        let sources = discover(
            dir.path(),
            &Budget {
                directory_entries: 3,
                ..budget()
            },
        )
        .unwrap();
        assert_eq!(sources.entries, 3);
        assert_eq!(sources.bytes, 0);
        assert!(!sources.complete);
    }

    #[test]
    fn private_raw_io_and_retained_metadata_have_independent_limits() {
        let dir = tempfile::tempdir().unwrap();
        let name = "codex/example/large";
        let key = format!("{:x}", Sha256::digest(name.as_bytes()));
        let record = json!({"version":1,"name":name,"agent":"codex","run_id":"run",
            "conversation_id":"conversation","shell_jobs":{"job":{"output":"x".repeat(4096)}}});
        save(dir.path(), "", &key, &record);
        let small = discover(
            dir.path(),
            &Budget {
                bytes: 1024,
                ..budget()
            },
        )
        .unwrap();
        assert_eq!(small.records.len(), 1);
        assert!(small.bytes < 1024);
        assert!(small.metadata_io > 4096);
        assert!(small.complete);
        let admitted = discover(dir.path(), &budget()).unwrap();
        assert!(admitted.complete);
        assert_eq!(admitted.metadata_io, serde_json::to_vec(&record).unwrap().len());
        assert!(admitted.bytes < 1024);
        assert_eq!(admitted.records.len(), 1);
        assert!(admitted.records[0].get("shell_jobs").is_none());
    }

    #[test]
    fn id_layout_and_archive_identity_are_verified_and_duplicates_collapse() {
        let dir = tempfile::tempdir().unwrap();
        let id = "00000000-0000-4000-8000-000000000001";
        let archive = "00000000-0000-4000-8000-000000000002";
        let record = json!({"version":1,"name":"codex/example/saved","agent":"codex",
            "run_id":"run","conversation_id":"conversation","session_id":id,"layout":"id"});
        save(dir.path(), "sessions", id, &record);
        let sources = discover(dir.path(), &budget()).unwrap();
        assert!(sources.complete);
        assert_eq!(sources.records.len(), 1);
        let mut archived = record.clone();
        archived["archive_id"] = json!(archive);
        save(dir.path(), "archives", archive, &archived);
        let sources = discover(dir.path(), &budget()).unwrap();
        assert!(sources.complete);
        assert_eq!(sources.records.len(), 1);
        assert_eq!(sources.records[0]["archive_id"], archive);
        fs::remove_file(dir.path().join("archives").join(format!("{archive}.json"))).unwrap();
        save(dir.path(), "sessions", archive, &record);
        let sources = discover(dir.path(), &budget()).unwrap();
        assert!(!sources.complete);
        assert_eq!(sources.records.len(), 1);
    }

    #[test]
    fn missing_state_is_complete_but_an_expired_read_is_not() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            discover(&dir.path().join("absent"), &budget())
                .unwrap()
                .complete
        );
        let sources = discover(
            dir.path(),
            &Budget {
                deadline: Instant::now(),
                ..budget()
            },
        )
        .unwrap();
        assert!(!sources.complete);
        assert_eq!(sources.entries, 0);
    }

    #[test]
    fn old_absent_and_empty_run_identities_are_not_equivalent() {
        let dir = tempfile::tempdir().unwrap();
        let name = "codex/example/legacy";
        let stem = format!("{:x}", Sha256::digest(name.as_bytes()));
        let record =
            json!({"version":1,"name":name,"agent":"codex","conversation_id":"conversation"});
        save(dir.path(), "", &stem, &record);
        let id = "00000000-0000-4000-8000-000000000001";
        let mut archived = record;
        archived["archive_id"] = json!(id);
        archived["run_id"] = json!("");
        save(dir.path(), "archives", id, &archived);
        let sources = discover(dir.path(), &budget()).unwrap();
        assert!(sources.complete);
        assert_eq!(sources.records.len(), 2);
    }
    #[test]
    fn native_legacy_names_and_archives_keep_their_metadata_without_host_calls() {
        let dir=tempfile::tempdir().unwrap();
        let record=json!({"backend":"dsh","name":"dsh/example/saved","run_id":"native-run",
            "conversation_id":"session-fixture","created":1});
        save(dir.path(),"dsh/bindings","session-fixture",&record);
        let archive="00000000-0000-4000-8000-000000000001";
        let mut archived=record.clone();archived["archive_id"]=json!(archive);
        save(dir.path(),"archives",archive,&archived);
        let sources=legacy_sources(dir.path()).unwrap();
        assert!(sources.complete);
        assert_eq!(sources.records.len(),1,"an archive and its abandoned native binding are one source");
        assert_eq!(sources.records[0]["agent"],"dsh");
        assert_eq!(sources.records[0]["archive_id"],archive);
    }

}
