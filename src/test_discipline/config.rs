//! U62: a project's test configuration, `.zerus/tests.toml` (owner,
//! 2026-10-30: the mechanism is Zerus's, a repository's suites, commands and
//! thresholds are its own configuration). The owner's overrides in the team
//! settings apply over the file; without a file Zerus proposes what it detects,
//! and a project with neither keeps today's flow.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

pub const FILE: &str = ".zerus/tests.toml";

/// Where the configuration came from; only `File` turns the discipline on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The project's own `.zerus/tests.toml`.
    File,
    /// No file: a proposal from the stacks Zerus found.
    Detected,
    /// Neither a file nor a known stack: today's flow.
    None,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TestsConfig {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub map: MapSettings,
    #[serde(default)]
    pub full: FullRun,
    #[serde(default)]
    pub retries: Retries,
    #[serde(default)]
    pub budgets: Budgets,
    #[serde(default)]
    pub isolation: Isolation,
    #[serde(default)]
    pub build_cache: BuildCache,
    #[serde(default)]
    pub merge_queue: MergeQueue,
    /// U62 (#717): the night window in which long suites may run.
    #[serde(default)]
    pub schedule: Schedule,
    /// Resource classes by name: what a suite needs of the machine.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub resources: std::collections::BTreeMap<String, Resources>,
    #[serde(default, rename = "provider", skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<Provider>,
}

fn version() -> u32 {
    1
}

/// U62 (#717): the night window, local hours; `night_start > night_end` wraps
/// midnight. A suite longer than the configured budget runs only inside it, or
/// the CI runs it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    #[serde(default = "night_start")]
    pub night_start: u32,
    #[serde(default = "night_end")]
    pub night_end: u32,
}

fn night_start() -> u32 {
    22
}

fn night_end() -> u32 {
    6
}

impl Default for Schedule {
    fn default() -> Self {
        Self { night_start: night_start(), night_end: night_end() }
    }
}

/// Whether `hour` (local, 0-23) is inside the night window.
pub fn is_night(hour: u32, schedule: &Schedule) -> bool {
    let (start, end) = (schedule.night_start % 24, schedule.night_end % 24);
    if start == end {
        return false;
    }
    if start < end {
        hour >= start && hour < end
    } else {
        hour >= start || hour < end
    }
}

