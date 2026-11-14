use super::*;
use std::fs;
use std::process::{Child, Command};

fn fixture() -> (tempfile::TempDir, PathBuf, Value) {
    let root = tempfile::tempdir().unwrap();
    let scope = scope(root.path()).unwrap();
    let mut record = json!({"version":1,"name":"claude/private/one","agent":"claude","session_id":uuid::Uuid::new_v4().to_string(),"run_id":"original-run","conversation_id":null,"pid":42,"process_start":"private-start","supervisor":{"pid":41,"start":"private-supervisor"}});
    let pin = index::pins::Pin::capture(
        &record,
        &scope.boot,
        Process {
            pid: 42,
            birth: Birth::Linux(100),
        },
        Process {
            pid: 41,
            birth: Birth::Linux(90),
        },
        41,
    )
    .unwrap();
    record[index::pins::FIELD] = serde_json::to_value(pin).unwrap();
    let path = root
        .path()
        .join(format!("{}.json", hash(b"claude/private/one")));
    (root, path, record)
}
fn write(root: &Path, path: &Path, record: &mut Value) -> SourceResult<()> {
    write_at(root, path, record, |record| {
        super::super::storage::atomic(path, &format!("{record}\n"))
    })
}
fn open(root: &Path) -> Store {
    assert!(
        cache_path(root).unwrap().is_file(),
        "the production record adapter must publish the original binding"
    );
    Store::open(
        rusqlite::Connection::open(cache_path(root).unwrap()).unwrap(),
        scope(root).unwrap(),
    )
    .unwrap()
}
#[test]
fn canonical_record_and_projection_share_the_original_publication_witness() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    let mut store = open(root.path());
    let current = store.current_source(&source).unwrap().unwrap();
    assert_eq!(current.publication.anchors.len(), 2);
    assert_eq!(current.publication.edges.len(), 2);
    assert_eq!(
        saved[WITNESS]["witness"]["epoch"],
        store.stamp().unwrap().epoch
    );
    assert!(!store.stamp().unwrap().complete);
    let first = store.stamp().unwrap().high_water;
    record["activity"] = json!("busy");
    write(root.path(), &path, &mut record).unwrap();
    assert_eq!(
        open(root.path()).stamp().unwrap().high_water,
        first,
        "activity-only writes must not grow the intent journal"
    );
    record["conversation_id"] = json!("confirmed-conversation");
    write(root.path(), &path, &mut record).unwrap();
    let current = open(root.path()).current_source(&source).unwrap().unwrap();
    assert!(
        current
            .publication
            .edges
            .iter()
            .all(|e| e.conversation.as_deref() == Some("confirmed-conversation"))
    );
}
#[test]
fn full_index_and_unwritable_gap_marker_never_fail_a_canonical_write() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let db = rusqlite::Connection::open(cache_path(root.path()).unwrap()).unwrap();
    db.execute(
        "UPDATE process_meta SET high_water=?1 WHERE id=1",
        [index::JOURNAL_ROWS],
    )
    .unwrap();
    drop(db);
    let marker = root.path().join("process-index/coverage-lost.json");
    if marker.is_file() {
        fs::remove_file(&marker).unwrap();
    }
    fs::create_dir(&marker).unwrap();
    record["conversation_id"] = json!("new source despite full index");
    write(root.path(), &path, &mut record).unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["conversation_id"], "new source despite full index");
    assert!(
        saved[WITNESS]["witness"]["epoch"].is_null(),
        "bootstrap must detect the unaccounted source even when its marker failed"
    );
    assert!(!open(root.path()).stamp().unwrap().complete);
}
#[test]
fn source_refusal_leaves_the_original_bytes_and_pending_intent() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let before = fs::read(&path).unwrap();
    record["conversation_id"] = json!("unpublished-conversation");
    let result = write_at::<()>(root.path(), &path, &mut record, |_| {
        Err("original canonical refusal".into())
    });
    assert_eq!(result.unwrap_err(), "original canonical refusal");
    assert_eq!(fs::read(&path).unwrap(), before);
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    assert!(open(root.path()).pending_source(&source).unwrap().is_some());
}
#[test]
fn canonical_removal_retires_the_exact_binding_without_replaying_the_file_action() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let mut called = 0;
    remove_at(root.path(), &path, &record, || {
        called += 1;
        fs::remove_file(&path).map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(called, 1);
    assert!(!path.exists());
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    assert_eq!(
        open(root.path())
            .current_source(&source)
            .unwrap()
            .unwrap()
            .publication,
        Publication::default()
    );
}
#[test]
fn original_spawn_captures_the_precise_child_and_correlated_supervisor() {
    struct Owned(Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = Owned(Command::new("sleep").arg("30").spawn().unwrap());
    let identity = crate::platform::process_identity(child.0.id()).unwrap();
    let own = std::process::id();
    let mut record = json!({"name":"claude/private/spawn","agent":"claude","session_id":uuid::Uuid::new_v4().to_string(),"run_id":"owned-run","pid":child.0.id(),"process_start":super::super::storage::process_start(child.0.id()),"supervisor":{"pid":own,"start":super::super::storage::process_start(own)}});
    capture_spawn(&mut record, &identity);
    let pin = index::pins::Pin::read(&record, &system().unwrap().1)
        .unwrap()
        .expect("capture must happen at original spawn");
    assert_eq!(pin.child.pid, child.0.id());
    assert_eq!(pin.supervisor.pid, own);
    #[cfg(target_os = "linux")]
    assert_eq!(
        pin.child.birth,
        Birth::Linux(identity.birth.parse().unwrap())
    );
    #[cfg(target_os = "macos")]
    {
        let (seconds, micros) = identity.birth.split_once(':').unwrap();
        assert_eq!(
            pin.child.birth,
            Birth::Mac {
                seconds: seconds.parse().unwrap(),
                micros: micros.parse().unwrap()
            }
        );
    }
}

#[test]
fn stale_removal_metadata_never_retires_a_newer_run() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let stale = record.clone();
    record["run_id"] = json!("replacement-run");
    let mut pin: index::pins::Pin =
        serde_json::from_value(record[index::pins::FIELD].clone()).unwrap();
    pin.run = "replacement-run".into();
    pin.child.birth = Birth::Linux(200);
    record[index::pins::FIELD] = json!(pin);
    write(root.path(), &path, &mut record).unwrap();
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    let before = open(root.path()).current_source(&source).unwrap().unwrap();
    assert_eq!(
        remove_at(root.path(), &path, &stale, || Ok(
            "unchanged original outcome"
        ))
        .unwrap(),
        "unchanged original outcome"
    );
    assert_eq!(
        open(root.path()).current_source(&source).unwrap().unwrap(),
        before
    );
}

#[test]
fn bootstrap_finds_untracked_deletion_when_the_index_and_gap_marker_both_fail() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let db = rusqlite::Connection::open(cache_path(root.path()).unwrap()).unwrap();
    db.execute(
        "UPDATE process_meta SET high_water=?1 WHERE id=1",
        [index::JOURNAL_ROWS],
    )
    .unwrap();
    drop(db);
    let marker = root.path().join("process-index/coverage-lost.json");
    if marker.is_file() {
        fs::remove_file(&marker).unwrap();
    }
    fs::create_dir(&marker).unwrap();
    remove_at(root.path(), &path, &record, || {
        fs::remove_file(&path).map_err(|e| e.to_string())
    })
    .unwrap();
    let mut background = None;
    let mut gaps = 0;
    for _ in 0..20 {
        let step = background_at(root.path(), &mut background).unwrap();
        gaps += step.gaps;
        if step.done {
            break;
        }
    }
    assert!(
        gaps > 0,
        "a missing canonical witness must reveal the gap even when neither intent nor marker was written"
    );
    assert!(!open(root.path()).stamp().unwrap().complete);
}

