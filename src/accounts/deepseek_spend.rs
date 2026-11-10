//! Account-wide DeepSeek spend from successive official balance readings of
//! an API-key account. DeepSeek publishes no usage or cost endpoint, but every
//! account-status refresh already reads `/user/balance`; a falling balance
//! between two readings is money spent by any key, tool or computer.
//!
//! A topped-up increase means a top-up whose amount mixes with any spend in
//! that interval, so the whole interval is left out. A granted decrease while
//! nothing was topped up counts as spend and is reported separately, because
//! an expiring grant looks the same. The journal keeps the last reading and
//! daily totals for a fixed retention, per account profile and key identity.
use super::*;
use chrono::{Datelike, Local, NaiveDate, TimeZone};

const RETENTION_DAYS: i64 = 400;
const JOURNAL_BYTES: u64 = 512 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Parts {
    topped_up: i64,
    granted: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Reading {
    at: u64,
    currencies: BTreeMap<String, Parts>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Spent {
    topped_up: i64,
    granted: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Day {
    day: String,
    /// The earliest previous reading of an interval counted here; before the
    /// day's start when the interval spanned midnight without a reading.
    from: u64,
    top_up: bool,
    spent: BTreeMap<String, Spent>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Journal {
    schema: u32,
    identity: String,
    /// The first reading: spend before it is unknown.
    since: u64,
    last: Option<Reading>,
    days: Vec<Day>,
}

/// Micro-units of a decimal amount ("6.20" is 6,200,000); None past 6 decimals.
fn micros(amount: &str) -> Option<i64> {
    let (negative, digits) = match amount.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, amount),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty()
        || whole.len() > 12
        || fraction.len() > 6
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let value =
        whole.parse::<i64>().ok()? * 1_000_000 + format!("{fraction:0<6}").parse::<i64>().ok()?;
    Some(if negative { -value } else { value })
}

/// "6.20" for 6,200,000 micro-units: at least two decimals, at most six.
fn amount(micros: i64) -> String {
    let sign = if micros < 0 { "-" } else { "" };
    let value = micros.unsigned_abs();
    let mut fraction = format!("{:06}", value % 1_000_000);
    while fraction.len() > 2 && fraction.ends_with('0') {
        fraction.pop();
    }
    format!("{sign}{}.{fraction}", value / 1_000_000)
}

/// The Topped up and Granted parts per currency of the projected balance rows.
fn reading(balances: &Value) -> BTreeMap<String, Parts> {
    let mut parts: BTreeMap<String, (Option<i64>, Option<i64>)> = BTreeMap::new();
    for row in balances.as_array().into_iter().flatten() {
        let currency = row["currency"].as_str().unwrap_or_default();
        if !matches!(currency, "USD" | "CNY") {
            continue;
        }
        let value = row["balance"].as_str().and_then(micros);
        let entry = parts.entry(currency.to_owned()).or_default();
        match row["kind"].as_str() {
            Some("Topped up") => entry.0 = value,
            Some("Granted") => entry.1 = value,
            _ => {}
        }
    }
    parts
        .into_iter()
        .filter_map(|(currency, part)| match part {
            (Some(topped_up), Some(granted)) => Some((currency, Parts { topped_up, granted })),
            _ => None,
        })
        .collect()
}

/// Adds one reading taken at `at` on local `day`; days before `oldest` are dropped.
fn update(
    journal: &mut Journal,
    identity: &str,
    at: u64,
    day: &str,
    oldest: &str,
    now: BTreeMap<String, Parts>,
) {
    if journal.schema != 1 || journal.identity != identity {
        *journal = Journal {
            schema: 1,
            identity: identity.to_owned(),
            since: at,
            last: None,
            days: Vec::new(),
        };
    }
    if let Some(last) = &journal.last {
        // A clock that went back keeps the later reading as the baseline.
        if at <= last.at {
            return;
        }
        let mut spent = BTreeMap::new();
        let mut top_up = false;
        for (currency, parts) in &now {
            let Some(before) = last.currencies.get(currency) else {
                continue;
            };
            let topped_up = parts.topped_up - before.topped_up;
            if topped_up > 0 {
                top_up = true;
                continue;
            }
            let granted = (before.granted - parts.granted).max(0);
            if topped_up < 0 || granted > 0 {
                spent.insert(
                    currency.clone(),
                    Spent {
                        topped_up: -topped_up,
                        granted,
                    },
                );
            }
        }
        if !spent.is_empty() || top_up {
            let index = match journal.days.iter().position(|d| d.day == day) {
                Some(index) => index,
                None => {
                    journal.days.push(Day {
                        day: day.to_owned(),
                        from: last.at,
                        top_up: false,
                        spent: BTreeMap::new(),
                    });
                    journal.days.len() - 1
                }
            };
            let entry = &mut journal.days[index];
            entry.from = entry.from.min(last.at);
            entry.top_up |= top_up;
            for (currency, add) in spent {
                let total = entry.spent.entry(currency).or_default();
                total.topped_up += add.topped_up;
                total.granted += add.granted;
            }
        }
    }
    journal.last = Some(Reading {
        at,
        currencies: now,
    });
    journal.days.retain(|d| d.day.as_str() >= oldest);
    journal.days.sort_by(|a, b| a.day.cmp(&b.day));
    let excess = journal.days.len().saturating_sub(RETENTION_DAYS as usize);
    journal.days.drain(..excess);
}

/// Today's and this month's spend per currency. `gap_from` names the previous
/// reading when the first counted interval began before the period.
fn project(journal: &Journal, today: &str, today_start: u64, month_start: u64) -> Value {
    let month = &today[..7.min(today.len())];
    let currencies: BTreeSet<&String> = journal
        .last
        .iter()
        .flat_map(|r| r.currencies.keys())
        .chain(journal.days.iter().flat_map(|d| d.spent.keys()))
        .collect();
    let periods: Vec<Value> = [("today", today_start), ("month", month_start)]
        .into_iter()
        .map(|(period, start)| {
            let days: Vec<&Day> = journal
                .days
                .iter()
                .filter(|d| if period == "today" { d.day == today } else { d.day.starts_with(month) })
                .collect();
            let from = days.iter().map(|d| d.from).min();
            let amounts: Vec<Value> = currencies
                .iter()
                .take(4)
                .map(|currency| {
                    let (topped_up, granted) = days
                        .iter()
                        .filter_map(|d| d.spent.get(*currency))
                        .fold((0, 0), |(t, g), s| (t + s.topped_up, g + s.granted));
                    json!({"currency":currency,"spent":amount(topped_up + granted),"granted":amount(granted)})
                })
                .collect();
            json!({"period":period,"amounts":amounts,"top_up":days.iter().any(|d| d.top_up),
                "gap_from":from.filter(|from| *from < start)})
        })
        .collect();
    json!({"since":journal.since,"today":today,"periods":periods})
}

fn local_midnight(date: NaiveDate) -> u64 {
    date.and_hms_opt(0, 0, 0)
        .and_then(|t| Local.from_local_datetime(&t).earliest())
        .map(|t| t.timestamp().max(0) as u64)
        .unwrap_or(0)
}

/// Records this inspection's API-key balance and returns the spend summary,
/// or None without an identified key and a complete balance reading.
pub(super) fn record(config: &Config, id: &str, home: &Path, data: &Value) -> Option<Value> {
    record_at(config, id, home, data, super::inspection::now())
}

fn record_at(config: &Config, id: &str, home: &Path, data: &Value, at: u64) -> Option<Value> {
    let identity = data["identity"]["account_id"]
        .as_str()
        .filter(|v| v.starts_with("api-key:"))?;
    let now = reading(&data["balances"]);
    if now.is_empty() {
        return None;
    }
    let path = super::deepseek::state_path(config, "account-spend", id, home);
    platform::private_dir(path.parent()?).ok()?;
    let local = Local.timestamp_opt(at as i64, 0).single()?;
    let today = local.date_naive();
    let oldest = (today - chrono::Duration::days(RETENTION_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let day = today.format("%Y-%m-%d").to_string();
    let mut journal: Journal =
        super::deepseek::read_private(&path, JOURNAL_BYTES).unwrap_or_default();
    // Another process updating the journal makes this reading redundant.
    if let Some(_lock) = super::deepseek::try_lock(&path) {
        journal = super::deepseek::read_private(&path, JOURNAL_BYTES).unwrap_or_default();
        update(&mut journal, identity, at, &day, &oldest, now);
        let _ = super::deepseek::write_private(&path, &journal);
    }
    if journal.identity != identity {
        return None;
    }
    let month_start = local_midnight(today.with_day(1)?);
    Some(project(&journal, &day, local_midnight(today), month_start))
}

/// The cached summary's known fields only, bounded.
pub(super) fn public(value: &Value) -> Option<Value> {
    let decimal = |v: &Value| {
        v.as_str()
            .filter(|s| s.len() <= 24 && micros(s).is_some())
            .map(str::to_owned)
    };
    let periods: Vec<Value> = value["periods"]
        .as_array()?
        .iter()
        .take(2)
        .filter(|p| matches!(p["period"].as_str(), Some("today" | "month")))
        .map(|p| {
            let amounts: Vec<Value> = p["amounts"]
                .as_array()
                .into_iter()
                .flatten()
                .take(4)
                .filter_map(|a| {
                    let currency = a["currency"].as_str().filter(|c| matches!(*c, "USD" | "CNY"))?;
                    Some(json!({"currency":currency,"spent":decimal(&a["spent"])?,"granted":decimal(&a["granted"])?}))
                })
                .collect();
            json!({"period":p["period"],"amounts":amounts,"top_up":p["top_up"].as_bool().unwrap_or(false),
                "gap_from":p["gap_from"].as_u64()})
        })
        .collect();
    let today = value["today"].as_str().filter(|d| d.len() == 10)?;
    Some(json!({"since":value["since"].as_u64()?,"today":today,"periods":periods}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(topped_up: &str, granted: &str) -> BTreeMap<String, Parts> {
        BTreeMap::from([(
            "USD".to_owned(),
            Parts {
                topped_up: micros(topped_up).unwrap(),
                granted: micros(granted).unwrap(),
            },
        )])
    }

    fn spent(journal: &Journal, day: &str) -> Option<Spent> {
        journal
            .days
            .iter()
            .find(|d| d.day == day)
            .and_then(|d| d.spent.get("USD").copied())
    }

    #[test]
    fn amounts_round_trip_as_micro_units() {
        assert_eq!(micros("6.20"), Some(6_200_000));
        assert_eq!(micros("-0.03"), Some(-30_000));
        assert_eq!(micros("110"), Some(110_000_000));
        assert_eq!(micros("0.1234567"), None);
        assert_eq!(micros("1e3"), None);
        assert_eq!(micros(""), None);
        assert_eq!(amount(6_200_000), "6.20");
        assert_eq!(amount(-30_000), "-0.03");
        assert_eq!(amount(1_234_500), "1.2345");
        assert_eq!(amount(0), "0.00");
    }

    #[test]
    fn falling_topped_up_balance_is_spend_and_the_first_reading_only_starts_coverage() {
        let mut journal = Journal::default();
        update(
            &mut journal,
            "api-key:a",
            1000,
            "2026-11-10",
            "2025-10-06",
            usd("6.24", "0.00"),
        );
        assert_eq!(journal.since, 1000);
        assert!(
            journal.days.is_empty(),
            "nothing is known before the first reading"
        );
        update(
            &mut journal,
            "api-key:a",
            1300,
            "2026-11-10",
            "2025-10-06",
            usd("6.20", "0.00"),
        );
        update(
            &mut journal,
            "api-key:a",
            1600,
            "2026-11-10",
            "2025-10-06",
            usd("6.10", "0.00"),
        );
        assert_eq!(
            spent(&journal, "2026-11-10"),
            Some(Spent {
                topped_up: 140_000,
                granted: 0
            })
        );
        assert_eq!(journal.days[0].from, 1000);
        let summary = project(&journal, "2026-11-10", 900, 100);
        assert_eq!(summary["since"], 1000);
        assert_eq!(
            summary["periods"][0],
            json!({"period":"today","amounts":[{"currency":"USD","spent":"0.14","granted":"0.00"}],"top_up":false,"gap_from":null})
        );
        assert_eq!(summary["periods"][1]["amounts"][0]["spent"], "0.14");
    }

    #[test]
    fn a_top_up_interval_is_left_out_and_flagged() {
        let mut journal = Journal::default();
        update(
            &mut journal,
            "api-key:a",
            1000,
            "2026-11-10",
            "2025-10-06",
            usd("1.00", "0.00"),
        );
        // Topped up 10 and spent 0.20 in the same interval: neither is knowable.
        update(
            &mut journal,
            "api-key:a",
            1300,
            "2026-11-10",
            "2025-10-06",
            usd("10.80", "0.00"),
        );
        assert_eq!(spent(&journal, "2026-11-10"), None);
        assert!(journal.days[0].top_up);
        update(
            &mut journal,
            "api-key:a",
            1600,
            "2026-11-10",
            "2025-10-06",
            usd("10.50", "0.00"),
        );
        assert_eq!(
            spent(&journal, "2026-11-10"),
            Some(Spent {
                topped_up: 300_000,
                granted: 0
            })
        );
        assert_eq!(
            project(&journal, "2026-11-10", 900, 100)["periods"][0]["top_up"],
            true
        );
    }

    #[test]
    fn granted_decreases_count_separately_and_a_new_grant_is_not_spend() {
        let mut journal = Journal::default();
        update(
            &mut journal,
            "api-key:a",
            1000,
            "2026-11-10",
            "2025-10-06",
            usd("5.00", "2.00"),
        );
        update(
            &mut journal,
            "api-key:a",
            1300,
            "2026-11-10",
            "2025-10-06",
            usd("5.00", "1.50"),
        );
        update(
            &mut journal,
            "api-key:a",
            1600,
            "2026-11-10",
            "2025-10-06",
            usd("4.90", "0.00"),
        );
        assert_eq!(
            spent(&journal, "2026-11-10"),
            Some(Spent {
                topped_up: 100_000,
                granted: 2_000_000
            })
        );
        // A new grant raises granted without a top-up: no spend, no flag.
        update(
            &mut journal,
            "api-key:a",
            1900,
            "2026-11-10",
            "2025-10-06",
            usd("4.90", "3.00"),
        );
        assert_eq!(
            spent(&journal, "2026-11-10"),
            Some(Spent {
                topped_up: 100_000,
                granted: 2_000_000
            })
        );
        assert!(!journal.days[0].top_up);
        let today = &project(&journal, "2026-11-10", 900, 100)["periods"][0]["amounts"][0];
        assert_eq!(today["spent"], "2.10");
        assert_eq!(today["granted"], "2.00");
    }

    #[test]
    fn an_interval_across_midnight_counts_on_its_end_day_and_names_the_gap() {
        let mut journal = Journal::default();
        update(
            &mut journal,
            "api-key:a",
            1000,
            "2026-11-09",
            "2025-10-06",
            usd("6.00", "0.00"),
        );
        update(
            &mut journal,
            "api-key:a",
            90_000,
            "2026-11-10",
            "2025-10-06",
            usd("5.50", "0.00"),
        );
        let summary = project(&journal, "2026-11-10", 86_400, 100);
        assert_eq!(summary["periods"][0]["gap_from"], 1000);
        assert_eq!(
            summary["periods"][1]["gap_from"],
            json!(null),
            "the gap began inside the month"
        );
        assert_eq!(summary["periods"][0]["amounts"][0]["spent"], "0.50");
    }

    #[test]
    fn a_new_key_restarts_coverage_and_a_clock_step_back_is_ignored() {
        let mut journal = Journal::default();
        update(
            &mut journal,
            "api-key:a",
            1000,
            "2026-11-10",
            "2025-10-06",
            usd("6.00", "0.00"),
        );
        update(
            &mut journal,
            "api-key:a",
            1300,
            "2026-11-10",
            "2025-10-06",
            usd("5.00", "0.00"),
        );
        update(
            &mut journal,
            "api-key:a",
            1200,
            "2026-11-10",
            "2025-10-06",
            usd("4.00", "0.00"),
        );
        assert_eq!(journal.last.as_ref().unwrap().at, 1300);
        assert_eq!(spent(&journal, "2026-11-10").unwrap().topped_up, 1_000_000);
        update(
            &mut journal,
            "api-key:b",
            1400,
            "2026-11-10",
            "2025-10-06",
            usd("50.00", "0.00"),
        );
        assert_eq!(
            (journal.identity.as_str(), journal.since, journal.days.len()),
            ("api-key:b", 1400, 0)
        );
    }

    #[test]
    fn retention_drops_old_days_and_month_sums_its_days() {
        let mut journal = Journal::default();
        update(
            &mut journal,
            "api-key:a",
            1000,
            "2025-01-01",
            "2024-01-01",
            usd("9.00", "0.00"),
        );
        update(
            &mut journal,
            "api-key:a",
            2000,
            "2025-01-01",
            "2024-01-01",
            usd("8.00", "0.00"),
        );
        update(
            &mut journal,
            "api-key:a",
            3000,
            "2026-11-01",
            "2025-10-06",
            usd("7.50", "0.00"),
        );
        update(
            &mut journal,
            "api-key:a",
            4000,
            "2026-11-10",
            "2025-10-06",
            usd("7.25", "0.00"),
        );
        assert_eq!(
            journal
                .days
                .iter()
                .map(|d| d.day.as_str())
                .collect::<Vec<_>>(),
            ["2026-11-01", "2026-11-10"]
        );
        let month = &project(&journal, "2026-11-10", 3500, 2500)["periods"][1];
        assert_eq!(month["amounts"][0]["spent"], "0.75");
    }

    #[test]
    fn readings_need_both_parts_of_a_known_currency() {
        let rows = json!([
            {"kind":"Total","balance":"6.20","currency":"USD"},{"kind":"Topped up","balance":"6.20","currency":"USD"},
            {"kind":"Granted","balance":"0.00","currency":"USD"},
            {"kind":"Total","balance":"1.00","currency":"CNY"},{"kind":"Topped up","balance":"1.00","currency":"CNY"},
            {"kind":"Wallet","balance":"3","currency":"EUR"}]);
        assert_eq!(reading(&rows), usd("6.20", "0.00"));
        assert!(reading(&json!([{"kind":"Wallet","balance":"5","currency":"USD"}])).is_empty());
    }

    #[test]
    fn readings_persist_privately_per_profile_and_key() {
        let root = tempfile::tempdir().unwrap();
        let config = Config {
            home: root.path().to_string_lossy().into(),
            dir: root.path().join("config"),
            state: root.path().join("state"),
            selves: vec!["fixture".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        };
        platform::private_dir(&config.dir).unwrap();
        let home = root.path().join("dsh");
        let data = |topped: &str| {
            json!({"identity":{"auth_method":"API key","account_id":"api-key:abc"},"balances":[
            {"kind":"Total","balance":topped,"currency":"USD"},{"kind":"Topped up","balance":topped,"currency":"USD"},
            {"kind":"Granted","balance":"0.00","currency":"USD"}]})
        };
        let at = Local::now().timestamp() as u64;
        let first = record_at(&config, "native-dsh", &home, &data("6.24"), at - 60).unwrap();
        assert_eq!(first["since"], at - 60);
        assert_eq!(first["periods"][0]["amounts"][0]["spent"], "0.00");
        let second = record_at(&config, "native-dsh", &home, &data("6.20"), at).unwrap();
        assert_eq!(second["periods"][0]["amounts"][0]["spent"], "0.04");
        assert_eq!(second["periods"][1]["amounts"][0]["spent"], "0.04");
        let path =
            super::super::deepseek::state_path(&config, "account-spend", "native-dsh", &home);
        let file = fs::File::open(&path).unwrap();
        assert!(platform::verify_private_file(&file).unwrap());
        // Another profile keeps its own journal; a sign-in without an API key keeps none.
        assert_eq!(
            record_at(&config, "other", &home, &data("1.00"), at).unwrap()["since"],
            at
        );
        assert!(record_at(
            &config,
            "native-dsh",
            &home,
            &json!({"identity":{"email":"x"},"balances":[]}),
            at + 60
        )
        .is_none());
    }

    #[test]
    fn the_cached_summary_keeps_only_known_bounded_fields() {
        let summary = json!({"since":1000,"today":"2026-11-10","extra":"x","periods":[
            {"period":"today","amounts":[{"currency":"USD","spent":"0.14","granted":"0.00","note":"x"},{"currency":"EUR","spent":"1","granted":"0"}],"top_up":true,"gap_from":null},
            {"period":"week","amounts":[]}]});
        assert_eq!(
            public(&summary),
            Some(json!({"since":1000,"today":"2026-11-10","periods":[
                {"period":"today","amounts":[{"currency":"USD","spent":"0.14","granted":"0.00"}],"top_up":true,"gap_from":null}]}))
        );
        assert_eq!(public(&json!({"today":"2026-11-10","periods":[]})), None);
    }
}
