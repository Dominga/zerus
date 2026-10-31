//! U62: the test map, from source areas to test targets, written from the
//! build by `hgs test map --write` and checked by `hgs test map --check`.

use super::cargo_graph;
use super::cmake;
use super::config::{Kind, Provider, Rule, TestsConfig, Widen};
use super::glob;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct TestMap {
    pub schema: u32,
    pub providers: BTreeMap<String, ProviderMap>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ProviderMap {
    pub kind: Kind,
    pub rule: Rule,
    /// What the map could not read, such as a build without CMake's reply.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub targets: BTreeMap<String, Target>,
    /// A source file to the targets that use it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, Vec<String>>,
    /// A test file to the targets it holds.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub test_files: BTreeMap<String, Vec<String>>,
    /// Path rules whose `mentions` were resolved: globs to targets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<ResolvedRule>,
    /// cargo: each crate's root file, to name a run's output.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub groups: BTreeMap<String, String>,
    /// How long reading it took; not written.
    #[serde(skip)]
    pub seconds: f64,
    /// #732: the platform classes its suites run on; empty is any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Target {
    /// The invocation it belongs to: a crate (`--bin hgs`), a CTest build, a
    /// Gradle project; empty is the provider's one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// What it adds to the command: cargo filters, CTest names, test files,
    /// test classes.
    pub run: Vec<String>,
    /// cargo: filters of its child modules, skipped when not chosen too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip: Vec<String>,
    /// How many tests (cargo, a test file) or test functions (QtTest).
    pub tests: usize,
    /// QtTest: its test functions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<String>,
    /// ctest: the test binary, relative to the project, to run single functions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub binary: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ResolvedRule {
    pub files: Vec<String>,
    pub targets: Vec<String>,
}

/// The project's tracked files (#741): a checkout's untracked logs and
/// artifacts are no area, and a new test counts once it is added.
pub fn project_files(root: &Path) -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "--cached", "-z"])
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|error| anyhow::anyhow!("Cannot run git: {error}"))?;
    anyhow::ensure!(output.status.success(), "git ls-files failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    let mut files: Vec<String> = output.stdout.split(|byte| *byte == 0).filter(|name| !name.is_empty()).map(|name| String::from_utf8_lossy(name).into_owned()).collect();
    files.sort();
    files.dedup();
    // A deleted file Git still lists is gone.
    files.retain(|file| root.join(file).exists());
    Ok(files)
}

pub fn in_area(provider: &Provider, file: &str) -> bool {
    glob::any(&provider.sources, file) || glob::any(&provider.tests, file)
}

/// The map of every enabled provider. `previous` keeps a provider whose build
/// cannot be read now (no CMake reply yet), marked by its note.
pub fn generate(root: &Path, config: &TestsConfig, files: &[String], previous: Option<&TestMap>) -> anyhow::Result<TestMap> {
    generate_only(root, config, files, previous, None)
}

