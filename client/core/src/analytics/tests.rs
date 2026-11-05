// Mirrors tray/tests/test_analytics.cpp.
use std::sync::Arc;

use super::page::{self, Page, ALL};
use super::*;

const MACHINE: &str = "10000000-0000-4000-8000-000000000001";
const EPOCH: &str = "20000000-0000-4000-8000-000000000001";
const RESET: &str = "20000000-0000-4000-8000-000000000002";
/// 2026-01-02 00:00 UTC.
const NOW_MS: i64 = 1_767_312_000_000;

fn day(text: &str) -> Day {
    Day::parse(text).unwrap()
}

fn first() -> Day {
    day("2026-01-01")
}

fn last() -> Day {
    day("2026-01-30")
}

fn bucket(date: Day, provider: &str, completed: u64) -> Value {
    json!({
        "date": date.iso(), "provider": provider, "producer": "main",
        "counters": {"turn_started": completed, "turn_completed": completed, "work_ms": 60000,
            "input_tokens": 17, "failure_network": 1},
        "histograms": {"work_ms": {"version": 1, "bounds": [100, 1000, 10000, 60000, 600000],
            "counts": [0, 0, 0, 1, 0, 0], "count": 1, "sum": 60000}},
    })
}

fn work_bucket(counts: [u64; 6], count: u64, sum: u64, provider: &str) -> Value {
    let mut row = bucket(first(), provider, 3);
    row["histograms"] = json!({"work_ms": {"version": 1,
        "bounds": [100, 1000, 10000, 60000, 600000], "counts": counts, "count": count, "sum": sum}});
    row
}

fn raw(rows: Vec<Value>) -> Value {
    json!({
        "schema": 1, "machine_id": MACHINE, "source": "agent", "source_epoch": EPOCH, "revision": 7,
        "generated_at": NOW_MS as f64 / 1000.0,
        "range": {"from": first().iso(), "through": last().iso()},
        "collection": {"enabled": false, "event_retention_days": 14, "retention_days": 90},
        "coverage": {"start": 1_767_225_600.0, "end": 1_767_225_660.0, "partial": true,
            "gaps": ["collection_disabled"], "unsupported": ["cost", "tokens"],
            "providers": {"codex": {"supported": ["turns", "work_ms"], "unsupported": ["tokens"],
                "partial": true}}},
        "buckets": rows,
    })
}

