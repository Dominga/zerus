//! Native Windows controls for the portable frontend binding. Fixture files
//! and fixture processes (copies of this test binary) under temporary folders
//! only; no installed app, task, registration, host or agent. Task 180: each
//! temporary root is private to this user, so the runner's inherited access
//! entries never decide a result (staging::private_tempdir).
use super::*;
use std::{fs, io::Write, process::Command, sync::Arc};

/// The original fixture bytes; `CHANGED` has exactly the same length.
const ORIGINAL: &[u8] = b"portable app fixture, original bytes";
const CHANGED: &[u8] = b"portable app fixture, changed! bytes";
/// Generous for tests; production callers pass their own original deadline.
const DIGEST_TIME: Duration = Duration::from_secs(30);

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = staging::private_tempdir();
    let zip = dir.path().join("zip");
    fs::create_dir(&zip).unwrap();
    let gui = zip.join(staging::FRONTEND_IMAGE);
    fs::write(&gui, ORIGINAL).unwrap();
    (dir, gui)
}

/// What the app's own staging would record for `gui` with `cli` (by default
/// this process's image) as the staged CLI, taken through the same pins.
fn binding_with(gui: &Path, cli: &PinnedImage) -> FrontendBinding {
    let app = PinnedImage::open(gui).unwrap();
    FrontendBinding {
        schema: FRONTEND_SCHEMA,
        frontend: object(&app),
        frontend_sha256: app
            .sha256(DIGEST_BYTES, Instant::now() + DIGEST_TIME)
            .unwrap(),
        cli: object(cli),
    }
}

fn binding_for(gui: &Path) -> FrontendBinding {
    binding_with(gui, &PinnedImage::for_process(&own_process()).unwrap())
}

/// Publish `binding` into the private home `home`, as the app's own install does.
fn publish_into(home: &Path, binding: &FrontendBinding) -> Layout {
    let layout = Layout::new(&home.to_string_lossy(), &home.join("config"));
    let source = home.with_extension("source").join(staging::EXECUTABLE);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"staged CLI fixture").unwrap();
    staging::install_bound(&layout, &source, "1.0.0", |_, publish| {
        assert!(publish(binding, &|| true)?);
        Ok(Some(binding.clone()))
    })
    .unwrap();
    layout
}

/// A serving scope of this process (its own image is the bound CLI) over a
/// private home in `root` whose record binds `gui`.
fn serve(root: &Path, gui: &Path) -> (PortableScope, FrontendBinding, Layout) {
    let binding = binding_for(gui);
    let home = root.join("serve-home");
    let layout = publish_into(&home, &binding);
    let scope = scope_over(&home);
    (scope, binding, layout)
}

/// This process's own loaded image, as a scope records it.
fn own_cli() -> BoundObject {
    object(&PinnedImage::for_process(&own_process()).unwrap())
}

/// A scope of this process over `home`'s images folder. Tests run from the
/// build folder, not from `<home>\.local\bin`, so the folder is given here;
/// `materialize` itself derives it from the CLI (see the serving-child test).
fn scope_over(home: &Path) -> PortableScope {
    PortableScope::with_images(staging::images_below(&home.to_string_lossy()), own_cli())
        .expect("this process's own image")
}

fn until() -> Instant {
    Instant::now() + DIGEST_TIME
}

#[test]
fn a_closed_launch_proof_keeps_its_own_pin_until_released() {
    let (dir, gui) = fixture();
    let zip = gui.parent().unwrap().to_owned();
    let (scope, _, _) = serve(dir.path(), &gui);
    let pin = scope
        .closed(&gui, until())
        .expect("the bound app is proven");
    // While the launch holds its pin, neither the object nor a folder above it
    // can be written, replaced, deleted or renamed.
    assert!(fs::OpenOptions::new().write(true).open(&gui).is_err());
    assert!(fs::rename(&gui, zip.join("aside.exe")).is_err());
    assert!(fs::remove_file(&gui).is_err());
    assert!(fs::rename(&zip, zip.with_file_name("moved")).is_err());
    assert!(pin.revalidate().is_ok());
    drop(pin);
    // The scope itself pins nothing: the same swap succeeds now.
    fs::rename(&gui, zip.join("aside.exe")).unwrap();
    fs::rename(zip.join("aside.exe"), &gui).unwrap();
    assert!(scope.closed(&gui, until()).is_some());
    // A deadline that has already passed is never a bounded success.
    assert!(scope.closed(&gui, Instant::now()).is_none());
}

#[test]
fn an_edit_that_keeps_the_length_and_write_time_is_refused() {
    let (dir, gui) = fixture();
    let (scope, binding, _) = serve(dir.path(), &gui);
    let written = fs::metadata(&gui).unwrap().modified().unwrap();
    {
        let mut file = fs::OpenOptions::new().write(true).open(&gui).unwrap();
        file.write_all(CHANGED).unwrap();
        file.set_modified(written).unwrap();
    }
    // The same object, length and write time: only the digest differs.
    let pinned = PinnedImage::open(&gui).unwrap();
    assert!(is(&pinned, &binding.frontend));
    drop(pinned);
    assert!(scope.closed(&gui, until()).is_none());
}

