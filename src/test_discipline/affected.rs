//! U62: the tests a change runs, from the map, with their recorded time. A
//! file the map does not know runs its whole area; a file in no area is named,
//! never skipped silently.

use super::config::{Kind, Provider, Rule, TestsConfig, Unmapped};
use super::durations::{self, Durations};
use super::glob;
use super::map::{self, ProviderMap, TestMap, ALL};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default, Serialize)]
pub struct Selection {
    pub full: bool,
    pub changed: usize,
    pub runs: Vec<Run>,
    /// Seconds of the runs with a recorded time.
    pub seconds: f64,
    /// Targets without one.
    pub unknown: usize,
    pub widened: Vec<Widening>,
    /// In no provider's area: nothing runs for them.
    pub unmapped: Vec<String>,
    pub ignored: Vec<String>,
    /// Changed test files that only run by hand.
    pub manual: Vec<String>,
    /// Changed test files another platform's build runs (the CI).
    pub elsewhere: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Run {
    pub provider: String,
    pub kind: Kind,
    /// The folder it runs in, relative to the project.
    pub cwd: String,
    pub command: String,
    pub targets: Vec<String>,
    pub tests: usize,
    /// The recorded part of its time; None when nothing in it was measured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    /// Its targets without a recorded time.
    pub unknown: usize,
    pub whole: bool,
    /// QtTest: only these functions changed; `functions_command` runs them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub functions_command: String,
    /// #741: the same run as an `hgs team run` request's template and args,
    /// when a team template expresses it (cargo-test, qt-test).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_run: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Widening {
    pub file: String,
    pub provider: String,
}

/// Lines a change touched in each file, new side, from `git diff -U0`.
pub type Hunks = BTreeMap<String, Vec<(usize, usize)>>;

pub fn select(root: &std::path::Path, config: &TestsConfig, map: &TestMap, durations: &Durations, changed: &[String], hunks: &Hunks, full: bool) -> Selection {
    let providers: Vec<&Provider> = config.providers.iter().filter(|provider| !provider.disabled).collect();
    let mut chosen: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut whole: BTreeSet<&str> = BTreeSet::new();
    let mut selection = Selection { full, changed: changed.len(), ..Selection::default() };
    // Test files whose change only touched some functions: file to functions.
    let mut narrowed: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut broad: BTreeSet<String> = BTreeSet::new();
    if full {
        whole.extend(providers.iter().map(|provider| provider.name.as_str()));
    }
    for file in changed.iter().filter(|_| !full) {
        // map.ignore never hides a file the map knows: a text compiled into a
        // binary (include_str!) is that module's.
        let known = map.providers.values().any(|section| section.files.contains_key(file) || section.test_files.contains_key(file));
        if glob::any(&config.map.ignore, file) && !known {
            selection.ignored.push(file.clone());
            continue;
        }
        let mut claimed = false;
        for provider in &providers {
            let area = map::in_area(provider, file);
            if area && glob::any(&provider.manual, file) {
                selection.manual.push(file.clone());
                claimed = true;
                continue;
            }
            if area && glob::any(&provider.elsewhere, file) {
                selection.elsewhere.push(file.clone());
                claimed = true;
                continue;
            }
            let Some(section) = map.providers.get(&provider.name) else {
                if area {
                    whole.insert(&provider.name);
                    selection.widened.push(Widening { file: file.clone(), provider: provider.name.clone() });
                    claimed = true;
                }
                continue;
            };
            let mut hits: BTreeSet<String> = BTreeSet::new();
            hits.extend(section.files.get(file).into_iter().flatten().cloned());
            let test_hits: Vec<String> = section.test_files.get(file).into_iter().flatten().cloned().collect();
            hits.extend(test_hits.iter().cloned());
            for rule in &section.rules {
                if glob::any(&rule.files, file) {
                    hits.extend(rule.targets.iter().cloned());
                }
            }
            if (section.rule == Rule::All && area) || hits.contains(ALL) {
                whole.insert(&provider.name);
                claimed = true;
            } else if !hits.is_empty() {
                // A QtTest file whose change stays inside test functions runs
                // only those, unless something else chose its binary too.
                if provider.kind == Kind::Ctest && !test_hits.is_empty() && section.files.get(file).is_none_or(|ids| ids.iter().all(|id| test_hits.contains(id))) {
                    match changed_functions(root, file, hunks.get(file), section, &test_hits) {
                        Some(functions) => {
                            narrowed.entry(file.clone()).or_default().extend(functions);
                        }
                        None => {
                            broad.extend(test_hits.iter().cloned());
                        }
                    }
                } else {
                    broad.extend(hits.iter().filter(|id| test_hits.contains(id)).cloned());
                }
                chosen.entry(&provider.name).or_default().extend(hits);
                claimed = true;
            } else if area {
                whole.insert(&provider.name);
                selection.widened.push(Widening { file: file.clone(), provider: provider.name.clone() });
                claimed = true;
            }
        }
        if !claimed {
            selection.unmapped.push(file.clone());
            if config.map.unmapped == Unmapped::All {
                whole.extend(providers.iter().map(|provider| provider.name.as_str()));
            }
        }
    }
    for provider in &providers {
        let Some(section) = map.providers.get(&provider.name) else {
            if whole.contains(provider.name.as_str()) {
                selection.runs.push(whole_run(provider, &ProviderMap::empty(provider.kind), durations, map));
            }
            continue;
        };
        if whole.contains(provider.name.as_str()) {
            selection.runs.push(whole_run(provider, section, durations, map));
            continue;
        }
        let Some(ids) = chosen.get(provider.name.as_str()) else { continue };
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in ids {
            if let Some(target) = section.targets.get(id) {
                groups.entry(target.group.clone()).or_default().push(id.clone());
            }
        }
        for (group, ids) in groups {
            let mut run = run(provider, section, &group, &ids, durations);
            // Only test functions changed in a binary nothing else chose.
            if provider.kind == Kind::Ctest {
                let functions: BTreeSet<String> = narrowed.values().flatten().cloned().collect();
                let all_narrow = ids.iter().all(|id| !broad.contains(id) && section.test_files.iter().any(|(file, owners)| owners.contains(id) && narrowed.contains_key(file)));
                if all_narrow && !functions.is_empty() && ids.len() == 1 {
                    let target = &section.targets[&ids[0]];
                    run.functions = functions.into_iter().collect();
                    run.functions_command = format!("{} {}", target.binary, run.functions.join(" ")).trim().to_owned();
                    if target.tests > 0 {
                        run.seconds = run.seconds.map(|seconds| seconds * run.functions.len() as f64 / target.tests as f64);
                    }
                    run.tests = run.functions.len();
                    run.team_run = Some(serde_json::json!({"template": "qt-test", "args": {"suite": ids[0], "functions": run.functions}}));
                }
            }
            selection.runs.push(run);
        }
    }
    for run in &selection.runs {
        selection.seconds += run.seconds.unwrap_or(0.0);
        selection.unknown += run.unknown;
    }
    selection
}

impl ProviderMap {
    fn empty(kind: Kind) -> Self {
        Self {
            kind,
            rule: Rule::All,
            note: String::new(),
            targets: BTreeMap::new(),
            files: BTreeMap::new(),
            test_files: BTreeMap::new(),
            rules: Vec::new(),
            groups: BTreeMap::new(),
            seconds: 0.0,
            platforms: Vec::new(),
        }
    }
}

/// The functions whose bodies hold every changed line of a QtTest file, or
/// None when a change is outside them (a helper, an include, a fixture).
fn changed_functions(root: &std::path::Path, file: &str, hunks: Option<&Vec<(usize, usize)>>, section: &ProviderMap, owners: &[String]) -> Option<Vec<String>> {
    let hunks = hunks.filter(|hunks| !hunks.is_empty())?;
    let known: BTreeSet<&String> = owners.iter().filter_map(|id| section.targets.get(id)).flat_map(|target| target.functions.iter()).collect();
    let text = std::fs::read_to_string(root.join(file)).ok()?;
    let bodies = function_bodies(&text);
    let mut out = BTreeSet::new();
    for (start, end) in hunks {
        let (name, _, _) = bodies.iter().find(|(_, from, to)| from <= start && end <= to)?;
        let name = name.strip_suffix("_data").unwrap_or(name).to_owned();
        if !known.contains(&name) {
            return None;
        }
        out.insert(name);
    }
    Some(out.into_iter().collect())
}

/// `void Class::name()` definitions with their first and last lines.
fn function_bodies(text: &str) -> Vec<(String, usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let head = line.strip_prefix("void ").and_then(|rest| rest.split_once("::")).and_then(|(_, rest)| rest.split_once('('));
        if let Some((name, _)) = head.filter(|(name, _)| !name.contains(' ') && !line.trim_end().ends_with(';')) {
            let start = index + 1;
            let end = (index + 1..lines.len()).find(|cursor| lines[*cursor].starts_with('}')).map(|cursor| cursor + 1).unwrap_or(lines.len());
            out.push((name.to_owned(), start, end));
            index = end;
            continue;
        }
        index += 1;
    }
    out
}