fn bytes(value: &Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn read(value: &Value) -> Option<Summary> {
    summary(&bytes(value), first(), last())
}

fn parsed(rows: Vec<Value>) -> Summary {
    read(&raw(rows)).unwrap()
}

fn voice() -> Value {
    let mut row = json!({"day": first().iso(), "profile_id": "private-profile-sentinel",
        "error_codes": {"http_timeout": 1}});
    for name in SPEECH_COUNTERS {
        row[name] = json!(if name.ends_with("_ms") { 60000 } else { 2 });
    }
    json!({"schema": 1, "enabled": true, "retention_days": 90, "days": [row],
        "totals": {"private": "never forward"}})
}

fn observation(route: &str, agent: Summary, current: bool) -> Observation {
    Observation {
        route: route.into(),
        agent: Some(Arc::new(agent)),
        current,
        ..Observation::default()
    }
}

#[test]
fn days_read_as_qdate_reads_them() {
    assert_eq!(day("2026-01-01").iso(), "2026-01-01");
    assert_eq!(Day::of_ms(NOW_MS), day("2026-01-02"));
    assert_eq!(day("2026-01-01").plus(-29), day("2025-12-03"));
    assert_eq!(day("1970-01-01"), Day::of_ms(0));
    assert_eq!(day("2028-02-29").plus(1), day("2028-03-01"));
    for (text, read) in [
        ("2026/01/05", Some("2026-01-05")),
        ("2026-01-05T10:00", Some("2026-01-05")),
        ("2026-01-05 ", Some("2026-01-05")),
        ("2026-01-051", None),
        ("2026+01-05", None),
        ("2026-1-05", None),
        ("0000-01-01", None),
        ("2026-02-29", None),
        ("2026-13-01", None),
        ("2026-00-10", None),
        ("+026-01-05", None),
        ("", None),
    ] {
        assert_eq!(
            Day::parse_qt(text).map(Day::iso).as_deref(),
            read,
            "{text:?}"
        );
    }
    assert!(Day::parse("2026/01/05").is_none());
    assert!(valid_range(first(), first().plus(89)));
    assert!(!valid_range(first(), first().plus(90)));
    assert!(!valid_range(last(), first()));
}

#[test]
fn a_summary_keeps_only_reviewed_fields_and_unknowns() {
    let mut value = raw(vec![bucket(first(), "codex", 3)]);
    value["prompt"] = json!("private-content-sentinel");
    value["buckets"][0]["path"] = json!("private-path-sentinel");
    value["buckets"][0]["counters"]["arbitrary_counter_sentinel"] = json!(123);
    value["coverage"]["gaps"] = json!(["private-error-sentinel", "history_purged", "x"]);
    value["coverage"]["providers"]["kimi"] = json!({"supported": ["turns"], "partial": true});
    let summary = read(&value).unwrap();
    assert_eq!(summary.coverage.gaps, ["unknown", "history_purged"]);
    assert_eq!(summary.coverage.unsupported, ["cost", "tokens"]);
    // A provider without both lists is left out, not refused.
    assert_eq!(
        summary.coverage.providers.keys().collect::<Vec<_>>(),
        ["codex"]
    );
    let counters = &summary.buckets[0].counters;
    assert_eq!(counters["input_tokens"], 17.0);
    assert!(!counters.contains_key("output_tokens"));
    assert!(!counters.contains_key("arbitrary_counter_sentinel"));
    assert_eq!(summary.coverage.start, Some(1_767_225_600.0));
    assert_eq!(summary.unresolved_waits, None);
    value["unresolved_waits"] = json!({"observed": 2, "complete": false});
    assert_eq!(
        read(&value).unwrap().unresolved_waits,
        Some(Waits {
            observed: 2.0,
            complete: false
        })
    );
    value["unresolved_waits"] = json!({"observed": -2, "complete": false});
    assert_eq!(read(&value).unwrap().unresolved_waits, None);
    value["source_epoch"] = json!("00000000-0000-0000-0000-000000000000");
    assert!(read(&value).is_some());
    value["coverage"]["start"] = Value::Null;
    assert_eq!(read(&value).unwrap().coverage.start, None);
}

#[test]
fn invalid_and_unbounded_summaries_are_refused() {
    let base = raw(vec![bucket(first(), "codex", 3)]);
    assert!(summary(&vec![b'x'; MAX_BYTES + 1], first(), last()).is_none());
    assert!(summary(&bytes(&base), first(), first().plus(90)).is_none());
    // Another range than the one asked for.
    assert!(summary(&bytes(&base), first(), last().plus(-1)).is_none());
    let refused = |change: &dyn Fn(&mut Value)| {
        let mut value = base.clone();
        change(&mut value);
        read(&value).is_none()
    };
    for key in ["machine_id", "source_epoch", "generated_at", "revision"] {
        assert!(refused(&|v| v[key] = json!("private")), "{key}");
    }
    assert!(refused(&|v| v["schema"] = json!(2)));
    assert!(refused(&|v| v["source"] = json!("speech")));
    assert!(refused(
        &|v| v["machine_id"] = json!("A0000000-0000-4000-8000-00000000000B")
    ));
    assert!(!refused(
        &|v| v["machine_id"] = json!("a0000000-0000-4000-8000-00000000000b")
    ));
    assert!(refused(
        &|v| v["machine_id"] = json!(format!("{{{MACHINE}}}"))
    ));
    assert!(refused(
        &|v| v["machine_id"] = json!("00000000-0000-0000-0000-000000000000")
    ));
    assert!(refused(&|v| v["revision"] = json!(1.5)));
    assert!(refused(&|v| v["generated_at"] = json!(253_402_300_800.0)));
    assert!(refused(&|v| v["collection"]["retention_days"] = json!(0)));
    assert!(refused(&|v| v["collection"]["retention_days"] = json!(91)));
    assert!(refused(
        &|v| v["collection"]["event_retention_days"] = json!(15)
    ));
    assert!(refused(&|v| v["collection"]["enabled"] = json!("yes")));
    assert!(refused(&|v| v["coverage"]["partial"] = Value::Null));
    assert!(refused(&|v| {
        v["coverage"].as_object_mut().unwrap().remove("end");
    }));
    assert!(refused(&|v| v["coverage"]["start"] = json!(1_767_225_661.0)));
    assert!(refused(&|v| v["coverage"]["gaps"] = json!([1])));
    assert!(refused(&|v| v["coverage"]["gaps"] = json!(vec!["x"; 33])));
    assert!(refused(&|v| v["buckets"] = json!({})));
    assert!(refused(
        &|v| v["buckets"][0]["counters"]["work_ms"] = json!(-1)
    ));
    assert!(refused(
        &|v| v["buckets"][0]["counters"]["work_ms"] = Value::Null
    ));
    assert!(refused(
        &|v| v["buckets"][0]["provider"] = json!("private-account-name")
    ));
    assert!(refused(&|v| v["buckets"][0]["producer"] = json!("agent")));
    assert!(refused(&|v| v["buckets"][0]["date"] = json!("2026/01/01")));
    assert!(refused(&|v| v["buckets"][0]["date"] = json!("2025-12-31")));
    assert!(refused(
        &|v| v["buckets"][0]["histograms"]["work_ms"]["count"] = json!(2)
    ));
    assert!(refused(&|v| v["buckets"] = json!([
        bucket(first(), "codex", 3),
        bucket(first(), "codex", 4)
    ])));
    assert!(read(&raw(vec![
        bucket(first(), "codex", 3),
        bucket(first(), "claude", 4)
    ]))
    .is_some());
    assert!(refused(
        &|v| v["buckets"] = json!(vec![bucket(first(), "codex", 3); MAX_BUCKETS + 1])
    ));
}

#[test]
fn a_fleet_keeps_one_snapshot_per_computer_and_epoch() {
    let a = observation("route-a", parsed(vec![bucket(first(), "codex", 3)]), true);
    let mut b = observation("route-b", parsed(vec![bucket(first(), "codex", 3)]), true);
    let unique = unique_computers(&[a.clone(), b.clone()]);
    assert_eq!(unique.len(), 1);
    assert_eq!(
        totals(&unique[0].agent.as_ref().unwrap().buckets, None)["turn_completed"],
        3.0
    );
    let agent = Arc::make_mut(b.agent.as_mut().unwrap());
    agent.revision = 8.0;
    agent.buckets = parsed(vec![bucket(first(), "codex", 5)]).buckets;
    assert_eq!(
        unique_computers(&[a.clone(), b.clone()])[0]
            .agent
            .as_ref()
            .unwrap()
            .revision,
        8.0
    );
    let agent = Arc::make_mut(b.agent.as_mut().unwrap());
    agent.source_epoch = RESET.into();
    agent.revision = 0.0;
    agent.generated_at += 1.0;
    agent.buckets.clear();
    b.current = false;
    let unique = unique_computers(&[b, a.clone()]);
    assert_eq!(unique.len(), 1);
    assert_eq!(unique[0].agent.as_ref().unwrap().source_epoch, RESET);
    assert!(!unique[0].current && unique[0].agent.as_ref().unwrap().buckets.is_empty());
    // On a tie a current read wins, then the later one; machines in order.
    let stale = observation("stale", (*a.agent.clone().unwrap()).clone(), false);
    assert_eq!(
        unique_computers(&[stale.clone(), a.clone()])[0].route,
        "route-a"
    );
    let mut other = a.clone();
    other.route = "other".into();
    Arc::make_mut(other.agent.as_mut().unwrap()).machine_id =
        "00000000-0000-4000-8000-000000000009".into();
    let routes: Vec<String> = unique_computers(&[a, other])
        .into_iter()
        .map(|o| o.route)
        .collect();
    assert_eq!(routes, ["other", "route-a"]);
    // Only the first 32 routes are counted.
    let mut many: Vec<Observation> = (0..33)
        .map(|n| {
            let mut o = stale.clone();
            Arc::make_mut(o.agent.as_mut().unwrap()).machine_id =
                format!("10000000-0000-4000-8000-{n:012}");
            o
        })
        .collect();
    assert_eq!(unique_computers(&many).len(), 32);
    many.truncate(3);
    assert_eq!(unique_computers(&many).len(), 3);
}

#[test]
fn histograms_merge_samples_before_the_mean() {
    let one = parsed(vec![work_bucket([1, 0, 0, 0, 0, 0], 1, 100, "codex")]).buckets;
    let nine = parsed(vec![work_bucket([0, 9, 0, 0, 0, 0], 9, 9000, "claude")]).buckets;
    let both: Vec<&Bucket> = one.iter().chain(&nine).collect();
    let merged = histogram_totals(both.iter().copied(), None)["work_ms"].clone();
    assert_eq!(
        merged,
        Histogram {
            counts: [1, 9, 0, 0, 0, 0],
            count: 10,
            sum: 9100
        }
    );
    // The mean of machine means would be 550 ms; pooled samples weigh 910 ms.
    assert_eq!(merged.sum as f64 / merged.count as f64, 910.0);
    assert_eq!(
        histogram_totals(nine.iter().chain(&one), None)["work_ms"],
        merged
    );
    assert_eq!(
        histogram_totals(both.iter().copied(), Some("codex"))["work_ms"].count,
        1
    );
    assert!(histogram_totals(both.iter().copied(), Some("kimi")).is_empty());
    assert!(histogram_totals(&[], None).is_empty());
    let zero = parsed(vec![work_bucket([0; 6], 0, 0, "codex")]).buckets;
    assert_eq!(
        histogram_totals(zero.iter().chain(&one).chain(&nine), None)["work_ms"],
        merged
    );
}

#[test]
fn invalid_missing_and_overflowing_histograms_stay_unavailable() {
    let valid = work_bucket([1, 0, 0, 0, 0, 0], 1, 100, "codex");
    let histogram = valid["histograms"]["work_ms"].clone();
    let mut malformed = Vec::new();
    for (key, value) in [
        ("version", json!(2)),
        ("bounds", json!([100, 1000, 10000, 60000, 600001])),
        ("count", json!(2)),
        ("counts", json!([1, 0, 0, 0, 0])),
        ("counts", json!([-1, 0, 0, 0, 0, 0])),
        ("sum", json!(1.5)),
        ("sum", json!(9_007_199_254_740_992.0)),
    ] {
        let mut bad = histogram.clone();
        bad[key] = value;
        malformed.push(bad);
    }
    let mut bad = histogram.clone();
    bad["counts"] = json!([0, 0, 0, 0, 0, 0]);
    bad["count"] = json!(0);
    malformed.push(bad);
    for bad in malformed {
        let mut invalid = valid.clone();
        invalid["histograms"] = json!({"work_ms": bad});
        assert!(read(&raw(vec![invalid])).is_none(), "{bad}");
    }
    let mut missing = valid.clone();
    missing["histograms"] = json!({});
    let missing = parsed(vec![missing]).buckets;
    let valid = parsed(vec![valid]).buckets;
    assert!(
        !histogram_totals(valid.iter().chain(&missing).chain(&valid), None).contains_key("work_ms")
    );
    assert!(!histogram_totals(missing.iter().chain(&valid), None).contains_key("work_ms"));
    let maximum = MAX_EXACT;
    let count_limit = parsed(vec![work_bucket(
        [maximum, 0, 0, 0, 0, 0],
        maximum,
        0,
        "codex",
    )])
    .buckets;
    // Both same-bin overflow and total-count overflow across bins fail closed.
    assert!(!histogram_totals(count_limit.iter().chain(&valid), None).contains_key("work_ms"));
    let other_bin = parsed(vec![work_bucket([0, 1, 0, 0, 0, 0], 1, 101, "codex")]).buckets;
    assert!(!histogram_totals(count_limit.iter().chain(&other_bin), None).contains_key("work_ms"));
    let sum_limit = parsed(vec![work_bucket([0, 0, 0, 0, 0, 1], 1, maximum, "codex")]).buckets;
    assert!(!histogram_totals(sum_limit.iter().chain(&valid), None).contains_key("work_ms"));
    let below = parsed(vec![work_bucket(
        [0, 0, 0, 0, 0, 1],
        1,
        maximum - 100,
        "codex",
    )])
    .buckets;
    assert_eq!(
        histogram_totals(below.iter().chain(&valid), None)["work_ms"].sum,
        maximum
    );
}

#[test]
fn totals_leave_out_what_any_bucket_lacks_or_overflows() {
    let mut lacking = bucket(first().plus(1), "claude", 4);
    lacking["counters"]
        .as_object_mut()
        .unwrap()
        .remove("input_tokens");
    let mut huge = bucket(first().plus(2), "kimi", 1);
    huge["counters"]["failure_network"] = json!(MAX_EXACT);
    let summary = parsed(vec![bucket(first(), "codex", 3), lacking, huge]);
    let sums = totals(&summary.buckets, None);
    assert_eq!(sums["turn_completed"], 8.0);
    assert!(!sums.contains_key("input_tokens"));
    assert!(!sums.contains_key("failure_network"));
    assert!(!sums.contains_key("output_tokens"));
    assert_eq!(
        totals(&summary.buckets, Some("codex"))["input_tokens"],
        17.0
    );
    assert!(totals(&[], None).is_empty());
}

#[test]
fn voice_collapses_profiles_and_keeps_units() {
    let mut value = voice();
    let mut second = value["days"][0].clone();
    second["profile_id"] = json!("second-private-profile");
    second["queue_ms"] = json!(1.5);
    second["error_codes"] = json!({"private-provider-message": 2, "http_timeout": 3});
    let mut outside = second.clone();
    outside["day"] = json!("2026/02/15");
    value["days"]
        .as_array_mut()
        .unwrap()
        .extend([second, outside]);
    let parsed = speech(&bytes(&value), first(), last()).unwrap();
    assert!(!format!("{parsed:?}").contains("private"));
    assert_eq!(parsed.days.len(), 1);
    let day = &parsed.days[0];
    assert_eq!(day.date, "2026-01-01");
    assert_eq!(day.counters["submitted"], 4.0);
    assert_eq!(day.counters["queue_ms"], 60001.5);
    assert_eq!(day.error_codes["unknown"], 2.0);
    assert_eq!(day.error_codes["http_timeout"], 4.0);
    // A repeated profile on a day, even outside the range, is refused.
    let mut repeated = value.clone();
    let again = repeated["days"][2].clone();
    repeated["days"].as_array_mut().unwrap().push(again);
    assert!(speech(&bytes(&repeated), first(), last()).is_none());
    let refused = |change: &dyn Fn(&mut Value)| {
        let mut value = voice();
        change(&mut value);
        speech(&bytes(&value), first(), last()).is_none()
    };
    assert!(refused(&|v| v["retention_days"] = json!(30)));
    assert!(refused(&|v| v["enabled"] = Value::Null));
    assert!(refused(&|v| v["days"][0]["submitted"] = json!(1.5)));
    assert!(refused(&|v| {
        v["days"][0].as_object_mut().unwrap().remove("failed");
    }));
    assert!(refused(&|v| v["days"][0]["profile_id"] = json!("")));
    assert!(refused(
        &|v| v["days"][0]["profile_id"] = json!("p".repeat(257))
    ));
    assert!(refused(&|v| v["days"][0]["error_codes"] = json!([])));
    assert!(refused(&|v| {
        let codes: Map<String, Value> = (0..65).map(|n| (format!("e{n}"), json!(1))).collect();
        v["days"][0]["error_codes"] = Value::Object(codes);
    }));
    assert!(refused(&|v| v["days"][0]["day"] = json!("01/01/2026")));
    assert!(speech(&bytes(&voice()), first(), first().plus(90)).is_none());
    let sums = speech_totals(&parsed.days);
    assert_eq!(sums.counters["audio_ms"], 120_000.0);
    assert_eq!(sums.error_codes["http_timeout"], 4.0);
}

#[test]
fn a_retired_epoch_never_revives_with_a_new_timestamp() {
    let mut guard = EpochGuard::default();
    let mut original = parsed(vec![]);
    assert!(guard.accept(&original));
    let mut reset = original.clone();
    reset.source_epoch = RESET.into();
    reset.generated_at = original.generated_at + 1.0;
    reset.revision = 0.0;
    assert!(guard.accept(&reset));
    original.generated_at = reset.generated_at + 100.0;
    original.revision = 999.0;
    assert!(!guard.accept(&original));
    reset.revision = 5.0;
    assert!(guard.accept(&reset));
    reset.revision = 4.0;
    assert!(!guard.accept(&reset));
    let mut next = reset.clone();
    for n in 0..63 {
        next.source_epoch = format!("30000000-0000-4000-8000-{n:012}");
        next.generated_at = original.generated_at + f64::from(n) + 1.0;
        assert!(guard.accept(&next));
    }
    next.source_epoch = "40000000-0000-4000-8000-000000000001".into();
    next.generated_at = original.generated_at + 1000.0;
    assert!(!guard.accept(&next));
    // At most 32 machines.
    let mut guard = EpochGuard::default();
    for n in 0..33u32 {
        let mut summary = reset.clone();
        summary.machine_id = format!("10000000-0000-4000-8000-{n:012}");
        assert_eq!(guard.accept(&summary), n < 32);
    }
}

#[test]
fn diagnostics_leave_out_identity_content_and_history() {
    let agent = parsed(vec![bucket(first(), "codex", 3)]);
    let speech = speech(&bytes(&voice()), first(), last()).unwrap();
    let observation = Observation {
        route: "private-host-sentinel".into(),
        agent: Some(Arc::new(agent)),
        voice: Some(Arc::new(speech)),
        current: true,
        voice_current: true,
        ..Observation::default()
    };
    let missing = Observation {
        route: "offline".into(),
        ..Observation::default()
    };
    let value = diagnostic(&[observation, missing], first(), last());
    let text = diagnostic_text(&value);
    for absent in [
        MACHINE,
        EPOCH,
        "private",
        "profile_id",
        "buckets",
        "\"days\":",
    ] {
        assert!(!text.contains(absent), "{absent}");
    }
    assert!(text.contains("reported_totals"));
    assert_eq!(value["selected_routes"], 2);
    assert_eq!(value["unavailable_routes"], 1);
    assert_eq!(value["computers"][0]["computer"], 1);
    assert_eq!(
        value["computers"][0]["agent"]["reported_totals"]["turn_completed"],
        3
    );
    assert_eq!(
        value["computers"][0]["voice"]["reported_totals"]["error_codes"]["http_timeout"],
        1
    );
    // As QJsonDocument::Indented writes it: keys in order, four spaces,
    // whole numbers without a fraction.
    assert!(text.starts_with("{\n    \"computers\": [\n        {\n            \"agent\": {"));
    assert!(text.contains("\"work_ms\": 60000\n"));
    assert!(text.ends_with("\"unavailable_routes\": 1\n}\n"));
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    let mut ordered = keys.clone();
    ordered.sort();
    assert_eq!(keys, ordered);
}

#[test]
fn calls_probe_the_help_first_and_word_failures_as_qt_does() {
    let read = Request::Summary {
        host: "mac".into(),
        from: first(),
        through: last(),
    };
    assert_eq!(read.probe().args, ["@mac", "--help"]);
    assert_eq!(read.probe().stdin, None);
    let invocation = read.invocation();
    assert_eq!(invocation.args, ["@mac", "analytics", "--json"]);
    assert_eq!(invocation.timeout, Duration::from_secs(30));
    let payload: Value = serde_json::from_slice(&invocation.stdin.unwrap()).unwrap();
    assert_eq!(
        payload,
        json!({"schema": 1, "action": "summary", "from": "2026-01-01", "through": "2026-01-30"})
    );
    let voice_read = Request::Speech {
        host: String::new(),
        from: first(),
        through: last(),
    };
    assert_eq!(voice_read.invocation().args, ["speech", "--json"]);
    assert_eq!(
        voice_read.invocation().stdin.unwrap(),
        br#"{"action":"stats"}"#
    );
    let configure = Request::Configure {
        epoch: EPOCH.into(),
        enabled: true,
        retention_days: 45,
    };
    assert_eq!(configure.invocation().args, ["analytics", "--json"]);
    let payload: Value = serde_json::from_slice(&configure.invocation().stdin.unwrap()).unwrap();
    assert_eq!(
        payload,
        json!({"schema": 1, "action": "configure", "expected_epoch": EPOCH,
        "enabled": true, "retention_days": 45})
    );
    let purge = Request::Purge {
        epoch: EPOCH.into(),
    };
    let payload: Value = serde_json::from_slice(&purge.invocation().stdin.unwrap()).unwrap();
    assert_eq!(
        payload,
        json!({"schema": 1, "action": "purge", "expected_epoch": EPOCH, "confirm": true})
    );

    let help = |text: &str| {
        Some(Exit::Code {
            code: 0,
            stdout: text.as_bytes().to_vec(),
            stderr: String::new(),
        })
    };
    // An older hgs treats an unknown verb as a launch: it is never tried.
    assert_eq!(
        read.probed(help("hgs [@host] ls")).unwrap_err(),
        "This computer does not support the requested analytics read."
    );
    assert!(read
        .probed(help("  hgs [@host] analytics --json   private"))
        .is_ok());
    assert!(voice_read
        .probed(help("hgs [@host] analytics --json"))
        .is_err());
    assert!(voice_read.probed(help("hgs [@host] speech --json")).is_ok());
    for failed in [
        None,
        Some(Exit::TimedOut),
        Some(Exit::Code {
            code: 1,
            stdout: Vec::new(),
            stderr: "private-stderr-sentinel".into(),
        }),
    ] {
        assert_eq!(
            read.probed(failed).unwrap_err(),
            "Computer unavailable. No current analytics read."
        );
    }

    let answer = |code: i32, stdout: &[u8]| {
        Some(Exit::Code {
            code,
            stdout: stdout.to_vec(),
            stderr: "private-stderr-sentinel".into(),
        })
    };
    let good = bytes(&raw(vec![bucket(first(), "codex", 3)]));
    assert!(matches!(
        read.answer(answer(0, &good)),
        Ok(Reply::Summary(_))
    ));
    assert_eq!(
        read.answer(None).unwrap_err(),
        "Analytics read failed or exceeded its response limit."
    );
    assert_eq!(
        read.answer(answer(1, &good)).unwrap_err(),
        "Analytics read failed or exceeded its response limit."
    );
    assert_eq!(
        read.answer(answer(0, b"{}")).unwrap_err(),
        "Computer returned invalid analytics data."
    );
    assert!(matches!(
        voice_read.answer(answer(0, &bytes(&voice()))),
        Ok(Reply::Speech(_))
    ));
    let unconfirmed = "Change was not confirmed. Refresh and review before another action.";
    assert_eq!(
        configure.answer(answer(0, br#"{"schema":1,"source":"agent"}"#)),
        Ok(Reply::Changed)
    );
    for reply in [
        &br#"{"schema":1,"source":"agent","error":"x"}"#[..],
        br#"{"schema":2,"source":"agent"}"#,
        br#"{"schema":1}"#,
        b"[]",
    ] {
        assert_eq!(purge.answer(answer(0, reply)).unwrap_err(), unconfirmed);
    }
    assert_eq!(purge.answer(Some(Exit::TimedOut)).unwrap_err(), unconfirmed);
    // A range Qt would not read is refused before anything runs.
    let wide = Request::Summary {
        host: String::new(),
        from: first(),
        through: first().plus(90),
    };
    assert_eq!(
        call(Path::new("/nonexistent/hgs"), &wide).unwrap_err(),
        "Choose an inclusive UTC range of at most 90 days."
    );
}

/// A route's summary and speech answers.
type Answers = (Result<Summary, String>, Result<Speech, String>);

/// A fake `hgs` that logs each call (and whether the agent's attribution
/// reached it), prints `help` for `--help` and `reply` (or fails) otherwise.
#[cfg(unix)]
fn fake_hgs(dir: &Path, help: &str, reply: &[u8], fail: bool) -> std::path::PathBuf {
    std::fs::write(dir.join("help"), help).unwrap();
    std::fs::write(dir.join("reply"), reply).unwrap();
    if fail {
        std::fs::write(dir.join("fail"), "").unwrap();
    } else {
        let _ = std::fs::remove_file(dir.join("fail"));
    }
    let script = dir.join("hgs");
    let _ = std::fs::remove_file(&script);
    std::os::unix::fs::symlink(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_hgs_analytics.sh"),
        &script,
    )
    .unwrap();
    script
}

#[cfg(unix)]
#[test]
fn an_older_cli_is_only_asked_for_help_and_errors_never_echo_its_text() {
    let dir = std::env::temp_dir().join(format!("zerus-analytics-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    struct Gone(std::path::PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _gone = Gone(dir.clone());
    let calls = || std::fs::read_to_string(dir.join("calls")).unwrap_or_default();
    let read = Request::Summary {
        host: "old".into(),
        from: first(),
        through: last(),
    };
    let attribution = [
        ("HGS_SESSION", "claude/sample/agent"),
        ("HGS_RUN_ID", "run-1"),
    ];

    // An older CLI: only its help is read, never the unknown verb.
    let hgs = fake_hgs(&dir, "hgs [@host] ls", b"{}", false);
    assert_eq!(
        call_in(&hgs, &read, &attribution).unwrap_err(),
        "This computer does not support the requested analytics read."
    );
    assert_eq!(calls(), "@old --help session=unset run=unset\n");

    // A reply past 1 MiB, and a failing CLI: Qt's words, never its stderr.
    std::fs::remove_file(dir.join("calls")).unwrap();
    let help = "hgs [@host] analytics --json\nhgs [@host] speech --json";
    let hgs = fake_hgs(&dir, help, &vec![b'x'; MAX_BYTES + 1], false);
    assert_eq!(
        call_in(&hgs, &read, &attribution).unwrap_err(),
        "Analytics read failed or exceeded its response limit."
    );
    let hgs = fake_hgs(&dir, help, b"", true);
    let failed = call_in(&hgs, &read, &attribution).unwrap_err();
    assert_eq!(
        failed,
        "Analytics read failed or exceeded its response limit."
    );
    assert!(!failed.contains("sentinel"));
    // A good reply, read without the agent's attribution.
    let good = bytes(&raw(vec![bucket(first(), "codex", 3)]));
    let hgs = fake_hgs(&dir, help, &good, false);
    assert!(matches!(
        call_in(&hgs, &read, &attribution),
        Ok(Reply::Summary(_))
    ));
    let log = calls();
    assert!(
        log.lines()
            .all(|line| line.ends_with("session=unset run=unset")),
        "{log}"
    );
    assert!(log.contains("@old analytics --json"));
}

/// Runs a page's read with the given answers per route.
fn read_all(page: &mut Page, answer: &dyn Fn(&str) -> Answers) -> Vec<String> {
    let mut routes = Vec::new();
    while let Some((route, from, through)) = page.next_route() {
        assert_eq!((from, through), (first(), last()));
        let (agent, voice) = answer(&route);
        page.summary_answered(&route, agent, NOW_MS);
        page.speech_answered(&route, voice, NOW_MS);
        routes.push(route);
    }
    // As the service draws after each read.
    page.draw(NOW_MS);
    routes
}

/// The page as the service shows it: drawn whenever a drawing is due.
fn shown(page: &mut Page) -> page::View {
    page.draw(NOW_MS);
    page.view(NOW_MS)
}

fn local_voice() -> Result<Speech, String> {
    Ok(speech(&bytes(&voice()), first(), last()).unwrap())
}

#[test]
fn a_refresh_reads_each_route_in_turn_and_pages_its_rows() {
    let rows: Vec<Value> = (0..60)
        .map(|n| bucket(first().plus(n), "codex", 3))
        .collect();
    let mut value = raw(rows);
    value["range"]["through"] = json!(first().plus(59).iso());
    let agent = summary(&bytes(&value), first(), first().plus(59)).unwrap();
    let mut page = Page::new();
    assert!(shown(&mut page).report.is_none());
    assert!(page.refresh("", "2026-01-01", "2026-03-01"));
    let view = shown(&mut page);
    assert_eq!(view.status, "Reading 1 configured route(s)…");
    assert!(view.busy && !view.can_refresh && !view.can_export);
    // A second refresh while one runs does nothing.
    assert!(!page.refresh(ALL, "2026-01-01", "2026-03-01"));
    let (from, through) = (first(), first().plus(59));
    let (route, ..) = page.next_route().unwrap();
    assert_eq!(route, "");
    page.summary_answered("", Ok(agent), NOW_MS);
    page.speech_answered("", local_voice(), NOW_MS);
    assert!(page.next_route().is_none());
    let view = shown(&mut page);
    assert_eq!(
        view.status,
        "Read finished. Last-known or missing data is excluded from current totals."
    );
    assert!(view.can_refresh && view.can_export && view.can_change_local);
    let local = view.local.unwrap();
    assert!(!local.enabled);
    assert_eq!((local.retention_days, local.reads), (90, 1));
    let report = view.report.unwrap();
    assert_eq!(
        (report.from.as_str(), report.through.as_str()),
        ("2026-01-01", "2026-03-01")
    );
    assert_eq!(report.buckets, 60);
    assert_eq!(report.totals["turn_completed"], 180.0);
    let rows = page.rows("", 0);
    assert_eq!((rows.rows.len(), rows.offset, rows.total), (50, 0, 60));
    let rows = page.rows("", 50);
    assert_eq!(rows.rows.len(), 10);
    assert_eq!(rows.rows[0].date, from.plus(50).iso());
    assert!(!rows.rows[0].counters.contains_key("output_tokens"));
    // Past the end, the last page; another provider, nothing.
    assert_eq!(page.rows("", 500).offset, 50);
    let none = page.rows("claude", 50);
    assert_eq!((none.rows.len(), none.offset, none.total), (0, 0, 0));
    assert_eq!(through, first().plus(59));
}

#[test]
fn repeated_refreshes_and_routes_to_one_computer_do_not_add_up() {
    let mut page = Page::new();
    page.set_peers(["same-computer".to_owned()]);
    for _ in 0..2 {
        assert!(page.refresh(ALL, "2026-01-01", "2026-01-30"));
        let routes = read_all(&mut page, &|_| {
            (Ok(parsed(vec![bucket(first(), "codex", 3)])), local_voice())
        });
        assert_eq!(routes, ["", "same-computer"]);
        let report = shown(&mut page).report.unwrap();
        assert_eq!((report.routes, report.present), (2, 1));
        assert_eq!(report.totals["turn_completed"], 3.0);
        assert_eq!(report.work.as_ref().map(|work| work.count), Some(1));
        assert_eq!(page.rows("", 0).total, 1);
    }
}

#[test]
fn the_fleet_overview_pools_samples_and_excludes_stale_computers() {
    let local = parsed(vec![work_bucket([1, 0, 0, 0, 0, 0], 1, 100, "codex")]);
    let mut peer = parsed(vec![work_bucket([0, 9, 0, 0, 0, 0], 9, 9000, "codex")]);
    peer.machine_id = "10000000-0000-4000-8000-000000000002".into();
    let mut page = Page::new();
    page.set_peers(["same-computer".to_owned(), "peer".to_owned()]);
    page.refresh(ALL, "2026-01-01", "2026-01-30");
    let answer = |route: &str| {
        let agent = if route == "peer" {
            peer.clone()
        } else {
            local.clone()
        };
        (Ok(agent), local_voice())
    };
    read_all(&mut page, &answer);
    let report = shown(&mut page).report.unwrap();
    assert_eq!(report.present, 2);
    assert_eq!(
        report.work,
        Some(Histogram {
            counts: [1, 9, 0, 0, 0, 0],
            count: 10,
            sum: 9100
        })
    );
    page.refresh(ALL, "2026-01-01", "2026-01-30");
    read_all(&mut page, &|route| {
        if route == "peer" {
            (
                Err("Computer unavailable. No current analytics read.".into()),
                Err("Computer unavailable. No current analytics read.".into()),
            )
        } else {
            answer(route)
        }
    });
    let report = shown(&mut page).report.unwrap();
    assert_eq!(report.present, 1);
    assert_eq!(
        report.work.as_ref().map(|work| (work.count, work.sum)),
        Some((1, 100))
    );
    // The peer's last summary stays as last known, excluded from totals.
    let peer = report.computers.iter().find(|c| c.route == "peer").unwrap();
    assert!(!peer.current);
    assert_eq!(
        peer.error,
        "Computer unavailable. No current analytics read."
    );
    let voice = report.voice.iter().find(|v| v.route == "peer").unwrap();
    assert!(voice.read.as_ref().is_some_and(|read| !read.current));
    assert_eq!(
        voice.error,
        "Computer unavailable. No current analytics read."
    );
}

#[test]
fn a_failed_read_keeps_the_last_known_but_excludes_its_totals() {
    let mut page = Page::new();
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| {
        (Ok(parsed(vec![bucket(first(), "codex", 3)])), local_voice())
    });
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| {
        (
            Err("Computer unavailable. No current analytics read.".into()),
            local_voice(),
        )
    });
    let view = shown(&mut page);
    let report = view.report.unwrap();
    assert_eq!(report.present, 0);
    assert!(report.totals.is_empty() && report.work.is_none());
    assert!(!report.computers[0].current);
    assert_eq!(page.rows("", 0).total, 1);
    assert!(!page.rows("", 0).rows[0].current);
    assert!(!view.can_change_local && view.local.is_none());
    // Another range forgets what was read for the last one.
    page.refresh("", "2026-01-01", "2026-01-29");
    while let Some((route, ..)) = page.next_route() {
        page.summary_answered(
            &route,
            Err("Computer unavailable. No current analytics read.".into()),
            NOW_MS,
        );
        page.speech_answered(
            &route,
            Err("Computer unavailable. No current analytics read.".into()),
            NOW_MS,
        );
    }
    let report = shown(&mut page).report.unwrap();
    assert!(report.computers.is_empty());
    assert_eq!(
        report.missing[0].error,
        "Computer unavailable. No current analytics read."
    );
}

#[test]
fn stale_retired_and_peer_summaries_never_set_the_local_controls() {
    let mut page = Page::new();
    page.set_peers(["peer".to_owned()]);
    page.refresh("peer", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| (Ok(parsed(vec![])), local_voice()));
    let view = shown(&mut page);
    assert!(view.local.is_none() && !view.can_change_local);
    assert_eq!(view.report.unwrap().present, 1);
    // A summary generated more than five minutes ago is stale.
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| {
        let mut old = parsed(vec![]);
        old.generated_at -= 301.0;
        old.revision = 9.0;
        (Ok(old), local_voice())
    });
    let report = shown(&mut page).report.unwrap();
    assert_eq!(
        report.computers[0].error,
        "Summary is stale or its clock is unconfirmed."
    );
    assert!(shown(&mut page).local.is_none());
    // A lower revision of the same epoch is refused by the epochs seen.
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| (Ok(parsed(vec![])), local_voice()));
    let report = shown(&mut page).report.unwrap();
    assert_eq!(
        report.computers[0].error,
        "Retired, older or ambiguous analytics source. Excluded from current totals."
    );
}

#[test]
fn reads_age_out_of_the_current_totals() {
    let mut page = Page::new();
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| {
        (Ok(parsed(vec![bucket(first(), "codex", 3)])), local_voice())
    });
    let later = NOW_MS + 301_000;
    let observation = &page.observations(later)[0];
    assert!(!observation.current && !observation.voice_current);
    assert!(!page.view(later).can_change_local);
    // The minute timer draws it again, and only then.
    assert_eq!(page.view(later).report.unwrap().present, 1);
    page.age();
    page.draw(later);
    assert_eq!(page.view(later).report.unwrap().present, 0);
}

