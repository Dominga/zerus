//! Native Windows controls for the portable frontend binding. Fixture files
//! and fixture processes (copies of this test binary) under temporary folders
//! only; no installed app, task, registration, host or agent.
use super::*;
use std::{fs, io::Write, process::Command, sync::Arc};

/// The original fixture bytes; `CHANGED` has exactly the same length.
const ORIGINAL: &[u8] = b"portable app fixture, original bytes";
const CHANGED: &[u8] = b"portable app fixture, changed! bytes";
/// Generous for tests; production callers pass their own original deadline.
const DIGEST_TIME: Duration = Duration::from_secs(30);

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
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

fn scope_for(gui: &Path) -> PortableScope {
    PortableScope::from_binding(binding_for(gui)).expect("this process is the bound CLI")
}

fn until() -> Instant {
    Instant::now() + DIGEST_TIME
}

#[test]
fn a_closed_launch_proof_keeps_its_own_pin_until_released() {
    let (_dir, gui) = fixture();
    let zip = gui.parent().unwrap().to_owned();
    let scope = scope_for(&gui);
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
    let (_dir, gui) = fixture();
    let scope = scope_for(&gui);
    let written = fs::metadata(&gui).unwrap().modified().unwrap();
    {
        let mut file = fs::OpenOptions::new().write(true).open(&gui).unwrap();
        file.write_all(CHANGED).unwrap();
        file.set_modified(written).unwrap();
    }
    // The same object, length and write time: only the digest differs.
    let pinned = PinnedImage::open(&gui).unwrap();
    assert!(is(&pinned, &scope.binding.frontend));
    drop(pinned);
    assert!(scope.closed(&gui, until()).is_none());
}

#[test]
fn a_swapped_object_is_refused_and_only_the_original_object_returns() {
    let (dir, gui) = fixture();
    let scope = scope_for(&gui);
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
    assert!(PortableScope::from_binding(binding).is_none());
}

#[test]
fn a_missing_or_corrupt_record_proves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_string_lossy().into_owned();
    assert!(materialize(&home).is_none());
    let images = staging::images_below(&home);
    fs::create_dir_all(&images).unwrap();
    fs::write(images.join(staging::FRONTEND_BINDING), b"{\"schema\":1").unwrap();
    assert!(materialize(&home).is_none());
}

#[test]
fn a_process_not_started_by_the_app_beside_it_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let gui = FixtureGui::start(&dir.path().join("zip"));
    let scope = scope_for(&gui.image);
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
    let dir = tempfile::tempdir().unwrap();
    let gui = FixtureGui::start(&dir.path().join("zip"));
    let other = FixtureGui::start(&dir.path().join("other"));
    let scope = scope_for(&gui.image);
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
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let layout = Layout::new(&home.to_string_lossy(), &home.join("config"));
    // A serving process whose own image is the staged stable CLI, with a scope.
    let old_gui = FixtureGui::start(&dir.path().join("zip-1"));
    fs::create_dir_all(layout.cli.parent().unwrap()).unwrap();
    fs::copy(std::env::current_exe().unwrap(), &layout.cli).unwrap();
    let binding = binding_with(&old_gui.image, &PinnedImage::open(&layout.cli).unwrap());
    let request = dir.path().join("service-binding.json");
    fs::write(&request, serde_json::to_vec(&binding).unwrap()).unwrap();
    let mut service = run_role(&layout.cli, "scope")
        .env(HOME, dir.path())
        .env(REPORT, &request)
        .env_remove(AGENT_MARKER)
        .spawn()
        .unwrap();
    wait_until("the serving process materialized its scope", || {
        dir.path().join("scope-ready").exists()
    });
    // This test process holds old-GUI work of its own: a running proof.
    let scope = scope_for(&old_gui.image);
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
    fs::write(dir.path().join("scope-done"), b"").unwrap();
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
        "app" => {
            let mode = std::env::var(MODE).unwrap();
            let mut install = run_role(&own.with_file_name(staging::EXECUTABLE), "install");
            install
                .env_remove(AGENT_MARKER)
                .env_remove(BARRIER)
                .env_remove(DELAY);
            let barrier = std::env::var(HOME)
                .map(|home| Path::new(&home).join("barrier"))
                .unwrap();
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
            assert!(child.wait().unwrap().success());
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
            let binding: FrontendBinding =
                serde_json::from_slice(&fs::read(std::env::var(REPORT).unwrap()).unwrap()).unwrap();
            let scope = PortableScope::from_binding(binding).expect("serving scope");
            fs::write(dir.join("scope-ready"), b"").unwrap();
            let end = Instant::now() + Duration::from_secs(120);
            while !dir.join("scope-done").exists() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(20));
            }
            drop(scope);
        }
        // The app's own staging step: the real install with its binder.
        "install" => {
            let home = std::env::var(HOME).unwrap();
            let report = PathBuf::from(std::env::var(REPORT).unwrap());
            let layout = Layout::new(&home, &Path::new(&home).join("config"));
            let outcome = match staging::install_bound(&layout, &own, "1.0.0", record) {
                Ok(_) => match staging::read_frontend(&layout.images) {
                    Some(binding) => serde_json::to_string(&binding).unwrap(),
                    None => "none".into(),
                },
                Err(error) => format!("error: {error}"),
            };
            fs::write(&report, outcome).unwrap();
        }
        other => panic!("unknown role {other}"),
    }
}

