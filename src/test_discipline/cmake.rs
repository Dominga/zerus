//! U62: a CMake build's tests and the files each one uses, from the build:
//! CMake's file API names every target's sources and the libraries it links,
//! CTest's files name the tests and the binaries they run, and the include
//! graph narrows a library to the part a test reaches.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const QUERY: &str = ".cmake/api/v1/query/codemodel-v2";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target {
    pub name: String,
    pub kind: String,
    /// Project-relative sources.
    pub sources: Vec<String>,
    /// Ids of the targets it depends on.
    pub dependencies: Vec<String>,
    pub includes: Vec<PathBuf>,
    /// Absolute, with `/`.
    pub artifacts: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Test {
    /// CTest's name.
    pub name: String,
    /// The executable target it runs.
    pub target: String,
    /// What it passes after the executable (a shard's functions, flags).
    pub args: Vec<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Build {
    pub targets: BTreeMap<String, Target>,
    pub tests: Vec<Test>,
}

#[derive(Debug, PartialEq)]
pub enum Reply {
    Ready(Build),
    /// The query was missing: written now, and the next configure answers it.
    Asked,
}

/// Reads `build` (absolute) for the project at `project`.
pub fn read(project: &Path, build: &Path) -> anyhow::Result<Reply> {
    let query = build.join(QUERY);
    let reply = build.join(".cmake/api/v1/reply");
    let index = std::fs::read_dir(&reply).ok().and_then(|entries| {
        entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("index-") && name.ends_with(".json")))
            .max()
    });
    let Some(index) = index else {
        anyhow::ensure!(build.is_dir(), "The build folder {} does not exist; configure it first", build.display());
        if !query.exists() {
            std::fs::create_dir_all(query.parent().unwrap_or(build))?;
            std::fs::write(&query, b"")?;
        }
        return Ok(Reply::Asked);
    };
    let index: Value = json(&index)?;
    let codemodel = index["reply"]["codemodel-v2"]["jsonFile"].as_str().map(|file| reply.join(file));
    let Some(codemodel) = codemodel else {
        std::fs::create_dir_all(query.parent().unwrap_or(build))?;
        std::fs::write(&query, b"")?;
        return Ok(Reply::Asked);
    };
    let codemodel: Value = json(&codemodel)?;
    let source_root = PathBuf::from(codemodel["paths"]["source"].as_str().unwrap_or_default());
    let build_root = PathBuf::from(codemodel["paths"]["build"].as_str().unwrap_or_default());
    let mut targets = BTreeMap::new();
    let configuration = &codemodel["configurations"][0];
    for entry in configuration["targets"].as_array().into_iter().flatten() {
        let Some(file) = entry["jsonFile"].as_str() else { continue };
        let target: Value = json(&reply.join(file))?;
        let id = target["id"].as_str().unwrap_or_default().to_owned();
        let sources = target["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|source| !source["isGenerated"].as_bool().unwrap_or(false))
            .filter_map(|source| source["path"].as_str())
            .filter_map(|path| project_path(project, &source_root, path))
            .collect();
        let mut includes = BTreeSet::new();
        for group in target["compileGroups"].as_array().into_iter().flatten() {
            for include in group["includes"].as_array().into_iter().flatten() {
                if let Some(path) = include["path"].as_str() {
                    includes.insert(PathBuf::from(path));
                }
            }
        }
        targets.insert(
            id,
            Target {
                name: target["name"].as_str().unwrap_or_default().to_owned(),
                kind: target["type"].as_str().unwrap_or_default().to_owned(),
                sources,
                dependencies: target["dependencies"].as_array().into_iter().flatten().filter_map(|dep| dep["id"].as_str()).map(str::to_owned).collect(),
                includes: includes.into_iter().collect(),
                artifacts: target["artifacts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|artifact| artifact["path"].as_str())
                    .map(|path| build_root.join(path).to_string_lossy().replace('\\', "/"))
                    .collect(),
            },
        );
    }
    let tests = ctest_tests(build, &targets);
    Ok(Reply::Ready(Build { targets, tests }))
}

fn json(path: &Path) -> anyhow::Result<Value> {
    let text = std::fs::read_to_string(path).map_err(|error| anyhow::anyhow!("Cannot read {}: {error}", path.display()))?;
    Ok(serde_json::from_str(&text)?)
}

fn normal(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_ascii_lowercase()
}

/// A path CMake names, relative to the project, or None outside it.
fn project_path(project: &Path, source_root: &Path, path: &str) -> Option<String> {
    let absolute = if Path::new(path).is_absolute() { PathBuf::from(path) } else { source_root.join(path) };
    // Windows paths compare without case; the file keeps its own spelling.
    let prefix = format!("{}/", normal(project).trim_end_matches('/'));
    let absolute = lexical(&absolute.to_string_lossy().replace('\\', "/"));
    absolute.to_ascii_lowercase().starts_with(&prefix).then(|| absolute[prefix.len()..].to_owned())
}

/// `a/tests/../src/x.h` as `a/src/x.h`: CMake keeps include folders as written.
fn lexical(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for (index, part) in path.split('/').enumerate() {
        match part {
            "." => {}
            "" if index > 0 => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// Every `add_test` of the build's CTest files, with the target it runs.
fn ctest_tests(build: &Path, targets: &BTreeMap<String, Target>) -> Vec<Test> {
    let mut files = Vec::new();
    collect(build, "CTestTestfile.cmake", &mut files, 0);
    files.sort();
    let artifacts: Vec<(String, &Target)> =
        targets.values().filter(|target| target.kind == "EXECUTABLE").flat_map(|target| target.artifacts.iter().map(move |path| (path.clone(), target))).collect();
    let mut tests = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        for line in text.lines() {
            let Some(inside) = line.trim().strip_prefix("add_test(").and_then(|rest| rest.strip_suffix(')')) else { continue };
            let words = arguments(inside);
            let Some((name, command)) = words.split_first() else { continue };
            let mut target = None;
            let mut args = Vec::new();
            for word in command {
                let lower = word.replace('\\', "/").to_ascii_lowercase();
                let found = artifacts.iter().find(|(path, _)| {
                    let path = path.to_ascii_lowercase();
                    lower == path || lower.ends_with(&format!("={path}"))
                });
                match found {
                    Some((_, found)) if target.is_none() => target = Some(found.name.clone()),
                    _ if target.is_some() => args.push(word.clone()),
                    _ => {}
                }
            }
            if let Some(target) = target {
                tests.push(Test { name: name.clone(), target, args });
            }
        }
    }
    tests
}

fn collect(dir: &Path, name: &str, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 6 {
        return;
    }
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !path.file_name().is_some_and(|file| file.to_string_lossy().starts_with('.')) {
                collect(&path, name, out, depth + 1);
            }
        } else if path.file_name().is_some_and(|file| file == name) {
            out.push(path);
        }
    }
}

/// CMake's arguments: `"quoted"`, `[=[bracketed]=]` or bare words.
fn arguments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        if let Some(inner) = rest.strip_prefix('"') {
            let mut value = String::new();
            let mut chars = inner.char_indices();
            let mut end = inner.len();
            while let Some((at, ch)) = chars.next() {
                match ch {
                    '\\' => {
                        if let Some((_, next)) = chars.next() {
                            value.push(next);
                        }
                    }
                    '"' => {
                        end = at + 1;
                        break;
                    }
                    ch => value.push(ch),
                }
            }
            out.push(value);
            rest = inner[end.min(inner.len())..].trim_start();
        } else if rest.starts_with('[') && rest[1..].trim_start_matches('=').starts_with('[') {
            let equals = rest[1..].len() - rest[1..].trim_start_matches('=').len();
            let close = format!("]{}]", "=".repeat(equals));
            let start = equals + 2;
            let end = rest[start..].find(&close).map(|at| start + at).unwrap_or(rest.len());
            out.push(rest[start..end].to_owned());
            rest = rest[(end + close.len()).min(rest.len())..].trim_start();
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push(rest[..end].to_owned());
            rest = rest[end..].trim_start();
        }
    }
    out
}