#[test]
fn a_swapped_object_is_refused_and_only_the_original_object_returns() {
    let (dir, gui) = fixture();
    let (scope, _, _) = serve(dir.path(), &gui);
    let aside = dir.path().join("original.exe");
    // A out, B in: identical bytes in another object are refused.
    fs::rename(&gui, &aside).unwrap();
    fs::write(&gui, ORIGINAL).unwrap();
    assert!(scope.closed(&gui, until()).is_none());
    // A back: the recorded object itself is proven again.
    fs::remove_file(&gui).unwrap();
    fs::rename(&aside, &gui).unwrap();
    assert!(scope.closed(&gui, until()).is_some());
}

#[test]
fn a_binding_for_another_cli_object_is_refused() {
    let (dir, gui) = fixture();
    // A byte-identical copy of this process image is another object, as a CLI
    // staged without a new binding would be.
    let copy = dir.path().join(staging::EXECUTABLE);
    fs::copy(std::env::current_exe().unwrap(), &copy).unwrap();
    let binding = binding_with(&gui, &PinnedImage::open(&copy).unwrap());
    let home = dir.path().join("serve-home");
    publish_into(&home, &binding);
    let scope = scope_over(&home);
    assert!(scope.snapshot().is_none());
    assert!(scope.closed(&gui, until()).is_none());
}

#[test]
fn a_missing_or_corrupt_record_proves_nothing() {
    let dir = staging::private_tempdir();
    let (_, gui) = fixture();
    let home = dir.path().to_string_lossy().into_owned();
    let scope = scope_over(dir.path());
    assert!(scope.snapshot().is_none() && scope.closed(&gui, until()).is_none());
    let images = staging::images_below(&home);
    fs::create_dir_all(&images).unwrap();
    fs::write(images.join(staging::FRONTEND_BINDING), b"{\"schema\":1").unwrap();
    assert!(scope.closed(&gui, until()).is_none());
}

#[test]
fn a_process_not_started_by_the_app_beside_it_records_nothing() {
    let dir = staging::private_tempdir();
    let layout = Layout::new(
        &dir.path().join("home").to_string_lossy(),
        &dir.path().join("config"),
    );
    fs::create_dir_all(layout.cli.parent().unwrap()).unwrap();
    fs::write(&layout.cli, b"staged CLI fixture").unwrap();
    // This test runs under the test harness, not under an hgs-tray.exe beside it.
    let outcome = record(&layout, &mut |_, _| panic!("nothing may be published"));
    assert!(matches!(outcome, Ok(None)));
}

/// A fixture GUI: a copy of this test binary named hgs-tray.exe in `folder`,
/// running until `quit` appears beside it, then exiting normally.
struct FixtureGui {
    image: PathBuf,
    child: std::process::Child,
}

