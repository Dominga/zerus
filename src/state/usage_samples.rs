//! Timestamped readings of a DeepSeek (dsh) session's cumulative token counters.
//!
//! dsh reports only the running total of its session (`projections.tokenUsage`)
//! and keeps no history, so a window's tokens (a team task's attempt, a stage)
//! cannot be read afterwards. Zerus therefore records bounded samples of the
//! counters when it looks at the session (an inspect, the team worker's polls,
//! an attempt's hand-out) and takes a window as the difference of the readings
//! at its two ends, as cost and work are taken from a cost-state. A window
//! whose end has no reading, or whose counters went backwards, is unknown,
//! never zero. Counters only: no text is stored.
use super::*;
use sha2::{Digest, Sha256};
use std::fs;

/// The samples kept per session: about eleven hours of continuous work at one
/// sample per `MIN_GAP`; an unchanged counter keeps only the first and last
/// sample of its run. Past the bound the older half is thinned to every other
/// sample, so a long-ago window loses precision (and turns unknown when no
/// reading is near its ends) instead of the oldest history vanishing at once.
const MAX_SAMPLES: usize = 4000;
/// Two readings of a changing counter are at least this many seconds apart
/// unless the caller forces one (an attempt's hand-out), so a watching window
/// that inspects every second does not fill the list.
const MIN_GAP: f64 = 10.0;
/// A hand-out reading taken more than this many seconds after the hand-out
/// moment is dropped (see `dsh::sample_usage`).
pub(super) const HANDOUT_FRESH: f64 = 10.0;
/// A reading this near a window's end (seconds) stands for it when the counters
/// changed between the two samples around the moment.
const NEAR: f64 = 45.0;
/// The counters kept, in this order.
pub(super) const BUCKETS: [&str; 4] = ["uncached_input", "output", "cache_read", "cache_write"];

pub(super) type Counters = [u64; 4];

#[derive(Clone, Debug, PartialEq)]
struct Sample {
    at: f64,
    conversation: String,
    counters: Counters,
}

fn path(name: &str) -> PathBuf {
    root()
        .join("usage-samples")
        .join(format!("{:x}.json", Sha256::digest(name.as_bytes())))
}

/// The four counters of a dsh `tokenUsage` projection. Input and output must be
/// numbers; a cache bucket dsh does not report counts zero. None when the
/// projection has no usable counters.
pub(super) fn counters(projections: &Value) -> Option<Counters> {
    let totals = usage::buckets(&projections["tokenUsage"], "dsh");
    let get = |key: &str| totals[key].as_u64();
    Some([get("uncached_input")?, get("output")?, get("cache_read").unwrap_or(0), get("cache_write").unwrap_or(0)])
}

fn load(name: &str) -> Vec<Sample> {
    let value: Value = fs::read(path(name))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    value["samples"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sample| {
            let at = sample["at"].as_f64().filter(|at| at.is_finite() && *at > 0.0)?;
            let numbers = sample["t"].as_array()?;
            let mut counters = [0u64; 4];
            if numbers.len() != counters.len() {
                return None;
            }
            for (slot, number) in counters.iter_mut().zip(numbers) {
                *slot = number.as_u64()?;
            }
            Some(Sample { at, conversation: sample["conv"].as_str().unwrap_or_default().to_owned(), counters })
        })
        .collect()
}

fn store(name: &str, samples: &[Sample]) -> Result<()> {
    let rows: Vec<Value> = samples
        .iter()
        .map(|sample| json!({"at":sample.at,"conv":sample.conversation,"t":sample.counters}))
        .collect();
    atomic(&path(name), &json!({"version":1,"name":name,"samples":rows}).to_string())
}

/// Adds a sample to a session's list. A sample equal to the last two extends
/// the last (an unchanged run keeps its first and last moment, which is all a
/// reading across it needs). Returns None when nothing was recorded (the list
/// is untouched, so nothing is written): a sample not after the last, or one
/// closer than `MIN_GAP` to it (unless forced), changed or not. Otherwise
/// whether the counters differ from the previous sample.
fn push(samples: &mut Vec<Sample>, sample: Sample, force: bool) -> Option<bool> {
    let same_run = |a: &Sample, b: &Sample| a.conversation == b.conversation && a.counters == b.counters;
    if samples.last().is_some_and(|last| sample.at <= last.at) {
        return None;
    }
    let changed = !samples.last().is_some_and(|last| same_run(last, &sample));
    if !force && samples.last().is_some_and(|last| sample.at - last.at < MIN_GAP && last.conversation == sample.conversation) {
        return None;
    }
    let len = samples.len();
    if len >= 2 && same_run(&samples[len - 1], &sample) && same_run(&samples[len - 2], &sample) {
        samples[len - 1].at = sample.at;
    } else {
        samples.push(sample);
    }
    if samples.len() > MAX_SAMPLES {
        let older = samples.len() / 2;
        let mut index = 0;
        samples.retain(|_| {
            index += 1;
            index > older || index % 2 == 1
        });
    }
    Some(changed)
}

/// Records the counters `projections` shows for the session `name` at `at`
/// (epoch seconds). Returns whether they changed since the previous sample; a
/// projection without counters records nothing, so the gap stays unknown. The
/// file is written only when a sample was recorded: a session inspected every
/// second must not rewrite its (up to 4000) samples every second.
pub(super) fn record(name: &str, conversation: &str, at: f64, projections: &Value, force: bool) -> Result<bool> {
    let Some(counters) = counters(projections) else { return Ok(false) };
    if name.is_empty() || conversation.is_empty() || !at.is_finite() || at <= 0.0 {
        return Ok(false);
    }
    let _guard = lock(Some(&path(name).with_extension("lock")))?;
    let mut samples = load(name);
    let Some(changed) = push(&mut samples, Sample { at, conversation: conversation.to_owned(), counters }, force) else {
        return Ok(false);
    };
    store(name, &samples)?;
    Ok(changed)
}

