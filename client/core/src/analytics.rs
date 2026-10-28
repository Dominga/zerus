//! Private analytics: a port of tray/src/AnalyticsData.{h,cpp} and
//! AnalyticsClient.cpp. Summaries (`hgs [@host] analytics --json`) and
//! speech statistics (`hgs [@host] speech --json`) pass through a
//! deliberately closed projection: transport metadata and arbitrary provider
//! fields never become display text or diagnostic attachments. [`page`]
//! holds the Settings page's state (AnalyticsSettings.cpp). Every read is
//! run and parsed on the thread that asked for it, never the main thread.

pub mod page;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::actions::{Exit, Invocation};

/// The largest reply read.
pub const MAX_BYTES: usize = 1024 * 1024;
/// The most daily buckets one summary may hold.
pub const MAX_BUCKETS: usize = 1080;
/// The most computers one view reads and counts.
pub const MAX_COMPUTERS: usize = 32;
/// Daily buckets per page.
pub const PAGE_SIZE: usize = 50;
/// The most a `--help` probe may print.
const HELP_BYTES: usize = 128 * 1024;
/// How long a probe or a call may run.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Exact JSON integers.
const MAX_EXACT: u64 = 9_007_199_254_740_991;
const MAX_INTEGER: f64 = MAX_EXACT as f64;

pub const PROVIDERS: [&str; 6] = ["codex", "claude", "kimi", "dsh", "opencode", "other"];
pub const COUNTERS: [&str; 27] = [
    "turn_started",
    "turn_completed",
    "turn_interrupted",
    "work_ms",
    "approval_wait_ms",
    "input_wait_ms",
    "approval_requested",
    "approval_resolved",
    "input_requested",
    "input_resolved",
    "delivery_submitted",
    "delivery_failed",
    "delivery_uncertain",
    "recovery_attempt",
    "recovery_succeeded",
    "recovery_exhausted",
    "failure_authentication",
    "failure_rate_limit",
    "failure_quota",
    "failure_policy",
    "failure_network",
    "failure_service",
    "failure_unknown",
    "input_tokens",
    "output_tokens",
    "cache_tokens",
    "reasoning_tokens",
];
pub const SPEECH_COUNTERS: [&str; 8] = [
    "submitted",
    "completed",
    "failed",
    "cancelled",
    "uncertain",
    "audio_ms",
    "queue_ms",
    "processing_ms",
];
const HISTOGRAMS: [&str; 3] = ["work_ms", "approval_wait_ms", "input_wait_ms"];
const BOUNDS: [f64; 5] = [100.0, 1000.0, 10000.0, 60000.0, 600000.0];
const GAPS: [&str; 8] = [
    "collection_disabled",
    "history_purged",
    "retention_boundary",
    "quota_exhausted",
    "observation_gap",
    "clock_discontinuity",
    "missing_identity",
    "unsupported_provider",
];
const MEASURES: [&str; 10] = [
    "turns",
    "work_ms",
    "approval_wait_ms",
    "input_wait_ms",
    "delivery",
    "recovery",
    "failures",
    "tokens",
    "cost",
    "background_jobs",
];
/// The speech error categories that leave a read; any other is `unknown`.
const SPEECH_ERRORS: [&str; 9] = [
    "http_authentication",
    "http_rate_limited",
    "http_status",
    "http_response_invalid",
    "http_address_blocked",
    "http_timeout",
    "http_connection",
    "inference_failed",
    "service_restarted",
];

/// A UTC calendar day: days since 1970-01-01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(i64);

impl Day {
    fn from_ymd(year: i64, month: i64, day: i64) -> Option<Day> {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        if !(1..=days).contains(&day) {
            return None;
        }
        // Days from the civil date (proleptic Gregorian, as QDate counts).
        let y = if month <= 2 { year - 1 } else { year };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (month + 9) % 12;
        let doy = (153 * mp + 2) / 5 + day - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Some(Day(era * 146_097 + doe - 719_468))
    }

