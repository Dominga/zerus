# One provider model for builds and tests of any stack

Status: specification (task 84, owner request of 2026-11-06). Nothing here is
implemented yet. Today's behaviour is described in `docs/reference.md`
("Affected tests (U62)", "Remote test runners", "Build broker"); this document
says how it grows into one model for every stack, and what stays exactly as it
is.

The owner's request: Zerus's test and build subsystem must work for C++,
Unreal, Python, Cython and any other stack, not only for Rust and Qt.
Everything stays per project (`.zerus/tests.toml`, with the owner's overrides
in the team settings). Commands are typed argv templates, never shell strings.
Each run has an isolated environment and bounded output, and goes through the
build broker and the merge queue as builds do today.

## 1. Today

Each item names its code so a reviewer can check it.

**Project configuration.** `.zerus/tests.toml` (`src/test_discipline/config.rs`, `TestsConfig`, `deny_unknown_fields`).
- `[[provider]]` has `kind` = `cargo`, `ctest`, `pytest`, `unittest`, `gradle`, `npm` or `command`, with `root`, `sources`, `tests`, `manual`, `elsewhere`, `affected` (build, paths, all), `paths` rules, `command`, `manifest`, `widen`, `build_dir`, `resources`, `platforms`, budgets, `reuse` and `disabled`.
- Without a file, `detect::detect` proposes providers.
- The owner's override is `team.settings.tests`; providers merge by name.

**Affected map.** `hgs test map` (`map.rs`) knows:
- the Cargo module graph;
- CMake's file API with CTest and QtTest functions;
- an import or file-name heuristic for pytest, unittest, Gradle and npm.

`hgs test affected` (`affected.rs`) prints, for each run, a shell command
string built from the provider's `command` or the kind's default. Only cargo
and ctest runs also print `team_run`, a template request (`cargo-test` with
filters and skips, a CTest provider as `cmake-build` then `ctest`).

**Team run templates.** `hgs team run` (`src/state/teams/templates.rs`) has ten fixed ids.

| Template | What it runs |
|---|---|
| `cargo-build`, `cargo-test`, `cargo-clippy` | `cargo +{toolchain} … --package={package}` |
| `android-test` | wired to `mobile/android` |
| `clean-build` | removes build folders by name |
| `git-merge-lead`, `git-commit` | the Git operations |

- Arguments are typed: Identifier, Identifiers, Paths, Text.
- Output keeps 128 KiB per stream, a 4000-character tail and a failure excerpt.
- The process tree is killed on timeout.
- The owner's *project templates* (`state/teams/templates.json`) are fixed argv lists. They refuse every argument ("takes no placeholders").

**Isolation.** Every template run gets its own `TEMP`/`TMP` under `<folder>/.zerus-team/tmp`, and `CARGO_TARGET_DIR` is removed. Templates listed in `[isolation].templates` (default cargo-test, ctest, android-test, web-test) also get:
- a run folder under `.zerus-team/runs/`;
- their own `HOME`, `XDG_*`, `APPDATA` and `LOCALAPPDATA`;
- kept `CARGO_HOME` and `RUSTUP_HOME`;
- a copied `.gitconfig`;
- a fingerprint of the person's paths before and after.

The build cache (`[build_cache]`) sets sccache or ccache variables.

**Build broker.** `broker.rs`.
- The key is sha256 of the tree hash, template, arguments and the toolchain's version lines: `rustc -vV` and `cargo -V` for cargo-*, `cmake --version` for qt-*, otherwise `template:<id>`.
- Verdicts (cargo-test, cargo-clippy, ctest, vstest) serve every member.
- A root `*.uproject` selects the shared mode.

**Results.** Each suite is one record (`test_queue::Record`: status, duration, excerpt). Failure names are scraped from text (`failures_in`: Qt `FAIL!`, cargo `FAILED`, pytest `FAILED`, CTest `(Failed)`, Gradle task failures). Durations come from cargo, CTest, QtTest, unittest and pytest closing lines. No JUnit XML is read anywhere.

**Remote runners.** `test_runners.rs` matches a Machine with `test_runner` = windows, linux, macos or android by the provider's `platforms` class. The runner refuses a provider without an explicit `command` (`test_queue::suite_from_config`).

