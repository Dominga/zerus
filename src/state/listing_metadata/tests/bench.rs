//! Opt-in real-file benchmark. The driver owns and removes the private root.
use super::*;
use std::time::Instant;

fn timed_write(root: &Path, path: &Path, value: &Value, index: bool) -> f64 {
    let started = Instant::now();
    if index {
        let mut calls = 0;
        write_at(root, path, value, |publication| {
            calls += 1;
            storage::atomic_observed(path, &value.to_string(), publication)
        })
        .unwrap();
        assert_eq!(calls, 1, "advisory work replayed a canonical callback");
    } else {
        storage::atomic(path, &value.to_string()).unwrap();
    }
    started.elapsed().as_secs_f64() * 1000.0
}
fn percentiles(mut values: Vec<f64>) -> Value {
    values.sort_by(f64::total_cmp);
    json!({"p50_ms":values[values.len()/2],"p95_ms":values[(values.len()*95/100).min(values.len()-1)]})
}
fn io_bytes() -> u64 {
    fs::read_to_string("/proc/self/io")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|line| {
                line.strip_prefix("rchar: ")
                    .and_then(|v| v.trim().parse().ok())
            })
        })
        .unwrap_or(0)
}

#[test]
#[ignore = "opt-in benchmark: scripts/bench/listing-metadata.py owns its private real-file fixture"]
fn canonical_write_scale() {
    if std::env::var("ZERUS_LISTING_SCALE_BENCH").as_deref() != Ok("1") {
        return;
    }
    let root = PathBuf::from(std::env::var_os("ZERUS_LISTING_SCALE_ROOT").unwrap());
    assert!(root.join(".listing-metadata-scale-fixture").is_file());
    assert!(!root.join("listing-metadata").exists());
    enable(&root);
    let counts: Vec<usize> = std::env::var("ZERUS_LISTING_SCALE_COUNTS")
        .unwrap()
        .split(',')
        .map(|v| v.parse().unwrap())
        .collect();
    assert!(counts.windows(2).all(|v| v[0] < v[1]));
    assert!(counts.last().is_some_and(|n| *n <= 100_000));
    let mut value = record();
    let mut jobs = serde_json::Map::new();
    for n in 0..24 {
        let command = format!("printf synthetic-result-{n}");
        let digest = storage::label_key(&command);
        jobs.insert(format!("job-{n}"),json!({"call_id":format!("job-{n}"),"status":if n<8{"unknown"}else{"completed"},"command":command,"command_digest":digest,"command_truncated":false,
            "output":"synthetic bounded output\n".repeat(2000).chars().take(if n<8{28*1024}else{1024}).collect::<String>(),"updated_at":1794650400+n}));
    }
    value["shell_jobs"] = Value::Object(jobs);
    value["cwd"] = json!("/srv/example/project");
    let mut created = 0;
    let mut reports = Vec::new();
    for count in counts {
        let started = Instant::now();
        while created < count {
            value["name"] = json!(format!(
                "codex/project-{:03}/session-{created:06}",
                created % 100
            ));
            value["session_id"] = json!(uuid::Uuid::new_v4().to_string());
            value["run_id"] = json!(uuid::Uuid::new_v4().to_string());
            value["conversation_id"] = json!(uuid::Uuid::new_v4().to_string());
            value["journal_name"] = json!(format!("session:{}", string(&value, "session_id")));
            value["created"] = json!(1794650400 + created);
            value["updated"] = json!(1794650400 + created);
            value["cwd"] = json!(format!("/srv/example/project-{:03}", created % 100));
            value["launch_dir"] = value["cwd"].clone();
            value["base"] = json!(["codex"]);
            let path = root.join(format!(
                "{}.json",
                storage::label_key(string(&value, "name"))
            ));
            // Each cardinality has a genuine cold-index migration. Publication
            // latency is measured separately against these real durable files.
            timed_write(&root, &path, &value, false);
            created += 1;
            if created % 5000 == 0 {
                println!("listing metadata scale: {created} canonical files");
            }
        }
        let population_seconds = started.elapsed().as_secs_f64();
        let cli = if let (Ok(binary), Ok(clock)) = (
            std::env::var("ZERUS_LISTING_BENCH_BIN"),
            std::env::var("ZERUS_LISTING_BENCH_CLOCK"),
        ) {
            let output = root.join(format!("cli-{count}.json"));
            let status = std::process::Command::new("python3")
                .args(["scripts/bench/listing-reader-stage.py", "--root"])
                .arg(&root)
                .args([
                    "--hgs",
                    &binary,
                    "--clock",
                    &clock,
                    "--count",
                    &count.to_string(),
                    "--output",
                ])
                .arg(&output)
                .status()
                .unwrap();
            assert!(status.success(), "private ordinary-ls stage failed");
            serde_json::from_slice::<Value>(&fs::read(output).unwrap()).unwrap()
        } else {
            Value::Null
        };
        let first_repair = benchmark_repair(&root);
        let mut baseline = Vec::new();
        let mut indexed = Vec::new();
        let mut added = Vec::new();
        let path = root.join(format!(
            "{}.json",
            storage::label_key(string(&value, "name"))
        ));
        for n in 0..200 {
            value["prompt"] = json!(format!("Synthetic public prompt {n}"));
            let a = timed_write(&root, &path, &value, false);
            let b = timed_write(&root, &path, &value, true);
            baseline.push(a);
            indexed.push(b);
            added.push(b - a);
        }
        let repair = benchmark_repair(&root);
        let resident = resident_samples(&root, &path, &mut value, false);
        let pinned_resident = resident_samples(&root, &path, &mut value, true);
        let settled = benchmark_repair(&root);
        let listing_started = Instant::now();
        let read_before = io_bytes();
        let reader = pages::Reader::open(&root).unwrap();
        let mut cursor = None;
        let mut rows = 0;
        let mut pages = 0;
        loop {
            let page = reader.page(cursor.as_ref()).unwrap();
            assert!(page.fresh);
            rows += page.rows.len();
            pages += 1;
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(rows, count);
        let listing_seconds = listing_started.elapsed().as_secs_f64();
        let listing_rchar = io_bytes().saturating_sub(read_before);
        assert!(
            listing_rchar < (count as u64) * 16384 + (16 << 20),
            "metadata paging read canonical bodies"
        );
        let files: std::collections::BTreeMap<_, _> =
            ["index.sqlite3", "index.sqlite3-wal", "index.sqlite3-shm"]
                .into_iter()
                .map(|name| {
                    (
                        name,
                        fs::metadata(root.join("listing-metadata").join(name))
                            .map(|m| m.len())
                            .unwrap_or(0),
                    )
                })
                .collect();
        reports.push(json!({"sessions":count,"record_bytes":serde_json::to_vec(&value).unwrap().len(),"population_seconds":population_seconds,"samples":200,
            "ordinary_ls":cli,"resident_worker":resident,"resident_worker_pinned_reader":pinned_resident,"settled_repair":settled,"canonical_only":percentiles(baseline),"canonical_with_index":percentiles(indexed),"paired_added":percentiles(added),
            "index_files_bytes":files,"initial_repair":first_repair,"post_write_repair":repair,"metadata_pages":pages,"metadata_page_seconds":listing_seconds,"metadata_page_rchar":listing_rchar}));
    }
    let rss = peak_rss();
    assert!(
        rss == 0 || rss <= 128 << 20,
        "display metadata benchmark exceeded its RSS bound"
    );
    let result = json!({"version":1,"peak_rss_bytes":rss,"profile":"unoptimized unit-test owner; actual fsync/rename/SQLite FULL durability","results":reports});
    fs::write(root.join("result.json"), format!("{result}\n")).unwrap();
    println!("LISTING_METADATA_SCALE_RESULT {result}");
}

fn benchmark_repair(root: &Path) -> Value {
    let started = Instant::now();
    let mut work = None;
    let mut bytes = 0;
    let mut steps = 0;
    loop {
        let step = background_at(root, &mut work).unwrap();
        bytes += step.bytes;
        steps += 1;
        assert!(step.bytes <= 16 << 20 && step.entries <= 256 && step.rows <= 128);
        if step.done {
            assert!(step.complete);
            break;
        }
        assert!(
            started.elapsed().as_secs() < 1200,
            "private benchmark bootstrap did not finish"
        );
    }
    json!({"seconds":started.elapsed().as_secs_f64(),"canonical_bytes_read":bytes,"steps":steps})
}
fn peak_rss() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find_map(|line| {
                line.strip_prefix("VmHWM:")
                    .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            })
        })
        .unwrap_or(0)
        * 1024
}

