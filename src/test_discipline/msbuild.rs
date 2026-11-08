//! Task 137: the affected map of Visual Studio C++ projects. A file belongs
//! to the projects whose item lists (ClCompile, ClInclude and the rest) name
//! it; a test project (a native unit test or Google Test project, or one the
//! provider's `tests` globs name) answers for itself and for every project it
//! references, to the end. A project no test project reaches is its own
//! target: the build is its check.

use super::config::{Provider, Rule};
use super::map::{ProviderMap, Target};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// One project file as the map needs it.
#[derive(Debug, Default, PartialEq)]
pub struct Project {
    /// The project file, relative to the project root.
    pub path: String,
    pub name: String,
    /// Files its item lists name, relative to the project root.
    pub items: Vec<String>,
    /// Its ClCompile items: where its tests are counted.
    pub compiled: Vec<String>,
    /// Project files it references, relative to the project root.
    pub references: Vec<String>,
    pub test: bool,
    /// Its first configuration and platform (`Debug|x64`).
    pub config: String,
    pub platform: String,
    /// The program it builds, relative to the project root, when the project
    /// file is built on its own (MSBuild's default OutDir).
    pub output: String,
}

/// Item kinds that are not files of the project.
const NOT_FILES: [&str; 5] = ["ProjectConfiguration", "ProjectReference", "Reference", "PackageReference", "Filter"];

/// A project file's text: UTF-8 with or without a BOM, or UTF-16 LE.
fn read_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
        return String::from_utf16(&units).ok();
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).ok()
}

/// `dir/relative` with `.` and `..` resolved and forward slashes; None when
/// it leaves the project.
fn join(dir: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for part in relative.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// The value of `<name>...</name>`, the first one.
fn element<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(&format!("<{name}>"))? + name.len() + 2;
    let end = text[start..].find(&format!("</{name}>"))? + start;
    Some(text[start..end].trim())
}

/// Every `<Kind Include="...">` of the text: (kind, value).
fn includes(text: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(" Include=\"") {
        let head = &rest[..at];
        let kind = head.rfind('<').map(|open| &head[open + 1..]).unwrap_or_default();
        let after = &rest[at + 10..];
        let Some(end) = after.find('"') else { break };
        if !kind.is_empty() && kind.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            out.push((kind, &after[..end]));
        }
        rest = &after[end..];
    }
    out
}

pub fn read(root: &Path, path: &str, tests: &[String]) -> Option<Project> {
    let text = read_text(&root.join(path))?;
    let dir = path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or_default();
    let stem = Path::new(path).file_stem()?.to_string_lossy().into_owned();
    let mut project = Project { path: path.to_owned(), name: stem.clone(), ..Project::default() };
    for (kind, value) in includes(&text) {
        for one in value.split(';').map(str::trim).filter(|one| !one.is_empty()) {
            // A macro or a wildcard names no file the map can hold.
            if one.contains("$(") || one.contains('*') || one.contains("%(") {
                continue;
            }
            let Some(file) = join(dir, one) else { continue };
            match kind {
                "ProjectConfiguration" => {
                    if project.config.is_empty() {
                        if let Some((config, platform)) = one.split_once('|') {
                            project.config = config.to_owned();
                            project.platform = platform.to_owned();
                        }
                    }
                }
                "ProjectReference" => project.references.push(file),
                kind if NOT_FILES.contains(&kind) => {}
                kind => {
                    if kind == "ClCompile" {
                        project.compiled.push(file.clone());
                    }
                    project.items.push(file);
                }
            }
        }
    }
    project.items.sort();
    project.items.dedup();
    project.test = super::glob::any(tests, path)
        || ["NativeUnitTestProject", "CppUnitTest", "UnitTest\\include", "gtest", "googletest"].iter().any(|marker| text.contains(marker));
    let library = element(&text, "ConfigurationType").is_some_and(|kind| kind == "DynamicLibrary");
    let name = element(&text, "TargetName").filter(|name| !name.contains("$(")).unwrap_or(&stem).to_owned();
    // MSBuild's default OutDir for a project built on its own: its folder,
    // then the platform (none for Win32), then the configuration.
    let config = if project.config.is_empty() { "Debug" } else { project.config.as_str() };
    let platform_dir = if project.platform.is_empty() || project.platform.eq_ignore_ascii_case("Win32") { String::new() } else { format!("{}/", project.platform) };
    let output = format!("{platform_dir}{config}/{name}.{}", if library { "dll" } else { "exe" });
    project.output = join(dir, &output).unwrap_or(output);
    Some(project)
}