    /// `QDate::fromString(text, Qt::ISODate)` as Qt 6.12 parses it: four
    /// digits, punctuation, two digits, punctuation, two digits, then the end
    /// or anything but a digit; year 1 to 9999. Only ASCII punctuation counts
    /// here, where Qt also takes other Unicode punctuation: stricter, and no
    /// `hgs` writes another.
    pub fn parse_qt(text: &str) -> Option<Day> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let punct = |unit: u16| {
            u8::try_from(unit).is_ok_and(|byte| b"!\"#%&'()*,-./:;?@[\\]_{}".contains(&byte))
        };
        let digit_after = units
            .get(10)
            .is_some_and(|&unit| char::from_u32(u32::from(unit)).is_some_and(char::is_numeric));
        if units.len() < 10 || !punct(units[4]) || !punct(units[7]) || digit_after {
            return None;
        }
        let read = |range: std::ops::Range<usize>| {
            units[range].iter().try_fold(0i64, |value, &unit| {
                let byte = u8::try_from(unit).ok().filter(u8::is_ascii_digit)?;
                Some(value * 10 + i64::from(byte - b'0'))
            })
        };
        let year = read(0..4).filter(|year| (1..=9999).contains(year))?;
        Day::from_ymd(year, read(5..7)?, read(8..10)?)
    }

    /// Only the `yyyy-MM-dd` spelling QDate writes.
    pub fn parse(text: &str) -> Option<Day> {
        Day::parse_qt(text).filter(|day| day.iso() == text)
    }

    /// The UTC day of a time in milliseconds since the epoch.
    pub fn of_ms(ms: i64) -> Day {
        Day(ms.div_euclid(86_400_000))
    }

    /// QDate::addDays.
    pub fn plus(self, days: i64) -> Day {
        Day(self.0 + days)
    }

    /// QDate::toString(Qt::ISODate).
    pub fn iso(self) -> String {
        let z = self.0 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!("{year:04}-{month:02}-{day:02}")
    }
}

/// An inclusive UTC range of at most 90 days.
pub fn valid_range(from: Day, through: Day) -> bool {
    from <= through && through.0 - from.0 < 90
}

/// An exact nonnegative JSON integer.
fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= MAX_INTEGER && n.floor() == *n)
}

/// Seconds since the epoch before the year 10000.
fn timestamp(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && *n >= 0.0 && *n < 253_402_300_800.0)
}

/// QJsonValue's numeric equality.
fn equals(value: &Value, n: f64) -> bool {
    value.as_f64() == Some(n)
}

/// A UUID as QUuid writes it without braces; the nil UUID only when allowed.
fn uuid(value: &Value, allow_nil: bool) -> bool {
    let Some(bytes) = value.as_str().map(str::as_bytes) else {
        return false;
    };
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
            }
        })
        && (allow_nil || bytes.iter().any(|byte| !matches!(byte, b'0' | b'-')))
}

/// A reply object of schema 1, read only up to [`MAX_BYTES`].
fn object(bytes: &[u8]) -> Option<Map<String, Value>> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    match serde_json::from_slice::<Value>(bytes).ok()? {
        Value::Object(object) if equals(get(&object, "schema"), 1.0) => Some(object),
        _ => None,
    }
}

/// A list of at most 32 codes; one outside `allowed` reads `unknown`, and
/// each appears once.
fn codes(value: &Value, allowed: &[&str]) -> Option<Vec<String>> {
    let list = value.as_array().filter(|list| list.len() <= 32)?;
    let mut result: Vec<String> = Vec::new();
    for code in list {
        let code = code.as_str()?;
        let safe = if allowed.contains(&code) {
            code
        } else {
            "unknown"
        };
        if !result.iter().any(|kept| kept == safe) {
            result.push(safe.to_owned());
        }
    }
    Some(result)
}

/// Adds `value` to `into[key]` unless the sum passes [`MAX_INTEGER`]; a
/// duration may have a fraction.
fn add(into: &mut BTreeMap<String, f64>, key: &str, value: &Value, duration: bool) -> bool {
    let value = if duration {
        value
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0 && *n <= MAX_INTEGER)
    } else {
        number(value)
    };
    let Some(value) = value else {
        return false;
    };
    let sum = into.get(key).copied().unwrap_or(0.0) + value;
    if sum > MAX_INTEGER {
        return false;
    }
    into.insert(key.to_owned(), sum);
    true
}

