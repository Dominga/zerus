//! Search parity harness over a lore checkout: the same query set as
//! `tools/lore_bench.py`, scoring hit@1 and hit@5 per query class on the text
//! side. It never writes the corpus and prints only numbers.
use serde_json::Value;
use std::path::{Path, PathBuf};
use zerus_lore::index::Index;
use zerus_lore::parse;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).expect("usage: parity <root> <queries.jsonl>"));
    let queries = PathBuf::from(args.get(2).expect("usage: parity <root> <queries.jsonl>"));
    let cache = std::env::temp_dir().join("lore-parity-index.sqlite");
    let _ = std::fs::remove_file(&cache);
    let mut index = Index::open(&cache).expect("index");
    let mut records = Vec::new();
    for entry in std::fs::read_dir(root.join("lore")).expect("lore dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let bytes = std::fs::read(&path).expect("read");
            records.extend(parse(&bytes, &path.display().to_string()).expect("parse"));
        }
    }
    let plans = root.join("lore/plans");
    if let Ok(entries) = std::fs::read_dir(&plans) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                let bytes = std::fs::read(&path).expect("read");
                records.extend(parse(&bytes, &path.display().to_string()).expect("parse"));
            }
        }
    }
    index.sync(&records).expect("sync");
    let by_id: std::collections::BTreeMap<String, String> = records.iter()
        .filter_map(|record| Some((record.get("id")?.as_str()?.to_owned(), record.to_string())))
        .collect();
    let text = std::fs::read_to_string(&queries).expect("queries");
    let mut stats: std::collections::BTreeMap<String, (usize, usize, usize)> = Default::default();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let row: Value = serde_json::from_str(line).expect("query");
        let keys: Vec<&str> = row.get("keys").and_then(Value::as_array).into_iter().flatten()
            .filter_map(Value::as_str).collect();
        for kind in ["exact", "para"] {
            let Some(query) = row.get(kind).and_then(Value::as_str) else { continue };
            let (hits, _) = index.search_all(query, 10).expect("search");
            if std::env::var("PARITY_DEBUG").is_ok() {
                let rank = hits.iter().position(|hit| by_id.get(&hit.id).is_some_and(|text| keys.iter().any(|key| text.contains(key)))).map_or(0, |index| index + 1);
                eprintln!("RANK {kind} {rank} {query}");
            }
            let at1 = hits.first().is_some_and(|hit| by_id.get(&hit.id).is_some_and(|text| keys.iter().any(|key| text.contains(key))));
            let at5 = hits.iter().take(5).any(|hit| by_id.get(&hit.id).is_some_and(|text| keys.iter().any(|key| text.contains(key))));
            let entry = stats.entry(kind.to_owned()).or_default();
            entry.0 += at1 as usize;
            entry.1 += at5 as usize;
            entry.2 += 1;
        }
    }
    for (kind, (at1, at5, total)) in stats {
        println!("{kind}: hit@1 {:.3} ({at1}/{total}), hit@5 {:.3} ({at5}/{total})",
            at1 as f64 / total as f64, at5 as f64 / total as f64);
    }
    let _ = Path::new(&root);
}