impl FixtureGui {
    fn start(folder: &Path) -> Self {
        fs::create_dir_all(folder).unwrap();
        let _ = fs::remove_file(folder.join("quit"));
        let _ = fs::remove_file(folder.join("ready"));
        let image = folder.join(staging::FRONTEND_IMAGE);
        fs::copy(std::env::current_exe().unwrap(), &image).unwrap();
        let child = run_role(&image, "gui")
            .env(HOME, folder)
            .env_remove(AGENT_MARKER)
            .spawn()
            .unwrap();
        // Ready once it waits for its quit file.
        let ready = folder.join("ready");
        let end = Instant::now() + Duration::from_secs(60);
        while !ready.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(ready.exists(), "fixture GUI did not start");
        Self { image, child }
    }
    fn original(&self) -> Original {
        Original::open(self.child.id()).unwrap()
    }
    /// A normal quit: the process ends by itself.
    fn quit(mut self) {
        fs::write(self.image.with_file_name("quit"), b"").unwrap();
        assert!(self.child.wait().unwrap().success());
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !done() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(done(), "{what}");
}

#[test]
fn a_running_gui_proof_is_released_on_exit_while_borrowers_keep_their_pin() {
    let dir = staging::private_tempdir();
    let gui = FixtureGui::start(&dir.path().join("zip"));
    let (scope, _, _) = serve(dir.path(), &gui.image);
    let original = gui.original();
    let proof = scope
        .running(&original, until())
        .expect("the running app is proven");
    assert!(proof.proves(&original));
    assert_eq!(scope.cache.len(), 1);
    // The same process is answered from its retained proof.
    let again = scope.running(&original, until()).unwrap();
    assert!(Arc::ptr_eq(&proof, &again));
    drop(again);
    // Windows would let a running image be renamed; the proof's pin forbids it.
    let aside = gui.image.with_file_name("aside.exe");
    assert!(fs::rename(&gui.image, &aside).is_err());
    // A normal quit: the cache lets go at once, without another request.
    let image = gui.image.clone();
    gui.quit();
    wait_until("the cache drops the proof of an exited GUI", || {
        scope.cache.len() == 0
    });
    assert!(!proof.current());
    // A borrower that has not settled still blocks replacement at that path.
    assert!(fs::copy(std::env::current_exe().unwrap(), &image).is_err());
    drop(proof);
    // Settled: the same path can be replaced, and the new object is not the bound one.
    fs::remove_file(&image).unwrap();
    fs::write(&image, ORIGINAL).unwrap();
    assert!(scope.closed(&image, until()).is_none());
}

#[test]
fn another_process_or_an_exited_one_has_no_proof() {
    let dir = staging::private_tempdir();
    let gui = FixtureGui::start(&dir.path().join("zip"));
    let other = FixtureGui::start(&dir.path().join("other"));
    let (scope, _, _) = serve(dir.path(), &gui.image);
    // Another process running another image is not the bound app.
    assert!(scope.running(&other.original(), until()).is_none());
    let original = gui.original();
    assert!(scope.running(&original, until()).is_some());
    other.quit();
    gui.quit();
    wait_until("no proof outlives its process", || scope.cache.len() == 0);
    // The exited original proves nothing any more.
    assert!(scope.running(&original, until()).is_none());
}

#[test]
fn maintained_staging_proceeds_while_old_portable_work_remains() {
    let dir = staging::private_tempdir();
    let home = dir.path().join("home");
    let layout = Layout::new(&home.to_string_lossy(), &home.join("config"));
    // A serving process whose own image is the staged stable CLI, with a scope.
    let old_gui = FixtureGui::start(&dir.path().join("zip-1"));
    fs::create_dir_all(layout.cli.parent().unwrap()).unwrap();
    fs::copy(std::env::current_exe().unwrap(), &layout.cli).unwrap();
    let binding = binding_with(&old_gui.image, &PinnedImage::open(&layout.cli).unwrap());
    // Its record, as the old app's install published it (the CLI unchanged).
    staging::install_bound(&layout, &layout.cli, "1.0.0", |_, publish| {
        assert!(publish(&binding, &|| true)?);
        Ok(Some(binding.clone()))
    })
    .unwrap();
    let mut service = run_role(&layout.cli, "scope")
        .env(HOME, &home)
        .env(REPORT, &old_gui.image)
        .env_remove(AGENT_MARKER)
        .spawn()
        .unwrap();
    wait_until("the serving process materialized its scope", || {
        home.join("scope-ready").exists()
    });
    // This test process holds old-GUI work of its own: a running proof.
    let (scope, _, _) = serve(&dir.path().join("parent"), &old_gui.image);
    let old_work = scope.running(&old_gui.original(), until()).unwrap();
    // A newer zip elsewhere: its staging replaces the running stable CLI and
    // binds its own app while both the scope and the old work are alive.
    let new_zip = dir.path().join("zip-2");
    fs::create_dir_all(&new_zip).unwrap();
    let new_source = new_zip.join(staging::EXECUTABLE);
    fs::write(&new_source, b"newer staged CLI build").unwrap();
    let new_gui = new_zip.join(staging::FRONTEND_IMAGE);
    fs::write(&new_gui, ORIGINAL).unwrap();
    let report = staging::install_bound(&layout, &new_source, "9.9.9", |layout, publish| {
        let binding = binding_with(&new_gui, &PinnedImage::open(&layout.cli).unwrap());
        assert!(publish(&binding, &|| true)?);
        Ok(Some(binding))
    })
    .expect("maintained staging is not blocked by a scope or by old work");
    assert!(report.cli.changed);
    assert_eq!(fs::read(&layout.cli).unwrap(), b"newer staged CLI build");
    let published = staging::read_frontend(&layout.images).unwrap();
    assert!(published
        .frontend
        .path
        .as_os_str()
        .eq_ignore_ascii_case(PinnedImage::open(&new_gui).unwrap().path().as_os_str()));
    // The old work finishes on its own proof; the serving process is untouched.
    assert!(old_work.current());
    assert!(service.try_wait().unwrap().is_none());
    fs::write(home.join("scope-done"), b"").unwrap();
    assert!(service.wait().unwrap().success());
    drop(old_work);
    // Normal quit of the old app, then a same-path replacement of it.
    let old_image = old_gui.image.clone();
    old_gui.quit();
    wait_until("the cache drops the old app's proof", || {
        scope.cache.len() == 0
    });
    fs::remove_file(&old_image).unwrap();
    fs::write(&old_image, CHANGED).unwrap();
}

const ROLE: &str = "HGS_TEST_PORTABLE_ROLE";
const MODE: &str = "HGS_TEST_PORTABLE_MODE";
const HOME: &str = "HGS_TEST_PORTABLE_HOME";
const REPORT: &str = "HGS_TEST_PORTABLE_REPORT";
const BARRIER: &str = "HGS_TEST_PORTABLE_BARRIER";
const DELAY: &str = "HGS_TEST_PORTABLE_DELAY_MS";
const AGENT_MARKER: &str = "HGS_SESSION";
/// Every variable the ancestry check reads as an agent's mark or an injection.
/// Task #43 (team ede724c4, 2026-11-10): the fixtures removed only
/// HGS_SESSION, so a run from an agent session (HGS_SESSION_ID, HGS_RUN_ID)
/// marked the fixture app itself and its proof was refused.
const AGENT_MARKERS: [&str; 7] = [
    "HGS_SESSION",
    "HGS_SESSION_ID",
    "HGS_RUN_ID",
    "LD_PRELOAD",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_FORCE_FLAT_NAMESPACE",
];
/// The app waits for this file before it starts its hgs: the test writes it
/// once the launcher that started the app has exited.
const LAUNCHED: &str = "launched";

fn without_agent_markers(command: &mut Command) -> &mut Command {
    for name in AGENT_MARKERS {
        command.env_remove(name);
    }
    command
}
const GUI: &str = "HGS_TEST_PORTABLE_GUI";

/// Called by `record` between its proof and its last check, in test builds
/// only: optionally sleep, and optionally wait for the test to act first.
pub(super) fn barrier() {
    if let Some(delay) = std::env::var(DELAY).ok().and_then(|ms| ms.parse().ok()) {
        std::thread::sleep(Duration::from_millis(delay));
    }
    if let Ok(dir) = std::env::var(BARRIER) {
        let dir = PathBuf::from(dir);
        fs::write(dir.join("ready"), b"").unwrap();
        let end = Instant::now() + Duration::from_secs(30);
        while !dir.join("go").exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn run_role(image: &Path, role: &str) -> Command {
    let mut command = Command::new(image);
    command
        .args([
            "--exact",
            "desktop::portable_frontend::tests::creation_role",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, role);
    command
}

/// A child role of `only_the_app_s_own_install_outside_an_agent_tree_records_a_binding`;
/// a plain test run does nothing here.
#[test]
fn creation_role() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let own = std::env::current_exe().unwrap();
    match role.as_str() {
        // The app: start the hgs beside it, as its staging step does, without
        // passing on an agent marker of its own.
        // The launcher: start the app and exit at once, so the app's ancestry
        // ends at an exited parent, as an app the person started from
        // Explorer does, whoever started this test run (task #43).
        "launch" => {
            run_role(&own, "app").spawn().unwrap();
        }
        "app" => {
            let mode = std::env::var(MODE).unwrap();
            let home = PathBuf::from(std::env::var(HOME).unwrap());
            let end = Instant::now() + Duration::from_secs(60);
            while !home.join(LAUNCHED).exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(20));
            }
            let mut install = run_role(&own.with_file_name(staging::EXECUTABLE), "install");
            without_agent_markers(&mut install)
                .env_remove(BARRIER)
                .env_remove(DELAY);
            let barrier = home.join("barrier");
            match mode.as_str() {
                "late-marker" => {
                    fs::create_dir_all(&barrier).unwrap();
                    install.env(BARRIER, &barrier);
                }
                // Past the original deadline of the whole record.
                "delay" => {
                    install.env(DELAY, (RECORD_TIME.as_millis() + 1000).to_string());
                }
                _ => {}
            }
            let mut child = install.spawn().unwrap();
            if mode == "late-marker" {
                let end = Instant::now() + Duration::from_secs(60);
                while !barrier.join("ready").exists() && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(20));
                }
                // The app becomes agent-marked after the proof, before the last check.
                std::env::set_var(AGENT_MARKER, "late-agent-marker-fixture");
                fs::write(barrier.join("go"), b"").unwrap();
            }
            let status = child.wait().unwrap();
            let report = PathBuf::from(std::env::var(REPORT).unwrap());
            fs::write(report.with_extension("app"), if status.success() { "ok".to_owned() } else { format!("install failed: {status}") }).unwrap();
        }
        // A fixture GUI: run until its quit file appears.
        "gui" => {
            let folder = PathBuf::from(std::env::var(HOME).unwrap());
            fs::write(folder.join("ready"), b"").unwrap();
            let end = Instant::now() + Duration::from_secs(120);
            while !folder.join("quit").exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        // A serving process: materialize a scope from the given binding and
        // keep it until told to finish.
        "scope" => {
            let dir = PathBuf::from(std::env::var(HOME).unwrap());
            let gui = PathBuf::from(std::env::var(REPORT).unwrap());
            let scope = materialize().expect("serving scope beside its own CLI");
            let launch = scope.closed(&gui, Instant::now() + DIGEST_TIME);
            assert!(launch.is_some(), "the serving scope admits the bound app");
            drop(launch);
            fs::write(dir.join("scope-ready"), b"").unwrap();
            let end = Instant::now() + Duration::from_secs(120);
            while !dir.join("scope-done").exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(scope);
        }
        // The app's own staging step: the real install with its binder.
        // A process checking whether it serves the bound app at GUI.
        "serve-check" => {
            let report = PathBuf::from(std::env::var(REPORT).unwrap());
            let gui = PathBuf::from(std::env::var(GUI).unwrap());
            let outcome = match materialize() {
                None => "no-scope",
                Some(scope) => match scope.closed(&gui, Instant::now() + DIGEST_TIME) {
                    Some(_) => "admitted",
                    None => "refused",
                },
            };
            fs::write(&report, outcome).unwrap();
        }
        "install" => {
            let home = std::env::var(HOME).unwrap();
            let report = PathBuf::from(std::env::var(REPORT).unwrap());
            let layout = Layout::new(&home, &Path::new(&home).join("config"));
            let outcome = match staging::install_bound(&layout, &own, "1.0.0", record) {
                Ok(_) => match staging::read_frontend(&layout.images) {
                    Some(binding) => serde_json::to_string(&binding).unwrap(),
                    // Task #43: a refused proof says why.
                    None => format!("none: {}", last_refusal().unwrap_or_else(|| "no reason recorded".into())),
                },
                Err(error) => format!("error: {error}"),
            };
            fs::write(&report, outcome).unwrap();
        }
        other => panic!("unknown role {other}"),
    }
}

