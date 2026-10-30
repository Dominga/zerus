//! U62 test discipline (owner, 2026-10-30): a task runs only the tests its
//! change touches. `hgs test` reads the project's `.zerus/tests.toml` (with the
//! owner's overrides from the team settings), writes and checks the test map,
//! names a change's affected tests with their recorded time, records times and
//! compares the map with coverage. See `hgs guide tests`.

pub mod affected;
pub mod cargo_graph;
pub mod cmake;
pub mod config;
pub mod coverage;
pub mod detect;
pub mod durations;
pub mod glob;
pub mod map;

use crate::cli::{Error, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: hgs test config [--propose|--write-proposal] | map [--write|--check] | affected [RANGE] [--staged] [--files FILE...] [--full] | durations --record FILE... --provider NAME [--target ID] | coverage --provider NAME --report TARGET=FILE... [--fail-on-gaps]; each takes --dir DIR, --team TEAM and --json";

fn fail(message: impl Into<String>) -> Error {
    Error::new(1, message)
}

fn usage() -> Error {
    Error::new(2, USAGE)
}

/// The options every action reads.
struct Options {
    root: PathBuf,
    team: Option<String>,
    json: bool,
    rest: Vec<String>,
}

fn options(args: &[String]) -> Result<Options> {
    let mut dir: Option<PathBuf> = None;
    let mut team = None;
    let mut json = false;
    let mut rest = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dir" => {
                dir = Some(PathBuf::from(args.get(index + 1).ok_or_else(usage)?));
                index += 2;
                continue;
            }
            "--team" => {
                team = Some(args.get(index + 1).ok_or_else(usage)?.clone());
                index += 2;
                continue;
            }
            "--json" => json = true,
            other => rest.push(other.to_owned()),
        }
        index += 1;
    }
    let start = match dir {
        Some(dir) => dir,
        None => std::env::current_dir().map_err(|error| fail(format!("Cannot read the current folder: {error}")))?,
    };
    // The project is the Git checkout around the folder, or the folder.
    let root = git(&start, &["rev-parse", "--show-toplevel"]).ok().map(|top| PathBuf::from(top.trim())).filter(|top| top.is_dir()).unwrap_or(start);
    Ok(Options { root, team, json, rest })
}

fn git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()?;
    anyhow::ensure!(output.status.success(), "git {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim());
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The owner's override of a team's test configuration.
fn team_overrides(team: Option<&str>) -> Result<Option<Value>> {
    let Some(team) = team else { return Ok(None) };
    let store = crate::state::teams::store::Store::open(false).map_err(|error| fail(format!("{error:#}")))?;
    let found = store.load(team).map_err(|error| fail(format!("{error:#}")))?.ok_or_else(|| fail(format!("No team {team}")))?;
    Ok(found.settings.tests.clone())
}

/// The project's configuration as an action uses it.
pub fn load(root: &Path, team: Option<&str>) -> Result<config::Loaded> {
    let overrides = team_overrides(team)?;
    config::load(root, overrides.as_ref()).map_err(|error| fail(format!("{error:#}")))
}

/// U62 (#715): the recorded seconds of a full run (the map's whole set), for a
/// team's full-run refusal; none when the project has no map.
pub fn full_seconds(root: &Path, team: Option<&str>) -> Result<Option<f64>> {
    let loaded = load(root, team)?;
    let config = &loaded.config;
    let Some(map) = read_map(root, config)? else { return Ok(None) };
    let durations = durations::Durations::read(&root.join(&config.map.durations))
        .map_err(|error| fail(format!("{error:#}")))?;
    let selection = affected::select(root, config, &map, &durations, &[], &affected::Hunks::new(), true);
    Ok(Some(selection.seconds))
}

pub fn dispatch(args: &[String]) -> Result<i32> {
    let Some((action, args)) = args.split_first() else { return Err(usage()) };
    let options = options(args)?;
    match action.as_str() {
        "config" => show_config(&options),
        "map" => write_map(&options),
        "affected" => affected_tests(&options),
        "durations" => record_durations(&options),
        "coverage" => compare_coverage(&options),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(0)
        }
        _ => Err(usage()),
    }
}

fn print(options: &Options, value: Value, text: &str) {
    if options.json {
        println!("{value}");
    } else {
        print!("{text}");
    }
}

