use super::*;
use crate::state::teams::templates::{execute_as, Plan};
use std::time::{Duration, SystemTime};

/// A member's folder and a person of its own: home, temporary, crash dump and
/// Zerus state folders, none of them the real ones.
struct Fixture {
    _dir: tempfile::TempDir,
    folder: PathBuf,
    person: Person,
}

fn fixture(tests_toml: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("member");
    let home = dir.path().join("person");
    for path in [folder.join(".zerus"), home.join("temp"), home.join("CrashDumps"), home.join("state"), home.join(".local/state/hgs")] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(folder.join(CONFIG), tests_toml).unwrap();
    let person = Person { home: Some(home.clone()), temp: home.join("temp"), dumps: Some(home.join("CrashDumps")), state: home.join("state") };
    Fixture { _dir: dir, folder, person }
}

const PROBE: &str = "[isolation]\ntemplates = [\"probe\"]\n";

fn shell(template: &str, windows: &str, unix: &str) -> Plan {
    let (program, flag, command) = if cfg!(windows) { ("cmd", "/c", windows) } else { ("sh", "-c", unix) };
    Plan { template: template.into(), program: program.into(), argv: vec![flag.into(), command.into()], commit: None, more: Vec::new(), cwd: None }
}

fn run(fixture: &Fixture, plan: &Plan) -> crate::state::teams::templates::Outcome {
    execute_as(plan, &fixture.folder, Duration::from_secs(60), &[], &fixture.person)
}

#[test]
fn every_platform_has_defaults_and_the_project_file_changes_them() {
    let defaults = Config::default();
    for name in ["HOME", "TEMP", "TMP", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
        assert!(defaults.env.contains_key(name), "{name}");
    }
    let platform = if cfg!(windows) { ["APPDATA", "LOCALAPPDATA"] } else { ["TMPDIR", "XDG_RUNTIME_DIR"] };
    assert!(platform.iter().all(|name| defaults.env.contains_key(*name)), "{defaults:?}");
    assert_eq!(defaults.templates, ["cargo-test", "qt-test", "android-test", "web-test"]);
    // A project names its own tests, adds and drops variables, per platform.
    let fixture = fixture(
        "[isolation]\ntemplates = [\"make-check\"]\nprefixes = [\"suite_\"]\nfiles = [\"~/app.ini\"]\n\
         [isolation.env]\nAPP_HOME = \"home/app\"\nXDG_CACHE_HOME = \"\"\n\
         [isolation.windows.env]\nWIN_ONLY = \"w\"\n[isolation.unix.env]\nUNIX_ONLY = \"u\"\n",
    );
    let (config, note) = config(&fixture.folder);
    assert_eq!(note, None);
    assert_eq!(config.templates, ["make-check"]);
    assert_eq!(config.env.get("APP_HOME").map(String::as_str), Some("home/app"));
    assert!(!config.env.contains_key("XDG_CACHE_HOME"));
    assert_eq!(config.env.contains_key("WIN_ONLY"), cfg!(windows));
    assert_eq!(config.env.contains_key("UNIX_ONLY"), !cfg!(windows));
    assert_eq!((config.prefixes.as_slice(), config.files.as_slice()), (["suite_".to_owned()].as_slice(), ["~/app.ini".to_owned()].as_slice()));
    // A folder outside the run folder, an unknown key or a broken file keep the
    // defaults and say why.
    for bad in ["[isolation.env]\nHOME = \"../elsewhere\"\n", "[isolation]\nsurprise = 1\n", "[isolation\n"] {
        fs::write(fixture.folder.join(CONFIG), bad).unwrap();
        let (config, note) = super::config(&fixture.folder);
        assert_eq!(config, Config::default(), "{bad}");
        assert!(note.is_some_and(|note| note.contains(CONFIG)), "{bad}");
    }
    // No file: the defaults, and a template that is not a test is not isolated.
    fs::remove_file(fixture.folder.join(CONFIG)).unwrap();
    assert_eq!(super::config(&fixture.folder), (Config::default(), None));
    assert!(prepare("git-merge-lead", "git", &fixture.folder, &fixture.person).is_none());
}