/// `generate` for the providers `only` names (all without it); the others
/// keep `previous`'s sections. A change's affected set reads the providers it
/// touches this way, from the sources as they are now (#741).
pub fn generate_only(root: &Path, config: &TestsConfig, files: &[String], previous: Option<&TestMap>, only: Option<&BTreeSet<String>>) -> anyhow::Result<TestMap> {
    let mut map = TestMap { schema: SCHEMA, providers: BTreeMap::new() };
    for provider in config.providers.iter().filter(|provider| !provider.disabled) {
        if only.is_some_and(|only| !only.contains(&provider.name)) {
            if let Some(kept) = previous.and_then(|map| map.providers.get(&provider.name)) {
                map.providers.insert(provider.name.clone(), kept.clone());
            }
            continue;
        }
        let started = std::time::Instant::now();
        let area: Vec<String> = files.iter().filter(|file| in_area(provider, file)).cloned().collect();
        let mut section = match (provider.affected, provider.kind) {
            (Rule::All, _) => whole(provider),
            (Rule::Paths, _) => ProviderMap { rule: Rule::Paths, ..whole(provider) },
            (Rule::Build, Kind::Cargo) => cargo(root, provider)?,
            (Rule::Build, Kind::Ctest) => match ctest(root, provider)? {
                Some(section) => section,
                None => {
                    let note = format!("{} is not configured with CMake's reply here: configure it, then write the map again", provider.build_dir);
                    let kept = previous.and_then(|map| map.providers.get(&provider.name)).cloned();
                    ProviderMap { note, ..kept.unwrap_or_else(|| ProviderMap { rule: Rule::Build, ..whole(provider) }) }
                }
            },
            (Rule::Build, _) => references(root, provider, &area),
        };
        section.rules = resolve_rules(root, provider, &area, &section);
        // A file three quarters of a larger set uses runs all of it: a few
        // more tests, and a map that stays small.
        let count = section.targets.len();
        if count > 2 {
            for ids in section.files.values_mut() {
                if ids.len() * 4 >= count * 3 {
                    *ids = vec![ALL.to_owned()];
                }
            }
        }
        section.seconds = started.elapsed().as_secs_f64();
        section.platforms = provider.platform_classes();
        map.providers.insert(provider.name.clone(), section);
    }
    Ok(map)
}

/// A provider whose every test runs as one: `all`, and `paths` before its
/// rules name targets.
fn whole(provider: &Provider) -> ProviderMap {
    let mut targets = BTreeMap::new();
    targets.insert(ALL.to_owned(), Target::default());
    ProviderMap {
        kind: provider.kind,
        rule: Rule::All,
        note: String::new(),
        targets,
        files: BTreeMap::new(),
        test_files: BTreeMap::new(),
        rules: Vec::new(),
        groups: BTreeMap::new(),
        seconds: 0.0,
        platforms: provider.platform_classes(),
    }
}

/// The target of a provider's whole set, and a file every target uses.
pub const ALL: &str = "(all)";

fn cargo(root: &Path, provider: &Provider) -> anyhow::Result<ProviderMap> {
    let manifest = root.join(&provider.root).join(if provider.manifest.is_empty() { "Cargo.toml" } else { &provider.manifest });
    let graph = cargo_graph::read(root, &manifest)?;
    let mut section = ProviderMap { rule: Rule::Build, ..whole(provider) };
    section.targets.clear();
    for (key, module) in &graph.modules {
        if module.path.is_empty() {
            section.groups.insert(module.crate_arg.clone(), module.file.clone());
        }
        if module.tests.is_empty() {
            continue;
        }
        // The crate root's tests are named one by one: its prefix is every test's.
        let run = if module.path.is_empty() { module.tests.clone() } else { vec![format!("{}::", module.path)] };
        let skip = module
            .children
            .iter()
            .filter(|child| graph.modules.get(*child).is_some_and(|child| has_tests(&graph, child)))
            .map(|child| format!("{}::", graph.modules[child].path))
            .collect();
        let skip = if module.path.is_empty() { Vec::new() } else { skip };
        section.targets.insert(
            key.clone(),
            Target { group: module.crate_arg.clone(), run, skip, tests: module.tests.len(), ..Target::default() },
        );
        section.test_files.entry(module.file.clone()).or_default().push(key.clone());
        let mut used = BTreeSet::from([key.clone()]);
        match provider.widen {
            Widen::None => {}
            Widen::Dependants => used.extend(graph.uses(key, false)),
            Widen::Transitive => used.extend(graph.uses(key, true)),
        }
        for dependency in used {
            let Some(found) = graph.modules.get(&dependency) else { continue };
            for file in std::iter::once(&found.file).chain(found.includes.iter()) {
                let list = section.files.entry(file.clone()).or_default();
                if !list.contains(key) {
                    list.push(key.clone());
                }
            }
        }
    }
    Ok(section)
}