/// Start `app` (a copy of this test binary named hgs-tray.exe) in `mode`; it
/// starts the hgs.exe beside it, whose real install may bind it. Returns the
/// install's report: the published binding, "none" or an error.
fn bind_through_app(dir: &Path, app: &Path, home: &Path, mode: &str) -> String {
    let report = dir.join(format!("report-{mode}.txt"));
    let done = report.with_extension("app");
    for stale in [&report, &done, &home.join(LAUNCHED)] {
        let _ = fs::remove_file(stale);
    }
    let _ = fs::remove_dir_all(home.join("barrier"));
    fs::create_dir_all(home).unwrap();
    // The app's ancestry must not reach this test run, which may descend
    // from an agent session or the running GUI (the team worker): a launcher
    // starts it and exits, and only then may the app start its hgs.
    let mut command = run_role(app, "launch");
    without_agent_markers(&mut command).env(MODE, mode).env(HOME, home).env(REPORT, &report);
    if mode == "agent" {
        command.env(AGENT_MARKER, "agent-descended-fixture");
    }
    assert!(command.status().unwrap().success(), "{mode}: the launcher");
    fs::write(home.join(LAUNCHED), b"").unwrap();
    // The delay mode alone outlasts the record's deadline.
    let end = Instant::now() + Duration::from_secs(120);
    while !done.exists() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(done.exists(), "{mode}: the app's install never finished");
    let app_outcome = fs::read_to_string(&done).unwrap();
    let outcome = fs::read_to_string(&report).unwrap_or_else(|_| "no report".into());
    assert_eq!(app_outcome, "ok", "{mode}: {outcome}");
    outcome
}

