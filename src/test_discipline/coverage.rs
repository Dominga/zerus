//! U62: the weekly check of the map against coverage. Each report is one
//! target's run (lcov from gcov or `cargo llvm-cov --lcov`, Cobertura XML from
//! OpenCppCoverage, or llvm-cov's JSON export); a file a target covers that the
//! map does not give it is a gap, and a file the map gives it that its run
//! never executed is over-selection.

use super::map::{in_area, TestMap};
use super::config::TestsConfig;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Comparison {
    pub provider: String,
    pub target: String,
    /// Covered by the run, not in the map: the map misses them.
    pub gaps: Vec<String>,
    /// In the map, measured and never executed by this target.
    pub unused: Vec<String>,
    pub covered: usize,
}

/// Per file, whether any of its measured lines ran.
pub fn read_report(text: &str) -> BTreeMap<String, bool> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        return llvm_json(trimmed);
    }
    if trimmed.starts_with('<') {
        return cobertura(trimmed);
    }
    lcov(text)
}

fn lcov(text: &str) -> BTreeMap<String, bool> {
    let mut out = BTreeMap::new();
    let mut file: Option<String> = None;
    let mut hit = false;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            file = Some(path.trim().to_owned());
            hit = false;
        } else if let Some(data) = line.strip_prefix("DA:") {
            if data.split(',').nth(1).and_then(|count| count.trim().parse::<u64>().ok()).is_some_and(|count| count > 0) {
                hit = true;
            }
        } else if line.trim() == "end_of_record" {
            if let Some(file) = file.take() {
                let known = out.entry(file).or_insert(false);
                *known |= hit;
            }
        }
    }
    out
}

fn cobertura(text: &str) -> BTreeMap<String, bool> {
    let mut out = BTreeMap::new();
    let mut current: Option<String> = None;
    for piece in text.split('<').skip(1) {
        if let Some(rest) = piece.strip_prefix("class ") {
            current = attribute(rest, "filename");
            if let Some(file) = &current {
                out.entry(file.clone()).or_insert(false);
            }
        } else if let Some(rest) = piece.strip_prefix("line ") {
            let hits = attribute(rest, "hits").and_then(|hits| hits.parse::<u64>().ok()).unwrap_or(0);
            if let (Some(file), true) = (&current, hits > 0) {
                out.insert(file.clone(), true);
            }
        }
    }
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].replace("&amp;", "&"))
}

fn llvm_json(text: &str) -> BTreeMap<String, bool> {
    let value: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let mut out = BTreeMap::new();
    for data in value["data"].as_array().into_iter().flatten() {
        for file in data["files"].as_array().into_iter().flatten() {
            if let Some(name) = file["filename"].as_str() {
                let covered = file["summary"]["lines"]["covered"].as_u64().unwrap_or(0) > 0;
                *out.entry(name.to_owned()).or_insert(false) |= covered;
            }
        }
    }
    out
}

/// A report's path relative to the project, or None outside it.
fn project_relative(root: &Path, path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let base = root.to_string_lossy().replace('\\', "/");
    let base = format!("{}/", base.trim_end_matches('/'));
    if path.to_ascii_lowercase().starts_with(&base.to_ascii_lowercase()) {
        return Some(path[base.len()..].to_owned());
    }
    // A relative report path is the project's already.
    (!Path::new(&path).is_absolute()).then(|| path.trim_start_matches("./").to_owned())
}

pub fn compare(root: &Path, config: &TestsConfig, map: &TestMap, provider: &str, target: &str, report: &BTreeMap<String, bool>, files: &[String]) -> anyhow::Result<Comparison> {
    let settings = config.providers.iter().find(|known| known.name == provider).ok_or_else(|| anyhow::anyhow!("No test provider is named {provider}"))?;
    let section = map.providers.get(provider).ok_or_else(|| anyhow::anyhow!("The map has no provider {provider}"))?;
    anyhow::ensure!(section.targets.contains_key(target), "The map of {provider} has no target {target}");
    // The project's spelling of each file, for reports that lower its case.
    let spelled: BTreeMap<String, &String> = files.iter().map(|file| (file.to_ascii_lowercase(), file)).collect();
    let mapped: BTreeSet<&String> = section
        .files
        .iter()
        .chain(section.test_files.iter())
        .filter(|(_, ids)| ids.iter().any(|id| id == target || id == super::map::ALL))
        .map(|(file, _)| file)
        .collect();
    let mut comparison = Comparison { provider: provider.to_owned(), target: target.to_owned(), ..Comparison::default() };
    let mut measured = BTreeSet::new();
    for (path, hit) in report {
        let Some(relative) = project_relative(root, path) else { continue };
        let relative = spelled.get(&relative.to_ascii_lowercase()).map(|file| (*file).clone()).unwrap_or(relative);
        if !in_area(settings, &relative) || super::glob::any(&config.map.ignore, &relative) {
            continue;
        }
        measured.insert(relative.clone());
        if *hit {
            comparison.covered += 1;
            if !mapped.contains(&relative) {
                comparison.gaps.push(relative);
            }
        } else if mapped.contains(&relative) {
            comparison.unused.push(relative);
        }
    }
    comparison.gaps.sort();
    comparison.unused.sort();
    Ok(comparison)
}