#[test]
fn an_invalid_range_reads_nothing_and_says_why() {
    let mut page = Page::new();
    assert!(!page.refresh("", "2026-01-01", "2026-04-01"));
    assert!(!page.refresh("", "2026-01-01", "not a day"));
    assert_eq!(
        shown(&mut page).status,
        "Choose an inclusive UTC range of at most 90 days."
    );
    assert!(page.next_route().is_none());
    assert!(page.diagnostic(NOW_MS).is_none());
}

#[test]
fn peers_are_bounded_and_a_leaving_peer_takes_its_reads() {
    let mut page = Page::new();
    let mut peers: Vec<String> = (0..40).map(|n| format!("peer-{n}")).collect();
    peers.insert(0, ALL.to_owned());
    peers.insert(1, String::new());
    peers.insert(2, "peer-0".to_owned());
    peers.push("x".repeat(257));
    page.set_peers(peers);
    let view = shown(&mut page);
    assert_eq!(view.peers.len(), 31);
    assert!(view.peer_limit);
    page.set_peers(["a".to_owned(), "b".to_owned()]);
    page.refresh(ALL, "2026-01-01", "2026-01-30");
    // A peer that leaves during the read is skipped.
    assert_eq!(page.next_route().unwrap().0, "");
    page.set_peers(["b".to_owned()]);
    assert_eq!(page.next_route().unwrap().0, "b");
    assert!(page.next_route().is_none());
    assert!(!shown(&mut page).peer_limit);
}