fn whole_run(provider: &Provider, section: &ProviderMap, durations: &Durations, map: &TestMap) -> Run {
    let (seconds, unknown) = durations::provider_seconds(durations, map, &provider.name);
    let explicit = section.rule == Rule::Build && !provider.manual.is_empty() && matches!(provider.kind, Kind::Pytest | Kind::Unittest | Kind::Npm);
    // A provider with tests run only by hand lists the others by name.
    let ids: Vec<String> = if explicit { section.targets.keys().cloned().collect() } else { Vec::new() };
    let command = if explicit { command(provider, &items(provider, section, &ids)) } else { command(provider, "") };
    let tests = section.targets.values().map(|target| target.tests).sum();
    let unknown = unknown.max(usize::from(section.targets.is_empty()));
    let all: Vec<String> = section.targets.keys().filter(|id| *id != ALL).cloned().collect();
    let team_run = match provider.kind {
        Kind::Cargo => Some(cargo_team_run(provider, "", &[], &[])),
        Kind::Ctest if !all.is_empty() => Some(serde_json::json!({"template": "qt-test", "args": {"suites": all}})),
        _ => None,
    };
    Run {
        provider: provider.name.clone(),
        kind: provider.kind,
        cwd: cwd(provider),
        command,
        targets: vec![ALL.to_owned()],
        tests,
        seconds: (unknown < section.targets.len()).then_some(seconds),
        unknown,
        whole: true,
        functions: Vec::new(),
        functions_command: String::new(),
        team_run,
    }
}

