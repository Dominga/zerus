//! U62: recorded test times, kept in the repository and read back for an
//! estimate. `hgs test durations --record` reads what runs print: CTest's
//! summary lines, a QtTest `Totals:` line after its binary's name, Cargo's
//! `Running` and `test result` lines (and libtest's JSON `exec_time`), and
//! unittest's or pytest's closing line for a named target.

use super::map::{ProviderMap, TestMap};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Durations {
    pub schema: u32,
    /// `provider/target` to its time.
    #[serde(default)]
    pub targets: BTreeMap<String, Timing>,
    /// `provider/group` (a cargo crate, a QtTest binary) to its whole time
    /// and test count, for targets with no time of their own.
    #[serde(default)]
    pub groups: BTreeMap<String, Timing>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Timing {
    pub seconds: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tests: usize,
    /// The day it was measured.
    pub recorded: String,
    /// Measurements merged into it; the largest is kept.
    pub runs: u32,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

impl Durations {
    pub fn read(path: &std::path::Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(serde_json::from_str(&text).map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self { schema: 1, ..Self::default() }),
            Err(error) => anyhow::bail!("Cannot read {}: {error}", path.display()),
        }
    }

    fn put(list: &mut BTreeMap<String, Timing>, key: String, seconds: f64, tests: usize, day: &str) {
        let entry = list.entry(key).or_default();
        // A suite's slowest measured run is its budget: one fast run on an
        // idle machine must not promise less than the busy one took. A crate's
        // runs differ in how many tests they filtered: the slower per test wins.
        let rate = |seconds: f64, tests: usize| if tests > 0 { seconds / tests as f64 } else { seconds };
        let slower = if tests > 0 && entry.tests > 0 { rate(seconds, tests) > rate(entry.seconds, entry.tests) } else { seconds > entry.seconds };
        if entry.runs == 0 || slower {
            entry.seconds = (seconds * 1000.0).round() / 1000.0;
            if tests > 0 {
                entry.tests = tests;
            }
        }
        entry.recorded = day.to_owned();
        entry.runs += 1;
    }

    /// Records what `text` measured for `provider`; returns how many times.
    pub fn record(&mut self, provider: &str, section: &ProviderMap, text: &str, target: Option<&str>, day: &str) -> usize {
        let mut count = 0;
        // A log without Cargo's `Running` lines (stdout only) is the one crate's.
        let mut crate_arg: Option<String> = (section.groups.len() == 1).then(|| section.groups.keys().next().cloned()).flatten();
        let mut qt_binary: Option<String> = None;
        let mut per_test: BTreeMap<String, f64> = BTreeMap::new();
        // A JSON object of test ids to seconds, such as a runner's shard
        // seed (`"test_cli.Cli[1/2]": 12`): summed per test module.
        if let Ok(serde_json::Value::Object(times)) = serde_json::from_str::<serde_json::Value>(text) {
            let mut sums: BTreeMap<String, f64> = BTreeMap::new();
            for (name, seconds) in &times {
                let Some(seconds) = seconds.as_f64() else { continue };
                let module = name.split(['.', '[']).next().unwrap_or_default();
                if section.targets.contains_key(module) {
                    *sums.entry(module.to_owned()).or_default() += seconds;
                }
            }
            for (module, seconds) in sums {
                Self::put(&mut self.targets, format!("{provider}/{module}"), seconds, 0, day);
                count += 1;
            }
            return count;
        }
        for line in text.lines() {
            let line = line.trim();
            // Cargo: `Running unittests src/main.rs (target/debug/deps/hgs-1)`.
            if let Some(rest) = line.strip_prefix("Running ") {
                // Windows prints `src\main.rs`.
                let file = rest.split_whitespace().find(|word| word.ends_with(".rs")).unwrap_or_default().replace('\\', "/");
                crate_arg = section.groups.iter().find(|(_, root)| **root == file).map(|(group, _)| group.clone());
                continue;
            }
            if let Some(rest) = line.strip_prefix("test result: ") {
                let passed = number_before(rest, " passed").unwrap_or(0) + number_before(rest, " failed").unwrap_or(0);
                if let (Some(group), Some(seconds)) = (crate_arg.as_ref(), seconds_after(rest, "finished in ")) {
                    if passed > 0 {
                        Self::put(&mut self.groups, format!("{provider}/{group}"), seconds, passed, day);
                        count += 1;
                    }
                }
                continue;
            }
            // libtest JSON (`--format json --report-time`).
            if line.starts_with('{') && line.contains("\"exec_time\"") {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                    if let (Some(name), Some(seconds)) = (event["name"].as_str(), event["exec_time"].as_f64()) {
                        per_test.insert(name.to_owned(), seconds);
                    }
                }
                continue;
            }
            // CTest: `12/190 Test  #12: hgsclient ......   Passed   26.58 sec`.
            if let Some(rest) = line.split_once(" Test ").map(|(_, rest)| rest).filter(|_| line.ends_with(" sec")) {
                let name = rest.split(':').nth(1).map(|part| part.split_whitespace().next().unwrap_or_default()).unwrap_or_default();
                let seconds = rest.trim_end_matches(" sec").rsplit(' ').next().and_then(|value| value.parse::<f64>().ok());
                if let Some(seconds) = seconds {
                    if let Some((id, _)) = section.targets.iter().find(|(_, target)| target.run.iter().any(|run| run == name)) {
                        Self::put(&mut self.targets, format!("{provider}/{id}#{name}"), seconds, 0, day);
                        count += 1;
                    }
                }
                continue;
            }
            // QtTest: `********* Start testing of TestFoo *********` names
            // nothing a map knows; `test_foo: ... Totals: ..., 812ms` does.
            if let Some((head, totals)) = line.split_once("Totals:") {
                let binary = head.split(':').next().map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned).or(qt_binary.clone());
                let millis = totals.split(',').filter_map(|part| part.trim().strip_suffix("ms")).filter_map(|value| value.trim().parse::<f64>().ok()).next_back();
                if let (Some(binary), Some(millis)) = (binary, millis) {
                    let id = binary.trim_end_matches(".exe").to_owned();
                    if section.targets.contains_key(&id) {
                        let tests = section.targets[&id].tests;
                        Self::put(&mut self.groups, format!("{provider}/{id}"), millis / 1000.0, tests, day);
                        count += 1;
                    }
                }
                continue;
            }
            if let Some(name) = line.strip_prefix("Testing: ").or_else(|| line.strip_prefix("Test binary: ")) {
                qt_binary = Some(name.trim().to_owned());
                continue;
            }
            // unittest `Ran 12 tests in 3.456s`, pytest `== 12 passed in 3.45s ==`.
            if let Some(target) = target {
                let seconds = line
                    .strip_prefix("Ran ")
                    .and_then(|rest| rest.split_once(" in ").map(|(_, time)| time.trim_end_matches('s')))
                    .or_else(|| line.contains(" passed").then(|| line.rsplit(" in ").next().unwrap_or_default().trim_end_matches(['=', ' ', 's'])))
                    .and_then(|value| value.trim().parse::<f64>().ok());
                if let Some(seconds) = seconds {
                    Self::put(&mut self.targets, format!("{provider}/{target}"), seconds, 0, day);
                    count += 1;
                }
            }
        }
        // libtest's per-test times, summed into the map's module targets.
        if !per_test.is_empty() {
            let mut sums: BTreeMap<String, (f64, usize)> = BTreeMap::new();
            for (name, seconds) in &per_test {
                let best = section
                    .targets
                    .iter()
                    .filter(|(_, target)| target.run.iter().any(|run| name.starts_with(run.as_str()) || name == run))
                    .max_by_key(|(_, target)| target.run.iter().map(String::len).max().unwrap_or(0));
                if let Some((id, _)) = best {
                    let sum = sums.entry(id.clone()).or_default();
                    sum.0 += seconds;
                    sum.1 += 1;
                }
            }
            for (id, (seconds, tests)) in sums {
                Self::put(&mut self.targets, format!("{provider}/{id}"), seconds, tests, day);
                count += 1;
            }
        }
        count
    }

    /// A target's expected seconds: its own time, its CTest shards' times, or
    /// its share of its group's.
    pub fn estimate(&self, provider: &str, id: &str, map: &ProviderMap) -> Option<f64> {
        if let Some(timing) = self.targets.get(&format!("{provider}/{id}")) {
            return Some(timing.seconds);
        }
        let target = map.targets.get(id)?;
        let shards: Vec<f64> = target.run.iter().filter_map(|run| self.targets.get(&format!("{provider}/{id}#{run}")).map(|timing| timing.seconds)).collect();
        if !shards.is_empty() && shards.len() == target.run.len() {
            return Some(shards.iter().sum());
        }
        let group = if target.group.is_empty() { id } else { target.group.as_str() };
        let timing = self.groups.get(&format!("{provider}/{group}"))?;
        if group == id || timing.tests == 0 {
            return Some(timing.seconds);
        }
        Some(timing.seconds * target.tests as f64 / timing.tests as f64)
    }
}

