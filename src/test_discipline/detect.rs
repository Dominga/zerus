//! U62: the stacks Zerus recognises in a project without `.zerus/tests.toml`.
//! It is a proposal (`hgs test config --propose` writes it): until the file
//! exists the team keeps today's flow.

use super::config::{Kind, Provider, Rule, TestsConfig, Widen};
use std::path::Path;

pub fn detect(root: &Path) -> TestsConfig {
    let mut providers = Vec::new();
    // The project itself and its first level of folders.
    let mut places = vec![String::new()];
    if let Ok(entries) = std::fs::read_dir(root) {
        let mut folders: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.') && !["target", "build", "node_modules"].contains(&name.as_str()))
            .collect();
        folders.sort();
        places.extend(folders);
    }
    for place in places {
        let dir = root.join(&place);
        let prefix = if place.is_empty() { String::new() } else { format!("{place}/") };
        let name = |kind: &str| if place.is_empty() { kind.to_owned() } else { format!("{place}-{kind}") };
        let area = vec![format!("{prefix}**")];
        if dir.join("Cargo.toml").is_file() {
            providers.push(Provider {
                manifest: "Cargo.toml".to_owned(),
                widen: Widen::Dependants,
                ..provider(name("cargo"), Kind::Cargo, &place, area.clone(), vec![format!("{prefix}tests/**/*.rs")], Rule::Build)
            });
        }
        let cmake = std::fs::read_to_string(dir.join("CMakeLists.txt")).unwrap_or_default();
        if cmake.contains("enable_testing") || cmake.contains("add_test") {
            providers.push(Provider {
                build_dir: format!("{prefix}build"),
                ..provider(name("ctest"), Kind::Ctest, &place, area.clone(), Vec::new(), Rule::Build)
            });
        }
        let package = std::fs::read_to_string(dir.join("package.json")).unwrap_or_default();
        let scripts = serde_json::from_str::<serde_json::Value>(&package).ok();
        if scripts.as_ref().and_then(|value| value["scripts"]["test"].as_str()).is_some() {
            providers.push(provider(name("npm"), Kind::Npm, &place, area.clone(), Vec::new(), Rule::All));
        }
        if ["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"].iter().any(|file| dir.join(file).is_file()) {
            providers.push(provider(name("gradle"), Kind::Gradle, &place, area.clone(), vec![format!("{prefix}**/src/test/**")], Rule::Build));
        }
        let pyproject = std::fs::read_to_string(dir.join("pyproject.toml")).unwrap_or_default();
        let pytest = dir.join("pytest.ini").is_file() || dir.join("conftest.py").is_file() || pyproject.contains("[tool.pytest");
        let python_tests = std::fs::read_dir(dir.join("tests")).into_iter().flatten().flatten().any(|entry| {
            entry.file_name().to_str().is_some_and(|file| file.starts_with("test_") && file.ends_with(".py"))
        });
        // Task 137: Cython extensions built by setup.py rebuild before the tests.
        let cython = dir.join("setup.py").is_file() && has_extension(&dir, &["pyx"]);
        if pytest || python_tests || cython {
            let kind = if cython { Kind::Cython } else if pytest { Kind::Pytest } else { Kind::Unittest };
            providers.push(provider(name(kind.name()), kind, &place, area.clone(), vec![format!("{prefix}tests/**/test_*.py")], Rule::Build));
        }
        // Task 137: a Visual Studio solution, or else a C++ project; one at the
        // top covers the folders below it.
        let solution = first_with(&dir, &["sln", "slnx"]).or_else(|| first_with(&dir, &["vcxproj"]));
        if let Some(manifest) = solution.filter(|_| !providers.iter().any(|known| known.kind == Kind::Msbuild && known.root.is_empty())) {
            providers.push(Provider { manifest, ..provider(name("msbuild"), Kind::Msbuild, &place, area.clone(), Vec::new(), Rule::Build) });
        }
        if let Some(manifest) = first_with(&dir, &["uproject"]) {
            let sources = ["Source", "Content", "Config", "Plugins"].iter().map(|folder| format!("{prefix}{folder}/**")).collect();
            providers.push(Provider { manifest, ..provider(name("unreal"), Kind::Unreal, &place, sources, Vec::new(), Rule::Build) });
        }
    }
    // A folder's provider owns its files before the project's own catch-all.
    TestsConfig { providers, ..TestsConfig::default() }
}