fn has_tests(graph: &cargo_graph::Graph, module: &cargo_graph::Module) -> bool {
    !module.tests.is_empty() || module.children.iter().any(|child| graph.modules.get(child).is_some_and(|child| has_tests(graph, child)))
}

fn ctest(root: &Path, provider: &Provider) -> anyhow::Result<Option<ProviderMap>> {
    let build = root.join(&provider.build_dir);
    // A checkout without this build (a CI job, another computer) keeps the
    // committed map of it.
    if !build.is_dir() {
        return Ok(None);
    }
    let build = match cmake::read(root, &build)? {
        cmake::Reply::Asked => return Ok(None),
        cmake::Reply::Ready(build) => build,
    };
    let used = cmake::used_files(root, &build);
    let mut section = ProviderMap { rule: Rule::Build, ..whole(provider) };
    section.targets.clear();
    let mut by_target: BTreeMap<&str, Vec<&cmake::Test>> = BTreeMap::new();
    for test in &build.tests {
        by_target.entry(test.target.as_str()).or_default().push(test);
    }
    for (name, tests) in by_target {
        let Some(files) = used.get(name) else { continue };
        let own: Vec<&String> = build.targets.values().filter(|target| target.name == name).flat_map(|target| target.sources.iter()).collect();
        let mut functions = Vec::new();
        for source in own.iter().filter(|source| glob::any(&provider.tests, source) || provider.tests.is_empty()) {
            if let Ok(text) = std::fs::read_to_string(root.join(source)) {
                functions.extend(cmake::qt_functions(&text));
            }
            section.test_files.entry((*source).clone()).or_default().push(name.to_owned());
        }
        let count = if functions.is_empty() { tests.len() } else { functions.len() };
        section.targets.insert(
            name.to_owned(),
            Target {
                group: String::new(),
                run: tests.iter().map(|test| test.name.clone()).collect(),
                skip: Vec::new(),
                tests: count,
                functions,
                binary: build.targets.values().find(|target| target.name == name).and_then(|target| target.artifacts.first()).map(|path| super::cargo_graph::relative(root, Path::new(path))).unwrap_or_default(),
            },
        );
        for file in files {
            if !in_area(provider, file) {
                continue;
            }
            section.files.entry(file.clone()).or_default().push(name.to_owned());
        }
    }
    Ok(Some(section))
}

/// Short names every project has many of; mentioning one says nothing.
const COMMON: [&str; 14] = ["index", "main", "mod", "lib", "init", "__init__", "util", "utils", "types", "common", "test", "tests", "conftest", "setup"];