impl Default for TestsConfig {
    fn default() -> Self {
        Self {
            version: version(),
            map: MapSettings::default(),
            full: FullRun::default(),
            retries: Retries::default(),
            budgets: Budgets::default(),
            isolation: Isolation::default(),
            build_cache: BuildCache::default(),
            merge_queue: MergeQueue::default(),
            schedule: Schedule::default(),
            resources: std::collections::BTreeMap::new(),
            providers: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MapSettings {
    /// The committed test map, written by `hgs test map --write`.
    #[serde(default = "map_file")]
    pub file: String,
    /// Recorded durations, written by `hgs test durations --record`.
    #[serde(default = "durations_file")]
    pub durations: String,
    /// A changed file no provider's area holds: reported, or every test.
    #[serde(default)]
    pub unmapped: Unmapped,
    /// Files no test covers by design (documentation, artwork).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
}

impl Default for MapSettings {
    fn default() -> Self {
        Self { file: map_file(), durations: durations_file(), unmapped: Unmapped::default(), ignore: Vec::new() }
    }
}

fn map_file() -> String {
    ".zerus/test-map.json".to_owned()
}
fn durations_file() -> String {
    ".zerus/test-durations.json".to_owned()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Unmapped {
    /// Named in the output; nothing runs for it.
    #[default]
    Report,
    /// Every provider's whole set runs.
    All,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FullRun {
    /// Where the full run happens.
    #[serde(default, rename = "where")]
    pub place: Place,
    /// Who may start it.
    #[serde(default)]
    pub policy: FullPolicy,
    /// The CI workflow or runner that does it, when it is not here.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    #[default]
    Here,
    Ci,
    Runner,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FullPolicy {
    /// Only on the owner's confirmation, told how long it takes (U62).
    #[default]
    Owner,
    /// The owner gave a standing permission.
    Standing,
    /// Anyone may start it.
    Anyone,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Retries {
    /// Retries of a failed suite before it fails; a pass on a retry marks it
    /// flaky. A crash is never retried.
    #[serde(default = "one")]
    pub flaky: u32,
    /// Days a flaky quarantine lasts before the suite counts again.
    #[serde(default = "quarantine_days")]
    pub quarantine_days: u32,
}

impl Default for Retries {
    fn default() -> Self {
        Self { flaky: one(), quarantine_days: quarantine_days() }
    }
}

fn one() -> u32 {
    1
}
fn quarantine_days() -> u32 {
    14
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// A suite whose time grows by more than this share of its recorded time
    /// is flagged; 0 is off.
    #[serde(default)]
    pub grow_percent: u32,
    /// A suite's default time budget in minutes; 0 is none.
    #[serde(default)]
    pub suite_minutes: f64,
}

/// `[isolation]`: only its switch is read here; its other keys (templates,
/// prefixes, env, logs and the platform tables) belong to
/// `state::teams::isolation`, which checks them.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Isolation {
    /// Each suite gets its own home, temp and settings folders.
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl Default for Isolation {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BuildCache {
    #[serde(default)]
    pub enabled: bool,
    /// The tool, such as sccache or ccache.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool: String,
    /// The shared cache folder, its size and the compilers it serves
    /// ("rust", "cxx"); the team runner reads them (#721).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dir: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub size: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compilers: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MergeQueue {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    #[serde(default)]
    pub cpu: u32,
    #[serde(default)]
    pub ram_mb: u32,
    #[serde(default)]
    pub gpu: bool,
    /// Needs a display, a device or anything else one at a time.
    #[serde(default)]
    pub exclusive: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Cargo,
    Ctest,
    Pytest,
    Unittest,
    Gradle,
    Npm,
    Command,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Cargo => "cargo",
            Kind::Ctest => "ctest",
            Kind::Pytest => "pytest",
            Kind::Unittest => "unittest",
            Kind::Gradle => "gradle",
            Kind::Npm => "npm",
            Kind::Command => "command",
        }
    }
}

/// How a provider's affected set is found.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// From the build: Cargo's module graph, CMake's link lists, the files a
    /// test names or imports.
    Build,
    /// Only the provider's path map.
    Paths,
    /// Any change in its area runs everything.
    All,
}

/// How far a change in a shared Rust module reaches (cargo, `build`).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Widen {
    /// The changed module's own tests.
    None,
    /// And the tests of every module that uses it.
    #[default]
    Dependants,
    /// And of every module that uses those, to the end.
    Transitive,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub name: String,
    pub kind: Kind,
    /// Its working directory, relative to the project.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub root: String,
    /// The area: globs of the files whose changes its tests answer for.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Globs of its test files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
    /// Test files never chosen automatically; they run only by hand.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub manual: Vec<String>,
    /// Test files another platform's build holds: this map cannot name them,
    /// and a change in them runs there (the CI), not here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elsewhere: Vec<String>,
    #[serde(default = "rule_build")]
    pub affected: Rule,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<PathRule>,
    /// The command; `{targets}` is replaced by what the map chose, and by the
    /// whole set without it for a full run. Empty is the kind's own.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// cargo: the manifest, relative to `root`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub manifest: String,
    /// cargo: how far a change in a shared module reaches.
    #[serde(default)]
    pub widen: Widen,
    /// ctest: the configured build folder holding CMake's file API reply.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build_dir: String,
    /// A resource class from `[resources]`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub resources: String,
    /// The platforms it runs on; empty is every one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
    /// Its own time budget in minutes, over `budgets.suite_minutes`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub budget_minutes: f64,
    /// Turned off, here or by the owner's override.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

fn rule_build() -> Rule {
    Rule::Build
}
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

/// A path map entry: a change in `files` runs `targets` (map target ids or
/// test files), or the tests whose text mentions `mentions`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PathRule {
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mentions: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Loaded {
    pub source: Source,
    /// The file read, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The owner's override from the team settings applied.
    pub overridden: bool,
    pub config: TestsConfig,
}

impl Loaded {
    /// The discipline applies: a file says so, possibly through the owner.
    pub fn active(&self) -> bool {
        self.source == Source::File || self.overridden
    }
}

/// Reads the project's configuration under `root`, the owner's `overrides`
/// (the team settings' `tests` object) over it.
pub fn load(root: &Path, overrides: Option<&Value>) -> anyhow::Result<Loaded> {
    let path = root.join(FILE);
    let (source, mut value, read) = match std::fs::read_to_string(&path) {
        Ok(text) => (Source::File, parse_toml(&text).map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))?, Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let detected = super::detect::detect(root);
            let source = if detected.providers.is_empty() { Source::None } else { Source::Detected };
            (source, serde_json::to_value(detected)?, None)
        }
        Err(error) => anyhow::bail!("Cannot read {}: {error}", path.display()),
    };
    let overridden = overrides.is_some_and(|overrides| overrides.as_object().is_some_and(|fields| !fields.is_empty()));
    if let Some(overrides) = overrides.filter(|_| overridden) {
        merge(&mut value, overrides);
    }
    let config: TestsConfig = serde_json::from_value(value).map_err(|error| anyhow::anyhow!("Invalid test configuration: {error}"))?;
    validate(&config)?;
    Ok(Loaded { source, path: read, overridden, config })
}

/// The owner's override over the file: tables merge field by field, providers
/// by name (a new name is added), anything else is replaced.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(base), Value::Object(over)) => {
            for (key, value) in over {
                if key == "provider" {
                    merge_providers(base.entry(key.clone()).or_insert_with(|| Value::Array(Vec::new())), value);
                } else {
                    merge(base.entry(key.clone()).or_insert(Value::Null), value);
                }
            }
        }
        (base, over) => *base = over.clone(),
    }
}