**Gaps.**
- C++ outside `tray/` has no template.
- MSBuild, Unreal builds and automation tests, and Cython are unknown.
- Python runs never pick the project's environment.
- Project templates cannot take a target.
- Results are per suite, so the owner never sees which test failed in a JUnit-reporting stack.
- `team_run` exists for two kinds only.

**A defect found while reading.** broker.rs documents and reads `[build_broker]` from tests.toml, but `TestsConfig` does not declare it while it refuses unknown fields. A tests.toml carrying `[build_broker]` would therefore fail to load for `hgs test` and the merge queue. This is inferred from the code and was not run. Section 9 fixes it first.

## 2. The model

A **provider** is one buildable and testable part of a project, of one stack. It has seven parts, all declared or detected per project:

| Part | What it is |
|---|---|
| detection | the files that reveal the stack and what they fill in |
| steps | `configure`, `build`, `test` and `list`, each a typed argv template |
| affected map | how changed files map to test targets |
| results | the formats a run leaves and how they map to one per-test record |
| isolation | the environment a run gets, per stack |
| broker key | the toolchain version lines that make a build result reusable |
| runner class | the platform and capabilities a machine needs to run it |

A **kind** is a stack: today's seven kinds plus `cmake`, `msbuild`, `unreal`
and `cython`. A kind gives defaults for every part; a provider entry may
override any of them. `ctest` stays as a name of `cmake` with only the test
step.

### 2.1 Steps: typed argv templates

A step is a program and its arguments. It is never run through a shell; a
placeholder is replaced by one argument, or, for a list, by several.

```toml
[[provider]]
name = "engine"
kind = "cmake"
root = "engine"

[provider.steps.configure]
program = "cmake"
args = ["-S", "{source}", "-B", "{build_dir}", "-G", "{generator}", "-DCMAKE_BUILD_TYPE={config}"]

[provider.steps.build]
program = "cmake"
args = ["--build", "{build_dir}", "--config", "{config}", "--target", "{targets...}"]
timeout_secs = 3600

[provider.steps.test]
program = "ctest"
args = ["--test-dir", "{build_dir}", "-C", "{config}", "-R", "{regex}", "--output-junit", "{results}/ctest.xml"]
results = [{ format = "junit", path = "{results}/ctest.xml" }]
```

**Placeholder types.** These are today's template types plus a few.

| Type | Accepts |
|---|---|
| `path` | a path relative to the run folder; no `..`, no absolute path unless the parameter allows it |
| `paths` | a list of `path` |
| `identifier` | a word of letters, digits, `_`, `-`, `.`, `:` and `/`, at most 200 characters |
| `identifiers` | a list of `identifier` |
| `text` | one argument of any text, at most 4000 characters, never split |
| `enum` | one of the listed values |
| `int`, `bool` | numbers and true or false |

A list placeholder is written `{name...}`. Without a value it drops the whole
argument, as an absent optional placeholder does today. A `--flag={x}`
argument is dropped with it.

**Built-in placeholders.** Every step may use these.

| Placeholder | Value |
|---|---|
| `{folder}` | the member's folder |
| `{source}` | the provider's `root` |
| `{build_dir}` | the provider's build folder, per member, default `.zerus-team/build/<provider>` |
| `{results}` | an empty folder for this run's reports, under `.zerus-team/runs/<run>/results` |
| `{config}` | the build configuration, default `Development` for Unreal and `Release` otherwise |
| `{targets...}` | what the run builds or tests, as the affected selection or the member's request names it |
| `{regex}` | the targets as an anchored alternation, `^(a\|b)$` |
| `{functions...}` | test functions within targets, where the kind has them |
| `{engine}`, `{python}`, `{msbuild}`, `{node}`, `{java}` | the located tool of the stack (sections 4 and 5) |

**Declared parameters.** A provider may declare its own:

```toml
[provider.params.platform]
type = "enum"
values = ["Win64", "Linux"]
default = "Win64"
```

A parameter without a default must be passed.

**Steps are optional.** A missing `configure` is skipped. A missing `build`
means the test step builds what it needs, as `cargo test` and `pytest` do. A
missing `list` falls back to the map's own listing.