/// A zip folder with copies of this test binary as its app and its hgs.exe.
fn fixture_zip(dir: &Path) -> PathBuf {
    let zip = dir.join("zip");
    fs::create_dir_all(&zip).unwrap();
    let own = std::env::current_exe().unwrap();
    let app = zip.join(staging::FRONTEND_IMAGE);
    fs::copy(&own, &app).unwrap();
    fs::copy(&own, zip.join(staging::EXECUTABLE)).unwrap();
    app
}

#[test]
fn a_same_path_replacement_is_seen_at_start_and_rebinds_through_the_app_s_own_install() {
    let dir = staging::private_tempdir();
    let app = fixture_zip(dir.path());
    let home = dir.path().join("home");
    let layout = Layout::new(&home.to_string_lossy(), &home.join("config"));
    let outcome = bind_through_app(dir.path(), &app, &home, "plain");
    let old: FrontendBinding = serde_json::from_str(&outcome).unwrap_or_else(|_| panic!("plain: {outcome}"));
    assert_eq!(
        status(&layout).map(|(_, cli, app)| (cli, app)),
        Some((true, true))
    );
    // After a quit, another app at the same path (a new object).
    fs::remove_file(&app).unwrap();
    fs::copy(std::env::current_exe().unwrap(), &app).unwrap();
    let (path, cli, current) = status(&layout).unwrap();
    assert!(path
        .as_os_str()
        .eq_ignore_ascii_case(old.frontend.path.as_os_str()));
    // Same path, unchanged CLI, but not the bound app: the app's start must
    // stage again, and the old record never names the new object.
    assert!(cli && !current);
    assert!(!is(&PinnedImage::open(&app).unwrap(), &old.frontend));
    // The app's own install binds the new object.
    let outcome = bind_through_app(dir.path(), &app, &home, "plain");
    let new: FrontendBinding = serde_json::from_str(&outcome).unwrap_or_else(|_| panic!("rebind: {outcome}"));
    assert!(is(&PinnedImage::open(&app).unwrap(), &new.frontend));
    assert_ne!(new.frontend.file_id, old.frontend.file_id);
    assert_eq!(
        status(&layout).map(|(_, cli, app)| (cli, app)),
        Some((true, true))
    );
}

#[test]
fn only_the_app_s_own_install_outside_an_agent_tree_records_a_binding() {
    let dir = staging::private_tempdir();
    let zip = dir.path().join("zip");
    fs::create_dir(&zip).unwrap();
    let own = std::env::current_exe().unwrap();
    let app = zip.join(staging::FRONTEND_IMAGE);
    fs::copy(&own, &app).unwrap();
    fs::copy(&own, zip.join(staging::EXECUTABLE)).unwrap();
    let home = dir.path().join("home");
    let layout = Layout::new(&home.to_string_lossy(), &home.join("config"));
    // Each refusal names its reason (task #43).
    for (mode, refusal) in [
        ("plain", None),
        ("agent", Some("the ancestry: agent-descended frontend request refused: hop 1 (hgs-tray.exe) has HGS_SESSION in its environment")),
        ("late-marker", Some("the last check: the ancestry: frontend process evidence unavailable")),
        ("delay", Some("the last check: the ancestry: frontend process evidence unavailable")),
    ] {
        let outcome = bind_through_app(dir.path(), &app, &home, mode);
        if refusal.is_none() {
            let binding: FrontendBinding = serde_json::from_str(&outcome).unwrap_or_else(|_| panic!("{mode}: {outcome}"));
            assert!(binding.valid());
            assert!(binding
                .frontend
                .path
                .as_os_str()
                .eq_ignore_ascii_case(PinnedImage::open(&app).unwrap().path().as_os_str()));
            let staged = PinnedImage::open(&layout.cli).unwrap();
            assert!(is(&staged, &binding.cli));
        } else if let Some(refusal) = refusal {
            assert_eq!(outcome, format!("none: {refusal}"), "{mode}: nothing may be published, and the refusal says why");
        }
        // A refused or completed publication leaves no candidate behind.
        assert!(fs::read_dir(&layout.images).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("frontend-")));
    }
}