fn resident_samples(root: &Path, selected: &Path, value: &mut Value, pinned: bool) -> Value {
    use std::process::{Command, Stdio};
    let Ok(binary) = std::env::var("ZERUS_LISTING_BENCH_BIN") else {
        return Value::Null;
    };
    benchmark_repair(root);
    let directory = checked(Directory::open(root))
        .unwrap()
        .child("listing-metadata")
        .unwrap();
    let _ = fs::remove_file(directory.path.join("worker-status.json"));
    let home = root.join("benchmark-home");
    let mut command = Command::new(binary);
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &home)
        .env("HGS_STATE_DIR", root)
        .env("HGS_CONFIG_DIR", home.join("config"))
        .env("TMUX_TMPDIR", home.join("tmux"))
        .env("XDG_CONFIG_HOME", home.join("xdg-config"))
        .env("XDG_STATE_HOME", home.join("xdg-state"))
        .env("XDG_DATA_HOME", home.join("xdg-data"))
        .args(["__state", "listing-metadata-worker"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut worker = command.spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(600);
    loop {
        assert!(
            worker.try_wait().unwrap().is_none(),
            "private resident worker exited early"
        );
        let state = fs::read(directory.path.join("worker-status.json"))
            .ok()
            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
        if state.is_some_and(|state| state["state"] == "ready") {
            break;
        }
        assert!(Instant::now() < until, "resident census did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
    let pin = pinned.then(|| pages::Reader::open(root).unwrap());
    if let Some(reader) = &pin {
        assert!(reader.page(None).unwrap().fresh);
    }
    let gap_before = fs::metadata(directory.path.join("coverage-lost.json"))
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    let mut plain = Vec::new();
    let mut indexed = Vec::new();
    let mut added = Vec::new();
    let mut wal = 0;
    for n in 0..200 {
        value["prompt"] = json!(format!("Resident benchmark {pinned} {n}"));
        let a = timed_write(root, selected, value, false);
        let b = timed_write(root, selected, value, true);
        plain.push(a);
        indexed.push(b);
        added.push(b - a);
        wal = wal.max(
            fs::metadata(directory.path.join("index.sqlite3-wal"))
                .map(|metadata| metadata.len())
                .unwrap_or(0),
        );
    }
    assert_eq!(
        &serde_json::from_slice::<Value>(&fs::read(selected).unwrap()).unwrap(),
        value,
        "resident work changed the canonical result"
    );
    assert!(
        wal <= 128 << 20,
        "resident/pinned-reader WAL exceeded its bounded stress ceiling"
    );
    let worker_status = fs::read_to_string(format!("/proc/{}/status", worker.id())).unwrap();
    let rss = worker_status
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")
                .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
        })
        .unwrap()
        * 1024;
    let gap_after = fs::metadata(directory.path.join("coverage-lost.json"))
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    drop(pin);
    directory.replace_file("disabled", b"1\n").unwrap();
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = worker.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(
            Instant::now() < until,
            "private worker did not release its native stream after disable"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    fs::remove_file(directory.path.join("disabled")).unwrap();
    json!({"samples":200,"pinned_reader":pinned,"canonical_only":percentiles(plain),"canonical_with_index":percentiles(indexed),"paired_added":percentiles(added),"peak_wal_bytes":wal,"worker_peak_rss_bytes":rss,"coverage_gap_during_writes":gap_before!=gap_after,"canonical_callbacks_per_indexed_write":1,"last_canonical_body_unchanged":true})
}