fn show_config(options: &Options) -> Result<i32> {
    let loaded = load(&options.root, options.team.as_deref())?;
    let propose = options.rest.iter().any(|arg| arg == "--propose");
    let write = options.rest.iter().any(|arg| arg == "--write-proposal");
    if propose || write {
        let detected = detect::detect(&options.root);
        let text = detect::proposal(&detected);
        if write {
            let path = options.root.join(config::FILE);
            if path.exists() {
                return Err(fail(format!("{} exists; edit it instead", config::FILE)));
            }
            std::fs::create_dir_all(path.parent().unwrap_or(&options.root)).map_err(|error| fail(error.to_string()))?;
            std::fs::write(&path, &text).map_err(|error| fail(format!("Cannot write {}: {error}", path.display())))?;
        }
        print(options, json!({"proposal": text, "written": write}), &text);
        return Ok(0);
    }
    let source = match loaded.source {
        config::Source::File => format!("from {}", config::FILE),
        config::Source::Detected => format!("detected, a proposal: no {} yet (hgs test config --write-proposal)", config::FILE),
        config::Source::None => format!("none: no {} and no known stack, so today's flow stays", config::FILE),
    };
    let mut text = format!("Test configuration {source}{}.\n", if loaded.overridden { ", with the owner's overrides" } else { "" });
    for provider in &loaded.config.providers {
        let state = if provider.disabled { " (off)" } else { "" };
        text.push_str(&format!("  {} ({}){state}: {:?}, area {}\n", provider.name, provider.kind.name(), provider.affected, provider.sources.join(" ")));
    }
    let full = &loaded.config.full;
    text.push_str(&format!("Full run: {:?}, {:?}{}.\n", full.place, full.policy, if full.target.is_empty() { String::new() } else { format!(" ({})", full.target) }));
    print(options, json!({"active": loaded.active(), "loaded": loaded}), &text);
    Ok(0)
}

fn read_map(root: &Path, config: &config::TestsConfig) -> Result<Option<map::TestMap>> {
    let path = root.join(&config.map.file);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|error| fail(format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(fail(format!("Cannot read {}: {error}", path.display()))),
    }
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(|error| fail(error.to_string()))?;
    let value = serde_json::to_value(value).map_err(|error| fail(error.to_string()))?;
    let mut text = String::new();
    layout(&value, 0, &mut text);
    text.push('\n');
    std::fs::write(path, text).map_err(|error| fail(format!("Cannot write {}: {error}", path.display())))
}

/// JSON with one object field per line and every list on its line: a map
/// file's diff then shows the files and targets that changed, and it stays
/// small enough to commit.
fn layout(value: &Value, depth: usize, out: &mut String) {
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            out.push_str("{\n");
            for (index, (key, field)) in fields.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                out.push_str(&Value::String(key.clone()).to_string());
                out.push_str(": ");
                layout(field, depth + 1, out);
                if index + 1 < fields.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(depth));
            out.push('}');
        }
        Value::Array(items) if items.iter().any(|item| item.is_object()) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                out.push_str(&"  ".repeat(depth + 1));
                layout(item, depth + 1, out);
                if index + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(depth));
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

fn write_map(options: &Options) -> Result<i32> {
    let loaded = load(&options.root, options.team.as_deref())?;
    let config = &loaded.config;
    let files = map::project_files(&options.root).map_err(|error| fail(format!("{error:#}")))?;
    let committed = read_map(&options.root, config)?;
    let fresh = map::generate(&options.root, config, &files, committed.as_ref()).map_err(|error| fail(format!("{error:#}")))?;
    let mut text = String::new();
    for (name, section) in &fresh.providers {
        let tests: usize = section.targets.values().map(|target| target.tests).sum();
        text.push_str(&format!("  {name}: {} target(s), {tests} test(s), {} mapped file(s), read in {:.1} s\n", section.targets.len(), section.files.len(), section.seconds));
        if !section.note.is_empty() {
            text.push_str(&format!("    {}\n", section.note));
        }
    }
    if options.rest.iter().any(|arg| arg == "--check") {
        // #741: the map checked is the one the sources give now; the committed
        // file stands in only for a build this checkout cannot read.
        let committed = committed.unwrap_or_default();
        let findings = map::check(config, &files, &committed, &fresh);
        let mut report = String::new();
        let sections = [
            ("In no provider's area and not in map.ignore (a new area for .zerus/tests.toml)", &findings.outside, 50),
            ("Test files no provider maps", &findings.missing_tests, 50),
            ("Read from the committed map, the build could not be read", &findings.unread, 50),
        ];
        for (title, list, shown) in sections {
            if !list.is_empty() {
                report.push_str(&format!("{title}:\n"));
                for line in list.iter().take(shown) {
                    report.push_str(&format!("  {line}\n"));
                }
                if list.len() > shown {
                    report.push_str(&format!("  ... and {} more\n", list.len() - shown));
                }
            }
        }
        if !findings.stale.is_empty() {
            report.push_str(&format!(
                "The committed {} lags the sources in {} entr{} (not a failure: the map is read from the sources; hgs test map --write refreshes the snapshot).\n",
                config.map.file,
                findings.stale.len(),
                if findings.stale.len() == 1 { "y" } else { "ies" }
            ));
        }
        let ok = findings.ok();
        if ok {
            report.push_str(&format!("The test map is current: {} tracked file(s) checked.\n", files.len()));
        }
        print(options, json!({"ok": ok, "findings": findings}), &report);
        return Ok(if ok { 0 } else { 1 });
    }
    if options.rest.iter().any(|arg| arg == "--write") {
        write_json(&options.root.join(&config.map.file), &fresh)?;
        text = format!("Wrote {}:\n{text}", config.map.file);
    } else {
        text = format!("The map from the build now (hgs test map --write saves it):\n{text}");
    }
    print(options, json!({"map": config.map.file, "providers": fresh.providers.keys().collect::<Vec<_>>()}), &text);
    Ok(0)
}

