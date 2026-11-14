//! Explicit synthetic index benchmark; never opens the configured Zerus state.
#[path = "../src/state/process_index/mod.rs"]
mod process_index;
#[cfg(not(test))]
fn main() -> anyhow::Result<()> {
    use anyhow::ensure;
    use process_index::*;
    use std::time::{Duration, Instant};
    fn process(pid: u32) -> Process {
        Process {
            pid,
            birth: Birth::Linux(u64::from(pid) + 100),
        }
    }
    for count in [10_000u32, 100_000] {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("synthetic.sqlite3");
        let db = rusqlite::Connection::open(&path)?;
        let mut store = Store::create(
            db,
            Scope {
                machine: "benchmark-machine".into(),
                configuration: "private-benchmark".into(),
                boot: "synthetic-boot".into(),
            },
        )?;
        let build = store.rebuild()?;
        let start = Instant::now();
        for first in (1..=count).step_by(32) {
            let mut rows = Vec::new();
            let last = (first + 31).min(count);
            for n in first..=last {
                let src = Source {
                    kind: Kind::Native,
                    key: format!("source-{n:08}"),
                };
                let p = process(n);
                let pubn = Publication {
                    anchors: vec![Anchor {
                        process: p.clone(),
                        role: Role::Agent,
                        source: src.clone(),
                        revision: 1,
                        fingerprint: "a".repeat(64),
                        published_at: 1,
                    }],
                    edges: vec![Edge {
                        process: p,
                        role: Role::Agent,
                        session_id: uuid::Uuid::from_u128(u128::from(n)).to_string(),
                        run: format!("run-{n}"),
                        conversation: Some(format!("conversation-{n}")),
                        source: src.clone(),
                        revision: 1,
                        fingerprint: "a".repeat(64),
                        published_at: 1,
                    }],
                };
                rows.push(Snapshot {
                    source: src,
                    revision: 1,
                    publication: pubn,
                });
            }
            store.seed(&build, &format!("{last:010}"), &rows)?;
        }
        store.scan_done(&build)?;
        store.finish(&build, build.high_water)?;
        let seed_ms = start.elapsed().as_secs_f64() * 1000.;
        let mut chain = (1..64)
            .map(|n| process(count + 1000 + n))
            .collect::<Vec<_>>();
        chain.push(process(count));
        let mut samples = Vec::new();
        for i in 0..210 {
            let start = Instant::now();
            let answer = store.lookup(&chain, Instant::now() + Duration::from_secs(2))?;
            let nanos = start.elapsed().as_nanos() as u64;
            ensure!(
                matches!(answer, Lookup::Candidate { lookups: 64, .. }),
                "Expected exact synthetic 64th ancestor"
            );
            let work = store.work();
            ensure!(
                work.lookups == 64
                    && work.anchor_rows == 1
                    && work.source_reads == 1
                    && work.anchor_fullscan_steps == 0,
                "Unexpected index work"
            );
            if i >= 10 {
                samples.push(nanos);
            }
        }
        samples.sort_unstable();
        let w = store.work();
        println!(
            "{}",
            serde_json::json!({"sessions":count,"samples":samples.len(),"ancestors":64,"p50_ms":samples[samples.len()/2] as f64/1e6,"p95_ms":samples[(samples.len()*95/100).min(samples.len()-1)] as f64/1e6,"anchor_lookups":w.lookups,"anchor_rows_returned":w.anchor_rows,"source_metadata_point_reads":w.source_reads,"source_filesystem_reads":0,"anchor_vm_steps":w.anchor_vm_steps,"anchor_fullscan_steps":w.anchor_fullscan_steps,"seed_ms":seed_ms,"database_bytes":std::fs::metadata(&path)?.len(),"scope":"synthetic index data only; no native caller proof"})
        );
    }
    Ok(())
}
#[cfg(test)]
fn main() {}
