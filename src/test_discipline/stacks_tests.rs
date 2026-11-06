//! Task 137: qa's stack fixtures of task 89 (tests/fixtures/stacks): each
//! kind's affected run names its own templates, the build before the tests,
//! and each request plans as its template would run it.

use super::affected::{self, Hunks, Selection};
use super::config::{self, Kind, TestsConfig};
use super::durations::Durations;
use super::map::{self, ProviderMap, Target, TestMap};
use super::detect;
use crate::state::teams::templates;
use serde_json::{json, Value};
use std::path::Path;

/// A fixture's files written into a fresh folder.
fn place(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn parse(text: &str) -> TestsConfig {
    serde_json::from_value(config::parse_toml(text).unwrap()).unwrap()
}

fn select(root: &Path, config: &TestsConfig, map: &TestMap, changed: &[&str], full: bool) -> Selection {
    let changed: Vec<String> = changed.iter().map(|file| (*file).to_owned()).collect();
    affected::select(root, config, map, &Durations::default(), &changed, &Hunks::new(), full)
}

/// The request's template planned in the folder, as `hgs team run` would.
fn plans(root: &Path, request: &Value) -> templates::Plan {
    let template = request["template"].as_str().unwrap();
    templates::plan(template, &request["args"], root).unwrap_or_else(|error| panic!("{template} {}: {error:#}", request["args"]))
}

/// A project Python environment the py-test and cython-build templates use.
fn venv(root: &Path) {
    std::fs::create_dir_all(root.join(".venv/Scripts")).unwrap();
    std::fs::write(root.join(".venv/Scripts/python.exe"), "").unwrap();
}

const CMAKE: &[(&str, &str)] = &[
    ("CMakeLists.txt", include_str!("../../tests/fixtures/stacks/cmake/CMakeLists.txt")),
    ("src/math.cpp", include_str!("../../tests/fixtures/stacks/cmake/src/math.cpp")),
    ("src/math.h", include_str!("../../tests/fixtures/stacks/cmake/src/math.h")),
    ("tests/test_math.cpp", include_str!("../../tests/fixtures/stacks/cmake/tests/test_math.cpp")),
];

#[test]
fn a_generic_cmake_project_builds_with_cmake_and_runs_ctest_never_the_qt_template() {
    let dir = place(CMAKE);
    let config = parse(include_str!("../../tests/fixtures/stacks/cmake/tests.toml"));
    // The map hgs wrote from CMake's file API reply on the qa machine.
    let map: TestMap = serde_json::from_str(include_str!("../../tests/fixtures/stacks/cmake/test-map.json")).unwrap();
    let selection = select(dir.path(), &config, &map, &["src/math.cpp"], false);
    assert_eq!(selection.runs.len(), 1);
    let run = &selection.runs[0];
    assert!(!run.whole);
    assert_eq!(run.targets, ["demo_test"]);
    // qa's matrix printed `--test-dir ../build` for `root = "."`.
    assert_eq!(run.command, "ctest --test-dir build -R \"^(math\\.add)$\"");
    // Visual Studio's multi-config build: its folder names the configuration.
    assert_eq!(run.team_build, Some(json!({"template":"cmake-build","args":{"build_dir":"build","config":"Debug","targets":["demo_test"]}})));
    assert_eq!(run.team_run, Some(json!({"template":"ctest","args":{"build_dir":"build","config":"Debug","tests":["math.add"]}})));
    let text = affected::text(&selection);
    assert!(text.contains("hgs team run, the build first: {\"args\":{\"build_dir\":\"build\""), "{text}");
    assert!(text.contains("hgs team run, then: {\"args\":{\"build_dir\":\"build\",\"config\":\"Debug\",\"tests\":[\"math.add\"]},\"template\":\"ctest\"}"), "{text}");
    assert!(!text.contains("qt-test"), "{text}");
    // Both requests plan in the member's folder once the build is configured.
    std::fs::create_dir_all(dir.path().join("build")).unwrap();
    let build = plans(dir.path(), run.team_build.as_ref().unwrap());
    assert!(build.argv.windows(2).any(|pair| pair == ["--build", "build"]), "{:?}", build.argv);
    let tests = plans(dir.path(), run.team_run.as_ref().unwrap());
    assert!(tests.argv.windows(2).any(|pair| pair == ["-R", "^(math\\.add)$"]), "{:?}", tests.argv);
    assert!(tests.argv.windows(2).any(|pair| pair == ["-C", "Debug"]), "{:?}", tests.argv);
    // The full set builds everything and runs every test.
    let full = select(dir.path(), &config, &map, &[], true);
    assert_eq!(full.runs[0].team_build, Some(json!({"template":"cmake-build","args":{"build_dir":"build","config":"Debug"}})));
    assert_eq!(full.runs[0].team_run, Some(json!({"template":"ctest","args":{"build_dir":"build","config":"Debug"}})));
}

#[test]
fn a_python_change_runs_py_test_on_the_test_files_it_reaches() {
    let files: &[(&str, &str)] = &[
        ("pyproject.toml", include_str!("../../tests/fixtures/stacks/python/pyproject.toml")),
        ("src/demo.py", include_str!("../../tests/fixtures/stacks/python/src/demo.py")),
        ("tests/test_demo.py", include_str!("../../tests/fixtures/stacks/python/tests/test_demo.py")),
    ];
    let dir = place(files);
    let config = parse(include_str!("../../tests/fixtures/stacks/python/tests.toml"));
    let tracked: Vec<String> = files.iter().map(|(path, _)| (*path).to_owned()).collect();
    let map = map::generate(dir.path(), &config, &tracked, None).unwrap();
    let selection = select(dir.path(), &config, &map, &["src/demo.py"], false);
    let run = &selection.runs[0];
    assert_eq!((run.command.as_str(), run.tests), ("python -m pytest tests/test_demo.py", 2));
    assert_eq!(run.team_build, None);
    assert_eq!(run.team_run, Some(json!({"template":"py-test","args":{"paths":["tests/test_demo.py"]}})));
    venv(dir.path());
    let plan = plans(dir.path(), run.team_run.as_ref().unwrap());
    assert!(plan.argv.contains(&"tests/test_demo.py".to_owned()), "{:?}", plan.argv);
}

#[test]
fn cython_is_proposed_and_a_pyx_change_rebuilds_then_runs_only_its_tests() {
    let files: &[(&str, &str)] = &[
        ("pyproject.toml", include_str!("../../tests/fixtures/stacks/cython/pyproject.toml")),
        ("setup.py", include_str!("../../tests/fixtures/stacks/cython/setup.py")),
        ("src/demo.pyx", include_str!("../../tests/fixtures/stacks/cython/src/demo.pyx")),
        ("tests/test_demo.py", include_str!("../../tests/fixtures/stacks/cython/tests/test_demo.py")),
    ];
    let dir = place(files);
    let config = detect::detect(dir.path());
    let kinds: Vec<(&str, Kind)> = config.providers.iter().map(|provider| (provider.name.as_str(), provider.kind)).collect();
    assert_eq!(kinds, [("cython", Kind::Cython)]);
    let proposal = detect::proposal(&config);
    assert!(proposal.contains("kind = \"cython\""), "{proposal}");
    assert_eq!(parse(&proposal).providers[0].kind, Kind::Cython);
    let tracked: Vec<String> = files.iter().map(|(path, _)| (*path).to_owned()).collect();
    let map = map::generate(dir.path(), &config, &tracked, None).unwrap();
    let selection = select(dir.path(), &config, &map, &["src/demo.pyx"], false);
    assert_eq!(selection.runs.len(), 1);
    let run = &selection.runs[0];
    // Not the whole suite: the extension, then the tests that import it.
    assert!(!run.whole);
    assert!(selection.widened.is_empty(), "{:?}", selection.widened);
    assert_eq!(run.targets, ["tests/test_demo.py"]);
    assert_eq!(run.command, "python setup.py build_ext --inplace && python -m pytest tests/test_demo.py");
    assert_eq!(run.team_build, Some(json!({"template":"cython-build","args":{}})));
    assert_eq!(run.team_run, Some(json!({"template":"py-test","args":{"paths":["tests/test_demo.py"]}})));
    venv(dir.path());
    let build = plans(dir.path(), run.team_build.as_ref().unwrap());
    assert_eq!(build.argv, ["setup.py", "build_ext", "--inplace"]);
    plans(dir.path(), run.team_run.as_ref().unwrap());
    // A change to a test alone rebuilds nothing.
    let test_only = select(dir.path(), &config, &map, &["tests/test_demo.py"], false);
    assert_eq!(test_only.runs[0].team_build, None);
    // The full set rebuilds first.
    assert!(select(dir.path(), &config, &map, &[], true).runs[0].team_build.is_some());
}

const DEMO_TESTS: &str = r#"<Project DefaultTargets="Build" xmlns="http://schemas.microsoft.com/developer/msbuild/2003">
  <ItemGroup Label="ProjectConfigurations">
    <ProjectConfiguration Include="Debug|x64"><Configuration>Debug</Configuration><Platform>x64</Platform></ProjectConfiguration>
  </ItemGroup>
  <PropertyGroup Label="Globals"><ProjectSubType>NativeUnitTestProject</ProjectSubType></PropertyGroup>
  <PropertyGroup Label="Configuration"><ConfigurationType>DynamicLibrary</ConfigurationType></PropertyGroup>
  <ItemGroup><ClCompile Include="test_demo.cpp" /></ItemGroup>
  <ItemGroup><ProjectReference Include="..\demo.vcxproj" /></ItemGroup>
</Project>
"#;

const TEST_DEMO: &str = "#include \"CppUnitTest.h\"\nTEST_CLASS(Demo) {\n  TEST_METHOD(Adds) {}\n  TEST_METHOD(Prints) {}\n};\n";

#[test]
fn msbuild_is_proposed_and_a_changed_main_cpp_maps_to_its_project() {
    let mut files: Vec<(&str, &str)> = vec![
        ("src/demo.vcxproj", include_str!("../../tests/fixtures/stacks/msbuild/src/demo.vcxproj")),
        ("src/main.cpp", include_str!("../../tests/fixtures/stacks/msbuild/src/main.cpp")),
    ];
    let dir = place(&files);
    let config = detect::detect(dir.path());
    assert_eq!(config.providers.len(), 1);
    let provider = &config.providers[0];
    assert_eq!((provider.name.as_str(), provider.kind, provider.root.as_str(), provider.manifest.as_str()), ("src-msbuild", Kind::Msbuild, "src", "demo.vcxproj"));
    let proposal = detect::proposal(&config);
    assert!(proposal.contains("kind = \"msbuild\"") && proposal.contains("manifest = \"demo.vcxproj\""), "{proposal}");
    assert_eq!(parse(&proposal).providers[0].manifest, "demo.vcxproj");
    let tracked: Vec<String> = files.iter().map(|(path, _)| (*path).to_owned()).collect();
    let map = map::generate(dir.path(), &config, &tracked, None).unwrap();
    assert_eq!(map.providers["src-msbuild"].files["src/main.cpp"], ["demo"]);
    let selection = select(dir.path(), &config, &map, &["src/main.cpp"], false);
    assert!(selection.unmapped.is_empty() && selection.widened.is_empty(), "{selection:?}");
    let run = &selection.runs[0];
    assert_eq!(run.targets, ["demo"]);
    // No test project: the project's build is the check.
    assert_eq!(run.team_build, None);
    assert_eq!(run.team_run, Some(json!({"template":"msbuild-build","args":{"project":["src/demo.vcxproj"],"config":"Debug","platform":"x64"}})));
    let build = plans(dir.path(), run.team_run.as_ref().unwrap());
    assert!(build.argv.contains(&"/p:Platform=x64".to_owned()), "{:?}", build.argv);

    // A native unit test project that references it answers for main.cpp.
    files.push(("src/tests/demo_tests.vcxproj", DEMO_TESTS));
    files.push(("src/tests/test_demo.cpp", TEST_DEMO));
    let dir = place(&files);
    let tracked: Vec<String> = files.iter().map(|(path, _)| (*path).to_owned()).collect();
    let map = map::generate(dir.path(), &config, &tracked, None).unwrap();
    let selection = select(dir.path(), &config, &map, &["src/main.cpp"], false);
    let run = &selection.runs[0];
    assert_eq!((run.targets.as_slice(), run.tests), (["demo_tests".to_owned()].as_slice(), 2));
    assert_eq!(run.team_build, Some(json!({"template":"msbuild-build","args":{"project":["src/tests/demo_tests.vcxproj"],"config":"Debug","platform":"x64"}})));
    assert_eq!(run.team_run, Some(json!({"template":"vstest","args":{"tests":["src/tests/x64/Debug/demo_tests.dll"]}})));
    plans(dir.path(), run.team_build.as_ref().unwrap());
    std::fs::create_dir_all(dir.path().join("src/tests/x64/Debug")).unwrap();
    std::fs::write(dir.path().join("src/tests/x64/Debug/demo_tests.dll"), "").unwrap();
    plans(dir.path(), run.team_run.as_ref().unwrap());
    // The test's own source runs the same project.
    assert_eq!(select(dir.path(), &config, &map, &["src/tests/test_demo.cpp"], false).runs[0].targets, ["demo_tests"]);
}

#[test]
fn an_unreal_change_builds_the_editor_then_runs_its_automation_tests() {
    let config = parse(
        "version = 1\n[[provider]]\nname = \"game\"\nkind = \"unreal\"\nmanifest = \"Game.uproject\"\nsources = [\"Source/**\", \"Content/**\"]\nprefixes = { Game = \"Game.Core\" }\n",
    );
    let mut section = map_of(Kind::Unreal);
    section.targets.insert("Game.Core.Math".into(), Target { run: vec!["Game.Core.Math".into()], tests: 1, ..Target::default() });
    section.files.insert("Source/Game/Math.cpp".into(), vec!["Game.Core.Math".into()]);
    let map = TestMap { schema: 1, providers: [("game".to_owned(), section)].into() };
    let dir = tempfile::tempdir().unwrap();
    let run = &select(dir.path(), &config, &map, &["Source/Game/Math.cpp"], false).runs[0];
    assert_eq!(run.team_build, Some(json!({"template":"ue-build","args":{"project":"Game.uproject","target":"GameEditor","platform":"Win64","config":"Development"}})));
    assert_eq!(run.team_run, Some(json!({"template":"ue-test","args":{"project":"Game.uproject","filters":"Game.Core.Math"}})));
    let full = &select(dir.path(), &config, &map, &[], true).runs[0];
    assert_eq!(full.team_run, Some(json!({"template":"ue-test","args":{"project":"Game.uproject","filters":"Game.Core"}})));
}

fn map_of(kind: Kind) -> ProviderMap {
    serde_json::from_value(json!({"kind": kind, "rule": "build", "targets": {}})).unwrap()
}

#[test]
fn an_msbuild_or_unreal_provider_names_its_manifest() {
    for kind in ["msbuild", "unreal"] {
        let text = format!("version = 1\n[[provider]]\nname = \"x\"\nkind = \"{kind}\"\nsources = [\"**\"]\n");
        let dir = place(&[(".zerus/tests.toml", text.as_str())]);
        let error = config::load(dir.path(), None).err().map(|error| error.to_string()).unwrap_or_default();
        assert!(error.contains("names its manifest"), "{kind}: {error}");
    }
}