#[test]
fn a_test_runs_with_its_own_home_data_temporary_and_xdg_folders() {
    let fixture = fixture(PROBE);
    fs::write(fixture.person.home.as_ref().unwrap().join(".gitconfig"), "[user]\n\tname = Example\n").unwrap();
    let plan = shell(
        "probe",
        "echo HOME=%HOME%& echo TEMP=%TEMP%& echo APPDATA=%APPDATA%& echo LOCALAPPDATA=%LOCALAPPDATA%& echo XDG=%XDG_STATE_HOME%",
        "echo HOME=$HOME; echo TEMP=$TEMP; echo TMPDIR=$TMPDIR; echo XDG=$XDG_STATE_HOME; echo RUNTIME=$XDG_RUNTIME_DIR",
    );
    let outcome = run(&fixture, &plan);
    assert_eq!(outcome.exit, Some(0), "{outcome:?}");
    let runs = fixture.folder.join(".zerus-team").join("runs");
    let run_folder = fs::read_dir(&runs).unwrap().flatten().next().unwrap().path();
    let lines: Vec<&str> = outcome.tail.lines().filter(|line| line.contains('=')).collect();
    assert!(lines.len() >= 5, "{}", outcome.tail);
    for line in lines {
        let value = line.split_once('=').unwrap().1.trim();
        assert!(Path::new(value).starts_with(&run_folder), "{line} is outside {}", run_folder.display());
    }
    // The person's git identity is a copy in the run's home.
    assert!(run_folder.join("home").join(".gitconfig").is_file());
    // A suite running beside this one owns its folder: a fresh run folder is
    // never pruned, however many there are (Task #418).
    for _ in 0..KEEP_RUNS + 2 {
        run(&fixture, &plan);
    }
    assert_eq!(fs::read_dir(&runs).unwrap().count(), KEEP_RUNS * 2 - 2, "a fresh run folder is kept");
    // Once they are old, the count bound applies again.
    prune_at(&runs, SystemTime::now() + Duration::from_secs(KEEP_FRESH_SECS + 3600));
    assert_eq!(fs::read_dir(&runs).unwrap().count(), KEEP_RUNS - 1);
}

#[test]
fn the_fingerprint_fails_a_run_that_writes_the_persons_paths() {
    // The cases of 2026-10-30, each written by the run itself: a line in the
    // person's zerus.log, a store registered in the real private-stores.json,
    // the person's settings, a temporary folder in the person's %TEMP% and a
    // crash dump of a program the member built.
    let fixture = fixture(
        "[isolation]\ntemplates = [\"probe\"]\nprefixes = [\"test_\"]\n\
         files = [\"~/AppData/Roaming/example/app.ini\"]\nlogs = [\"~/.local/state/hgs/zerus.log\"]\n",
    );
    let home = fixture.person.home.clone().unwrap();
    let settings = home.join("AppData/Roaming/example/app.ini");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, "[General]\n").unwrap();
    let log = home.join(".local/state/hgs/zerus.log");
    fs::write(&log, "earlier line\n").unwrap();
    let registry = fixture.person.state.join("private-stores.json");
    fs::create_dir_all(fixture.folder.join("target/debug/deps")).unwrap();
    fs::write(fixture.folder.join("target/debug/deps/hgs-0123abcd.exe"), "x").unwrap();
    let quote = |path: &Path| if cfg!(windows) { path.display().to_string().replace('/', "\\") } else { path.display().to_string() };
    let cases: Vec<(&str, String, String)> = vec![
        // cmd gets no quotes: std would escape them as \" for it. The test
        // folders have no spaces.
        ("zerus.log", format!("echo warning in %ZERUS_TEST_RUN%>> {}", quote(&log)), format!("echo \"warning in $ZERUS_TEST_RUN\" >> '{}'", quote(&log))),
        ("private-stores.json", format!("echo {{stores:[{{path:%ZERUS_TEST_RUN%\\x}}]}}> {}", quote(&registry)),
         format!("echo \"{{\\\"stores\\\":[{{\\\"path\\\":\\\"$ZERUS_TEST_RUN/x\\\"}}]}}\" > '{}'", quote(&registry))),
        ("app.ini", format!("echo width=a>> {}", quote(&settings)), format!("echo width=a >> '{}'", quote(&settings))),
        ("test_suite-", format!("mkdir {}", quote(&fixture.person.temp.join("test_suite-a1b2"))), format!("mkdir '{}'", quote(&fixture.person.temp.join("test_suite-a1b2")))),
        ("crash dump", format!("echo x> {}", quote(&home.join("CrashDumps").join("hgs-0123abcd.exe.4242.dmp"))), format!("echo x > '{}'", quote(&home.join("CrashDumps").join("hgs-0123abcd.exe.4242.dmp")))),
    ];
    for (what, windows, unix) in cases {
        let outcome = run(&fixture, &shell("probe", &windows, &unix));
        assert_eq!(outcome.exit, Some(1), "{what}: {outcome:?}");
        assert!(outcome.tail.contains("Isolation: the run touched the person's own paths") && outcome.tail.contains(what), "{what}: {}", outcome.tail);
        assert_eq!(outcome.error, "the run touched the person's own paths");
    }
    // What carries no trace of the run passes: the owner's own log line, an
    // unrelated program's temporary file, another program's crash dump.
    let unrelated = shell(
        "probe",
        &format!("echo tray line>> {}& echo x> {}& echo x> {}", quote(&log), quote(&fixture.person.temp.join("other.tmp")),
                 quote(&home.join("CrashDumps").join("explorer.exe.7.dmp"))),
        &format!("echo tray line >> '{}'; echo x > '{}'; echo x > '{}'", quote(&log), quote(&fixture.person.temp.join("other.tmp")),
                 quote(&home.join("CrashDumps").join("explorer.exe.7.dmp"))),
    );
    let outcome = run(&fixture, &unrelated);
    assert_eq!(outcome.exit, Some(0), "{outcome:?}");
    assert!(!outcome.tail.contains("Isolation"), "{}", outcome.tail);
}