/// Tests found by what they name: a test file uses a source file whose name
/// (or module name, or file name) it mentions, and through it what that file
/// mentions, as the provider widens.
fn references(root: &Path, provider: &Provider, area: &[String]) -> ProviderMap {
    let mut section = ProviderMap { rule: Rule::Build, ..whole(provider) };
    section.targets.clear();
    let tests: Vec<&String> = area.iter().filter(|file| glob::any(&provider.tests, file) && !glob::any(&provider.manual, file)).collect();
    let texts: BTreeMap<&String, String> = area.iter().filter_map(|file| std::fs::read_to_string(root.join(file)).ok().map(|text| (file, text))).collect();
    let words: BTreeMap<&String, BTreeSet<&str>> = texts.iter().map(|(file, text)| (*file, word_set(text))).collect();
    // File names as a text spells them, in paths or alone.
    let spelled: BTreeMap<&String, BTreeSet<&str>> = texts
        .iter()
        .map(|(file, text)| (*file, text.split(|ch: char| !(ch.is_alphanumeric() || "_.-".contains(ch))).filter(|word| word.contains('.')).collect()))
        .collect();
    // A test is no other test's dependency.
    let names: BTreeMap<&String, (String, String, Option<&str>)> = area
        .iter()
        .filter(|file| !glob::any(&provider.tests, file))
        .filter_map(|file| {
            let path = Path::new(file.as_str());
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            let name = path.file_name()?.to_string_lossy().into_owned();
            (stem.len() >= 4 && !COMMON.contains(&stem.as_str())).then_some((file, (stem, name, language(file))))
        })
        .collect();
    // Who mentions whom, among the area's files: code by its module or class
    // name in the same language (an import), anything by its file name.
    let mentions = |from: &String| -> BTreeSet<String> {
        let (Some(set), Some(spelled)) = (words.get(from), spelled.get(from)) else { return BTreeSet::new() };
        let own = language(from);
        if own.is_none() {
            return BTreeSet::new();
        }
        names
            .iter()
            .filter(|(file, _)| **file != from)
            .filter(|(_, (stem, name, language))| {
                let imported = language.is_some() && *language == own && (set.contains(stem.as_str()) || set.contains(stem.replace('-', "_").as_str()));
                imported || spelled.contains(name.as_str())
            })
            .map(|(file, _)| (*file).clone())
            .collect()
    };
    let graph: BTreeMap<String, BTreeSet<String>> = area.iter().map(|file| (file.clone(), mentions(file))).collect();
    for test in tests {
        // Named as its command names it: a module, a class, a file.
        let id = run_name(provider, root, test);
        let run = vec![id.clone()];
        let count = texts.get(test).map(|text| count_tests(provider.kind, text)).unwrap_or(0);
        section.targets.insert(id.clone(), Target { run, tests: count.max(1), ..Target::default() });
        section.test_files.insert(test.clone(), vec![id.clone()]);
        let mut used = BTreeSet::new();
        let mut frontier: Vec<String> = graph.get(test).into_iter().flatten().cloned().collect();
        let mut depth = 0;
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for file in frontier {
                if used.insert(file.clone()) {
                    next.extend(graph.get(&file).into_iter().flatten().cloned());
                }
            }
            depth += 1;
            frontier = match provider.widen {
                Widen::None => Vec::new(),
                Widen::Dependants if depth >= 2 => Vec::new(),
                _ => next,
            };
        }
        for file in used {
            section.files.entry(file).or_default().push(id.clone());
        }
    }
    section
}

/// The language a code file imports by name; None for data and the rest.
fn language(file: &str) -> Option<&'static str> {
    let ext = Path::new(file).extension()?.to_str()?;
    Some(match ext {
        "py" => "python",
        "kt" | "kts" | "java" => "jvm",
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => "js",
        "rs" => "rust",
        "swift" => "swift",
        "c" | "cc" | "cpp" | "cxx" | "h" | "hpp" | "mm" => "c",
        "sh" | "ps1" => "shell",
        _ => return None,
    })
}

fn word_set(text: &str) -> BTreeSet<&str> {
    text.split(|ch: char| !(ch.is_alphanumeric() || ch == '_')).filter(|word| word.len() >= 4).collect()
}

fn count_tests(kind: Kind, text: &str) -> usize {
    match kind {
        Kind::Pytest | Kind::Unittest => text.lines().filter(|line| line.trim_start().starts_with("def test")).count(),
        Kind::Gradle => text.matches("@Test").count(),
        Kind::Npm => text.matches("it(").count() + text.matches("test(").count(),
        _ => 1,
    }
}

/// How a test file is named on its command: a unittest module, a Gradle
/// class, or the file relative to the provider's folder.
pub fn run_name(provider: &Provider, root: &Path, file: &str) -> String {
    let relative = if provider.root.is_empty() {
        file.to_owned()
    } else {
        file.strip_prefix(&format!("{}/", provider.root.trim_end_matches('/'))).unwrap_or(file).to_owned()
    };
    match provider.kind {
        Kind::Unittest => relative.trim_end_matches(".py").replace('/', "."),
        Kind::Gradle => {
            let text = std::fs::read_to_string(root.join(file)).unwrap_or_default();
            let package = text.lines().find_map(|line| line.trim().strip_prefix("package ")).map(|name| name.trim().trim_end_matches(';').to_owned());
            let class = Path::new(file).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            match package {
                Some(package) if !package.is_empty() => format!("{package}.{class}"),
                _ => class,
            }
        }
        _ => relative,
    }
}