**`command` stays.** A provider with `command` and no `steps.test` keeps
running that command exactly as today, so every existing tests.toml behaves
as before. A provider with both prefers `steps.test` for templates and the
remote runner. Its `command` remains what `hgs test affected` prints for a
person to run by hand.

### 2.2 Team run templates

Members build and test through four generic templates. The ten existing ids
stay unchanged (section 9).

| Template | Arguments | Runs |
|---|---|---|
| `provider-configure` | `provider`, declared params | the provider's configure step |
| `provider-build` | `provider`, `targets`, `config`, declared params, `files` | configure if the build folder has no configuration, then build |
| `provider-test` | `provider`, `targets`, `functions`, `config`, declared params, `files` | build if the provider has one, then test, then parse results |
| `provider-list` | `provider` | the list step, for the map and the Tests tab |

- The provider is looked up in the member folder's tests.toml merged with the owner's override, so a member cannot run a step the project did not declare.
- Placeholder values are checked against their types before anything runs; a refusal names the parameter.
- `hgs test affected` sets `team_run` for every run of a provider that has `steps.test`: `{"template":"provider-test","args":{"provider":"…","targets":[…]}}`.

The project templates in `templates.json` gain the same typed placeholders
through an optional `params` object, like a provider's. A template without
`params` keeps refusing arguments, so every existing entry behaves as today.

### 2.3 Detection

`hgs test config --propose` and the no-file default call each kind's detector
in a fixed order.
- A detector reads only files tracked in Git (`git ls-files`), at most three directory levels deep.
- It proposes one provider per root it finds, with the evidence (the file that revealed it) in the proposal.
- Detection never runs a tool. Locating an engine or interpreter happens at run time (section 4).
- Two detectors that claim the same root produce two providers. A person or the lead removes one; nothing is guessed.

### 2.4 One per-test result

Every run's reports map to one record per test, kept beside today's suite record.

```text
TestResult {
  provider, target, name,          // name as the runner prints it, at most 400 chars
  status: passed | failed | skipped | error | flaky,
  duration_ms,
  message,                         // first failure message, at most 2000 chars
  file, line,                      // when the format carries them
  output,                          // the test's own captured output, at most 4000 chars
}
```

- At most 5000 results are kept per run. Past that, failed ones are kept first and the count of the rest is recorded.
- The suite record stays authoritative for pass or fail. Results add detail and never change the exit-code verdict.
- A report that is missing or unreadable is a note on the suite record, not a failure.
- `flaky` is set only by the retry rules that exist today (`[retries]`).

**Formats and their mapping.**

| Format | Produced by | Mapping |
|---|---|---|
| JUnit XML | ctest `--output-junit`, pytest `--junitxml`, Gradle `build/test-results/**/*.xml`, vitest/jest junit reporters, GoogleTest `--gtest_output=xml`, Catch2 `--reporter junit`, cargo-nextest | `testcase` (classname + name), `failure`/`error`, `skipped`, `time`, `system-out` |
| CTest text | `ctest` without junit (CMake < 3.21) | today's `N/M Test #k: name … Passed/Failed sec` lines |
| QtTest | `-o file,junitxml` per test executable | JUnit as above; the suite is the executable |
| libtest text | `cargo test` | `test x ... ok/FAILED/ignored`; durations as today |
| Unreal automation JSON | `-ReportExportPath={results}` → `index.json` | `tests[].fullTestPath`, `state` (Success, Fail, NotRun, InProcess), `entries[].event.message` for the first error, `duration` |
| TRX | `vstest.console /Logger:trx` | `UnitTestResult` (testName, outcome, duration, ErrorInfo/Message) |
| unittest text | `python -m unittest -v` | `name (module.Class) ... ok/FAIL/ERROR/skipped` |

Readers are bounded. A file over 64 MiB is read only for its counts. XML
entity expansion is off, and every text field is cut to the limits above.

## 3. Isolation per stack

Every provider run gets today's base isolation:
- its own run folder;
- `HOME`, `TEMP`/`TMP`, `TMPDIR`, `XDG_*`, `APPDATA` and `LOCALAPPDATA` inside it;
- the `HGS_*` state variables removed;
- the fingerprint of the person's paths.