fn run(provider: &Provider, section: &ProviderMap, group: &str, ids: &[String], durations: &Durations) -> Run {
    let mut seconds = 0.0;
    let mut unknown = 0;
    let mut tests = 0;
    for id in ids {
        tests += section.targets.get(id).map(|target| target.tests).unwrap_or(0);
        match durations.estimate(&provider.name, id, section) {
            Some(more) => seconds += more,
            None => unknown += 1,
        }
    }
    let seconds = (unknown < ids.len()).then_some(seconds);
    let mut targets = items(provider, section, ids);
    if provider.kind == Kind::Cargo {
        targets = format!("{group} -- {targets}");
    }
    let team_run = match provider.kind {
        Kind::Cargo => {
            let (filters, skips) = cargo_filters(section, ids);
            Some(cargo_team_run(provider, group, &filters, &skips))
        }
        Kind::Ctest => Some(serde_json::json!({"template": "qt-test", "args": {"suites": ids}})),
        _ => None,
    };
    Run {
        provider: provider.name.clone(),
        kind: provider.kind,
        cwd: cwd(provider),
        command: command(provider, &targets),
        targets: ids.to_vec(),
        tests,
        seconds,
        unknown,
        whole: false,
        functions: Vec::new(),
        functions_command: String::new(),
        team_run,
    }
}