/// A private home with a published binding for a fixture app, the way the
/// app's own install leaves it.
fn published(dir: &Path) -> (Layout, PathBuf) {
    let home = dir.join("home");
    let layout = Layout::new(&home.to_string_lossy(), &home.join("config"));
    let source = dir.join("source").join(staging::EXECUTABLE);
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"staged CLI fixture").unwrap();
    let gui = dir.join("zip").join(staging::FRONTEND_IMAGE);
    fs::create_dir_all(gui.parent().unwrap()).unwrap();
    fs::write(&gui, ORIGINAL).unwrap();
    let binding = binding_for(&gui);
    staging::install_bound(&layout, &source, "1.0.0", |_, publish| {
        assert!(publish(&binding, &|| true)?);
        Ok(Some(binding.clone()))
    })
    .unwrap();
    assert!(staging::read_frontend(&layout.images).is_some());
    let record = layout_record(&layout);
    (layout, record)
}

fn layout_record(layout: &Layout) -> PathBuf {
    layout.images.join(staging::FRONTEND_BINDING)
}

fn icacls(arguments: &[&std::ffi::OsStr]) {
    let program = Path::new(&std::env::var_os("SystemRoot").unwrap()).join(r"System32\icacls.exe");
    let output = Command::new(program).args(arguments).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn a_binding_that_is_not_a_private_record_of_this_user_is_refused() {
    // Others may read it.
    let dir = staging::private_tempdir();
    let (layout, record) = published(dir.path());
    icacls(&[
        record.as_os_str(),
        "/grant".as_ref(),
        "*S-1-1-0:(R)".as_ref(),
    ]);
    assert!(
        staging::read_frontend(&layout.images).is_none(),
        "permissive ACL"
    );
    icacls(&[
        record.as_os_str(),
        "/remove:g".as_ref(),
        "*S-1-1-0".as_ref(),
    ]);
    assert!(staging::read_frontend(&layout.images).is_some());
    // A second link.
    let other = dir.path().join("second-link.json");
    fs::hard_link(&record, &other).unwrap();
    assert!(
        staging::read_frontend(&layout.images).is_none(),
        "hard link"
    );
    fs::remove_file(&other).unwrap();
    assert!(staging::read_frontend(&layout.images).is_some());
    // A junction in place of a folder of the chain.
    let hgs = layout.images.parent().unwrap().to_owned();
    let moved = dir.path().join("real-hgs");
    fs::rename(&hgs, &moved).unwrap();
    let comspec = Path::new(&std::env::var_os("SystemRoot").unwrap()).join(r"System32\cmd.exe");
    let made = Command::new(comspec)
        .arg("/d")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(&hgs)
        .arg(&moved)
        .output()
        .unwrap();
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stdout)
    );
    assert!(
        staging::read_frontend(&layout.images).is_none(),
        "junction ancestor"
    );
    fs::remove_dir(&hgs).unwrap();
    fs::rename(&moved, &hgs).unwrap();
    assert!(staging::read_frontend(&layout.images).is_some());
    // Another owner (the Administrators group). Only an elevated suite may
    // assign it (task 180: a runner without elevation skips this one check
    // instead of failing the others).
    let program = Path::new(&std::env::var_os("SystemRoot").unwrap()).join(r"System32\icacls.exe");
    let owned = Command::new(program)
        .args([record.as_os_str(), "/setowner".as_ref(), "*S-1-5-32-544".as_ref()])
        .output()
        .unwrap();
    if owned.status.success() {
        assert!(
            staging::read_frontend(&layout.images).is_none(),
            "foreign owner"
        );
    }
}

#[test]
fn an_inherited_untrusted_ace_refuses_the_candidate_before_the_record_exists() {
    let dir = staging::private_tempdir();
    let (layout, record) = published(dir.path());
    fs::remove_file(&record).unwrap();
    // Inherit-only: the folder's own access stays private, new files inherit it.
    icacls(&[
        layout.images.as_os_str(),
        "/grant".as_ref(),
        "*S-1-1-0:(OI)(IO)(R)".as_ref(),
    ]);
    assert!(staging::read_frontend(&layout.images).is_none());
    let gui = dir.path().join("zip").join(staging::FRONTEND_IMAGE);
    let binding = binding_for(&gui);
    let source = dir.path().join("source").join(staging::EXECUTABLE);
    let committed = std::cell::Cell::new(false);
    let outcome = staging::install_bound(&layout, &source, "1.0.0", |_, publish| {
        publish(&binding, &|| {
            committed.set(true);
            true
        })?;
        Ok(Some(binding.clone()))
    });
    assert!(outcome.is_err(), "an unsafe candidate must be refused");
    // Refused before the commit: the record's name was never published.
    assert!(
        !committed.get(),
        "the commit guard must not run for an unsafe candidate"
    );
    assert!(
        !record.exists(),
        "nothing was published under the record's name"
    );
    assert!(fs::read_dir(&layout.images).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("frontend-")));
}