#[test]
fn rows_sort_by_day_then_the_computer_as_shown() {
    let mut page = Page::new();
    page.set_peers(["Alpha".to_owned(), "zulu".to_owned()]);
    page.refresh(ALL, "2026-01-01", "2026-01-30");
    read_all(&mut page, &|route| {
        let mut agent = parsed(vec![
            bucket(first().plus(1), "codex", 1),
            bucket(first(), "claude", 1),
        ]);
        agent.machine_id = match route {
            "" => "00000000-0000-4000-8000-000000000001",
            "Alpha" => "00000000-0000-4000-8000-000000000002",
            _ => "00000000-0000-4000-8000-000000000003",
        }
        .into();
        (Ok(agent), local_voice())
    });
    let rows = page.rows("", 0).rows;
    let order: Vec<(&str, &str)> = rows
        .iter()
        .map(|r| (r.date.as_str(), r.route.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            ("2026-01-01", "Alpha"),
            ("2026-01-01", ""),
            ("2026-01-01", "zulu"),
            ("2026-01-02", "Alpha"),
            ("2026-01-02", ""),
            ("2026-01-02", "zulu"),
        ]
    );
    assert_eq!(page.rows("claude", 0).total, 3);
}

/// A summary at the limits: 1080 daily buckets with every counter and
/// histogram.
fn limit_summary(machine: usize) -> Summary {
    let mut summary = parsed(vec![]);
    summary.machine_id = format!("10000000-0000-4000-8000-{machine:012}");
    let histogram = Histogram {
        counts: [1, 2, 3, 4, 5, 6],
        count: 21,
        sum: 21_000,
    };
    for day in 0..90 {
        for provider in PROVIDERS {
            for producer in ["main", "child"] {
                summary.buckets.push(Bucket {
                    date: first().plus(day).iso(),
                    provider: provider.into(),
                    producer: producer.into(),
                    counters: COUNTERS
                        .iter()
                        .map(|name| ((*name).to_owned(), 7.0))
                        .collect(),
                    histograms: ["work_ms", "approval_wait_ms", "input_wait_ms"]
                        .iter()
                        .map(|name| ((*name).to_owned(), histogram.clone()))
                        .collect(),
                });
            }
        }
    }
    assert_eq!(summary.buckets.len(), MAX_BUCKETS);
    summary
}