fn resolve_rules(root: &Path, provider: &Provider, area: &[String], section: &ProviderMap) -> Vec<ResolvedRule> {
    provider
        .paths
        .iter()
        .map(|rule| {
            let mut targets: Vec<String> = rule
                .targets
                .iter()
                .flat_map(|target| {
                    if section.targets.contains_key(target) {
                        vec![target.clone()]
                    } else {
                        // A test file or glob names the targets its files hold.
                        section.test_files.iter().filter(|(file, _)| glob::matches(target, file)).flat_map(|(_, ids)| ids.clone()).collect()
                    }
                })
                .collect();
            if !rule.mentions.is_empty() {
                for (file, ids) in &section.test_files {
                    if glob::any(&provider.manual, file) || !area.contains(file) {
                        continue;
                    }
                    if std::fs::read_to_string(root.join(file)).is_ok_and(|text| text.contains(&rule.mentions)) {
                        targets.extend(ids.iter().cloned());
                    }
                }
            }
            targets.sort();
            targets.dedup();
            ResolvedRule { files: rule.files.clone(), targets }
        })
        .collect()
}

/// What `--check` finds. The map it checks is the one the sources give now
/// (#741): the committed file only stands in for a build this checkout cannot
/// read, so a merge that changes sources never leaves the check failing; how
/// far the committed snapshot lags is told, not failed.
#[derive(Debug, Default, Serialize)]
pub struct Findings {
    /// Files in no provider's area and not ignored: a new area.
    pub outside: Vec<String>,
    /// Test files no provider maps.
    pub missing_tests: Vec<String>,
    /// Where the committed snapshot differs from the map now (not a failure).
    pub stale: Vec<String>,
    /// Providers whose build could not be read now.
    pub unread: Vec<String>,
}

impl Findings {
    pub fn ok(&self) -> bool {
        self.outside.is_empty() && self.missing_tests.is_empty()
    }
}