/// The project files a report's run executed, relative to the project.
pub fn covered_files(root: &Path, report: &BTreeMap<String, bool>) -> BTreeSet<String> {
    report.iter().filter(|(_, hit)| **hit).filter_map(|(path, _)| project_relative(root, path)).collect()
}

/// #718, monthly: the targets whose covered files are all covered by other
/// targets too - no unique coverage, so candidates to merge or drop.
pub fn without_unique_coverage(covered: &BTreeMap<String, BTreeSet<String>>) -> Vec<String> {
    let mut counts: BTreeMap<&String, usize> = BTreeMap::new();
    for files in covered.values() {
        for file in files {
            *counts.entry(file).or_default() += 1;
        }
    }
    covered
        .iter()
        .filter(|(_, files)| !files.is_empty() && files.iter().all(|file| counts.get(file).copied().unwrap_or(0) > 1))
        .map(|(target, _)| target.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_discipline::config::parse_toml;
    use crate::test_discipline::map::{ProviderMap, Target};
    use crate::test_discipline::config::{Kind, Rule};

    #[test]
    fn the_three_report_formats_name_the_files_that_ran() {
        let lcov = "TN:\nSF:/p/src/a.rs\nDA:1,0\nDA:2,3\nend_of_record\nSF:/p/src/b.rs\nDA:1,0\nend_of_record\n";
        assert_eq!(read_report(lcov), BTreeMap::from([("/p/src/a.rs".into(), true), ("/p/src/b.rs".into(), false)]));
        let xml = r#"<?xml version="1.0"?><coverage><packages><package><classes><class name="a" filename="D:\p\gui\A.cpp"><lines><line number="3" hits="0"/><line number="4" hits="2"/></lines></class><class filename="D:\p\gui\B.cpp"><lines><line number="1" hits="0"/></lines></class></classes></package></packages></coverage>"#;
        assert_eq!(read_report(xml), BTreeMap::from([("D:\\p\\gui\\A.cpp".into(), true), ("D:\\p\\gui\\B.cpp".into(), false)]));
        let json = r#"{"data":[{"files":[{"filename":"/p/src/a.rs","summary":{"lines":{"covered":4}}},{"filename":"/p/src/c.rs","summary":{"lines":{"covered":0}}}]}]}"#;
        assert_eq!(read_report(json), BTreeMap::from([("/p/src/a.rs".into(), true), ("/p/src/c.rs".into(), false)]));
    }

    #[test]
    fn tests_without_unique_coverage_are_named() {
        let set = |files: &[&str]| files.iter().map(|file| (*file).to_owned()).collect::<BTreeSet<String>>();
        let covered = BTreeMap::from([
            ("a".to_owned(), set(&["x.rs", "y.rs"])),
            ("b".to_owned(), set(&["x.rs"])),
            ("c".to_owned(), set(&["y.rs", "z.rs"])),
            ("d".to_owned(), set(&[])),
        ]);
        // a's files are b's and c's too, b's are a's; c alone covers z.rs.
        assert_eq!(without_unique_coverage(&covered), vec!["a", "b"]);
    }

    #[test]
    fn gaps_and_unused_files_are_found_against_the_map() {
        let config: TestsConfig = serde_json::from_value(
            parse_toml("[[provider]]\nname = \"gui\"\nkind = \"ctest\"\nbuild_dir = \"b\"\nsources = [\"gui/**\"]\n").unwrap(),
        )
        .unwrap();
        let mut section = ProviderMap {
            kind: Kind::Ctest,
            rule: Rule::Build,
            note: String::new(),
            targets: BTreeMap::from([("test_a".into(), Target::default())]),
            files: BTreeMap::new(),
            test_files: BTreeMap::new(),
            rules: Vec::new(),
            groups: BTreeMap::new(),
            seconds: 0.0,
        };
        section.files.insert("gui/A.cpp".into(), vec!["test_a".into()]);
        section.files.insert("gui/Unused.cpp".into(), vec!["test_a".into()]);
        let map = TestMap { schema: 1, providers: BTreeMap::from([("gui".into(), section)]) };
        let root = Path::new("D:/p");
        let report = BTreeMap::from([
            ("D:\\p\\gui\\a.cpp".to_owned(), true),
            ("D:\\p\\gui\\Missed.cpp".to_owned(), true),
            ("D:\\p\\gui\\Unused.cpp".to_owned(), false),
            ("C:\\Qt\\include\\qstring.h".to_owned(), true),
        ]);
        let files = vec!["gui/A.cpp".to_owned(), "gui/Missed.cpp".to_owned(), "gui/Unused.cpp".to_owned()];
        let comparison = compare(root, &config, &map, "gui", "test_a", &report, &files).unwrap();
        assert_eq!(comparison.gaps, vec!["gui/Missed.cpp"]);
        assert_eq!(comparison.unused, vec!["gui/Unused.cpp"]);
        assert_eq!(comparison.covered, 2);
        assert!(compare(root, &config, &map, "gui", "test_b", &report, &files).is_err());
    }
}