#[test]
fn a_fleet_at_the_limits_is_drawn_off_the_lock() {
    use std::time::{Duration, Instant};
    let mut page = Page::new();
    page.set_peers((1..MAX_COMPUTERS).map(|n| format!("peer-{n}")));
    assert!(page.refresh(ALL, "2026-01-01", "2026-01-30"));
    let mut machine = 0;
    while let Some((route, ..)) = page.next_route() {
        page.summary_answered(&route, Ok(limit_summary(machine)), NOW_MS);
        page.speech_answered(
            &route,
            Err("Computer unavailable. No current analytics read.".into()),
            NOW_MS,
        );
        machine += 1;
    }
    assert_eq!(machine, MAX_COMPUTERS);
    // Under the lock: only what a drawing needs, the reads shared.
    let start = Instant::now();
    let draw = page.draw_due(NOW_MS).unwrap();
    let taken = start.elapsed();
    let start = Instant::now();
    let drawing = draw.render();
    let rendered = start.elapsed();
    let start = Instant::now();
    drop(page.drawn(drawing));
    let rows = page.rows("", 34_000);
    let view = page.view(NOW_MS);
    let input = page.diagnostic_input(NOW_MS).unwrap();
    let locked = taken + start.elapsed();
    let start = Instant::now();
    let text = diagnostic_text(&diagnostic(&input.0, input.1, input.2));
    let exported = start.elapsed();
    eprintln!(
        "at the limits: {locked:?} under the lock, {rendered:?} drawing, {exported:?} diagnostic"
    );
    assert_eq!(
        view.report.as_ref().unwrap().buckets,
        MAX_COMPUTERS * MAX_BUCKETS
    );
    assert_eq!(
        (rows.total, rows.rows.len()),
        (MAX_COMPUTERS * MAX_BUCKETS, 50)
    );
    assert!(text.contains("\"computer\": 32"));
    // Loose bounds for an unoptimized build on a shared machine.
    assert!(locked < Duration::from_millis(200), "{locked:?}");
    assert!(rendered < Duration::from_secs(10), "{rendered:?}");
    // Replacing a fleet's drawing hands the old one back: only the swap
    // happens under the lock, freeing it after.
    page.age();
    let drawing = page.draw_due(NOW_MS).unwrap().render();
    let start = Instant::now();
    let replaced = page.drawn(drawing);
    let swapped = start.elapsed();
    let start = Instant::now();
    drop(replaced);
    let freed = start.elapsed();
    eprintln!("replacing it: {swapped:?} under the lock, {freed:?} freeing the old one after");
    assert!(swapped < Duration::from_millis(5), "{swapped:?}");
    assert_eq!(
        page.view(NOW_MS).report.unwrap().buckets,
        MAX_COMPUTERS * MAX_BUCKETS
    );
}