/// The changed files and lines of a range, the index or the working tree.
fn changes(options: &Options, range: Option<&str>, staged: bool) -> Result<(Vec<String>, affected::Hunks)> {
    let root = &options.root;
    let mut base: Vec<&str> = vec!["diff", "--no-renames"];
    if staged {
        base.push("--cached");
    }
    match range {
        Some(range) => base.push(range),
        None if !staged => base.push("HEAD"),
        None => {}
    }
    let names = [base.as_slice(), &["--name-only"]].concat();
    let listed = git(root, &names).map_err(|error| fail(format!("{error:#}")))?;
    let mut files: Vec<String> = listed.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect();
    if range.is_none() && !staged {
        let untracked = git(root, &["ls-files", "--others", "--exclude-standard"]).map_err(|error| fail(format!("{error:#}")))?;
        files.extend(untracked.lines().filter(|line| !line.is_empty()).map(str::to_owned));
    }
    files.sort();
    files.dedup();
    let diff = git(root, &[base.as_slice(), &["-U0"]].concat()).unwrap_or_default();
    Ok((files, affected::hunks(&diff)))
}

fn affected_tests(options: &Options) -> Result<i32> {
    let loaded = load(&options.root, options.team.as_deref())?;
    let config = &loaded.config;
    let full = options.rest.iter().any(|arg| arg == "--full");
    let staged = options.rest.iter().any(|arg| arg == "--staged");
    let mut range = None;
    let mut listed = Vec::new();
    let mut in_files = false;
    for arg in &options.rest {
        match arg.as_str() {
            "--full" | "--staged" => {}
            "--files" => in_files = true,
            other if other.starts_with("--") => return Err(usage()),
            other if in_files => listed.push(other.replace('\\', "/")),
            other if range.is_none() => range = Some(other.to_owned()),
            _ => return Err(usage()),
        }
    }
    let durations = durations::Durations::read(&options.root.join(&config.map.durations)).map_err(|error| fail(format!("{error:#}")))?;
    let (files, hunks) = if !listed.is_empty() {
        (listed, affected::Hunks::new())
    } else if full {
        (Vec::new(), affected::Hunks::new())
    } else {
        changes(options, range.as_deref(), staged)?
    };
    // #741: the providers a change touches are read from the sources now, so
    // a merge never leaves the selection behind; the rest, and a build this
    // checkout cannot read, come from the committed map.
    let committed = read_map(&options.root, config)?;
    let touched: std::collections::BTreeSet<String> = config
        .providers
        .iter()
        .filter(|provider| full || committed.is_none() || files.iter().any(|file| map::in_area(provider, file) || provider.paths.iter().any(|rule| glob::any(&rule.files, file))))
        .map(|provider| provider.name.clone())
        .collect();
    let tracked = map::project_files(&options.root).map_err(|error| fail(format!("{error:#}")))?;
    let map = map::generate_only(&options.root, config, &tracked, committed.as_ref(), Some(&touched)).map_err(|error| fail(format!("{error:#}")))?;
    let selection = affected::select(&options.root, config, &map, &durations, &files, &hunks, full);
    let mut text = affected::text(&selection);
    if !loaded.active() {
        text.push_str(&format!("\nNo {} here: this is Zerus's proposal, and nothing enforces it.\n", config::FILE));
    }
    if full && config.full.policy == config::FullPolicy::Owner {
        text.push_str("\nThe full run is the owner's call: ask with this time before starting it.\n");
    }
    print(options, json!({"active": loaded.active(), "full_policy": config.full.policy, "full_place": config.full.place, "selection": selection}), &text);
    Ok(0)
}