/// The project files each test executable uses: its own sources and, of the
/// libraries it links, what its include graph reaches (a header's own `.cpp`
/// and the `.cpp`s named after it, such as `WindowRows.cpp` for `Window.h`).
/// A linked file no test reaches is given to every test that links it.
pub fn used_files(project: &Path, build: &Build) -> BTreeMap<String, BTreeSet<String>> {
    let executables: Vec<&Target> = build.targets.values().filter(|target| target.kind == "EXECUTABLE").collect();
    let tested: BTreeSet<&str> = build.tests.iter().map(|test| test.target.as_str()).collect();
    let mut universes = BTreeMap::new();
    let mut reached = BTreeMap::new();
    let mut cache = Includes::default();
    for target in executables.iter().filter(|target| tested.contains(target.name.as_str())) {
        let mut universe = BTreeSet::new();
        let mut includes = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut queue: Vec<String> = build.targets.iter().filter(|(_, other)| other.name == target.name).map(|(id, _)| id.clone()).collect();
        while let Some(id) = queue.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(linked) = build.targets.get(&id) {
                universe.extend(linked.sources.iter().cloned());
                includes.extend(linked.includes.iter().cloned());
                queue.extend(linked.dependencies.iter().cloned());
            }
        }
        // Only the project's own folders can hold a project header.
        let project_dirs: Vec<PathBuf> = includes.into_iter().filter(|dir| project_path(project, project, &dir.to_string_lossy()).is_some()).collect();
        let closure = closure(project, &target.sources, &universe, &project_dirs, &mut cache);
        universes.insert(target.name.clone(), universe);
        reached.insert(target.name.clone(), closure);
    }
    let all_reached: BTreeSet<String> = reached.values().flatten().cloned().collect();
    let mut out = BTreeMap::new();
    for (name, universe) in &universes {
        let mut files = reached[name].clone();
        files.extend(universe.iter().filter(|file| !all_reached.contains(*file)).cloned());
        out.insert(name.clone(), files);
    }
    out
}