#[test]
fn a_failed_watcher_drops_the_cache_s_proofs_and_a_new_one_takes_over() {
    let dir = staging::private_tempdir();
    let gui = FixtureGui::start(&dir.path().join("zip"));
    let (scope, _, _) = serve(dir.path(), &gui.image);
    let original = gui.original();
    let borrower = scope.running(&original, until()).unwrap();
    assert_eq!(scope.cache.len(), 1);
    // The watcher fails: the cache gives up what it owns at once.
    scope
        .cache
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    scope.cache.signal();
    wait_until("a failed watcher leaves nothing cached", || {
        scope.cache.len() == 0 && !scope.cache.lock().watching
    });
    // The next insertion starts a valid watcher again.
    let again = scope.running(&original, until()).unwrap();
    assert!(!Arc::ptr_eq(&borrower, &again));
    assert!(scope.cache.lock().watching);
    drop(again);
    // A normal quit with no further request: the new watcher lets go.
    let image = gui.image.clone();
    gui.quit();
    wait_until("the new watcher drops the exited GUI's proof", || {
        scope.cache.len() == 0
    });
    // The active borrower still blocks replacement until it settles.
    assert!(fs::copy(std::env::current_exe().unwrap(), &image).is_err());
    drop(borrower);
    fs::remove_file(&image).unwrap();
    fs::write(&image, ORIGINAL).unwrap();
}

#[test]
fn a_running_service_admits_a_same_path_rebind_at_once_and_never_the_old_object() {
    let dir = staging::private_tempdir();
    let zip = dir.path().join("zip");
    let old = FixtureGui::start(&zip);
    let home = dir.path().join("serve-home");
    publish_into(&home, &binding_for(&old.image));
    // A serving scope made before anything changes.
    let scope = scope_over(&home);
    drop(
        scope
            .running(&old.original(), until())
            .expect("the old app"),
    );
    // A normal quit; the old object is kept aside and a new app takes its path.
    let image = old.image.clone();
    old.quit();
    wait_until("the old proof is released", || scope.cache.len() == 0);
    let aside = zip.join("old-app.exe");
    fs::rename(&image, &aside).unwrap();
    let new = FixtureGui::start(&zip);
    assert!(new.image == image);
    // Not admitted under the old record.
    assert!(scope.running(&new.original(), until()).is_none());
    // The app's own rebind (the real chain is covered by
    // a_same_path_replacement_is_seen_at_start_and_rebinds_through_the_app_s_own_install).
    publish_into(&home, &binding_for(&new.image));
    // The same serving scope, no restart: the new app is admitted at once,
    // and the old record's object never again.
    let proof = scope
        .running(&new.original(), until())
        .expect("admitted at once");
    assert!(proof.proves(&new.original()));
    assert!(scope.closed(&aside, until()).is_none());
    assert!(scope.closed(&new.image, until()).is_some());
    drop(proof);
    new.quit();
}

#[test]
fn a_corrupt_or_unsafe_record_during_serving_refuses_without_the_snapshot() {
    let (dir, gui) = fixture();
    let (scope, binding, layout) = serve(dir.path(), &gui);
    let record = layout.images.join(staging::FRONTEND_BINDING);
    assert!(scope.closed(&gui, until()).is_some());
    let good = serde_json::to_vec(&binding).unwrap();
    // Corrupt, though still a private file: refused, the snapshot is not used.
    fs::write(&record, b"{\"schema\":1").unwrap();
    assert!(scope.snapshot().is_some());
    assert!(scope.closed(&gui, until()).is_none());
    fs::write(&record, &good).unwrap();
    assert!(scope.closed(&gui, until()).is_some());
    // Readable by others: refused the same way.
    icacls(&[
        record.as_os_str(),
        "/grant".as_ref(),
        "*S-1-1-0:(R)".as_ref(),
    ]);
    assert!(scope.closed(&gui, until()).is_none());
    icacls(&[
        record.as_os_str(),
        "/remove:g".as_ref(),
        "*S-1-1-0".as_ref(),
    ]);
    assert!(scope.closed(&gui, until()).is_some());
    // Gone: refused too.
    fs::remove_file(&record).unwrap();
    assert!(scope.closed(&gui, until()).is_none());
}

#[test]
fn a_running_service_refuses_a_record_for_another_cli_object() {
    let (dir, gui) = fixture();
    let (scope, binding, _) = serve(dir.path(), &gui);
    assert!(scope.closed(&gui, until()).is_some());
    // A rebinding that names another CLI object, as a changed CLI would.
    let copy = dir.path().join("other-cli.exe");
    fs::copy(std::env::current_exe().unwrap(), &copy).unwrap();
    let other = binding_with(&gui, &PinnedImage::open(&copy).unwrap());
    publish_into(&dir.path().join("serve-home"), &other);
    assert!(scope.closed(&gui, until()).is_none());
    // The snapshot was not replaced by it.
    assert_eq!(scope.snapshot().as_deref(), Some(&binding));
}

