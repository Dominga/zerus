//! Opt-in real-file benchmark. The driver owns and removes the private root.
use super::*;
use std::time::Instant;

fn timed_write(root: &Path, path: &Path, value: &Value, index: bool) -> f64 {
    let started = Instant::now();
    if index {
        write_at(root, path, value, || {
            storage::atomic(path, &value.to_string())
        })
        .unwrap();
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
            let path = root.join(format!(
                "{}.json",
                storage::label_key(string(&value, "name"))
            ));
            // The first cardinality is a genuine older-store migration;
            // subsequent rows exercise the instrumented canonical publisher.
            timed_write(&root, &path, &value, !reports.is_empty());
            created += 1;
            if created % 5000 == 0 {
                println!("listing metadata scale: {created} canonical files");
            }
        }
        let population_seconds = started.elapsed().as_secs_f64();
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
        let listing_started = Instant::now();
        let read_before = io_bytes();
        let mut cursor = None;
        let mut rows = 0;
        let mut pages = 0;
        loop {
            let page = Store::open(&root).unwrap().page(cursor.as_ref()).unwrap();
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
            "canonical_only":percentiles(baseline),"canonical_with_index":percentiles(indexed),"paired_added":percentiles(added),
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