#[test]
fn the_project_file_cannot_turn_the_second_line_off() {
    // A project that turns the isolated folders off still gets the check of the
    // built-in paths: the person's temporary folder here.
    let fixture = fixture("[isolation]\nenabled = false\ntemplates = [\"probe\"]\n");
    let entry = fixture.person.temp.join("cmd-leftover");
    let entry_unix = fixture.person.temp.join("sh-leftover");
    let outcome = run(&fixture, &shell("probe", &format!("mkdir {}", entry.display()), &format!("mkdir '{}'", entry_unix.display())));
    assert_eq!(outcome.exit, Some(1), "{outcome:?}");
    assert!(outcome.tail.contains("leftover is new in"), "{}", outcome.tail);
    assert!(!fixture.folder.join(".zerus-team").join("runs").exists());
}

#[test]
fn a_crash_is_noted_in_the_run_folder() {
    let fixture = fixture(PROBE);
    // 0xC0000005, an access violation, as a crashed test exits on Windows; a
    // signal's 128 + 11 on Unix.
    let outcome = run(&fixture, &shell("probe", "exit -1073741819", "exit 139"));
    assert!(crashed(outcome.exit), "{outcome:?}");
    assert!(outcome.tail.contains("The test crashed"), "{}", outcome.tail);
    let runs = fixture.folder.join(".zerus-team").join("runs");
    let run_folder = fs::read_dir(&runs).unwrap().flatten().next().unwrap().path();
    assert!(run_folder.join("crash.txt").is_file());
    assert!(!crashed(Some(0)) && !crashed(Some(1)) && !crashed(Some(101)) && !crashed(None));
}

#[test]
fn a_test_without_a_state_folder_never_registers_in_the_persons_registry() {
    // 2026-10-30: read marks tests checked refused files in %TEMP% without a
    // state folder of their own, and the refusals went to the real
    // private-stores.json. An inherited HGS_STATE_DIR is not a test's own.
    let home = crate::platform::home().map(PathBuf::from).unwrap_or_default();
    assert!(!crate::state::storage::test_owned(&home.join(".local").join("state").join("hgs").join("sessions")));
    assert!(crate::state::storage::test_owned(&std::env::temp_dir().join("x")));
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("refused-by-a-test.json");
    crate::private_stores::write_failed(&store, crate::platform::Private::File, &"a test write");
    let default = crate::state::storage::test_default_root();
    let exe = std::env::current_exe().unwrap();
    assert!(default.starts_with(exe.parent().unwrap()), "{}", default.display());
    if let Some(home) = crate::platform::home() {
        let person = Path::new(&home).join(".local").join("state").join("hgs").join("sessions").join("private-stores.json");
        let text = fs::read_to_string(&person).unwrap_or_default();
        assert!(!mentions(&text, &store), "{} names {}", person.display(), store.display());
    }
}