/// Each file's project headers, read once for every test binary.
#[derive(Default)]
struct Includes {
    files: BTreeMap<(String, Vec<PathBuf>), Vec<String>>,
}

impl Includes {
    fn of(&mut self, project: &Path, file: &str, dirs: &[PathBuf]) -> Vec<String> {
        let key = (file.to_owned(), dirs.to_vec());
        if let Some(found) = self.files.get(&key) {
            return found.clone();
        }
        let text = std::fs::read_to_string(project.join(file)).unwrap_or_default();
        let folder = Path::new(file).parent().unwrap_or(Path::new("")).to_path_buf();
        let mut found = Vec::new();
        for include in quoted_includes(&text) {
            let mut candidates = vec![project.join(&folder).join(&include)];
            candidates.extend(dirs.iter().map(|dir| dir.join(&include)));
            let Some(header) = candidates.iter().find(|candidate| candidate.is_file()) else { continue };
            if let Some(relative) = project_path(project, project, &header.to_string_lossy()) {
                found.push(exact_case(project, &relative.to_ascii_lowercase()));
            }
        }
        self.files.insert(key, found.clone());
        found
    }
}

fn closure(project: &Path, seeds: &[String], universe: &BTreeSet<String>, includes: &[PathBuf], cache: &mut Includes) -> BTreeSet<String> {
    let lower: BTreeMap<String, String> = universe.iter().map(|file| (file.to_ascii_lowercase(), file.clone())).collect();
    let mut found = BTreeSet::new();
    let mut queue: Vec<String> = seeds.to_vec();
    while let Some(file) = queue.pop() {
        if !found.insert(file.clone()) {
            continue;
        }
        for relative in cache.of(project, &file, includes) {
            let relative = lower.get(&relative.to_ascii_lowercase()).cloned().unwrap_or(relative);
            queue.push(relative.clone());
            // The header's implementation: same folder, its name or longer.
            let stem = Path::new(&relative).file_stem().map(|stem| stem.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let dir = Path::new(&relative).parent().map(|dir| dir.to_string_lossy().replace('\\', "/").to_ascii_lowercase()).unwrap_or_default();
            for (key, original) in &lower {
                let path = Path::new(key);
                let same_dir = path.parent().map(|parent| parent.to_string_lossy().replace('\\', "/")).unwrap_or_default() == dir;
                let source = path.extension().is_some_and(|ext| ["cpp", "cc", "cxx", "c", "mm"].contains(&ext.to_string_lossy().as_ref()));
                let named = path.file_stem().is_some_and(|name| name.to_string_lossy().starts_with(&stem));
                if same_dir && source && named {
                    queue.push(original.clone());
                }
            }
        }
    }
    found
}

/// A lower-cased project path in the case it has on disk.
fn exact_case(project: &Path, lower: &str) -> String {
    let mut dir = project.to_path_buf();
    let mut parts = Vec::new();
    for part in lower.split('/') {
        let found = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .find(|name| name.to_ascii_lowercase() == part)
            .unwrap_or_else(|| part.to_owned());
        dir = dir.join(&found);
        parts.push(found);
    }
    parts.join("/")
}

fn quoted_includes(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start().strip_prefix('#')?.trim_start().strip_prefix("include")?.trim_start();
            let inner = line.strip_prefix('"')?;
            Some(inner[..inner.find('"')?].to_owned())
        })
        .collect()
}