/// The test functions a test project's sources declare, at least one.
fn count_tests(root: &Path, project: &Project) -> usize {
    let count: usize = project
        .compiled
        .iter()
        .filter_map(|file| std::fs::read_to_string(root.join(file)).ok())
        .map(|text| ["TEST_METHOD(", "TEST(", "TEST_F(", "TEST_P("].iter().map(|marker| text.matches(marker).count()).sum::<usize>())
        .sum();
    count.max(1)
}

pub fn section(root: &Path, provider: &Provider, area: &[String]) -> ProviderMap {
    let projects: BTreeMap<String, Project> = area
        .iter()
        .filter(|file| file.to_ascii_lowercase().ends_with(".vcxproj"))
        .filter_map(|file| read(root, file, &provider.tests))
        .map(|project| (project.path.clone(), project))
        .collect();
    let mut section = ProviderMap {
        kind: provider.kind,
        rule: Rule::Build,
        note: String::new(),
        targets: BTreeMap::new(),
        files: BTreeMap::new(),
        test_files: BTreeMap::new(),
        rules: Vec::new(),
        groups: BTreeMap::new(),
        seconds: 0.0,
        platforms: provider.platform_classes(),
    };
    if projects.is_empty() {
        section.note = "No .vcxproj in the provider's area: a change runs its whole set".to_owned();
        return section;
    }
    // Each test project with everything it references, to the end.
    let mut reaches: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for test in projects.values().filter(|project| project.test) {
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut frontier = vec![test.path.as_str()];
        while let Some(next) = frontier.pop() {
            if seen.insert(next) {
                frontier.extend(projects.get(next).into_iter().flat_map(|project| project.references.iter().map(String::as_str)));
            }
        }
        reaches.insert(test.path.as_str(), seen);
    }
    if reaches.is_empty() {
        section.note = "No test project here: a change builds its project, and the build is the check".to_owned();
    }
    let target = |project: &Project, tests: usize| Target {
        group: project.path.clone(),
        run: if project.test { vec![project.output.clone()] } else { Vec::new() },
        tests,
        binary: project.output.clone(),
        config: project.config.clone(),
        platform: project.platform.clone(),
        ..Target::default()
    };
    for project in projects.values() {
        let covering: Vec<String> = reaches
            .iter()
            .filter(|(_, reached)| reached.contains(project.path.as_str()))
            .map(|(test, _)| projects[*test].name.clone())
            .collect();
        let owners = if covering.is_empty() {
            section.targets.insert(project.name.clone(), target(project, 0));
            vec![project.name.clone()]
        } else {
            covering
        };
        for file in project.items.iter().chain(std::iter::once(&project.path)) {
            let entry = section.files.entry(file.clone()).or_default();
            entry.extend(owners.iter().cloned());
            entry.sort();
            entry.dedup();
        }
        if project.test {
            section.targets.insert(project.name.clone(), target(project, count_tests(root, project)));
            for file in &project.items {
                section.test_files.entry(file.clone()).or_default().push(project.name.clone());
            }
        }
    }
    section
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_paths_resolve_from_the_project_folder() {
        assert_eq!(join("src/demo", "..\\lib\\a.cpp").as_deref(), Some("src/lib/a.cpp"));
        assert_eq!(join("", "main.cpp").as_deref(), Some("main.cpp"));
        assert_eq!(join("", "..\\outside.cpp"), None);
        let text = "<ItemGroup><ClCompile Include=\"a.cpp;b.cpp\" /><ClInclude Include=\"a.h\"/><None Include=\"$(Dir)x.txt\" /></ItemGroup>";
        assert_eq!(includes(text), [("ClCompile", "a.cpp;b.cpp"), ("ClInclude", "a.h"), ("None", "$(Dir)x.txt")]);
    }
}