Each kind adds what its tools need, shared only where the tool's cache is
safe to share and slow to rebuild. Each kind's isolation is on by default.

| Kind | Kept shared (per member folder, outside the run folder) | Per run | Removed |
|---|---|---|---|
| cargo | `CARGO_HOME`, `RUSTUP_HOME` (as today), sccache | — | `CARGO_TARGET_DIR` |
| cmake | `{build_dir}`, ccache/sccache (`CCACHE_DIR`, `CCACHE_BASEDIR={folder}`, `CCACHE_NOHASHDIR`) | `{results}` | `CMAKE_*` from the person's environment except `CMAKE_PREFIX_PATH` when the provider keeps it |
| msbuild | `{build_dir}` as `OutDir`/`IntDir`, the NuGet cache (`NUGET_PACKAGES`) | `{results}`, `MSBUILDDISABLENODEREUSE=1` | `VSTEST_*` |
| unreal | the project's `Intermediate/` and `Binaries/`, the local DDC (`UE-LocalDataCachePath={folder}/.zerus-team/ue/ddc`) | `Saved/` redirected per run (`-saveddir={run}/Saved`, `-abslog={results}/editor.log`) | `UE-SharedDataCachePath` unless the owner sets one |
| pytest / unittest | the project's venv (section 5.4), `PIP_CACHE_DIR`, `UV_CACHE_DIR`, `POETRY_CACHE_DIR` under `{folder}/.zerus-team/py` | `PYTHONPYCACHEPREFIX={run}/pycache`, `PYTHONDONTWRITEBYTECODE=1` | `PYTHONHOME`, `PYTHONPATH` (unless the provider sets it), `VIRTUAL_ENV` of the person |
| cython | as Python, plus ccache for the C compiler | — | as Python |
| npm | `npm_config_cache={folder}/.zerus-team/npm` | — | `NODE_OPTIONS` of the person |
| gradle | `GRADLE_USER_HOME` and the project cache dir (as today) | — | — |
| command | the provider's `keep` list | — | the provider's `remove` list |

Two rules hold for every kind:
- **The workspace never leaks into the person's folders.** A build folder lives inside the member's folder; a cache lives under `.zerus-team/`. The fingerprint fails a run that wrote outside them.
- **The person's environment is opt-in.** Only variables a kind keeps, or the provider's `[isolation]` lists, reach the run.

## 4. Locating tools per machine

A step never relies on the person's `PATH` for a stack whose tool has several installed versions. It names `{engine}`, `{python}`, `{msbuild}` and so on, which Zerus locates per machine and records in the run.
- `{msbuild}`: vswhere, as qt-* finds MSVC today (`-latest -requires Microsoft.Component.MSBuild -find MSBuild\**\Bin\MSBuild.exe`).
- `{engine}`: the `.uproject`'s `EngineAssociation`.
  - A version (`5.4`) is looked up in the launcher's install list: on Windows `HKLM\SOFTWARE\EpicGames\Unreal Engine\<ver>\InstalledDirectory`, elsewhere `~/.config/Epic/UnrealEngine/Install.ini`.
  - A GUID (a source build) is looked up in `HKCU\Software\Epic Games\Unreal Engine\Builds`.
  - The Machine's own setting overrides both: `hgs machine set ALIAS --json '{"engines":{"unreal":{"5.4":"D:/UE_5.4"}}}'`.
- `{python}`: section 5.4.
- `{node}`, `{java}`: `PATH`, as today.