/// A QtTest file's test functions: the declarations under `private slots:`
/// (Q_SLOTS), without the framework's own init and cleanup functions.
pub fn qt_functions(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("private slots:") || line.starts_with("private Q_SLOTS:") {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if line.ends_with(':') && !line.contains('(') || line.starts_with("};") {
            inside = false;
            continue;
        }
        let Some(rest) = line.strip_prefix("void ") else { continue };
        let Some(name) = rest.split('(').next().map(str::trim) else { continue };
        let framework = ["initTestCase", "cleanupTestCase", "init", "cleanup"];
        if !name.is_empty() && !name.ends_with("_data") && !framework.contains(&name) && !name.contains(' ') {
            out.push(name.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A project with two libraries and two tests, configured: the file API's
    /// reply and CTest's file as CMake writes them.
    fn configured() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let src = root.join("src").to_string_lossy().replace('\\', "/");
        write(root, "src/Core.h", "#pragma once\n");
        write(root, "src/Core.cpp", "#include \"Core.h\"\n");
        write(root, "src/Window.h", "#include \"Core.h\"\n");
        write(root, "src/Window.cpp", "#include \"Window.h\"\n#include \"Panel.h\"\n");
        write(root, "src/WindowRows.cpp", "#include \"Window.h\"\n");
        write(root, "src/Panel.h", "");
        write(root, "src/Panel.cpp", "#include \"Panel.h\"\n");
        write(root, "src/Orphan.cpp", "");
        write(root, "tests/test_core.cpp", "#include \"Core.h\"\nclass T : public QObject {\nprivate slots:\n    void init();\n    void readsIt();\n    void rows_data();\n    void rows();\nprivate:\n    void helper();\n};\n");
        write(root, "tests/test_window.cpp", "#include \"Window.h\"\n");
        // CMake names include folders as written: `tests/../src`.
        let src_written = root.join("tests/../src").to_string_lossy().replace('\\', "/");
        let build = root.join("build");
        let reply = ".cmake/api/v1/reply";
        let target = |name: &str, kind: &str, sources: &[&str], deps: &[&str], artifact: &str| {
            json!({
                "id": format!("{name}::@1"), "name": name, "type": kind,
                "sources": sources.iter().map(|path| json!({"path": path})).collect::<Vec<_>>(),
                "dependencies": deps.iter().map(|dep| json!({"id": format!("{dep}::@1")})).collect::<Vec<_>>(),
                "compileGroups": [{"includes": [{"path": src}]}],
                "artifacts": if artifact.is_empty() { json!([]) } else { json!([{"path": artifact}]) },
            })
        };
        let targets = [
            ("core", target("core", "STATIC_LIBRARY", &["src/Core.cpp"], &[], "")),
            ("views", target("views", "STATIC_LIBRARY", &["src/Window.cpp", "src/WindowRows.cpp", "src/Panel.cpp", "src/Orphan.cpp"], &["core"], "")),
            ("test_core", target("test_core", "EXECUTABLE", &["tests/test_core.cpp"], &["core"], "tests/test_core.exe")),
            ("test_window", target("test_window", "EXECUTABLE", &["tests/test_window.cpp"], &["views"], "tests/test_window.exe")),
        ];
        let mut targets = targets;
        targets[3].1["compileGroups"] = json!([{"includes": [{"path": src_written}]}]);
        let mut list = Vec::new();
        for (name, value) in &targets {
            write(&build, &format!("{reply}/target-{name}.json"), &value.to_string());
            list.push(json!({"name": name, "jsonFile": format!("target-{name}.json")}));
        }
        let paths = json!({"source": root.to_string_lossy().replace('\\', "/"), "build": build.to_string_lossy().replace('\\', "/")});
        write(&build, &format!("{reply}/codemodel-v2-1.json"), &json!({"paths": paths, "configurations": [{"targets": list}]}).to_string());
        write(&build, &format!("{reply}/index-2026.json"), &json!({"reply": {"codemodel-v2": {"jsonFile": "codemodel-v2-1.json"}}}).to_string());
        let exe = |name: &str| build.join(format!("tests/{name}.exe")).to_string_lossy().replace('\\', "/");
        write(
            &build,
            "tests/CTestTestfile.cmake",
            &format!(
                "add_test([=[core]=] \"{}\")\nadd_test(window-0 \"cmake\" \"-DTEST={}\" \"-DSHARD=0\")\nset_tests_properties(core PROPERTIES COST 2)\n",
                exe("test_core"),
                exe("test_window")
            ),
        );
        dir
    }

    #[test]
    fn a_missing_reply_asks_cmake_for_one() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(dir.path(), &dir.path().join("build")).unwrap_err().to_string().contains("does not exist"));
        std::fs::create_dir_all(dir.path().join("build")).unwrap();
        assert_eq!(read(dir.path(), &dir.path().join("build")).unwrap(), Reply::Asked);
        assert!(dir.path().join("build").join(QUERY).is_file());
    }

    #[test]
    fn tests_and_their_files_come_from_the_link_lists_and_includes() {
        let dir = configured();
        let Reply::Ready(build) = read(dir.path(), &dir.path().join("build")).unwrap() else { panic!("no reply") };
        assert_eq!(
            build.tests,
            vec![
                Test { name: "core".into(), target: "test_core".into(), args: vec![] },
                Test { name: "window-0".into(), target: "test_window".into(), args: vec!["-DSHARD=0".into()] },
            ]
        );
        let used = used_files(dir.path(), &build);
        let names = |name: &str| used[name].iter().cloned().collect::<Vec<_>>();
        assert_eq!(names("test_core"), vec!["src/Core.cpp", "src/Core.h", "tests/test_core.cpp"]);
        // Window.h reaches its rows file by name and Panel through Window.cpp;
        // Orphan.cpp, linked and reached by nobody, stays with its linker.
        assert_eq!(
            names("test_window"),
            vec!["src/Core.cpp", "src/Core.h", "src/Orphan.cpp", "src/Panel.cpp", "src/Panel.h", "src/Window.cpp", "src/Window.h", "src/WindowRows.cpp", "tests/test_window.cpp"]
        );
    }

    #[test]
    fn qt_test_functions_are_the_private_slots() {
        let text = "private slots:\n    void init();\n    void readsIt();\n    void rows_data();\n    void rows();\nprivate:\n    void helper();\n";
        assert_eq!(qt_functions(text), vec!["readsIt", "rows"]);
    }

    #[test]
    fn cmake_arguments_keep_quotes_and_brackets() {
        assert_eq!(arguments(r#"[=[a b]=] "C:/x y/t.exe" -DX=1 "q\"t""#), vec!["a b", "C:/x y/t.exe", "-DX=1", "q\"t"]);
    }
}