pub fn check(config: &TestsConfig, files: &[String], committed: &TestMap, fresh: &TestMap) -> Findings {
    let mut findings = Findings::default();
    let providers: Vec<&Provider> = config.providers.iter().filter(|provider| !provider.disabled).collect();
    for file in files {
        if glob::any(&config.map.ignore, file) {
            continue;
        }
        if !providers.iter().any(|provider| in_area(provider, file)) {
            findings.outside.push(file.clone());
        }
    }
    for provider in &providers {
        let Some(now) = fresh.providers.get(&provider.name) else {
            findings.missing_tests.push(format!("{}: no map (its build was never read)", provider.name));
            continue;
        };
        if !now.note.is_empty() {
            findings.unread.push(format!("{}: {}", provider.name, now.note));
        }
        if now.rule == Rule::All {
            continue;
        }
        let named = |file: &&String| glob::any(&provider.tests, file) && !glob::any(&provider.manual, file) && !glob::any(&provider.elsewhere, file);
        for file in files.iter().filter(named) {
            if !now.test_files.contains_key(file) {
                findings.missing_tests.push(format!("{}: {file}", provider.name));
            }
        }
        let Some(section) = committed.providers.get(&provider.name) else {
            findings.stale.push(format!("{}: not in the committed map", provider.name));
            continue;
        };
        if !now.note.is_empty() {
            continue;
        }
        for id in now.targets.keys().filter(|id| !section.targets.contains_key(*id)) {
            findings.stale.push(format!("{}: new target {id}", provider.name));
        }
        for id in section.targets.keys().filter(|id| !now.targets.contains_key(*id)) {
            findings.stale.push(format!("{}: target {id} is gone", provider.name));
        }
        for (file, ids) in &now.files {
            if section.files.get(file) != Some(ids) {
                findings.stale.push(format!("{}: {file} maps to other targets now", provider.name));
            }
        }
        for file in section.files.keys().filter(|file| !now.files.contains_key(*file)) {
            findings.stale.push(format!("{}: {file} maps to nothing now", provider.name));
        }
        for file in now.test_files.keys().filter(|file| !section.test_files.contains_key(*file)) {
            findings.stale.push(format!("{}: {file} is a new test file", provider.name));
        }
    }
    for list in [&mut findings.outside, &mut findings.missing_tests, &mut findings.stale, &mut findings.unread] {
        list.sort();
        list.dedup();
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_discipline::config::parse_toml;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn config(text: &str) -> TestsConfig {
        serde_json::from_value(parse_toml(text).unwrap()).unwrap()
    }

    const CONFIG: &str = r#"
[map]
ignore = ["docs/**"]
[[provider]]
name = "rust"
kind = "cargo"
sources = ["src/**", "Cargo.toml", "fixtures/**"]
[[provider]]
name = "py"
kind = "unittest"
root = "tests"
sources = ["scripts/**"]
tests = ["tests/test_*.py"]
manual = ["tests/test_native.py"]
[[provider.paths]]
files = ["src/**"]
mentions = "target/debug/demo"
"#;

    fn files() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Cargo.toml", "[package]\nname = \"demo\"\n"),
            ("src/main.rs", "mod store;\nmod cli;\nfn main() { cli::run(); }\n"),
            ("src/cli.rs", "pub fn run() { crate::store::save(); }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn runs() {}\n}\n"),
            ("src/store.rs", "pub fn save() {}\nconst C: &str = include_str!(\"../fixtures/c.json\");\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn saves() {}\n    #[test]\n    fn loads() {}\n}\n"),
            ("fixtures/c.json", "{}"),
            ("scripts/release-notes.py", "def notes(): pass\n"),
            ("scripts/helper_tool.py", "import json\n"),
            ("tests/test_release.py", "SCRIPT = 'scripts/release-notes.py'\ndef test_a(): pass\ndef test_b(): pass\n"),
            ("tests/test_cli.py", "HGS = 'target/debug/demo'\ndef test_c(): pass\n"),
            ("tests/test_native.py", "HGS = 'target/debug/demo'\n"),
            ("docs/a.md", "x"),
        ]
    }

    fn listed(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                if entry.path().is_dir() {
                    walk(root, &entry.path(), out);
                } else {
                    out.push(entry.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
                }
            }
        }
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[test]
    fn the_map_names_cargo_modules_and_the_files_they_use() {
        let dir = project(&files());
        let map = generate(dir.path(), &config(CONFIG), &listed(dir.path()), None).unwrap();
        let rust = &map.providers["rust"];
        let ids: Vec<&String> = rust.targets.keys().collect();
        assert_eq!(ids, vec!["--bin demo cli", "--bin demo store"]);
        assert_eq!(rust.targets["--bin demo store"].run, vec!["store::"]);
        assert_eq!(rust.targets["--bin demo store"].tests, 2);
        // cli uses store, so a change in store runs both; the fixture is store's.
        assert_eq!(rust.files["src/store.rs"], vec!["--bin demo cli", "--bin demo store"]);
        assert_eq!(rust.files["fixtures/c.json"], vec!["--bin demo cli", "--bin demo store"]);
        assert_eq!(rust.files["src/cli.rs"], vec!["--bin demo cli"]);
        assert_eq!(rust.groups["--bin demo"], "src/main.rs");
    }

    #[test]
    fn python_tests_are_found_by_what_they_name_and_by_path_rules() {
        let dir = project(&files());
        let map = generate(dir.path(), &config(CONFIG), &listed(dir.path()), None).unwrap();
        let py = &map.providers["py"];
        assert_eq!(py.targets.keys().collect::<Vec<_>>(), vec!["test_cli", "test_release"]);
        assert_eq!(py.targets["test_release"].run, vec!["test_release"]);
        assert_eq!(py.targets["test_release"].tests, 2);
        assert_eq!(py.test_files["tests/test_release.py"], vec!["test_release"]);
        assert_eq!(py.files["scripts/release-notes.py"], vec!["test_release"]);
        assert!(!py.files.contains_key("scripts/helper_tool.py"));
        // The manual test is never chosen, even by its mention.
        assert_eq!(py.rules, vec![ResolvedRule { files: vec!["src/**".into()], targets: vec!["test_cli".into()] }]);
    }

    #[test]
    fn the_map_reads_only_tracked_files() {
        let dir = project(&[("src/main.rs", "fn main() {}\n"), ("artifacts-run.log", "x"), ("gone.rs", "")]);
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git").args(args).current_dir(dir.path()).stdout(std::process::Stdio::null()).status().unwrap();
            assert!(status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["add", "src/main.rs", "gone.rs"]);
        std::fs::remove_file(dir.path().join("gone.rs")).unwrap();
        // The untracked log is no new area; the deleted file is gone.
        assert_eq!(project_files(dir.path()).unwrap(), vec!["src/main.rs"]);
    }

    #[test]
    fn a_partial_map_reads_the_named_providers_and_keeps_the_others() {
        let dir = project(&files());
        let config = config(CONFIG);
        let committed = generate(dir.path(), &config, &listed(dir.path()), None).unwrap();
        std::fs::write(dir.path().join("src/cli.rs"), "pub fn run() {}\n#[test]\nfn runs() {}\n").unwrap();
        let only = BTreeSet::from(["rust".to_owned()]);
        let now = generate_only(dir.path(), &config, &listed(dir.path()), Some(&committed), Some(&only)).unwrap();
        // cli no longer uses store; py is the committed section as it was.
        assert_eq!(now.providers["rust"].files["src/store.rs"], vec!["--bin demo store"]);
        assert_eq!(now.providers["py"], committed.providers["py"]);
    }

    #[test]
    fn the_check_finds_new_areas_new_tests_and_a_stale_map() {
        let dir = project(&files());
        let config = config(CONFIG);
        let committed = generate(dir.path(), &config, &listed(dir.path()), None).unwrap();
        assert!(check(&config, &listed(dir.path()), &committed, &committed).ok());
        // A new folder, a new test file and a new test module.
        std::fs::create_dir_all(dir.path().join("web")).unwrap();
        std::fs::write(dir.path().join("web/app.ts"), "x").unwrap();
        std::fs::write(dir.path().join("tests/test_new.py"), "def test_x(): pass\n").unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "mod store;\nmod cli;\nfn main() { cli::run(); }\n#[test]\nfn boots() {}\n").unwrap();
        let files = listed(dir.path());
        let fresh = generate(dir.path(), &config, &files, Some(&committed)).unwrap();
        let findings = check(&config, &files, &committed, &fresh);
        assert_eq!(findings.outside, vec!["web/app.ts"]);
        // The map now has the new tests; the committed snapshot lags, which
        // is told, not failed (#741).
        assert!(findings.missing_tests.is_empty(), "{:?}", findings.missing_tests);
        assert!(findings.stale.contains(&"py: new target test_new".to_owned()), "{:?}", findings.stale);
        assert!(findings.stale.contains(&"rust: new target --bin demo ".to_owned()), "{:?}", findings.stale);
        assert!(findings.stale.contains(&"py: tests/test_new.py is a new test file".to_owned()), "{:?}", findings.stale);
        assert!(!findings.ok());
        std::fs::remove_file(dir.path().join("web/app.ts")).unwrap();
        let files = listed(dir.path());
        assert!(check(&config, &files, &committed, &fresh).ok());
        // A test file no provider maps (a ctest binary CMake does not build).
        let mut lacking = fresh.clone();
        lacking.providers.get_mut("py").unwrap().test_files.remove("tests/test_new.py");
        assert_eq!(check(&config, &files, &committed, &lacking).missing_tests, vec!["py: tests/test_new.py"]);
    }
}