/// A daily histogram: six counts for the bounds 100 ms, 1 s, 10 s, 1 min
/// and 10 min, their count and the sum of their samples (ms).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Histogram {
    pub counts: [u64; 6],
    pub count: u64,
    pub sum: u64,
}

fn added(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b).filter(|total| *total <= MAX_EXACT)
}

impl Histogram {
    fn read(value: &Value) -> Option<Histogram> {
        let raw = value.as_object()?;
        let bounds = raw.get("bounds")?.as_array()?;
        let counts = raw.get("counts")?.as_array()?;
        if !raw.get("version").is_some_and(|v| equals(v, 1.0))
            || bounds.len() != BOUNDS.len()
            || bounds
                .iter()
                .zip(BOUNDS)
                .any(|(bound, n)| !equals(bound, n))
            || counts.len() != 6
        {
            return None;
        }
        let reported = number(raw.get("count")?)? as u64;
        let sum = number(raw.get("sum")?)? as u64;
        let mut kept = [0u64; 6];
        let mut count = 0u64;
        for (slot, n) in kept.iter_mut().zip(counts) {
            *slot = number(n)? as u64;
            count = added(count, *slot)?;
        }
        (count == reported && (count > 0 || sum == 0)).then_some(Histogram {
            counts: kept,
            count,
            sum,
        })
    }