fn merge_providers(base: &mut Value, over: &Value) {
    let Some(list) = base.as_array_mut() else {
        *base = over.clone();
        return;
    };
    for provider in over.as_array().into_iter().flatten() {
        let name = provider.get("name");
        match list.iter_mut().find(|known| name.is_some() && known.get("name") == name) {
            Some(known) => merge(known, provider),
            None => list.push(provider.clone()),
        }
    }
}

fn validate(config: &TestsConfig) -> anyhow::Result<()> {
    anyhow::ensure!(config.version == 1, "The test configuration's version is 1, not {}", config.version);
    let mut names = std::collections::BTreeSet::new();
    for provider in &config.providers {
        anyhow::ensure!(!provider.name.trim().is_empty(), "Every test provider has a name");
        anyhow::ensure!(names.insert(provider.name.as_str()), "Two test providers are named {}", provider.name);
        anyhow::ensure!(
            provider.kind != Kind::Command || !provider.command.is_empty(),
            "The command provider {} names its command",
            provider.name
        );
        anyhow::ensure!(
            provider.kind != Kind::Ctest || !provider.build_dir.is_empty() || provider.affected != Rule::Build,
            "The ctest provider {} names its build_dir to read the map from the build",
            provider.name
        );
        anyhow::ensure!(
            provider.resources.is_empty() || config.resources.contains_key(&provider.resources),
            "The provider {} names an unknown resource class {}",
            provider.name,
            provider.resources
        );
        for rule in &provider.paths {
            anyhow::ensure!(!rule.files.is_empty(), "A path rule of {} names its files", provider.name);
            anyhow::ensure!(
                !rule.targets.is_empty() || !rule.mentions.is_empty(),
                "A path rule of {} names its targets or what its tests mention",
                provider.name
            );
        }
    }
    Ok(())
}

/// TOML as JSON, so the owner's JSON override merges over it field by field.
pub fn parse_toml(text: &str) -> anyhow::Result<Value> {
    let document: toml_edit::DocumentMut = text.parse()?;
    Ok(table(document.as_table()))
}

fn table(table: &toml_edit::Table) -> Value {
    let mut out = Map::new();
    for (key, item) in table.iter() {
        if let Some(value) = item_value(item) {
            out.insert(key.to_owned(), value);
        }
    }
    Value::Object(out)
}

fn item_value(item: &toml_edit::Item) -> Option<Value> {
    match item {
        toml_edit::Item::None => None,
        toml_edit::Item::Value(value) => Some(scalar(value)),
        toml_edit::Item::Table(inner) => Some(table(inner)),
        toml_edit::Item::ArrayOfTables(tables) => Some(Value::Array(tables.iter().map(table).collect())),
    }
}