#[test]
fn a_drawing_that_finishes_after_a_newer_one_never_replaces_it() {
    // Reads every route as `count` completed turns, leaving the drawing to
    // the test.
    let read = |page: &mut Page, count: u64| {
        page.refresh(ALL, "2026-01-01", "2026-01-30");
        while let Some((route, ..)) = page.next_route() {
            let agent = parsed(vec![bucket(first(), "codex", count)]);
            page.summary_answered(&route, Ok(agent), NOW_MS);
            page.speech_answered(&route, local_voice(), NOW_MS);
        }
        page.draw_due(NOW_MS).unwrap()
    };
    let totals = |page: &Page| page.view(NOW_MS).report.unwrap().totals["turn_completed"];
    let mut page = Page::new();
    let older = read(&mut page, 3).render();
    let newer = read(&mut page, 5).render();
    drop(page.drawn(newer));
    assert_eq!(totals(&page), 5.0);
    // The older drawing comes back untouched, for the caller to free.
    drop(page.drawn(older));
    assert_eq!(totals(&page), 5.0);
    assert_eq!(page.rows("", 0).total, 1);
    // A later drawing replaces it again.
    let latest = read(&mut page, 7).render();
    drop(page.drawn(latest));
    assert_eq!(totals(&page), 7.0);
}