/// A cargo run's filters, and the child modules to skip that were not chosen.
fn cargo_filters(section: &ProviderMap, ids: &[String]) -> (Vec<String>, Vec<String>) {
    let filters: BTreeSet<String> = ids.iter().filter_map(|id| section.targets.get(id)).flat_map(|target| target.run.iter().cloned()).collect();
    let skips: BTreeSet<String> = ids
        .iter()
        .filter_map(|id| section.targets.get(id))
        .flat_map(|target| target.skip.iter().cloned())
        .filter(|skip| !filters.iter().any(|filter| filter.starts_with(skip.as_str())))
        .collect();
    (filters.into_iter().collect(), skips.into_iter().collect())
}

/// The cargo-test template's request for a cargo run: its binary, the
/// provider's manifest when it is not the project's, its filters and skips.
fn cargo_team_run(provider: &Provider, group: &str, filters: &[String], skips: &[String]) -> serde_json::Value {
    let mut args = serde_json::Map::new();
    if let Some(bin) = group.strip_prefix("--bin ") {
        args.insert("bin".into(), bin.into());
    }
    let manifest = if provider.manifest.is_empty() { "Cargo.toml" } else { provider.manifest.as_str() };
    if !provider.root.is_empty() || manifest != "Cargo.toml" {
        let path = if provider.root.is_empty() { manifest.to_owned() } else { format!("{}/{manifest}", provider.root.trim_end_matches('/')) };
        args.insert("manifest".into(), serde_json::json!([path]));
    }
    if !filters.is_empty() {
        args.insert("filters".into(), serde_json::json!(filters));
    }
    if !skips.is_empty() {
        args.insert("skips".into(), serde_json::json!(skips));
    }
    serde_json::json!({"template": "cargo-test", "args": args})
}

fn cwd(provider: &Provider) -> String {
    if provider.root.is_empty() {
        ".".to_owned()
    } else {
        provider.root.clone()
    }
}

/// What the chosen targets add to the command, in the kind's form.
fn items(provider: &Provider, section: &ProviderMap, ids: &[String]) -> String {
    let runs: Vec<&String> = ids.iter().filter_map(|id| section.targets.get(id)).flat_map(|target| target.run.iter()).collect();
    match provider.kind {
        Kind::Cargo => {
            let (filters, skips) = cargo_filters(section, ids);
            let mut words: Vec<String> = filters.iter().map(|filter| quote(filter)).collect();
            words.extend(skips.iter().map(|skip| format!("--skip {}", quote(skip))));
            words.join(" ")
        }
        Kind::Ctest => {
            let names: Vec<String> = runs.iter().map(|name| regex_escape(name)).collect();
            format!("-R \"^({})$\"", names.join("|"))
        }
        Kind::Gradle => runs.iter().map(|class| format!("--tests {}", quote(class))).collect::<Vec<_>>().join(" "),
        _ => runs.iter().map(|item| quote(item)).collect::<Vec<_>>().join(" "),
    }
}

fn command(provider: &Provider, targets: &str) -> String {
    let template = if provider.command.is_empty() {
        match provider.kind {
            Kind::Cargo => "cargo test --manifest-path {manifest} {targets}",
            Kind::Ctest => "ctest --test-dir {build_dir} {targets}",
            Kind::Pytest => "python -m pytest {targets}",
            Kind::Unittest if targets.is_empty() => "python -m unittest discover -p \"test_*.py\"",
            Kind::Unittest => "python -m unittest {targets}",
            Kind::Gradle => "./gradlew test {targets}",
            Kind::Npm if targets.is_empty() => "npm test",
            Kind::Npm => "npm test -- {targets}",
            Kind::Command => "{targets}",
        }
        .to_owned()
    } else {
        provider.command.clone()
    };
    let manifest = if provider.manifest.is_empty() { "Cargo.toml" } else { provider.manifest.as_str() };
    let build_dir = relative_to(&provider.build_dir, &provider.root);
    let text = template.replace("{manifest}", manifest).replace("{build_dir}", &build_dir).replace("{targets}", targets);
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace("-- --", "--").trim_end_matches(" --").to_owned()
}