fn scalar(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(text) => Value::String(text.value().clone()),
        toml_edit::Value::Integer(number) => Value::from(*number.value()),
        toml_edit::Value::Float(number) => Value::from(*number.value()),
        toml_edit::Value::Boolean(flag) => Value::Bool(*flag.value()),
        toml_edit::Value::Datetime(time) => Value::String(time.value().to_string()),
        toml_edit::Value::Array(items) => Value::Array(items.iter().map(scalar).collect()),
        toml_edit::Value::InlineTable(inner) => {
            Value::Object(inner.iter().map(|(key, value)| (key.to_owned(), scalar(value))).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    const FILE_TEXT: &str = r#"
version = 1
[map]
ignore = ["docs/**"]
[full]
where = "ci"
target = "nightly.yml"
[retries]
flaky = 2
[resources.gui]
cpu = 2
exclusive = true
[[provider]]
name = "rust"
kind = "cargo"
sources = ["src/**"]
widen = "transitive"
[[provider]]
name = "gui"
kind = "ctest"
build_dir = "build"
resources = "gui"
sources = ["gui/**"]
[[provider.paths]]
files = ["gui/fixtures/**"]
targets = ["ctest:gui:views"]
"#;

    #[test]
    fn the_project_file_is_read_with_its_providers_and_defaults() {
        let dir = project(&[(FILE, FILE_TEXT)]);
        let loaded = load(dir.path(), None).unwrap();
        assert_eq!(loaded.source, Source::File);
        assert!(loaded.active());
        let config = &loaded.config;
        assert_eq!(config.full.place, Place::Ci);
        assert_eq!(config.full.policy, FullPolicy::Owner);
        assert_eq!(config.retries.flaky, 2);
        assert_eq!(config.retries.quarantine_days, 14);
        assert_eq!(config.map.file, ".zerus/test-map.json");
        assert_eq!(config.providers.len(), 2);
        assert_eq!(config.providers[0].widen, Widen::Transitive);
        assert_eq!(config.providers[1].affected, Rule::Build);
        assert_eq!(config.providers[1].paths[0].targets, vec!["ctest:gui:views"]);
        assert!(config.resources["gui"].exclusive);
    }

    #[test]
    fn the_owner_overrides_fields_and_providers_by_name() {
        let dir = project(&[(FILE, FILE_TEXT)]);
        let overrides = json!({
            "full": {"policy": "standing"},
            "merge_queue": {"enabled": true},
            "provider": [
                {"name": "gui", "disabled": true},
                {"name": "docs", "kind": "command", "command": "make check-docs", "sources": ["docs/**"], "affected": "all"}
            ]
        });
        let loaded = load(dir.path(), Some(&overrides)).unwrap();
        assert!(loaded.overridden);
        let config = &loaded.config;
        // The file's other fields stay.
        assert_eq!((config.full.place, config.full.policy), (Place::Ci, FullPolicy::Standing));
        assert!(config.merge_queue.enabled);
        assert_eq!(config.providers.len(), 3);
        assert!(config.providers[1].disabled && config.providers[1].build_dir == "build");
        assert_eq!(config.providers[2].kind, Kind::Command);
    }

    #[test]
    fn without_a_file_zerus_proposes_what_it_detects_and_stays_off() {
        let dir = project(&[("Cargo.toml", "[package]\nname = \"x\"\n"), ("src/main.rs", "fn main() {}\n")]);
        let loaded = load(dir.path(), None).unwrap();
        assert_eq!(loaded.source, Source::Detected);
        assert!(!loaded.active(), "a proposal changes nobody's flow");
        assert_eq!(loaded.config.providers[0].kind, Kind::Cargo);
        let empty = project(&[("README.md", "hello\n")]);
        let loaded = load(empty.path(), None).unwrap();
        assert_eq!(loaded.source, Source::None);
        assert!(loaded.config.providers.is_empty() && !loaded.active());
    }

    #[test]
    fn a_wrong_file_is_refused_with_its_reason() {
        let unknown = project(&[(FILE, "[full]\nwhen = \"ci\"\n")]);
        assert!(load(unknown.path(), None).unwrap_err().to_string().contains("unknown field `when`"));
        let twice = project(&[(FILE, "[[provider]]\nname = \"a\"\nkind = \"npm\"\n[[provider]]\nname = \"a\"\nkind = \"npm\"\n")]);
        assert!(load(twice.path(), None).unwrap_err().to_string().contains("Two test providers are named a"));
        let class = project(&[(FILE, "[[provider]]\nname = \"a\"\nkind = \"npm\"\nresources = \"gpu\"\n")]);
        assert!(load(class.path(), None).unwrap_err().to_string().contains("unknown resource class gpu"));
        let command = project(&[(FILE, "[[provider]]\nname = \"a\"\nkind = \"command\"\n")]);
        assert!(load(command.path(), None).unwrap_err().to_string().contains("names its command"));
    }
}