/// A session deleted for good (`hgs forget`) takes its samples with it.
pub(super) fn forget(name: &str) {
    let file = path(name);
    let _ = fs::remove_file(&file);
    let _ = fs::remove_file(file.with_extension("lock"));
}

/// A renamed session keeps its samples under its new name (the file is named
/// by the session name's hash).
pub(super) fn rename(old: &str, new: &str) -> Result<()> {
    if old == new {
        return Ok(());
    }
    let _guard = lock(Some(&path(new).with_extension("lock")))?;
    let samples = load(old);
    if samples.is_empty() {
        return Ok(());
    }
    store(new, &samples)?;
    forget(old);
    Ok(())
}

/// A sample file untouched for this long belongs to a session that is gone
/// (or has not been looked at for a month): its windows are old news.
pub(super) const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);
/// Entries one prune pass looks at, so a folder of tens of thousands of files
/// is walked in bounded slices.
pub(super) const PRUNE_SCAN: usize = 400;

/// One bounded pass over the folder: looks at up to `limit` entries from
/// `offset` on and removes the sample and lock files not modified within
/// `max_age`. Returns the offset the next pass continues from (0 after the end
/// of the folder), counting the entries kept.
pub(super) fn prune(max_age: std::time::Duration, offset: usize, limit: usize) -> usize {
    let Some(cutoff) = std::time::SystemTime::now().checked_sub(max_age) else { return 0 };
    let Ok(entries) = fs::read_dir(root().join("usage-samples")) else { return 0 };
    let (mut looked, mut kept) = (0usize, 0usize);
    for entry in entries.flatten().skip(offset).take(limit) {
        looked += 1;
        let file = entry.path();
        let stale = |file: &Path| fs::metadata(file).and_then(|meta| meta.modified()).is_ok_and(|modified| modified < cutoff);
        let remove = match file.extension().and_then(|extension| extension.to_str()) {
            Some("json") => stale(&file),
            // A lock file is never written: it goes with its samples, or alone
            // once they are gone, never while they are in use.
            Some("lock") => !file.with_extension("json").exists() || stale(&file.with_extension("json")),
            _ => false,
        };
        if remove && fs::remove_file(&file).is_ok() {
            continue;
        }
        kept += 1;
    }
    if looked < limit { 0 } else { offset + kept }
}

/// The counters at `moment`: where the samples around it agree, those; else the
/// nearer one if it is within `NEAR` seconds; else no reading.
fn reading(samples: &[&Sample], moment: f64) -> Option<Counters> {
    let before = samples.iter().rev().find(|sample| sample.at <= moment);
    let after = samples.iter().find(|sample| sample.at >= moment);
    match (before, after) {
        (Some(a), Some(b)) if a.counters == b.counters => Some(a.counters),
        (a, b) => {
            let distance = |sample: &&Sample| (sample.at - moment).abs();
            [a, b]
                .into_iter()
                .flatten()
                .min_by(|x, y| distance(x).total_cmp(&distance(y)))
                .filter(|sample| distance(sample) <= NEAR)
                .map(|sample| sample.counters)
        }
    }
}

/// The counters spent inside all `windows` of the session's `conversation`:
/// each window the difference of the readings at its two ends. Err names why
/// the span is unknown.
fn difference(samples: &[Sample], conversation: &str, windows: &[(f64, f64)]) -> std::result::Result<Counters, &'static str> {
    let own: Vec<&Sample> = samples.iter().filter(|sample| sample.conversation == conversation).collect();
    if own.is_empty() {
        return Err("No recorded counters");
    }
    let mut spent = [0u64; 4];
    for (low, high) in windows {
        let (Some(start), Some(end)) = (reading(&own, *low), reading(&own, *high)) else {
            return Err("No counters were recorded at the window's ends");
        };
        for index in 0..spent.len() {
            let Some(part) = end[index].checked_sub(start[index]) else {
                return Err("The counters went backwards inside the window");
            };
            spent[index] = spent[index].saturating_add(part);
        }
    }
    Ok(spent)
}

/// The tokens a DeepSeek session spent inside `windows` (disjoint, ordered),
/// from its recorded samples, in the answer shape of `usage::span_many`.
pub(super) fn span(record: &Value, windows: &[(f64, f64)]) -> Value {
    let name = string(record, "name");
    let conversation = string(record, "conversation_id");
    let (from, to) = (windows.first().map_or(0.0, |w| w.0), windows.last().map_or(0.0, |w| w.1));
    let unknown = |reason: &str| json!({"status":"unknown","scope":"span","source":"dsh","from":from,"to":to,"reason":reason});
    if name.is_empty() || conversation.is_empty() {
        return unknown("Native usage unavailable");
    }
    match difference(&load(name), conversation, windows) {
        Err(reason) => unknown(reason),
        Ok(spent) => {
            let mut tokens = json!({});
            for (key, count) in BUCKETS.iter().zip(spent) {
                tokens[*key] = json!(count);
            }
            let input = spent[0].saturating_add(spent[2]).saturating_add(spent[3]);
            tokens["input"] = json!(input);
            tokens["total"] = json!(input.saturating_add(spent[1]));
            json!({"status":"ok","scope":"span","source":"dsh","from":from,"to":to,"partial":false,
                "providers":[{"provider":"dsh","models":[{"model":Value::Null,"tokens":tokens,"requests":Value::Null}],
                    "cost_usd":Value::Null,"work_ms":Value::Null}]})
        }
    }
}

#[cfg(test)]
#[path = "usage_samples_tests.rs"]
mod tests;