/// A project path as seen from the provider's folder.
fn relative_to(path: &str, root: &str) -> String {
    if root.is_empty() {
        return path.to_owned();
    }
    match path.strip_prefix(&format!("{}/", root.trim_end_matches('/'))) {
        Some(inner) => inner.to_owned(),
        None => {
            let ups = root.trim_end_matches('/').split('/').count();
            format!("{}{path}", "../".repeat(ups))
        }
    }
}

fn quote(text: &str) -> String {
    if text.chars().all(|ch| ch.is_ascii_alphanumeric() || "_-:./@=+".contains(ch)) {
        text.to_owned()
    } else {
        format!("\"{}\"", text.replace('"', "\\\""))
    }
}

fn regex_escape(text: &str) -> String {
    text.chars().map(|ch| if "\\.^$|?*+()[]{}".contains(ch) { format!("\\{ch}") } else { ch.to_string() }).collect()
}

pub fn duration(seconds: f64) -> String {
    let total = seconds.round() as u64;
    match total {
        0 => "under a second".to_owned(),
        1..=59 => format!("{total} s"),
        60..=3599 => format!("{} min {} s", total / 60, total % 60),
        _ => format!("{} h {} min", total / 3600, total % 3600 / 60),
    }
}

/// The selection as an agent reads it.
pub fn text(selection: &Selection) -> String {
    let mut out = String::new();
    let estimate = if selection.unknown > 0 {
        format!("about {} recorded, {} target(s) without a recorded time", duration(selection.seconds), selection.unknown)
    } else {
        format!("about {}", duration(selection.seconds))
    };
    if selection.full {
        out.push_str(&format!("The full set: {} run(s), {estimate}.\n", selection.runs.len()));
    } else if selection.runs.is_empty() {
        out.push_str(&format!("{} changed file(s): no tests to run.\n", selection.changed));
    } else {
        out.push_str(&format!("{} changed file(s): {} run(s), {estimate}.\n", selection.changed, selection.runs.len()));
    }
    for run in &selection.runs {
        let time = match (run.seconds, run.unknown) {
            (None, _) => "time unknown".to_owned(),
            (Some(seconds), 0) => duration(seconds),
            (Some(seconds), unknown) => format!("{} and {unknown} target(s) without a recorded time", duration(seconds)),
        };
        let scope = if run.whole { "the whole set".to_owned() } else { format!("{} target(s)", run.targets.len()) };
        out.push_str(&format!("\n{} ({}): {scope}, {} test(s), {time}\n", run.provider, run.kind.name(), run.tests));
        if run.cwd != "." {
            out.push_str(&format!("  in {}:\n", run.cwd));
        }
        out.push_str(&format!("  {}\n", run.command));
        if !run.functions.is_empty() {
            out.push_str(&format!("  only the changed functions: {}\n", run.functions_command));
        }
        if let Some(request) = &run.team_run {
            out.push_str(&format!("  hgs team run: {request}\n"));
        }
    }
    if !selection.widened.is_empty() {
        out.push_str("\nWidened to the whole area (no test the map knows uses them):\n");
        for widening in &selection.widened {
            out.push_str(&format!("  {} ({})\n", widening.file, widening.provider));
        }
    }
    if !selection.unmapped.is_empty() {
        out.push_str("\nIn no area of .zerus/tests.toml, so nothing runs for them:\n");
        for file in &selection.unmapped {
            out.push_str(&format!("  {file}\n"));
        }
    }
    if !selection.manual.is_empty() {
        out.push_str("\nRun only by hand, not chosen:\n");
        for file in &selection.manual {
            out.push_str(&format!("  {file}\n"));
        }
    }
    if !selection.elsewhere.is_empty() {
        out.push_str("\nBuilt and run on another platform (the CI), not here:\n");
        for file in &selection.elsewhere {
            out.push_str(&format!("  {file}\n"));
        }
    }
    if !selection.ignored.is_empty() {
        out.push_str(&format!("\n{} file(s) need no tests (map.ignore).\n", selection.ignored.len()));
    }
    out
}