#[test]
fn only_the_app_s_own_install_outside_an_agent_tree_records_a_binding() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("zip");
    fs::create_dir(&zip).unwrap();
    let own = std::env::current_exe().unwrap();
    let app = zip.join(staging::FRONTEND_IMAGE);
    fs::copy(&own, &app).unwrap();
    fs::copy(&own, zip.join(staging::EXECUTABLE)).unwrap();
    let home = dir.path().join("home");
    let layout = Layout::new(&home.to_string_lossy(), &home.join("config"));
    for (mode, expected) in [
        ("plain", true),
        ("agent", false),
        ("late-marker", false),
        ("delay", false),
    ] {
        let report = dir.path().join(format!("report-{mode}.txt"));
        let _ = fs::remove_dir_all(home.join("barrier"));
        let mut command = run_role(&app, "app");
        command
            .env(MODE, mode)
            .env(HOME, &home)
            .env(REPORT, &report);
        if mode == "agent" {
            command.env(AGENT_MARKER, "agent-descended-fixture");
        } else {
            command.env_remove(AGENT_MARKER);
        }
        assert!(command.status().unwrap().success(), "{mode}");
        let outcome = fs::read_to_string(&report).unwrap();
        if expected {
            let binding: FrontendBinding = serde_json::from_str(&outcome).unwrap();
            assert!(binding.valid());
            assert!(binding
                .frontend
                .path
                .as_os_str()
                .eq_ignore_ascii_case(PinnedImage::open(&app).unwrap().path().as_os_str()));
            let staged = PinnedImage::open(&layout.cli).unwrap();
            assert!(is(&staged, &binding.cli));
        } else {
            assert_eq!(outcome, "none", "{mode}: nothing may be published");
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
    let dir = tempfile::tempdir().unwrap();
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
    // Another owner (the Administrators group, which this elevated suite may assign).
    icacls(&[
        record.as_os_str(),
        "/setowner".as_ref(),
        "*S-1-5-32-544".as_ref(),
    ]);
    assert!(
        staging::read_frontend(&layout.images).is_none(),
        "foreign owner"
    );
}

#[test]
fn an_inherited_untrusted_ace_refuses_the_candidate_before_the_record_exists() {
    let dir = tempfile::tempdir().unwrap();
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
    let outcome = staging::install_bound(&layout, &source, "1.0.0", |_, publish| {
        publish(&binding, &|| true)?;
        Ok(Some(binding.clone()))
    });
    assert!(outcome.is_err(), "an unsafe candidate must be refused");
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
    let dir = tempfile::tempdir().unwrap();
    let gui = FixtureGui::start(&dir.path().join("zip"));
    let scope = scope_for(&gui.image);
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