fn number_before(text: &str, word: &str) -> Option<usize> {
    let at = text.find(word)?;
    text[..at].rsplit(|ch: char| !ch.is_ascii_digit()).next()?.parse().ok()
}

fn seconds_after(text: &str, word: &str) -> Option<f64> {
    let at = text.find(word)? + word.len();
    text[at..].trim_end_matches(|ch: char| !ch.is_ascii_digit()).trim_end_matches('s').parse().ok()
}

/// The map's estimate of a whole provider, for `--full`.
pub fn provider_seconds(durations: &Durations, map: &TestMap, provider: &str) -> (f64, usize) {
    let Some(section) = map.providers.get(provider) else { return (0.0, 0) };
    let mut total = 0.0;
    let mut unknown = 0;
    for id in section.targets.keys() {
        match durations.estimate(provider, id, section) {
            Some(seconds) => total += seconds,
            None => unknown += 1,
        }
    }
    (total, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_discipline::config::{Kind, Rule};
    use crate::test_discipline::map::Target;

    fn section() -> ProviderMap {
        let target = |group: &str, run: &[&str], tests: usize| Target {
            group: group.into(),
            run: run.iter().map(|run| (*run).to_owned()).collect(),
            tests,
            ..Target::default()
        };
        let mut targets = BTreeMap::new();
        targets.insert("--bin hgs state::teams".to_owned(), target("--bin hgs", &["state::teams::"], 30));
        targets.insert("--bin hgs cli".to_owned(), target("--bin hgs", &["cli::"], 10));
        targets.insert("test_teams".to_owned(), target("", &["teams"], 5));
        targets.insert("test_sessionswindow".to_owned(), target("", &["sessionswindow-0", "sessionswindow-1"], 394));
        ProviderMap {
            kind: Kind::Cargo,
            rule: Rule::Build,
            note: String::new(),
            targets,
            files: BTreeMap::new(),
            test_files: BTreeMap::new(),
            rules: Vec::new(),
            groups: BTreeMap::from([("--bin hgs".to_owned(), "src/main.rs".to_owned())]),
            seconds: 0.0,
            platforms: Vec::new(),
        }
    }

    #[test]
    fn cargo_runs_are_recorded_per_crate_and_shared_by_test_count() {
        let mut durations = Durations::default();
        let text = "     Running unittests src/main.rs (target\\debug\\deps\\hgs-abc.exe)\n\ntest result: ok. 38 passed; 2 failed; 1 ignored; 0 measured; 0 filtered out; finished in 20.00s\n";
        assert_eq!(durations.record("rust", &section(), text, None, "2026-10-30"), 1);
        assert_eq!(durations.groups["rust/--bin hgs"].tests, 40);
        let estimate = durations.estimate("rust", "--bin hgs state::teams", &section()).unwrap();
        assert!((estimate - 15.0).abs() < 1e-9, "{estimate}");
        // A slower later run raises it; a faster one keeps the slower.
        let fast = text.replace("20.00s", "8.00s");
        durations.record("rust", &section(), &fast, None, "2026-10-31");
        assert_eq!(durations.groups["rust/--bin hgs"].seconds, 20.0);
        assert_eq!(durations.groups["rust/--bin hgs"].runs, 2);
        // A filtered run is compared per test: 10 tests in 8 s is slower.
        let filtered = "test result: ok. 10 passed; 0 failed; finished in 8.00s\n";
        durations.record("rust", &section(), filtered, None, "2026-10-31");
        assert_eq!((durations.groups["rust/--bin hgs"].seconds, durations.groups["rust/--bin hgs"].tests), (8.0, 10));
        // Only stdout, without `Running`: the provider's one crate.
        let mut bare = Durations::default();
        assert_eq!(bare.record("rust", &section(), "test result: ok. 4 passed; 0 failed; finished in 2.50s\n", None, "2026-10-30"), 1);
        assert_eq!(bare.groups["rust/--bin hgs"].seconds, 2.5);
    }

    #[test]
    fn libtest_times_sum_into_module_targets() {
        let mut durations = Durations::default();
        let text = "{ \"type\": \"test\", \"name\": \"state::teams::ops::tests::a\", \"event\": \"ok\", \"exec_time\": 0.5 }\n{ \"type\": \"test\", \"name\": \"state::teams::tests::b\", \"event\": \"ok\", \"exec_time\": 0.25 }\n{ \"type\": \"test\", \"name\": \"cli::tests::c\", \"event\": \"ok\", \"exec_time\": 1.0 }\n";
        assert_eq!(durations.record("rust", &section(), text, None, "2026-10-30"), 2);
        assert_eq!(durations.targets["rust/--bin hgs state::teams"].seconds, 0.75);
        assert_eq!(durations.estimate("rust", "--bin hgs cli", &section()), Some(1.0));
    }

    #[test]
    fn ctest_and_qttest_lines_are_recorded_by_binary_and_shard() {
        let mut durations = Durations::default();
        let text = "  1/3 Test  #1: sessionswindow-0 .........   Passed  120.50 sec\n  2/3 Test  #2: sessionswindow-1 .........   Passed   99.50 sec\n  3/3 Test  #3: teams ....................***Failed    4.00 sec\ntest_teams: exit 0, Totals: 5 passed, 0 failed, 0 skipped, 0 blacklisted, 3500ms, 4 s\n";
        assert_eq!(durations.record("tray", &section(), text, None, "2026-10-30"), 4);
        assert_eq!(durations.estimate("tray", "test_sessionswindow", &section()), Some(220.0));
        // The shard's own time wins over the binary's QtTest total.
        assert_eq!(durations.estimate("tray", "test_teams", &section()), Some(4.0));
        assert_eq!(durations.groups["tray/test_teams"].seconds, 3.5);
    }

    #[test]
    fn a_python_run_is_recorded_for_the_target_it_names() {
        let mut durations = Durations::default();
        let text = "......\n----------------------------------------------------------------------\nRan 6 tests in 3.456s\n\nOK\n";
        assert_eq!(durations.record("py", &section(), text, Some("tests/test_cli.py"), "2026-10-30"), 1);
        assert_eq!(durations.targets["py/tests/test_cli.py"].seconds, 3.456);
        let mut pytest = Durations::default();
        pytest.record("py", &section(), "======== 12 passed, 1 skipped in 2.50s ========\n", Some("tests/test_x.py"), "2026-10-30");
        assert_eq!(pytest.targets["py/tests/test_x.py"].seconds, 2.5);
        // A shard seed of seconds per test class, summed per module.
        let mut seed = Durations::default();
        let mut section = section();
        section.targets.insert("test_cli".into(), Target { run: vec!["test_cli".into()], tests: 3, ..Target::default() });
        let text = r#"{"test_cli.Cli[1/2]": 12, "test_cli.Cli[2/2]": 3, "test_gone.Gone": 9}"#;
        assert_eq!(seed.record("py", &section, text, None, "2026-10-30"), 1);
        assert_eq!(seed.targets["py/test_cli"].seconds, 15.0);
    }
}