#[test]
fn an_unknown_selection_reads_this_computer_and_the_view_keeps_the_choice() {
    let mut page = Page::new();
    page.set_peers(["mac".to_owned()]);
    assert!(page.refresh("not-a-peer", "2026-01-01", "2026-01-30"));
    assert_eq!(page.next_route().unwrap().0, "");
    assert!(page.next_route().is_none());
    let view = shown(&mut page);
    assert_eq!(view.selection, "");
    page.refresh(ALL, "2026-01-02", "2026-01-30");
    let view = page.view(NOW_MS);
    assert_eq!(
        (
            view.selection.as_str(),
            view.from.as_deref(),
            view.through.as_deref()
        ),
        ("*", Some("2026-01-02"), Some("2026-01-30"))
    );
}

#[test]
fn local_controls_stay_usable_when_a_peer_is_offline() {
    let mut page = Page::new();
    page.set_peers(["offline".to_owned()]);
    page.refresh(ALL, "2026-01-01", "2026-01-30");
    read_all(&mut page, &|route| {
        if route.is_empty() {
            (Ok(parsed(vec![bucket(first(), "codex", 3)])), local_voice())
        } else {
            let gone = "Computer unavailable. No current analytics read.";
            (Err(gone.into()), Err(gone.into()))
        }
    });
    let view = shown(&mut page);
    assert!(view.can_change_local);
    let report = view.report.unwrap();
    assert_eq!(report.totals["turn_completed"], 3.0);
    assert_eq!(report.missing[0].route, "offline");
    assert_eq!(
        report.missing[0].error,
        "Computer unavailable. No current analytics read."
    );
}

#[test]
fn a_negative_zero_reads_as_zero() {
    let mut value = raw(vec![bucket(first(), "codex", 3)]);
    value["buckets"][0]["counters"]["failure_network"] = json!(-0.0);
    let counter = read(&value).unwrap().buckets[0].counters["failure_network"];
    assert!(counter == 0.0 && counter.is_sign_positive());
    let mut speech_value = voice();
    speech_value["days"][0]["queue_ms"] = json!(-0.0);
    let parsed = speech(&bytes(&speech_value), first(), last()).unwrap();
    assert!(parsed.days[0].counters["queue_ms"].is_sign_positive());
}