/// The changed lines of each file in `git diff -U0` output.
pub fn hunks(diff: &str) -> Hunks {
    let mut out = Hunks::new();
    let mut file: Option<String> = None;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            file = path.strip_prefix("b/").map(str::to_owned);
            continue;
        }
        let Some(rest) = line.strip_prefix("@@ ") else { continue };
        let Some(new) = rest.split_whitespace().find(|word| word.starts_with('+')) else { continue };
        let mut parts = new[1..].split(',');
        let start: usize = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
        let count: usize = parts.next().and_then(|value| value.parse().ok()).unwrap_or(1);
        if let Some(file) = &file {
            let end = if count == 0 { start } else { start + count - 1 };
            out.entry(file.clone()).or_default().push((start.max(1), end.max(1)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_discipline::config::parse_toml;
    use crate::test_discipline::map::{ResolvedRule, Target};

    fn config() -> TestsConfig {
        let text = r#"
[map]
ignore = ["docs/**", "*.md"]
[[provider]]
name = "rust"
kind = "cargo"
sources = ["src/**", "Cargo.toml"]
[[provider]]
name = "gui"
kind = "ctest"
build_dir = "build/gui"
sources = ["gui/**"]
tests = ["gui/tests/test_*.cpp"]
[[provider]]
name = "py"
kind = "unittest"
root = "tests"
sources = ["scripts/**"]
tests = ["tests/test_*.py"]
manual = ["tests/test_native.py"]
[[provider]]
name = "web"
kind = "npm"
root = "web"
sources = ["web/**"]
affected = "all"
"#;
        serde_json::from_value(parse_toml(text).unwrap()).unwrap()
    }

    fn target(group: &str, run: &[&str], tests: usize) -> Target {
        Target { group: group.into(), run: run.iter().map(|run| (*run).to_owned()).collect(), tests, ..Target::default() }
    }

    fn map() -> TestMap {
        let mut map = TestMap { schema: 1, providers: BTreeMap::new() };
        let mut rust = ProviderMap::empty(Kind::Cargo);
        rust.rule = Rule::Build;
        rust.targets.insert("--bin hgs state::teams".into(), Target { skip: vec!["state::teams::ops::".into()], ..target("--bin hgs", &["state::teams::"], 20) });
        rust.targets.insert("--bin hgs state::teams::ops".into(), target("--bin hgs", &["state::teams::ops::"], 30));
        rust.targets.insert("--bin hgs cli".into(), target("--bin hgs", &["cli::"], 10));
        rust.files.insert("src/state/teams/mod.rs".into(), vec!["--bin hgs state::teams".into(), "--bin hgs cli".into()]);
        rust.files.insert("src/state/teams/ops.rs".into(), vec!["--bin hgs state::teams::ops".into()]);
        rust.files.insert("src/guide/teams.md".into(), vec!["--bin hgs cli".into()]);
        map.providers.insert("rust".into(), rust);
        let mut gui = ProviderMap::empty(Kind::Ctest);
        gui.rule = Rule::Build;
        gui.targets.insert(
            "test_teams".into(),
            Target { functions: vec!["opens".into(), "closes".into()], binary: "build/gui/tests/test_teams".into(), ..target("", &["teams-0", "teams-1"], 2) },
        );
        gui.files.insert("gui/src/Team.cpp".into(), vec!["test_teams".into()]);
        gui.files.insert("gui/tests/test_teams.cpp".into(), vec!["test_teams".into()]);
        gui.test_files.insert("gui/tests/test_teams.cpp".into(), vec!["test_teams".into()]);
        map.providers.insert("gui".into(), gui);
        let mut py = ProviderMap::empty(Kind::Unittest);
        py.rule = Rule::Build;
        py.targets.insert("tests/test_cli.py".into(), target("", &["test_cli"], 3));
        py.targets.insert("tests/test_release.py".into(), target("", &["test_release"], 2));
        py.test_files.insert("tests/test_release.py".into(), vec!["tests/test_release.py".into()]);
        py.files.insert("scripts/release.py".into(), vec!["tests/test_release.py".into()]);
        py.rules.push(ResolvedRule { files: vec!["src/**".into()], targets: vec!["tests/test_cli.py".into()] });
        map.providers.insert("py".into(), py);
        let mut web = ProviderMap::empty(Kind::Npm);
        web.targets.insert(ALL.into(), Target::default());
        map.providers.insert("web".into(), web);
        map
    }

    fn durations() -> Durations {
        let mut durations = Durations::default();
        durations.groups.insert("rust/--bin hgs".into(), durations::Timing { seconds: 60.0, tests: 60, recorded: "2026-10-30".into(), runs: 1 });
        durations.targets.insert("gui/test_teams#teams-0".into(), durations::Timing { seconds: 10.0, recorded: "2026-10-30".into(), runs: 1, tests: 0 });
        durations.targets.insert("gui/test_teams#teams-1".into(), durations::Timing { seconds: 30.0, recorded: "2026-10-30".into(), runs: 1, tests: 0 });
        durations
    }

    fn select_files(files: &[&str], hunks: &Hunks) -> Selection {
        let root = tempfile::tempdir().unwrap();
        let changed: Vec<String> = files.iter().map(|file| (*file).to_owned()).collect();
        select(root.path(), &config(), &map(), &durations(), &changed, hunks, false)
    }

    #[test]
    fn a_shared_module_runs_its_dependants_with_a_filtered_cargo_command() {
        let selection = select_files(&["src/state/teams/mod.rs"], &Hunks::new());
        assert_eq!(selection.runs.len(), 2, "{}", text(&selection));
        let rust = &selection.runs[0];
        assert_eq!(rust.provider, "rust");
        // ops is a child that was not chosen: skipped.
        assert_eq!(rust.command, "cargo test --manifest-path Cargo.toml --bin hgs -- cli:: state::teams:: --skip state::teams::ops::");
        assert_eq!(rust.tests, 30);
        // #741: the same run as a cargo-test request.
        assert_eq!(
            rust.team_run,
            Some(serde_json::json!({"template":"cargo-test","args":{"bin":"hgs","filters":["cli::","state::teams::"],"skips":["state::teams::ops::"]}}))
        );
        assert!(text(&selection).contains("\"template\":\"cargo-test\"}"), "{}", text(&selection));
        assert_eq!(rust.seconds, Some(30.0));
        // The path rule: every src change runs the CLI test too.
        let py = &selection.runs[1];
        assert_eq!(py.command, "python -m unittest test_cli");
        assert_eq!(py.cwd, "tests");
        assert_eq!(selection.unknown, 1);
        // Both chosen: no skip.
        let both = select_files(&["src/state/teams/mod.rs", "src/state/teams/ops.rs"], &Hunks::new());
        assert_eq!(both.runs[0].command, "cargo test --manifest-path Cargo.toml --bin hgs -- cli:: state::teams:: state::teams::ops::");
    }

    #[test]
    fn unmapped_files_widen_to_their_area_or_are_named() {
        let selection = select_files(&["src/new_module.rs", "scripts/new_tool.py", "web/app.ts", "design/logo.svg", "docs/a.md", "tests/test_native.py"], &Hunks::new());
        let names: Vec<(&str, bool)> = selection.runs.iter().map(|run| (run.provider.as_str(), run.whole)).collect();
        assert_eq!(names, vec![("rust", true), ("py", true), ("web", true)]);
        assert_eq!(
            selection.widened,
            vec![
                Widening { file: "src/new_module.rs".into(), provider: "rust".into() },
                Widening { file: "scripts/new_tool.py".into(), provider: "py".into() }
            ]
        );
        assert_eq!(selection.unmapped, vec!["design/logo.svg"]);
        assert_eq!(selection.ignored, vec!["docs/a.md"]);
        // An ignored pattern does not hide a text a module compiles in (#741).
        let guide = select_files(&["src/guide/teams.md"], &Hunks::new());
        assert!(guide.ignored.is_empty());
        assert_eq!(guide.runs[0].targets, vec!["--bin hgs cli"]);
        assert_eq!(selection.manual, vec!["tests/test_native.py"]);
        assert_eq!(selection.runs[0].command, "cargo test --manifest-path Cargo.toml");
        // The whole Python set names its tests, leaving the manual one out.
        assert_eq!(selection.runs[1].command, "python -m unittest test_cli test_release");
        assert_eq!(selection.runs[2].command, "npm test");
        assert!(text(&selection).contains("In no area of .zerus/tests.toml, so nothing runs for them:\n  design/logo.svg"));
    }

    #[test]
    fn a_change_inside_qt_test_functions_runs_only_them() {
        let root = tempfile::tempdir().unwrap();
        let source = "#include <QtTest>\nclass TestTeams : public QObject {\nprivate slots:\n    void opens();\n    void closes();\n};\n\nvoid TestTeams::opens()\n{\n    QVERIFY(true);\n}\n\nvoid TestTeams::closes()\n{\n    QVERIFY(true);\n}\n";
        std::fs::create_dir_all(root.path().join("gui/tests")).unwrap();
        std::fs::write(root.path().join("gui/tests/test_teams.cpp"), source).unwrap();
        let changed = vec!["gui/tests/test_teams.cpp".to_owned()];
        let inside = Hunks::from([("gui/tests/test_teams.cpp".to_owned(), vec![(10, 10)])]);
        let selection = select(root.path(), &config(), &map(), &durations(), &changed, &inside, false);
        let run = &selection.runs[0];
        assert_eq!(run.command, "ctest --test-dir build/gui -R \"^(teams-0|teams-1)$\"");
        assert_eq!(run.functions, vec!["opens"]);
        assert_eq!(run.functions_command, "build/gui/tests/test_teams opens");
        assert_eq!(run.team_run, Some(serde_json::json!({"template":"qt-test","args":{"suite":"test_teams","functions":["opens"]}})));
        assert_eq!(run.seconds, Some(20.0));
        // A change in the class declaration is every function's.
        let outside = Hunks::from([("gui/tests/test_teams.cpp".to_owned(), vec![(4, 4)])]);
        let selection = select(root.path(), &config(), &map(), &durations(), &changed, &outside, false);
        assert!(selection.runs[0].functions.is_empty());
        assert_eq!(selection.runs[0].team_run, Some(serde_json::json!({"template":"qt-test","args":{"suites":["test_teams"]}})));
        assert_eq!(selection.runs[0].seconds, Some(40.0));
    }

    #[test]
    fn the_full_set_runs_every_provider_with_its_estimate() {
        let root = tempfile::tempdir().unwrap();
        let selection = select(root.path(), &config(), &map(), &durations(), &[], &Hunks::new(), true);
        assert_eq!(selection.runs.len(), 4);
        assert_eq!(selection.runs[1].command, "ctest --test-dir build/gui");
        assert_eq!(selection.runs[0].team_run, Some(serde_json::json!({"template":"cargo-test","args":{}})));
        assert_eq!(selection.runs[1].team_run, Some(serde_json::json!({"template":"qt-test","args":{"suites":["test_teams"]}})));
        // Python has no team template: hgs run runs its command.
        assert_eq!(selection.runs[2].team_run, None);
        assert_eq!(selection.runs[0].seconds, Some(60.0));
        assert!(text(&selection).starts_with("The full set: 4 run(s), about 1 min 40 s recorded, 3 target(s) without"), "{}", text(&selection));
    }

    #[test]
    fn diff_hunks_are_read_on_the_new_side() {
        let diff = "diff --git a/x.cpp b/x.cpp\n--- a/x.cpp\n+++ b/x.cpp\n@@ -3,0 +4,2 @@ void f()\n+a\n+b\n@@ -10 +12 @@\n-c\n+d\n@@ -20,3 +21,0 @@\n";
        assert_eq!(hunks(diff)["x.cpp"], vec![(4, 5), (12, 12), (21, 21)]);
    }
}