    /// Both merged, or None when any part would pass the exact range.
    fn merged(&self, other: &Histogram) -> Option<Histogram> {
        let mut counts = [0u64; 6];
        for (index, slot) in counts.iter_mut().enumerate() {
            *slot = added(self.counts[index], other.counts[index])?;
        }
        Some(Histogram {
            counts,
            count: added(self.count, other.count)?,
            sum: added(self.sum, other.sum)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Bucket {
    /// The UTC day, `yyyy-MM-dd`.
    pub date: String,
    pub provider: String,
    /// `main` or `child`.
    pub producer: String,
    /// The counters reported, from [`COUNTERS`]; an absent one is unknown.
    pub counters: BTreeMap<String, f64>,
    pub histograms: BTreeMap<String, Histogram>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProviderCoverage {
    pub supported: Vec<String>,
    pub unsupported: Vec<String>,
    pub partial: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Coverage {
    pub start: Option<f64>,
    pub end: Option<f64>,
    pub gaps: Vec<String>,
    pub unsupported: Vec<String>,
    pub partial: bool,
    pub providers: BTreeMap<String, ProviderCoverage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Waits {
    pub observed: f64,
    pub complete: bool,
}

/// One computer's checked summary.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub machine_id: String,
    pub source_epoch: String,
    pub revision: f64,
    pub generated_at: f64,
    pub from: Day,
    pub through: Day,
    pub enabled: bool,
    pub retention_days: i64,
    pub coverage: Coverage,
    pub buckets: Vec<Bucket>,
    pub unresolved_waits: Option<Waits>,
}

/// A field, null when absent.
fn get<'a>(object: &'a Map<String, Value>, key: &str) -> &'a Value {
    object.get(key).unwrap_or(&Value::Null)
}

/// AnalyticsData::summary: the reply for exactly `from`..`through`, or
/// None when anything in it is out of shape.
pub fn summary(bytes: &[u8], from: Day, through: Day) -> Option<Summary> {
    if !valid_range(from, through) {
        return None;
    }
    let raw = object(bytes)?;
    let empty = Map::new();
    let section = |key: &str| raw.get(key).and_then(Value::as_object).unwrap_or(&empty);
    let (range, collection, coverage) =
        (section("range"), section("collection"), section("coverage"));
    let retention =
        number(get(collection, "retention_days")).filter(|days| (1.0..=90.0).contains(days));
    let buckets = raw.get("buckets").and_then(Value::as_array);
    if !uuid(get(&raw, "machine_id"), false)
        || !uuid(get(&raw, "source_epoch"), true)
        || get(&raw, "source") != "agent"
        || number(get(&raw, "revision")).is_none()
        || timestamp(get(&raw, "generated_at")).is_none()
        || get(range, "from").as_str() != Some(&from.iso())
        || get(range, "through").as_str() != Some(&through.iso())
        || !get(collection, "enabled").is_boolean()
        || !equals(get(collection, "event_retention_days"), 14.0)
        || retention.is_none()
        || !get(coverage, "partial").is_boolean()
        || buckets.is_none_or(|buckets| buckets.len() > MAX_BUCKETS)
    {
        return None;
    }
    let mut ends = [None; 2];
    for (slot, key) in ends.iter_mut().zip(["start", "end"]) {
        let value = coverage.get(key)?;
        if !value.is_null() {
            *slot = Some(timestamp(value)?);
        }
    }
    if let [Some(start), Some(end)] = ends {
        if start > end {
            return None;
        }
    }
    let gaps = codes(get(coverage, "gaps"), &GAPS)?;
    let unsupported = codes(get(coverage, "unsupported"), &MEASURES)?;
    let mut providers = BTreeMap::new();
    let reported = get(coverage, "providers");
    for provider in PROVIDERS {
        let entry = &reported[provider];
        let (Some(supported), Some(missing), Some(partial)) = (
            codes(&entry["supported"], &MEASURES),
            codes(&entry["unsupported"], &MEASURES),
            entry["partial"].as_bool(),
        ) else {
            continue;
        };
        providers.insert(
            provider.to_owned(),
            ProviderCoverage {
                supported,
                unsupported: missing,
                partial,
            },
        );
    }
    let mut kept_buckets = Vec::new();
    let mut keys = HashSet::new();
    for row in buckets.into_iter().flatten() {
        let text = row["date"].as_str().unwrap_or_default();
        let date = Day::parse(text).filter(|day| *day >= from && *day <= through)?;
        let provider = row["provider"].as_str().unwrap_or_default();
        let producer = row["producer"].as_str().unwrap_or_default();
        let counters = row["counters"].as_object()?;
        if !PROVIDERS.contains(&provider)
            || !matches!(producer, "main" | "child")
            || !keys.insert((date, provider, producer))
        {
            return None;
        }
        let mut kept = BTreeMap::new();
        for name in COUNTERS {
            if let Some(count) = counters.get(name) {
                kept.insert(name.to_owned(), number(count)?);
            }
        }
        let mut histograms = BTreeMap::new();
        if let Some(reported) = row["histograms"].as_object() {
            for name in HISTOGRAMS {
                if let Some(histogram) = reported.get(name) {
                    histograms.insert(name.to_owned(), Histogram::read(histogram)?);
                }
            }
        }
        kept_buckets.push(Bucket {
            date: date.iso(),
            provider: provider.to_owned(),
            producer: producer.to_owned(),
            counters: kept,
            histograms,
        });
    }
    let waits = get(&raw, "unresolved_waits");
    Some(Summary {
        machine_id: get(&raw, "machine_id").as_str()?.to_owned(),
        source_epoch: get(&raw, "source_epoch").as_str()?.to_owned(),
        revision: number(get(&raw, "revision"))?,
        generated_at: timestamp(get(&raw, "generated_at"))?,
        from,
        through,
        enabled: get(collection, "enabled").as_bool()?,
        retention_days: retention? as i64,
        coverage: Coverage {
            start: ends[0],
            end: ends[1],
            gaps,
            unsupported,
            partial: get(coverage, "partial").as_bool()?,
            providers,
        },
        buckets: kept_buckets,
        unresolved_waits: number(&waits["observed"])
            .zip(waits["complete"].as_bool())
            .map(|(observed, complete)| Waits { observed, complete }),
    })
}

/// One UTC day of a speech host's statistics, its profiles summed.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SpeechDay {
    pub date: String,
    pub counters: BTreeMap<String, f64>,
    pub error_codes: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Speech {
    pub enabled: bool,
    pub days: Vec<SpeechDay>,
}

/// AnalyticsData::speech: the days within `from`..`through`. Profile IDs
/// are read only to refuse a repeated source bucket, and never kept.
pub fn speech(bytes: &[u8], from: Day, through: Day) -> Option<Speech> {
    let raw = object(bytes).filter(|_| valid_range(from, through))?;
    let days = get(&raw, "days")
        .as_array()
        .filter(|days| days.len() <= 2880)?;
    if !get(&raw, "enabled").is_boolean() || !equals(get(&raw, "retention_days"), 90.0) {
        return None;
    }
    // Each day's counters and error counts.
    type Sums = (BTreeMap<String, f64>, BTreeMap<String, f64>);
    let mut dates: BTreeMap<Day, Sums> = BTreeMap::new();
    let mut keys = HashSet::new();
    for row in days {
        let date = Day::parse_qt(row["day"].as_str().unwrap_or_default())?;
        let profile = row["profile_id"].as_str().unwrap_or_default();
        if profile.is_empty()
            || profile.encode_utf16().count() > 256
            || !keys.insert((date, profile))
        {
            return None;
        }
        if date < from || date > through {
            continue;
        }
        let (counters, errors) = dates.entry(date).or_default();
        for name in SPEECH_COUNTERS {
            if !add(counters, name, &row[name], name.ends_with("_ms")) {
                return None;
            }
        }
        let reported = row["error_codes"]
            .as_object()
            .filter(|codes| codes.len() <= 64)?;
        for (code, count) in reported {
            let code = if SPEECH_ERRORS.contains(&code.as_str()) {
                code.as_str()
            } else {
                "unknown"
            };
            if !add(errors, code, count, false) {
                return None;
            }
        }
    }
    Some(Speech {
        enabled: get(&raw, "enabled").as_bool()?,
        days: dates
            .into_iter()
            .map(|(date, (counters, error_codes))| SpeechDay {
                date: date.iso(),
                counters,
                error_codes,
            })
            .collect(),
    })
}

/// One route's reads for the view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observation {
    /// The `hgs @route`; empty for this computer.
    pub route: String,
    pub agent: Option<Summary>,
    pub voice: Option<Speech>,
    pub current: bool,
    pub voice_current: bool,
    /// Fixed client messages, never remote standard error.
    pub error: String,
    pub voice_error: String,
    /// When the voice was read, in ms since the epoch.
    pub voice_read_at: i64,
}

#[derive(Clone, Debug, Default)]
struct Source {
    epoch: String,
    revision: f64,
    generated: f64,
    retired: HashSet<String>,
    blocked: bool,
}

/// Retired source epochs are tombstones for the view's lifetime: a route
/// whose stale store generates a newer timestamp cannot revive pre-purge
/// totals.
#[derive(Clone, Debug, Default)]
pub struct EpochGuard {
    sources: HashMap<String, Source>,
}

impl EpochGuard {
    pub fn accept(&mut self, summary: &Summary) -> bool {
        let (epoch, revision, generated) = (
            &summary.source_epoch,
            summary.revision,
            summary.generated_at,
        );
        if !self.sources.contains_key(&summary.machine_id) {
            if self.sources.len() >= MAX_COMPUTERS {
                return false;
            }
            self.sources.insert(
                summary.machine_id.clone(),
                Source {
                    epoch: epoch.clone(),
                    revision,
                    generated,
                    ..Source::default()
                },
            );
            return true;
        }
        let state = self.sources.get_mut(&summary.machine_id).expect("present");
        if state.blocked || state.retired.contains(epoch) {
            return false;
        }
        if state.epoch == *epoch {
            if revision < state.revision {
                return false;
            }
            state.revision = revision;
            state.generated = state.generated.max(generated);
            return true;
        }
        if generated <= state.generated {
            return false;
        }
        if state.retired.len() >= 64 {
            state.blocked = true;
            return false;
        }
        let old = std::mem::replace(&mut state.epoch, epoch.clone());
        state.retired.insert(old);
        state.revision = revision;
        state.generated = generated;
        true
    }
}

/// One whole response per machine and source, among the first
/// [`MAX_COMPUTERS`] routes: the newest revision of an epoch, or the
/// newer-generated of two epochs (a newer stale epoch rather than pre-purge
/// buckets through another route); on a tie a current read, then the later
/// generated. In machine order, as Qt's map keeps them.
pub fn unique_computers(observations: &[Observation]) -> Vec<Observation> {
    let mut selected: BTreeMap<String, Observation> = BTreeMap::new();
    for observation in observations.iter().take(MAX_COMPUTERS) {
        let Some(next) = &observation.agent else {
            continue;
        };
        let key = format!("{}/agent", next.machine_id);
        let replace = match selected
            .get(&key)
            .and_then(|old| Some((old, old.agent.as_ref()?)))
        {
            None => true,
            Some((old, previous)) => {
                let same_epoch = previous.source_epoch == next.source_epoch;
                let newer = if same_epoch {
                    next.revision > previous.revision
                } else {
                    next.generated_at > previous.generated_at
                };
                let equal = same_epoch && next.revision == previous.revision;
                newer
                    || (equal
                        && ((observation.current && !old.current)
                            || (observation.current == old.current
                                && next.generated_at > previous.generated_at)))
            }
        };
        if replace {
            selected.insert(key, observation.clone());
        }
    }
    selected.into_values().collect()
}

/// Sums of every counter all `buckets` report (for one `provider`, or
/// all); a counter some bucket lacks, or whose sum passes the exact range,
/// is unknown and left out.
pub fn totals<'a>(
    buckets: impl IntoIterator<Item = &'a Bucket>,
    provider: Option<&str>,
) -> BTreeMap<String, f64> {
    let mut result = BTreeMap::new();
    let mut unavailable = HashSet::new();
    for bucket in buckets {
        if provider.is_some_and(|provider| bucket.provider != provider) {
            continue;
        }
        for name in COUNTERS {
            let value = bucket.counters.get(name).map(|n| json!(n));
            if !value.is_some_and(|value| add(&mut result, name, &value, false)) {
                unavailable.insert(name);
            }
        }
    }
    for name in unavailable {
        result.remove(name);
    }
    result
}

/// The recorded samples merged, never local means or percentiles: a
/// missing or overflowing histogram makes that measure unavailable.
pub fn histogram_totals<'a>(
    buckets: impl IntoIterator<Item = &'a Bucket>,
    provider: Option<&str>,
) -> BTreeMap<String, Histogram> {
    let mut result: BTreeMap<String, Histogram> = BTreeMap::new();
    let mut unavailable = HashSet::new();
    for bucket in buckets {
        if provider.is_some_and(|provider| bucket.provider != provider) {
            continue;
        }
        for name in HISTOGRAMS {
            if unavailable.contains(name) {
                continue;
            }
            let next = bucket
                .histograms
                .get(name)
                .and_then(|next| match result.get(name) {
                    Some(sum) => sum.merged(next),
                    None => Some(next.clone()),
                });
            match next {
                Some(next) => {
                    result.insert(name.to_owned(), next);
                }
                None => {
                    unavailable.insert(name);
                    result.remove(name);
                }
            }
        }
    }
    result
}

/// A speech host's days summed.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SpeechTotals {
    pub counters: BTreeMap<String, f64>,
    pub error_codes: BTreeMap<String, f64>,
}