fn record_durations(options: &Options) -> Result<i32> {
    let loaded = load(&options.root, options.team.as_deref())?;
    let config = &loaded.config;
    let mut provider = None;
    let mut target = None;
    let mut day = None;
    let mut files = Vec::new();
    let mut index = 0;
    let rest = &options.rest;
    while index < rest.len() {
        match rest[index].as_str() {
            "--record" => {}
            "--provider" => {
                provider = rest.get(index + 1).cloned();
                index += 1;
            }
            "--target" => {
                target = rest.get(index + 1).cloned();
                index += 1;
            }
            "--day" => {
                day = rest.get(index + 1).cloned();
                index += 1;
            }
            other => files.push(other.to_owned()),
        }
        index += 1;
    }
    let provider = provider.ok_or_else(usage)?;
    if files.is_empty() {
        return Err(usage());
    }
    let map = read_map(&options.root, config)?.ok_or_else(|| fail(format!("No test map at {}: write it first", config.map.file)))?;
    let section = map.providers.get(&provider).ok_or_else(|| fail(format!("The map has no provider {provider}")))?;
    let path = options.root.join(&config.map.durations);
    let mut durations = durations::Durations::read(&path).map_err(|error| fail(format!("{error:#}")))?;
    durations.schema = 1;
    let day = day.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    let mut count = 0;
    for file in &files {
        let bytes = std::fs::read(file).map_err(|error| fail(format!("Cannot read {file}: {error}")))?;
        count += durations.record(&provider, section, &String::from_utf8_lossy(&bytes), target.as_deref(), &day);
    }
    if count == 0 {
        return Err(fail(format!("Nothing in {} names a test of {provider} the map knows", files.join(", "))));
    }
    write_json(&path, &durations)?;
    print(options, json!({"recorded": count, "file": config.map.durations}), &format!("Recorded {count} time(s) in {}.\n", config.map.durations));
    Ok(0)
}

fn compare_coverage(options: &Options) -> Result<i32> {
    let loaded = load(&options.root, options.team.as_deref())?;
    let config = &loaded.config;
    let mut provider = None;
    let mut reports = Vec::new();
    let mut fail_on_gaps = false;
    let rest = &options.rest;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--provider" => {
                provider = rest.get(index + 1).cloned();
                index += 1;
            }
            "--report" => {
                let pair = rest.get(index + 1).ok_or_else(usage)?;
                let (target, file) = pair.rsplit_once('=').ok_or_else(usage)?;
                reports.push((target.to_owned(), file.to_owned()));
                index += 1;
            }
            "--fail-on-gaps" => fail_on_gaps = true,
            _ => return Err(usage()),
        }
        index += 1;
    }
    let provider = provider.ok_or_else(usage)?;
    let map = read_map(&options.root, config)?.ok_or_else(|| fail(format!("No test map at {}", config.map.file)))?;
    let files = map::project_files(&options.root).map_err(|error| fail(format!("{error:#}")))?;
    let mut comparisons = Vec::new();
    let mut text = String::new();
    for (target, file) in &reports {
        let report = std::fs::read_to_string(file).map_err(|error| fail(format!("Cannot read {file}: {error}")))?;
        let report = coverage::read_report(&report);
        let comparison = coverage::compare(&options.root, config, &map, &provider, target, &report, &files).map_err(|error| fail(format!("{error:#}")))?;
        text.push_str(&format!("{target}: {} covered file(s), {} gap(s), {} mapped but unused\n", comparison.covered, comparison.gaps.len(), comparison.unused.len()));
        for gap in &comparison.gaps {
            text.push_str(&format!("  gap: {gap}\n"));
        }
        for unused in &comparison.unused {
            text.push_str(&format!("  unused: {unused}\n"));
        }
        comparisons.push(comparison);
    }
    let gaps: usize = comparisons.iter().map(|comparison| comparison.gaps.len()).sum();
    print(options, json!({"gaps": gaps, "targets": comparisons}), &text);
    Ok(if fail_on_gaps && gaps > 0 { 1 } else { 0 })
}