A tool that cannot be found is a missing tool, as today (`hgs test tools`, the owner's Waiting for you). The run is refused before anything starts, and names what it looked for.

## 5. The stacks

Every kind is described under the same headings.

### 5.1 Cargo (as today)

- **Detection:** `Cargo.toml` (today's detector).
- **Steps:** the existing `cargo-build`, `cargo-test` and `cargo-clippy` templates, unchanged. A cargo provider needs no `steps`. `provider-test` with a cargo provider maps to `cargo-test` with the same filters and skips, so both ids work.
- **Listing:** the module graph (`map.rs`).
- **Affected map:** unchanged.
- **Results:** libtest text. With `cargo-nextest` installed and `test_runner = "nextest"` set on the provider, its JUnit file instead.
- **Isolation:** as today.
- **Broker key:** `rustc -vV`, `cargo -V` (as today).
- **Runner class:** the provider's `platforms`.

### 5.2 CMake: generic C and C++ (and Qt)

- **Detection:** `CMakeLists.txt` at a root with a `project()` call; `CMakePresets.json` when present.
- **Steps (defaults):**
  - configure: `cmake -S {source} -B {build_dir} -G Ninja -DCMAKE_BUILD_TYPE={config}`, or `--preset {preset}` when the provider names one;
  - build: `cmake --build {build_dir} --config {config} --target {targets...}`;
  - test: `ctest --test-dir {build_dir} -C {config} -R {regex} --output-junit {results}/ctest.xml --output-on-failure`.
  - On Windows, the MSVC environment is loaded as qt-* does today.
- **Listing:** `ctest --test-dir {build_dir} --show-only=json-v1` (tests, labels, executables).
- **Affected map:** today's CMake file-API reader, for any build folder rather than `.ci-build/gui`.
- **Results:** CTest JUnit (CMake ≥ 3.21), else the CTest text lines. For a QtTest executable, also its own `-o {results}/<suite>.xml,junitxml`, which gives functions.
- **Isolation:** section 3 (cmake).
- **Broker key:** `cmake --version`, plus the C and C++ compiler id and version from the file-API toolchains reply (`CMAKE_CXX_COMPILER_ID`, `CMAKE_CXX_COMPILER_VERSION`), plus the generator.
- **Runner class:** `platforms`, and the capability `msvc` on Windows.
- **Qt in this repository:** gone with the Qt desktop (task #58); its `qt-configure`, `qt-build` and `qt-test` are refused with `cmake-configure`, `cmake-build` and `ctest` named instead.

### 5.3 MSBuild C++

- **Detection:** a `*.sln`, or a `*.vcxproj` not inside a CMake build folder, on Windows.
- **Steps (defaults):**
  - build: `{msbuild} {solution} /m /nologo /v:minimal /p:Configuration={config} /p:Platform={platform} /t:{targets...} /bl:{results}/msbuild.binlog /p:OutDir={build_dir}/bin/ /p:IntDir={build_dir}/obj/$(ProjectName)/`. `targets` are project names; empty builds the solution.
  - test, by the provider's `framework`:
    - `vstest`: `vstest.console.exe {build_dir}/bin/{tests...} /Logger:trx;LogFileName={results}/tests.trx`;
    - `gtest`: each test executable with `--gtest_output=xml:{results}/<exe>.xml`;
    - `catch2`: `--reporter junit --out {results}/<exe>.xml`;
    - `command`: the provider's own step.
- **Listing:** `vstest.console /ListTests`, `--gtest_list_tests`, `--list-tests`.
- **Affected map:** the solution's projects with their `ClCompile` and `ClInclude` items and `ProjectReference`s, read as XML. A changed source selects its project and every test project that references it. `widen` works as for Cargo.
- **Results:** TRX or JUnit (section 2.4).
- **Isolation:** section 3 (msbuild).
- **Broker key:** the Visual Studio `installationVersion` from vswhere, the `cl.exe` banner version, the Windows SDK version (`WindowsSDKVersion` after vcvars), and the platform toolset.
- **Runner class:** `windows` + `msvc`.

### 5.4 Python: pytest and unittest

- **Detection:**
  - pytest: `pytest.ini`, `conftest.py`, or `[tool.pytest.ini_options]` in `pyproject.toml` (today);
  - unittest: `test_*.py` without pytest.
- **The environment** is detected in this order, recorded in the run, and never created by Zerus:
  1. `uv.lock`: `{python}` = `uv run --frozen --no-sync python`. Its venv is the project's `.venv`. Syncing is the provider's own `configure` step (`uv sync --frozen`), never implicit.
  2. `poetry.lock` or `[tool.poetry]`: `poetry run python` (with `POETRY_VIRTUALENVS_IN_PROJECT=true`, so the venv is per member folder).
  3. A `.venv/` or `venv/` folder: its interpreter (`Scripts/python.exe` on Windows, `bin/python` elsewhere).
  4. Otherwise `python` on `PATH`, recorded as "no project environment". The suite record notes it.
- **Steps:**
  - pytest: `{python} -m pytest {targets...} --junitxml={results}/pytest.xml -p no:cacheprovider`;
  - unittest: `{python} -m unittest -v {targets...}` (discover when empty).
- **Listing:** `{python} -m pytest --collect-only -q`; for unittest, today's file scan.
- **Affected map:** today's import heuristic, extended to resolve `from a.b import c` to the file that defines it, and package `__init__` re-exports.
- **Results:** pytest JUnit, unittest `-v` text.
- **Isolation:** section 3 (Python). Each member folder has its own venv; a venv is never shared between members, because installs write into it.
- **Broker key:** `{python} -VV`, plus the hash of the lock file (`uv.lock`, `poetry.lock` or `requirements*.txt`).
- **Runner class:** `platforms`.

### 5.5 Cython

- **Detection:** tracked `*.pyx` or `*.pxd` files, plus a `setup.py` calling `cythonize`, or a `pyproject.toml` whose build requirements name Cython.
- **Steps:**
  - build, `mode = "inplace"` (default when `setup.py` exists): `{python} setup.py build_ext --inplace`;
  - build, `mode = "editable"`: `{python} -m pip install -e . --no-build-isolation` (with uv: `uv pip install -e . --no-build-isolation`). No build isolation, so the project's own Cython and NumPy are used;
  - test: the Python provider's test step. A Cython provider names its Python provider in `tests_with = "python"`, or carries the test step itself.
- **Rebuild rule:** the build step runs when any `.pyx`, `.pxd`, `.pxi`, `setup.py`, `setup.cfg` or `pyproject.toml` changed since the last successful build of this member folder. The stamp is the hash of those files, kept in `.zerus-team/build/<provider>/inputs`. Otherwise the step is skipped and the suite record says so.
- **Listing:** as Python.
- **Affected map:** a `.pyx` or `.pxd` is a source of the module of the same dotted name. Tests that import that module are affected, through the Python heuristic.
- **Results:** as Python. A build failure is the suite's failure, with the compiler's first error as the excerpt.
- **Isolation:** Python, plus ccache for the C compiler (`CC="ccache cc"` where the provider allows it).
- **Broker key:** `{python} -VV`, `cython --version`, and the C compiler's version (`cc --version`, or the `cl.exe` banner).
- **Runner class:** `platforms`.

### 5.6 Unreal

- **Detection:** a `*.uproject` at a root.
  - `EngineAssociation` names the engine.
  - The targets are `Source/*.Target.cs`: `<Name>Editor` and `<Name>`.
  - Today's broker shared mode and its asset folders (`Content`, `Plugins/**/Content`) stay as they are.
- **Steps:**
  - build, Windows: `{engine}/Engine/Build/BatchFiles/Build.bat {target} {platform} {config} -Project={folder_abs}/{uproject} -WaitMutex -NoHotReloadFromIDE`;
  - build, Linux and macOS: `{engine}/Engine/Build/BatchFiles/{Linux|Mac}/Build.sh` with the same arguments;
  - default target `<Name>Editor`, platform `Win64`/`Linux`/`Mac`, config `Development`;
  - test: `{engine}/Engine/Binaries/{platform}/UnrealEditor-Cmd{exe} {folder_abs}/{uproject} -ExecCmds="Automation RunTests {filter};Quit" -unattended -nopause -nullrhi -nosplash -nosound -stdout -FullStdOutLogOutput -ReportExportPath={results} -abslog={results}/editor.log -saveddir={run}/Saved`.
    - `{filter}` is the targets joined with `+`, Unreal's syntax; empty runs `Project`.
    - The whole `-ExecCmds=` value is one argument, never shell text.
- **Listing:** the same editor command with `Automation List`, read from the log. The list is cached per engine and project hash, because starting the editor is slow.
- **Affected map:** by module.
  - A changed file under `Source/<Module>/` selects the automation tests whose path starts with that module's test prefix. The provider sets `prefixes = {Module = "Project.Module"}`; by default the module name is the prefix.
  - A changed asset under `Content/` selects `affected = "all"` for that provider, because asset dependencies are not read.
  - The owner may narrow it with `paths` rules.
- **Results:** `{results}/index.json` (section 2.4). The editor's exit code alone is not trusted: a run whose report is missing or empty is `error`, with the log's last lines as the excerpt.
- **Isolation:** section 3 (unreal).
  - The local DerivedDataCache is per member folder.
  - A shared DDC is used only when the owner sets one per machine.
  - `Saved/` is per run.
  - In shared mode, `Intermediate/` and `Binaries/` stay in the folder that all members share, which is why shared mode builds one at a time (today).
- **Broker key:** the engine's `Engine/Build/Build.version` (MajorVersion, MinorVersion, PatchVersion, Changelist, BranchName), the platform toolchain (the `cl.exe` banner on Windows, `clang --version` elsewhere), and the target, platform and config. In shared mode results are not reused (today).
- **Runner class:** `platforms` + `unreal:<version>` (the machine must have that engine) + `gpu` only when the provider drops `-nullrhi`.

### 5.7 npm and Gradle (as today)

**npm**
- Detection, the import heuristic and listing stay as today.
- Steps: `steps.test` default `npm test -- {targets...}`, today's default as argv. With vitest, `npx vitest run {targets...} --reporter=default --reporter=junit --outputFile.junit={results}/vitest.xml`.
- Results: JUnit when written, else the closing lines.
- Isolation: section 3.
- Broker key: `node --version`, `npm --version`, the hash of `package-lock.json` (or `pnpm-lock.yaml` or `yarn.lock`).

**Gradle**
- Detection, the import heuristic and listing stay as today.
- Steps: test `{gradlew} {tasks...} --no-daemon {targets...}`, the existing android-test isolation.
- Results: `build/test-results/**/*.xml` (JUnit).
- Broker key: `java -version`, plus the wrapper's `distributionUrl`.

### 5.8 Generic command

`kind = "command"` keeps today's `command` string unchanged. It may also
declare `steps` with typed placeholders, `params` and `results`, for any stack
Zerus does not know:

```toml
[[provider]]
name = "firmware"
kind = "command"
affected = "paths"

[provider.steps.build]
program = "make"
args = ["-C", "{source}", "{targets...}"]

[provider.steps.test]
program = "python"
args = ["tools/hil.py", "--board", "{board}", "--junit", "{results}/hil.xml", "{targets...}"]
results = [{ format = "junit", path = "{results}/hil.xml" }]

[provider.params.board]
type = "enum"
values = ["nrf52", "stm32"]
default = "nrf52"
```

- **Broker key:** the provider's `toolchain` list of version commands, each typed argv (`[["make","--version"],["python","-VV"]]`). Without one, the key is `template:<provider>`, as unknown templates are today; such a result is reused only within the same tree hash.

## 6. Broker keys

The key stays `sha256(tree_hash, template, arguments without files, toolchain lines)`. The toolchain lines are the outputs of the kind's version commands (sections 5.1 to 5.8).
- A command that fails gives `<program>:unknown`, which disables reuse, as today.
- Every version command is typed argv with a 10-second limit, run once per tick per member folder and cached by the tool's path and modification time.
- `provider-test` is a verdict template (shared by every member), like cargo-test.
- `provider-build` serves only its own member, like the build templates today.

## 7. Remote runner classes

Today a runner has one platform class.
- A runner keeps it, and may add capabilities: `hgs machine set ALIAS --json '{"test_runner":"windows","capabilities":["msvc","unreal:5.4","gpu"]}'`.
- A provider's class is its `platforms` plus its kind's needs (sections 5.1 to 5.8) plus its own `requires = [...]`.
- A suite runs on a runner whose platform is in the class and which has every required capability. Otherwise it is held with the reason, as today. It never runs on a machine without the capability.
- **The runner side builds the steps.** It uses the provider's steps, or the kind's default steps, from the commit's tests.toml. A provider without `command` is no longer refused when its kind has a default step, which closes today's gap where only an explicit `command` runs remotely.
- Results come back in the runner's answer with the suite record, within the same size limits.

## 8. The merge queue

The merge queue keeps its rules: one merge under test at a time, a failure rolls the merge back and returns the task, and a suite with no machine of its class is `not_run`.
- It queues the affected runs as typed step requests (`provider-test` with targets) instead of command strings, when the provider has steps. Otherwise it queues the command string, as today.
- Per-test results of a failed merge go to the task's author with the failing test names.

## 9. Compatibility

Everything existing keeps its meaning.

**`.zerus/tests.toml`**
- Every key keeps its name, type and default.
- New keys are optional and declared on the structs:
  - on `[[provider]]`: `steps`, `params`, `results`, `requires`, `toolchain`, `mode`, `framework`, `tests_with`, `prefixes`;
  - at the top: `[build_broker]`.
  - So `deny_unknown_fields` still catches typos.
- `kind = "ctest"` stays valid (section 5.2).
- `command` keeps priority for what `hgs test affected` prints and for the merge queue of a provider without steps.
- `version` stays 1. Adding optional keys needs no new version, because an older hgs refuses them loudly instead of misreading them.

**The `[build_broker]` defect** (section 1) is fixed first. `TestsConfig` gains a `build_broker` field holding today's broker keys, so one file serves both readers. Until then, a project that sets `[build_broker]` cannot use `hgs test`. This is inferred from the code, not observed.

**Team run templates**
- The ten ids, their arguments, argv, environment, timeouts and output bounds stay exactly as they are; the test that pins the ten ids is extended, not replaced.
- `qt-*` stay wired to `tray/` and `.ci-build/gui` for this repository, and are documented as aliases of the tray provider.
- `templates.json` entries without `params` keep refusing arguments.

**`hgs test affected` output**
- Existing fields are unchanged; `team_run` for cargo and ctest runs stays as it is.
- A run of a provider with steps gains `team_run` with `provider-test`.

**Results**
- `test_queue::Record` keeps every field. Per-test results are a new, separate record list referenced by the suite record's id.
- Readers of today's records (the Tests tab, the phone) see nothing new until they ask for results.

**Remote runners:** `test_runner` keeps its four values. `capabilities` is optional.

**Isolation:** today's defaults (`[isolation]` keys and the default template list) stay. Kind-specific additions apply only to the new provider templates and to kinds that had no isolation.

## 10. Order of work

1. Declare `[build_broker]` in `TestsConfig`, with a test that a file with it loads for `hgs test` and the merge queue.
2. Typed steps, placeholders, `provider-*` templates and `params` for project templates, behind the existing templates. Port the tray provider to `kind = "cmake"` with qt-* as aliases. Tests: placeholder typing, refusals, a dropped absent argument, alias equality with today's argv.
3. The per-test result record and the JUnit reader. Then CTest, QtTest, TRX, unittest and Unreal readers, each with a fixture file per format.
4. Kinds one at a time, each with detection fixtures, map fixtures and a template test:
   - cmake (generic);
   - Python environments (uv, poetry, venv);
   - Cython;
   - MSBuild;
   - Unreal (a fake engine folder with a recorded `index.json`, because CI has no engine).
5. Runner capabilities and the remote side building steps.
6. The merge queue queuing typed requests.

Each step keeps every existing test green and adds its own.

## 11. Open questions for the owner (one proposal card)

The lead sends these as one question card. The recommended answers are this
document's defaults.

```json
{"kind":"question","reason":"Spec task 84: the build and test provider model needs seven choices before work starts",
 "question":"Task 84 (docs/test-providers.md): 1) Python environments: Zerus uses a project's uv, poetry or .venv but never creates or syncs one; syncing is the project's own configure step. 2) Unreal: one engine per machine is found from EngineAssociation and the launcher; you set a source build's folder per machine. Is a shared DerivedDataCache wanted, or only per member? 3) Unreal tests run with -nullrhi by default; a project that needs a GPU says so and runs only on machines marked gpu. 4) MSBuild tests: which framework matters first (VSTest, GoogleTest, Catch2)? 5) Per-test results: keep at most 5000 per run, failed first. 6) Remote runners gain capabilities (msvc, unreal:5.4, gpu) set per machine by you. 7) Order of work: fix [build_broker] in tests.toml first, then typed steps, results, then kinds (CMake, Python, Cython, MSBuild, Unreal). Accept these defaults, or answer the numbers you want changed.",
 "options":["Accept all defaults","I answer some numbers in my own words","Hold the work until we talk"],
 "recommended":"Accept all defaults"}
```