#[test]
fn configure_is_checked_at_the_click_and_never_retried() {
    use page::{Configure, CHANGING, NOT_CURRENT_APPLY, READING};
    let mut page = Page::new();
    // Nothing read from this computer yet: the status says why.
    assert_eq!(
        page.begin_configure(true, 30, NOW_MS),
        Err(NOT_CURRENT_APPLY.into())
    );
    assert_eq!(shown(&mut page).status, NOT_CURRENT_APPLY);
    page.refresh("", "2026-01-01", "2026-01-30");
    assert_eq!(page.begin_configure(true, 30, NOW_MS), Err(READING.into()));
    assert_eq!(shown(&mut page).status, READING);
    read_all(&mut page, &|_| (Ok(parsed(vec![])), local_voice()));
    // Qt checks fresh(m_local) at the click: a view that allowed the
    // change a while ago does not.
    assert!(shown(&mut page).can_change_local);
    let later = NOW_MS + 301_000;
    assert_eq!(
        page.begin_configure(true, 30, later),
        Err(NOT_CURRENT_APPLY.into())
    );
    assert!(!page.view(later).mutating);
    assert_eq!(
        page.begin_configure(true, 91, NOW_MS),
        Err("Choose a retention of 1 to 90 days.".into())
    );
    assert_eq!(
        page.view(NOW_MS).status,
        "Choose a retention of 1 to 90 days."
    );
    // The same or a longer retention is made at once, pinned to the
    // epoch of the read.
    let change = page.begin_configure(false, 90, NOW_MS).unwrap();
    assert_eq!(
        change,
        Some(Configure {
            epoch: EPOCH.into(),
            enabled: false,
            retention_days: 90
        })
    );
    let view = shown(&mut page);
    assert_eq!(view.status, "Applying local collection settings…");
    assert!(view.mutating && !view.can_refresh && !view.can_change_local);
    assert_eq!(view.shorten_pending, None);
    assert_eq!(page.begin_configure(true, 90, NOW_MS), Err(CHANGING.into()));
    assert!(!page.refresh("", "2026-01-01", "2026-01-30"));
    let unconfirmed = "Change was not confirmed. Refresh and review before another action.";
    assert!(!page.configure_answered(Err(unconfirmed.into())));
    let view = shown(&mut page);
    assert_eq!(view.status, unconfirmed);
    assert!(!view.mutating && view.local.is_none() && !view.can_change_local);
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| (Ok(parsed(vec![])), local_voice()));
    // A shorter retention holds the page while the person is asked.
    assert_eq!(page.begin_configure(true, 30, NOW_MS), Ok(None));
    let view = shown(&mut page);
    assert_eq!(view.shorten_pending, Some(30));
    assert!(view.mutating && !view.can_refresh && !view.can_change_local);
    assert!(!page.refresh("", "2026-01-01", "2026-01-30"));
    page.configure_cancelled();
    let view = shown(&mut page);
    assert!(!view.mutating && view.shorten_pending.is_none() && view.can_change_local);
    assert_eq!(page.configure_confirmed(), None);
    // Confirmed: the values and epoch of the click.
    assert_eq!(page.begin_configure(true, 30, NOW_MS), Ok(None));
    let change = page.configure_confirmed().unwrap();
    assert_eq!(
        (change.epoch.as_str(), change.enabled, change.retention_days),
        (EPOCH, true, 30)
    );
    let view = shown(&mut page);
    assert_eq!(view.status, "Applying local collection settings…");
    assert!(view.mutating && view.shorten_pending.is_none());
    // A cancel once the change runs leaves it alone.
    page.configure_cancelled();
    assert!(shown(&mut page).mutating);
    assert!(page.configure_answered(Ok(())));
}

#[test]
fn a_purge_waits_for_a_fresh_epoch_and_the_persons_confirmation() {
    let mut page = Page::new();
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| {
        (Ok(parsed(vec![bucket(first(), "codex", 3)])), local_voice())
    });
    assert_eq!(page.begin_purge(NOW_MS), Ok((first(), last())));
    assert!(shown(&mut page).mutating);
    // A second click while it checks says why nothing happens.
    assert_eq!(page.begin_purge(NOW_MS), Err(page::CHANGING.into()));
    // Cancelled: nothing changes and nothing is left pending.
    assert!(page.purge_checked(Ok(parsed(vec![])), NOW_MS));
    assert!(shown(&mut page).purge_pending);
    page.purge_cancelled();
    let view = shown(&mut page);
    assert!(!view.mutating && !view.purge_pending && view.can_change_local);
    assert_eq!(page.purge_confirmed(), None);
    // Leaving while the check runs: its answer ends the purge, unasked.
    page.begin_purge(NOW_MS).unwrap();
    page.purge_cancelled();
    assert!(shown(&mut page).mutating);
    assert!(!page.purge_checked(Ok(parsed(vec![])), NOW_MS));
    let view = shown(&mut page);
    assert!(!view.mutating && !view.purge_pending && view.can_change_local);
    // The next purge asks again.
    page.begin_purge(NOW_MS).unwrap();
    assert!(page.purge_checked(Ok(parsed(vec![])), NOW_MS));
    page.purge_cancelled();
    // A cancel outside a purge leaves a running change alone.
    page.begin_configure(true, 90, NOW_MS).unwrap();
    page.purge_cancelled();
    assert!(shown(&mut page).mutating);
    page.configure_answered(Ok(()));
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| {
        (Ok(parsed(vec![bucket(first(), "codex", 3)])), local_voice())
    });
    // A stale latest summary ends the purge.
    page.begin_purge(NOW_MS).unwrap();
    let mut stale = parsed(vec![]);
    stale.generated_at -= 400.0;
    assert!(!page.purge_checked(Ok(stale), NOW_MS));
    let view = shown(&mut page);
    assert_eq!(
        view.status,
        "Local analytics epoch is stale or ambiguous. Refresh before purging."
    );
    assert!(!view.mutating);
    // The confirmed purge pins the latest epoch read, a reset one here.
    page.begin_purge(NOW_MS).unwrap();
    let mut reset = parsed(vec![]);
    reset.source_epoch = RESET.into();
    reset.generated_at += 1.0;
    assert!(page.purge_checked(Ok(reset), NOW_MS));
    assert_eq!(page.purge_confirmed().as_deref(), Some(RESET));
    assert!(shown(&mut page).mutating);
    assert!(page.purge_answered(Ok(())));
    assert!(page.observations(NOW_MS)[0].agent.is_none());
    // A failed purge says so and draws the forgotten reads.
    page.refresh("", "2026-01-01", "2026-01-30");
    read_all(&mut page, &|_| {
        let mut agent = parsed(vec![]);
        agent.source_epoch = RESET.into();
        agent.generated_at += 1.0;
        (Ok(agent), local_voice())
    });
    page.begin_purge(NOW_MS).unwrap();
    assert!(!page.purge_checked(
        Err("Computer unavailable. No current analytics read.".into()),
        NOW_MS
    ));
    assert!(!shown(&mut page).mutating);
}

#[test]
fn the_diagnostic_review_waits_for_reads_and_changes() {
    let mut page = Page::new();
    page.refresh("", "2026-01-01", "2026-01-30");
    assert!(page.diagnostic(NOW_MS).is_none());
    read_all(&mut page, &|_| {
        (Ok(parsed(vec![bucket(first(), "codex", 3)])), local_voice())
    });
    let text = page.diagnostic(NOW_MS).unwrap();
    assert!(text.contains("\"kind\": \"local_aggregate_diagnostics\""));
    assert!(!text.contains(MACHINE) && !text.contains("profile_id"));
    page.begin_configure(true, 90, NOW_MS).unwrap();
    assert!(page.diagnostic(NOW_MS).is_none());
}