/// Task 53: a qt-test run's tail held nothing, so a failing suite showed only
/// its exit code: QtTest logs to OutputDebugString when stderr has no console,
/// and the worker has none. The suites run isolated, as the worker runs them,
/// with Qt's switch to log to their stream, and the tail keeps what each
/// suite printed, the failing test named.
#[test]
fn a_qt_test_run_keeps_each_suites_output_in_its_tail() {
    let fixture = fixture("");
    let mut suites = shell(
        "qt-test",
        "echo FAIL!  : TestTeams::feedLines() logging=%QT_FORCE_STDERR_LOGGING%& exit /b 1",
        "echo \"FAIL!  : TestTeams::feedLines() logging=$QT_FORCE_STDERR_LOGGING\"; exit 1",
    );
    let second = shell("qt-test", "echo Totals: 3 passed, 0 failed", "echo 'Totals: 3 passed, 0 failed'");
    suites.more = vec![(second.program.clone(), second.argv.clone())];
    let outcome = run(&fixture, &suites);
    assert_eq!(outcome.exit, Some(1), "{}", outcome.tail);
    assert!(outcome.tail.contains("FAIL!  : TestTeams::feedLines() logging=1"), "{}", outcome.tail);
    assert!(outcome.tail.contains("Totals: 3 passed, 0 failed"), "{}", outcome.tail);
    assert!(outcome.failures.iter().any(|name| name.contains("feedLines")), "{:?}", outcome.failures);
}


/// Task #418: a run folder a concurrent suite may still be using is never
/// pruned; only one older than the fresh window goes, so a live suite's
/// isolated TEMP survives a neighbour's cleanup.
#[test]
fn a_fresh_run_folder_is_never_pruned() {
    let now = SystemTime::now();
    assert!(!prunable(now, now));
    assert!(!prunable(now - Duration::from_secs(60), now));
    assert!(prunable(now - Duration::from_secs(KEEP_FRESH_SECS + 1), now));
}
/// Task #23 (team ede724c4): a run folder is owner-only even inside a member
/// folder that every user may change (a folder on a second drive inherits
/// Authenticated Users Modify), and what a test creates in its TEMP inherits
/// that, so the stores that check their files are private accept them.
#[test]
fn the_run_folder_is_private_inside_a_shared_member_folder() {
    let fixture = fixture("[isolation]\ntemplates = [\"cargo-test\"]\n");
    #[cfg(windows)]
    {
        // Authenticated Users (S-1-5-11) may change the member's folder.
        let granted = std::process::Command::new("icacls")
            .arg(&fixture.folder)
            .args(["/grant", "*S-1-5-11:(OI)(CI)M", "/Q"])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(granted.status.success(), "{}", String::from_utf8_lossy(&granted.stderr));
        let shared = fixture.folder.join("shared.txt");
        fs::write(&shared, b"x").unwrap();
        assert!(!crate::platform::verify_private(&shared, crate::platform::Private::File).unwrap(), "the member folder is shared");
    }
    let run = prepare("cargo-test", "cargo", &fixture.folder, &fixture.person).expect("a test template");
    let temp = run.env.iter().find(|(name, _)| name == "TEMP").map(|(_, value)| PathBuf::from(value)).unwrap();
    assert!(crate::platform::verify_private(&run.folder, crate::platform::Private::Directory).unwrap());
    assert!(crate::platform::verify_private(&temp, crate::platform::Private::Directory).unwrap());
    let file = temp.join("store.sqlite3");
    fs::write(&file, b"x").unwrap();
    assert!(crate::platform::verify_private(&file, crate::platform::Private::File).unwrap(), "a test's file inherits the run folder's privacy");
}