#[test]
fn a_warm_proof_never_outlives_its_record() {
    let dir = staging::private_tempdir();
    let gui = FixtureGui::start(&dir.path().join("zip"));
    let (scope, binding, layout) = serve(dir.path(), &gui.image);
    let record = layout.images.join(staging::FRONTEND_BINDING);
    let home = dir.path().join("serve-home");
    let good = fs::read(&record).unwrap();
    let original = gui.original();
    // Warm: the proof is cached and reused while the record still names it.
    let warm = scope.running(&original, until()).unwrap();
    assert!(Arc::ptr_eq(
        &warm,
        &scope.running(&original, until()).unwrap()
    ));
    drop(warm);
    // Each bad record refuses the very next request for the warm GUI, and
    // again after another failed read: nothing falls back to the cache.
    let refused = |why: &str| {
        assert!(scope.running(&original, until()).is_none(), "{why}");
        assert!(scope.closed(&gui.image, until()).is_none(), "{why}");
        assert!(scope.running(&original, until()).is_none(), "{why} (again)");
    };
    let restore = || {
        fs::write(&record, &good).unwrap();
        assert!(scope.running(&original, until()).is_some(), "restored");
    };
    fs::remove_file(&record).unwrap();
    refused("missing record");
    publish_into(&home, &binding);
    assert!(scope.running(&original, until()).is_some());
    fs::write(&record, b"{\"schema\":1").unwrap();
    refused("corrupt record");
    restore();
    icacls(&[
        record.as_os_str(),
        "/grant".as_ref(),
        "*S-1-1-0:(R)".as_ref(),
    ]);
    refused("unsafe record");
    icacls(&[
        record.as_os_str(),
        "/remove:g".as_ref(),
        "*S-1-1-0".as_ref(),
    ]);
    assert!(scope.running(&original, until()).is_some());
    let copy = dir.path().join("other-cli.exe");
    fs::copy(std::env::current_exe().unwrap(), &copy).unwrap();
    publish_into(
        &home,
        &binding_with(&gui.image, &PinnedImage::open(&copy).unwrap()),
    );
    refused("another CLI");
    publish_into(&home, &binding);
    // Another app object B while A's proof is cached and still borrowed: the
    // first request for A after the rebind refuses, with no prior request for B.
    let borrower = scope.running(&original, until()).unwrap();
    let other = dir.path().join("other").join(staging::FRONTEND_IMAGE);
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, ORIGINAL).unwrap();
    publish_into(&home, &binding_for(&other));
    assert!(scope.running(&original, until()).is_none(), "changed app");
    assert!(
        scope.running(&original, until()).is_none(),
        "changed app (again)"
    );
    assert_eq!(
        scope.cache.len(),
        0,
        "proofs for the old record are dropped"
    );
    // B is proven on its own.
    assert!(scope.closed(&other, until()).is_some());
    // The borrower admitted earlier keeps its pin until it settles.
    assert!(borrower.current());
    let image = gui.image.clone();
    gui.quit();
    assert!(fs::copy(std::env::current_exe().unwrap(), &image).is_err());
    drop(borrower);
    fs::remove_file(&image).unwrap();
}

#[test]
fn this_test_binary_outside_dot_local_bin_has_no_portable_scope() {
    // Built under target\..., not <dir>\.local\bin\hgs.exe: no scope, and no
    // fallback to HOME or the profile.
    assert!(materialize().is_none());
}

#[test]
fn a_serving_process_finds_the_record_beside_its_own_cli_whatever_its_home() {
    let dir = staging::private_tempdir();
    let home = dir.path().join("fixture-home");
    let layout = Layout::new(&home.to_string_lossy(), &home.join("config"));
    fs::create_dir_all(layout.cli.parent().unwrap()).unwrap();
    fs::copy(std::env::current_exe().unwrap(), &layout.cli).unwrap();
    let (_gui_dir, gui) = fixture();
    let binding = binding_with(&gui, &PinnedImage::open(&layout.cli).unwrap());
    staging::install_bound(&layout, &layout.cli, "1.0.0", |_, publish| {
        assert!(publish(&binding, &|| true)?);
        Ok(Some(binding.clone()))
    })
    .unwrap();
    let elsewhere = dir.path().join("another-home");
    fs::create_dir_all(&elsewhere).unwrap();
    let check = |cli: &Path, name: &str| {
        let report = dir.path().join(format!("{name}.txt"));
        let status = run_role(cli, "serve-check")
            .env(GUI, &gui)
            .env(REPORT, &report)
            .env("HOME", &elsewhere)
            .env_remove(AGENT_MARKER)
            .status()
            .unwrap();
        assert!(status.success(), "{name}");
        fs::read_to_string(&report).unwrap()
    };
    // From <home>\.local\bin, with HOME pointing elsewhere: the bound app is admitted.
    assert_eq!(check(&layout.cli, "beside"), "admitted");
    // The same bytes elsewhere (an installer folder): no portable scope at all.
    let programs = dir.path().join("Programs").join("Zerus");
    fs::create_dir_all(&programs).unwrap();
    let other = programs.join(staging::EXECUTABLE);
    fs::copy(std::env::current_exe().unwrap(), &other).unwrap();
    assert_eq!(check(&other, "programs"), "no-scope");
}