#[test]
fn native_bootstrap_publishes_qualified_sources_without_completing_other_families() {
    let (root, path, mut record) = fixture();
    // A new generation must find the original launch pin even if index I/O
    // was unavailable at that source publication. It never re-samples a PID.
    record[WITNESS] = json!({"witness":index::publishing::Witness::lost()});
    super::super::storage::atomic(&path, &format!("{record}\n")).unwrap();
    let mut background = None;
    let mut done = false;
    for _ in 0..20 {
        let step = background_at(root.path(), &mut background).unwrap();
        assert!(step.entries <= 256 && step.bytes <= 2 * 1024 * 1024);
        if step.done {
            done = true;
            break;
        }
    }
    assert!(done);
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    let mut store = open(root.path());
    assert_eq!(
        store
            .current_source(&source)
            .unwrap()
            .unwrap()
            .publication
            .anchors
            .len(),
        2
    );
    assert!(!store.stamp().unwrap().complete);
}

fn finish_background(root: &Path) -> usize {
    let mut state = None;
    let mut gaps = 0;
    for _ in 0..100 {
        let stats = background_at(root, &mut state).unwrap();
        gaps += stats.gaps;
        if stats.done {
            return gaps;
        }
    }
    panic!("private bootstrap failed to finish its bounded pass")
}
#[test]
fn restart_settles_only_an_exact_surviving_write_witness() {
    for cut in [
        "before-source",
        "after-source",
        "after-settle",
        "forged-witness",
    ] {
        let (root, path, mut record) = fixture();
        let mut owner = Owner::open(root.path()).unwrap();
        let pin = index::pins::Pin::read(&record, &owner.scope.boot)
            .unwrap()
            .unwrap();
        let key = Source {
            kind: Kind::Native,
            key: pin.session.clone(),
        };
        let location = location(root.path(), &path, &record).unwrap();
        owner.locate(&key, &location).unwrap();
        let publication = pin.publication(&record, &key, &location, 1, now()).unwrap();
        let change = Change {
            id: uuid::Uuid::new_v4().to_string(),
            source: key.clone(),
            before: 0,
            locator: Some(location.clone()),
            publication,
        };
        let ticket = owner.store.reserve(&change).unwrap();
        if cut != "before-source" {
            witness(
                &mut record,
                index::publishing::Witness {
                    epoch: Some(ticket.epoch.clone()),
                    intent: Some(ticket.id.clone()),
                    seq: Some(ticket.seq),
                    revision: 1,
                },
                &fingerprint(&change.publication, &location).unwrap(),
                &location,
            );
            if cut == "forged-witness" {
                record[WITNESS]["witness"]["intent"] = json!(uuid::Uuid::new_v4().to_string());
            }
            super::super::storage::atomic(&path, &format!("{record}\n")).unwrap();
        }
        if cut == "after-settle" {
            owner.store.settle(&ticket).unwrap();
        }
        drop(owner);
        let before = fs::read(&path).ok();
        finish_background(root.path());
        assert_eq!(
            fs::read(&path).ok(),
            before,
            "bootstrap must never replay canonical publication at {cut}"
        );
        let mut reopened = open(root.path());
        let exact = matches!(cut, "after-source" | "after-settle");
        assert_eq!(
            reopened.pending_source(&key).unwrap().is_none(),
            exact,
            "{cut}"
        );
        assert_eq!(
            reopened.current_source(&key).unwrap().is_some(),
            exact,
            "{cut}"
        );
        assert!(!reopened.stamp().unwrap().complete);
    }
}
#[test]
fn restart_keeps_a_retirement_pending_without_a_surviving_witness() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let mut owner = Owner::open(root.path()).unwrap();
    let key = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    let before = owner.store.current_source(&key).unwrap().unwrap();
    let change = Change {
        id: uuid::Uuid::new_v4().to_string(),
        source: key.clone(),
        before: before.revision,
        locator: Some(location(root.path(), &path, &record).unwrap()),
        publication: Publication::default(),
    };
    owner.store.reserve(&change).unwrap();
    fs::remove_file(&path).unwrap();
    drop(owner);
    assert!(finish_background(root.path()) > 0);
    let mut reopened = open(root.path());
    assert!(reopened.pending_source(&key).unwrap().is_some());
    assert_eq!(reopened.current_source(&key).unwrap(), Some(before));
    assert!(
        !path.exists(),
        "bootstrap cannot reconstruct or replay the deletion"
    );
}
#[test]
fn index_namespace_replacement_preserves_source_success_and_pending_evidence() {
    let (root, path, mut record) = fixture();
    let moved = root.path().join("retained-index");
    let mut calls = 0;
    write_at(root.path(), &path, &mut record, |record| {
        calls += 1;
        fs::rename(root.path().join("process-index"), &moved).map_err(|e| e.to_string())?;
        super::super::storage::atomic(&path, &format!("{record}\n"))
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert!(path.is_file());
    let filename = cache_path(root.path())
        .unwrap()
        .file_name()
        .unwrap()
        .to_owned();
    let db = Connection::open(moved.join(filename)).unwrap();
    let store = Store::open(db, scope(root.path()).unwrap()).unwrap();
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    assert!(
        store.pending_source(&source).unwrap().is_some(),
        "a replaced namespace must not settle into the detached index"
    );
}
#[test]
fn bootstrap_never_upgrades_a_legacy_coarse_pid_record() {
    let (root, path, mut record) = fixture();
    record.as_object_mut().unwrap().remove(index::pins::FIELD);
    super::super::storage::atomic(&path, &format!("{record}\n")).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(finish_background(root.path()) > 0);
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    assert!(open(root.path()).current_source(&source).unwrap().is_none());
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
#[ignore = "explicit private disk-backed write-cost benchmark"]
fn record_write_cost_at_10k_and_100k_sessions() {
    use std::time::Instant;
    let base =
        std::env::var_os("PROCESS_INDEX_BENCH_ROOT").expect("explicit private benchmark directory");
    fn percentiles(mut samples: Vec<i128>) -> Value {
        samples.sort_unstable();
        json!({"samples":samples.len(),"p50_ms":samples[samples.len()/2] as f64/1e6,"p95_ms":samples[samples.len()*95/100] as f64/1e6})
    }
    for count in [10_000u32, 100_000] {
        let directory = tempfile::tempdir_in(&base).unwrap();
        let root = directory.path();
        let init = Instant::now();
        let mut owner = Owner::open(root).unwrap();
        let cold_open_ms = init.elapsed().as_secs_f64() * 1000.;
        let build = owner.store.rebuild().unwrap();
        let mut page = Vec::new();
        let mut bytes = 0;
        let mut selected = None;
        for n in 1..=count {
            let name = format!("claude/private/bench-{n}");
            let mut record = json!({"name":name,"agent":"claude","session_id":uuid::Uuid::from_u128(u128::from(n)).to_string(),"run_id":format!("run-{n}"),"conversation_id":format!("conversation-{n}"),"pid":1000+n*2,"process_start":"private-child","supervisor":{"pid":1001+n*2,"start":"private-supervisor"}});
            let pin = index::pins::Pin::capture(
                &record,
                &owner.scope.boot,
                Process {
                    pid: 1000 + n * 2,
                    birth: Birth::Linux(u64::from(n) + 10),
                },
                Process {
                    pid: 1001 + n * 2,
                    birth: Birth::Linux(u64::from(n) + 9),
                },
                1001 + n * 2,
            )
            .unwrap();
            record[index::pins::FIELD] = json!(pin);
            let path = root.join(format!("{}.json", hash(name.as_bytes())));
            let location = location(root, &path, &record).unwrap();
            let source = Source {
                kind: Kind::Native,
                key: pin.session.clone(),
            };
            let publication = pin
                .publication(&record, &source, &location, 1, now())
                .unwrap();
            witness(
                &mut record,
                index::publishing::Witness {
                    epoch: Some(build.epoch.clone()),
                    intent: None,
                    seq: None,
                    revision: 1,
                },
                &fingerprint(&publication, &location).unwrap(),
                &location,
            );
            fs::write(&path, format!("{record}\n")).unwrap();
            let snapshot = Snapshot {
                source,
                revision: 1,
                publication,
            };
            let size = serde_json::to_vec(&snapshot).unwrap().len();
            if bytes + size > index::PAGE_BYTES || page.len() == index::PAGE_ROWS {
                owner
                    .store
                    .seed(&build, &format!("{n:010}"), &page)
                    .unwrap();
                page.clear();
                bytes = 0;
            }
            bytes += size;
            page.push(snapshot);
            if n == count {
                selected = Some((path, record));
            }
        }
        if !page.is_empty() {
            owner
                .store
                .seed(&build, &format!("{:010}", count + 1), &page)
                .unwrap();
        }
        owner.store.scan_done(&build).unwrap();
        drop(owner);
        let (path, mut record) = selected.unwrap();
        let mut baseline = Vec::new();
        let mut stable = Vec::new();
        let mut changed = Vec::new();
        let mut stable_added = Vec::new();
        let mut changed_added = Vec::new();
        for i in 0..110 {
            record["activity"] = json!(format!("private-{i}"));
            let start = Instant::now();
            super::super::storage::atomic(&path, &format!("{record}\n")).unwrap();
            let base_ns = start.elapsed().as_nanos() as i128;
            if i >= 10 {
                baseline.push(base_ns);
            }
            let start = Instant::now();
            write(root, &path, &mut record).unwrap();
            let stable_ns = start.elapsed().as_nanos() as i128;
            if i >= 10 {
                stable.push(stable_ns);
                stable_added.push(stable_ns - base_ns);
            }
            record["conversation_id"] = json!(format!("updated-{i}"));
            let start = Instant::now();
            write(root, &path, &mut record).unwrap();
            let changed_ns = start.elapsed().as_nanos() as i128;
            if i >= 10 {
                changed.push(changed_ns);
                changed_added.push(changed_ns - base_ns);
            }
        }
        let key = Source {
            kind: Kind::Native,
            key: record["session_id"].as_str().unwrap().into(),
        };
        let mut store = open(root);
        assert!(store.pending_source(&key).unwrap().is_none());
        assert_eq!(
            store
                .current_source(&key)
                .unwrap()
                .unwrap()
                .publication
                .edges[0]
                .conversation
                .as_deref(),
            Some("updated-109")
        );
        println!(
            "PROCESS_WRITER_BENCH {}",
            json!({"sessions":count,"canonical_bytes":fs::metadata(&path).unwrap().len(),"database_bytes":fs::metadata(cache_path(root).unwrap()).unwrap().len(),"cold_owner_open_ms":cold_open_ms,"baseline_canonical_fsync":percentiles(baseline),"stable_binding_write":percentiles(stable),"binding_change_write":percentiles(changed),"stable_added_paired":percentiles(stable_added),"change_added_paired":percentiles(changed_added),"foreground_inventory_reads":0,"inventory_evidence":"source review: no directory inventory API in the writer path","scope":"debug native publisher with canonical source fsync; 10 warmups then 100 samples; native system identity cached after first open; no process proof"})
        );
    }
}

#[test]
fn a_busy_advisory_writer_lock_cannot_refuse_the_canonical_write() {
    let (root, path, mut record) = fixture();
    let owner = Owner::open(root.path()).unwrap();
    let mut calls = 0;
    write_at(root.path(), &path, &mut record, |record| {
        calls += 1;
        super::super::storage::atomic(&path, &format!("{record}\n"))
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert!(path.is_file());
    assert!(record[WITNESS]["witness"]["epoch"].is_null());
    assert_eq!(owner.store.stamp().unwrap().high_water, 0);
}

#[test]
fn a_replaced_background_namespace_discards_the_pass_and_restarts_bounded() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let mut state = None;
    background_at(root.path(), &mut state).unwrap();
    fs::rename(
        root.path().join("process-index"),
        root.path().join("retained-index"),
    )
    .unwrap();
    assert!(background_at(root.path(), &mut state).is_err());
    assert!(
        state.is_none(),
        "a replaced namespace must not leave the worker permanently pinned to its old handles"
    );
    let step = background_at(root.path(), &mut state).unwrap();
    assert!(step.entries <= 256 && step.bytes <= 2 * 1024 * 1024);
    assert!(!open(root.path()).stamp().unwrap().complete);
}

#[test]
fn a_busy_background_gate_preserves_the_existing_bounded_cursor() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let mut state = None;
    background_at(root.path(), &mut state).unwrap();
    let owner = Owner::open(root.path()).unwrap();
    assert!(background_at(root.path(), &mut state).is_err());
    assert!(state.is_some(), "a concurrent writer is a temporary yield");
    drop(owner);
    assert!(background_at(root.path(), &mut state).is_ok());
}

#[test]
fn bootstrap_never_overwrites_a_settled_binding_with_stale_canonical_bytes() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let old_bytes = fs::read(&path).unwrap();
    record["conversation_id"] = json!("newer-confirmed-conversation");
    write(root.path(), &path, &mut record).unwrap();
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    let before = open(root.path()).current_source(&source).unwrap().unwrap();
    fs::write(&path, &old_bytes).unwrap();
    assert!(
        finish_background(root.path()) > 0,
        "an older canonical revision must report an unaccounted gap"
    );
    assert_eq!(
        open(root.path()).current_source(&source).unwrap(),
        Some(before),
        "bootstrap cannot turn older bytes into a newer index revision"
    );
    assert_eq!(fs::read(&path).unwrap(), old_bytes);
}

#[test]
fn matching_binding_with_a_stale_revision_witness_still_reports_a_gap() {
    let (root, path, mut record) = fixture();
    write(root.path(), &path, &mut record).unwrap();
    let source = Source {
        kind: Kind::Native,
        key: record["session_id"].as_str().unwrap().into(),
    };
    let before = open(root.path()).current_source(&source).unwrap().unwrap();
    record[WITNESS]["witness"]["revision"] = json!(0);
    super::super::storage::atomic(&path, &format!("{record}\n")).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(
        finish_background(root.path()) > 0,
        "matching payload is not a matching publication revision"
    );
    assert_eq!(
        open(root.path()).current_source(&source).unwrap(),
        Some(before)
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