pub fn speech_totals(days: &[SpeechDay]) -> SpeechTotals {
    let mut totals = SpeechTotals::default();
    let (mut unavailable, mut unavailable_errors) = (HashSet::new(), HashSet::new());
    for day in days {
        for name in SPEECH_COUNTERS {
            let value = day.counters.get(name).map_or(Value::Null, |n| json!(n));
            if !add(&mut totals.counters, name, &value, name.ends_with("_ms")) {
                unavailable.insert(name.to_owned());
            }
        }
        for (code, count) in &day.error_codes {
            if !add(&mut totals.error_codes, code, &json!(count), false) {
                unavailable_errors.insert(code.clone());
            }
        }
    }
    totals
        .counters
        .retain(|name, _| !unavailable.contains(name));
    totals
        .error_codes
        .retain(|code, _| !unavailable_errors.contains(code));
    totals
}

/// A number as Qt writes it: an exact integer without a fraction.
fn num(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() <= MAX_INTEGER {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn nums(values: &BTreeMap<String, f64>) -> Value {
    Value::Object(
        values
            .iter()
            .map(|(key, n)| (key.clone(), num(*n)))
            .collect(),
    )
}

/// The diagnostic projection of a summary: what the reply reported, never
/// its machine ID, epoch or buckets.
fn public_agent(agent: &Summary) -> Value {
    let coverage = &agent.coverage;
    let mut public = json!({
        "schema": 1,
        "source": "agent",
        "revision": num(agent.revision),
        "generated_at": num(agent.generated_at),
        "range": {"from": agent.from.iso(), "through": agent.through.iso()},
        "collection": {
            "enabled": agent.enabled,
            "event_retention_days": 14,
            "retention_days": agent.retention_days,
        },
        "coverage": {
            "start": coverage.start.map_or(Value::Null, num),
            "end": coverage.end.map_or(Value::Null, num),
            "gaps": coverage.gaps,
            "unsupported": coverage.unsupported,
            "partial": coverage.partial,
            "providers": coverage.providers,
        },
        "reported_totals": nums(&totals(&agent.buckets, None)),
    });
    if let Some(waits) = agent.unresolved_waits {
        public["unresolved_waits"] =
            json!({"observed": num(waits.observed), "complete": waits.complete});
    }
    public
}

/// Keys in order, as QJsonObject keeps them, whatever the map's own order.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(object) => {
            let ordered: BTreeMap<String, Value> = object
                .into_iter()
                .map(|(key, value)| (key, sorted(value)))
                .collect();
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// AnalyticsData::diagnostic: the closed aggregate a person reviews before
/// saving it. Computers are numbered within the file; no raw reply,
/// standard error, alias, native UUID or speech profile is in it.
pub fn diagnostic(observations: &[Observation], from: Day, through: Day) -> Value {
    let computers: Vec<Value> = unique_computers(observations)
        .iter()
        .enumerate()
        .map(|(index, observation)| {
            let mut row = json!({
                "computer": index + 1,
                "current": observation.current,
                "agent": observation.agent.as_ref().map_or(Value::Null, public_agent),
            });
            if let Some(voice) = &observation.voice {
                let sums = speech_totals(&voice.days);
                let mut totals = nums(&sums.counters);
                totals["error_codes"] = nums(&sums.error_codes);
                row["voice"] = json!({"enabled": voice.enabled, "reported_totals": totals});
                row["voice_current"] = json!(observation.voice_current);
            }
            row
        })
        .collect();
    sorted(json!({
        "schema": 1,
        "kind": "local_aggregate_diagnostics",
        "from": from.iso(),
        "through": through.iso(),
        "selected_routes": observations.len(),
        "unavailable_routes": observations.iter().filter(|o| !o.current).count(),
        "computers": computers,
    }))
}

/// The diagnostic as the file is saved: indented by four spaces, keys in
/// order, as QJsonDocument::Indented writes it (the spelling of an empty
/// list or a large number may differ).
pub fn diagnostic_text(diagnostic: &Value) -> String {
    let mut bytes = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
    serde::Serialize::serialize(diagnostic, &mut serializer).expect("a value serializes");
    bytes.push(b'\n');
    String::from_utf8(bytes).expect("JSON is UTF-8")
}

/// What one call reads or changes.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Summary {
        host: String,
        from: Day,
        through: Day,
    },
    Speech {
        host: String,
        from: Day,
        through: Day,
    },
    /// Local only; pins the epoch the person reviewed.
    Configure {
        epoch: String,
        enabled: bool,
        retention_days: i64,
    },
    /// Local only; pins the epoch the person reviewed.
    Purge { epoch: String },
}