/// The first file of the folder (by name) with one of the extensions.
fn first_with(dir: &Path, extensions: &[&str]) -> Option<String> {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| Path::new(name).extension().and_then(|ext| ext.to_str()).is_some_and(|ext| extensions.iter().any(|want| ext.eq_ignore_ascii_case(want))))
        .collect();
    found.sort();
    found.into_iter().next()
}

/// A file with one of the extensions in the folder or one folder down.
fn has_extension(dir: &Path, extensions: &[&str]) -> bool {
    if first_with(dir, extensions).is_some() {
        return true;
    }
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()) && !entry.file_name().to_string_lossy().starts_with('.'))
        .any(|entry| first_with(&entry.path(), extensions).is_some())
}

fn provider(name: String, kind: Kind, root: &str, sources: Vec<String>, tests: Vec<String>, affected: Rule) -> Provider {
    Provider {
        name,
        kind,
        root: root.to_owned(),
        sources,
        tests,
        manual: Vec::new(),
        elsewhere: Vec::new(),
        affected,
        paths: Vec::new(),
        command: String::new(),
        configure: String::new(),
        build: String::new(),
        probe: String::new(),
        manifest: String::new(),
        widen: Widen::default(),
        build_dir: String::new(),
        prefixes: Default::default(),
        resources: String::new(),
        platforms: Vec::new(),
        budget_minutes: 0.0,
        budget_lines: 0,
        reuse: Default::default(),
        disabled: false,
        results: None,
        performance: false,
    }
}

/// The proposal as the file Zerus would write.
pub fn proposal(config: &TestsConfig) -> String {
    let mut out = String::from("# Proposed by `hgs test config --propose` from the stacks found here;\n# see `hgs guide tests` for every field.\nversion = 1\n");
    for provider in &config.providers {
        out.push_str(&format!("\n[[provider]]\nname = {:?}\nkind = {:?}\n", provider.name, provider.kind.name()));
        if !provider.root.is_empty() {
            out.push_str(&format!("root = {:?}\n", provider.root));
        }
        out.push_str(&format!("sources = {:?}\n", provider.sources));
        if !provider.tests.is_empty() {
            out.push_str(&format!("tests = {:?}\n", provider.tests));
        }
        let rule = match provider.affected {
            Rule::Build => "build",
            Rule::Paths => "paths",
            Rule::All => "all",
        };
        out.push_str(&format!("affected = {rule:?}\n"));
        if !provider.manifest.is_empty() && provider.kind != Kind::Cargo {
            out.push_str(&format!("manifest = {:?}\n", provider.manifest));
        }
        if !provider.build_dir.is_empty() {
            out.push_str(&format!("build_dir = {:?}\n", provider.build_dir));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_stacks_are_found_at_the_top_and_one_folder_down() {
        let dir = tempfile::tempdir().unwrap();
        let write = |path: &str, text: &str| {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("Cargo.toml", "[package]\n");
        write("gui/CMakeLists.txt", "enable_testing()\n");
        write("web/package.json", r#"{"scripts":{"test":"vitest"}}"#);
        write("app/build.gradle.kts", "");
        write("tests/test_cli.py", "");
        write("node_modules/x/package.json", r#"{"scripts":{"test":"x"}}"#);
        let found = detect(dir.path());
        let names: Vec<(&str, Kind)> = found.providers.iter().map(|provider| (provider.name.as_str(), provider.kind)).collect();
        assert_eq!(
            names,
            vec![
                ("cargo", Kind::Cargo),
                ("unittest", Kind::Unittest),
                ("app-gradle", Kind::Gradle),
                ("gui-ctest", Kind::Ctest),
                ("web-npm", Kind::Npm),
            ]
        );
        assert_eq!(found.providers[3].build_dir, "gui/build");
        // The proposal reads back as the same configuration.
        let text = proposal(&found);
        let value = super::super::config::parse_toml(&text).unwrap();
        let back: TestsConfig = serde_json::from_value(value).unwrap();
        assert_eq!(back.providers.len(), 5);
        assert_eq!(back.providers[4].affected, Rule::All);
    }
}