/// A call's checked answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Summary(Box<Summary>),
    Speech(Speech),
    /// A change the computer confirmed.
    Changed,
}

impl Request {
    fn host(&self) -> &str {
        match self {
            Request::Summary { host, .. } | Request::Speech { host, .. } => host,
            Request::Configure { .. } | Request::Purge { .. } => "",
        }
    }

    fn mutation(&self) -> bool {
        matches!(self, Request::Configure { .. } | Request::Purge { .. })
    }

    fn verb(&self) -> &'static str {
        match self {
            Request::Speech { .. } => "speech",
            _ => "analytics",
        }
    }

    /// The help line that shows the verb exists. An older `hgs` treats an
    /// unknown verb as a session launch, so it is never tried blind.
    fn marker(&self) -> &'static str {
        match self {
            Request::Speech { .. } => "speech --json",
            _ => "hgs [@host] analytics --json",
        }
    }

    fn payload(&self) -> Value {
        match self {
            Request::Summary { from, through, .. } => json!({
                "schema": 1, "action": "summary", "from": from.iso(), "through": through.iso(),
            }),
            Request::Speech { .. } => json!({"action": "stats"}),
            Request::Configure {
                epoch,
                enabled,
                retention_days,
            } => json!({
                "schema": 1, "action": "configure", "expected_epoch": epoch,
                "enabled": enabled, "retention_days": retention_days,
            }),
            Request::Purge { epoch } => json!({
                "schema": 1, "action": "purge", "expected_epoch": epoch, "confirm": true,
            }),
        }
    }

    fn range(&self) -> Option<(Day, Day)> {
        match self {
            Request::Summary { from, through, .. } | Request::Speech { from, through, .. } => {
                Some((*from, *through))
            }
            _ => None,
        }
    }

    fn args(&self, rest: &[&str]) -> Vec<String> {
        let host = self.host();
        let mut args: Vec<String> = Vec::new();
        if !host.is_empty() {
            args.push(format!("@{host}"));
        }
        args.extend(rest.iter().map(|arg| (*arg).to_owned()));
        args
    }

    /// `hgs [@host] --help`.
    pub fn probe(&self) -> Invocation {
        Invocation {
            args: self.args(&["--help"]),
            stdin: None,
            timeout: TIMEOUT,
            request_id: None,
        }
    }

    /// `hgs [@host] analytics|speech --json` with the payload on stdin.
    pub fn invocation(&self) -> Invocation {
        Invocation {
            args: self.args(&[self.verb(), "--json"]),
            stdin: Some(self.payload().to_string().into_bytes()),
            timeout: TIMEOUT,
            request_id: None,
        }
    }

    fn failed(&self) -> String {
        if self.mutation() {
            "Change was not confirmed. Refresh and review before another action."
        } else {
            "Analytics read failed or exceeded its response limit."
        }
        .into()
    }

    /// Whether a probe passed: a clean exit whose help names the verb.
    pub fn probed(&self, exit: Option<Exit>) -> Result<(), String> {
        match exit {
            Some(Exit::Code {
                code: 0, stdout, ..
            }) => {
                if String::from_utf8_lossy(&stdout).contains(self.marker()) {
                    Ok(())
                } else {
                    Err("This computer does not support the requested analytics read.".into())
                }
            }
            _ => Err("Computer unavailable. No current analytics read.".into()),
        }
    }

    /// The call's answer, checked; None for output past the limit.
    pub fn answer(&self, exit: Option<Exit>) -> Result<Reply, String> {
        let Some(Exit::Code {
            code: 0, stdout, ..
        }) = exit
        else {
            return Err(self.failed());
        };
        if self.mutation() {
            // Mutations answer with metadata for the default range: only a
            // fresh explicit summary fills the page again.
            let reply = object(&stdout).filter(|reply| {
                reply.get("source").and_then(Value::as_str) == Some("agent")
                    && !reply.contains_key("error")
            });
            return reply.map(|_| Reply::Changed).ok_or_else(|| self.failed());
        }
        let (from, through) = self.range().expect("a read has a range");
        let reply = match self {
            Request::Speech { .. } => speech(&stdout, from, through).map(Reply::Speech),
            _ => summary(&stdout, from, through).map(|summary| Reply::Summary(Box::new(summary))),
        };
        reply.ok_or_else(|| "Computer returned invalid analytics data.".into())
    }
}

/// AnalyticsClient: probes the selected computer's `hgs`, then runs the
/// call, each bounded in time and output. Blocks; never on the main thread.
pub fn call(hgs: &Path, request: &Request) -> Result<Reply, String> {
    if let Some((from, through)) = request.range() {
        if !valid_range(from, through) {
            return Err("Choose an inclusive UTC range of at most 90 days.".into());
        }
    }
    let probe = crate::run::run_limited(hgs, &request.probe(), &[], HELP_BYTES).ok();
    request.probed(probe)?;
    request.answer(crate::run::run_limited(hgs, &request.invocation(), &[], MAX_BYTES).ok())
}

#[cfg(test)]
mod tests;
