//! Native Windows controls for the portable frontend binding. Fixture files
//! under temporary folders only; no installed app, task or registration.
use super::*;
use std::{fs, io::Write, process::Command};

/// The original fixture bytes; `CHANGED` has exactly the same length.
const ORIGINAL: &[u8] = b"portable app fixture, original bytes";
const CHANGED: &[u8] = b"portable app fixture, changed! bytes";

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("zip");
    fs::create_dir(&zip).unwrap();
    let gui = zip.join(staging::FRONTEND_IMAGE);
    fs::write(&gui, ORIGINAL).unwrap();
    (dir, gui)
}

/// What the app's own staging would record for `gui` with this process as the
/// staged CLI, taken through the same pins.
fn binding_for(gui: &Path) -> FrontendBinding {
    let cli = PinnedImage::for_process(&own_process()).unwrap();
    let app = PinnedImage::open(gui).unwrap();
    FrontendBinding {
        schema: FRONTEND_SCHEMA,
        frontend: object(&app),
        frontend_sha256: app
            .sha256(DIGEST_BYTES, Instant::now() + DIGEST_TIME)
            .unwrap(),
        cli: object(&cli),
    }
}

#[test]
fn the_recorded_pair_is_proven_once_and_stays_pinned_through_use() {
    let (_dir, gui) = fixture();
    let zip = gui.parent().unwrap().to_owned();
    let binding = binding_for(&gui);
    let portable = prove(&binding).expect("the recorded pair is proven");
    assert!(portable.current());
    assert!(portable
        .gui()
        .as_os_str()
        .eq_ignore_ascii_case(binding.frontend.path.as_os_str()));
    // While the scope holds the pins, neither the object nor a folder above it
    // can be written, replaced, deleted or renamed.
    assert!(fs::OpenOptions::new().write(true).open(&gui).is_err());
    assert!(fs::rename(&gui, zip.join("aside.exe")).is_err());
    assert!(fs::remove_file(&gui).is_err());
    assert!(fs::rename(&zip, zip.with_file_name("moved")).is_err());
    assert!(portable.current());
    drop(portable);
    // Released, the same swap succeeds: the refusals above came from the pins.
    fs::rename(&gui, zip.join("aside.exe")).unwrap();
    fs::rename(zip.join("aside.exe"), &gui).unwrap();
}

#[test]
fn an_edit_that_keeps_the_length_and_write_time_is_refused() {
    let (_dir, gui) = fixture();
    let binding = binding_for(&gui);
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
    assert!(prove(&binding).is_none());
}

#[test]
fn a_swapped_object_is_refused_and_only_the_original_object_returns() {
    let (dir, gui) = fixture();
    let binding = binding_for(&gui);
    let aside = dir.path().join("original.exe");
    // A out, B in: identical bytes in another object are refused.
    fs::rename(&gui, &aside).unwrap();
    fs::write(&gui, ORIGINAL).unwrap();
    assert!(prove(&binding).is_none());
    // A back: the recorded object itself is proven again.
    fs::remove_file(&gui).unwrap();
    fs::rename(&aside, &gui).unwrap();
    assert!(prove(&binding).is_some());
}

#[test]
fn a_binding_for_another_cli_object_is_refused() {
    let (dir, gui) = fixture();
    let mut binding = binding_for(&gui);
    // A byte-identical copy of this process image is another object, as a CLI
    // staged without a new binding would be.
    let copy = dir.path().join(staging::EXECUTABLE);
    fs::copy(std::env::current_exe().unwrap(), &copy).unwrap();
    binding.cli = object(&PinnedImage::open(&copy).unwrap());
    assert!(prove(&binding).is_none());
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
    assert!(record(&layout).is_none());
}

const ROLE: &str = "HGS_TEST_PORTABLE_ROLE";
const HOME: &str = "HGS_TEST_PORTABLE_HOME";
const REPORT: &str = "HGS_TEST_PORTABLE_REPORT";
const AGENT_MARKER: &str = "HGS_SESSION";

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
            let status = run_role(&own.with_file_name(staging::EXECUTABLE), "install")
                .env_remove(AGENT_MARKER)
                .status()
                .unwrap();
            assert!(status.success());
        }
        "install" => {
            let home = std::env::var(HOME).unwrap();
            let layout = Layout::new(&home, &Path::new(&home).join("config"));
            let outcome = match record(&layout) {
                Some(binding) => serde_json::to_string(&binding).unwrap(),
                None => "none".into(),
            };
            fs::write(std::env::var(REPORT).unwrap(), outcome).unwrap();
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
    fs::create_dir_all(layout.cli.parent().unwrap()).unwrap();
    fs::write(&layout.cli, b"staged CLI fixture").unwrap();
    for (agent, expected) in [(false, true), (true, false)] {
        let report = dir.path().join(format!("report-{agent}.txt"));
        let mut command = run_role(&app, "app");
        command.env(HOME, &home).env(REPORT, &report);
        if agent {
            command.env(AGENT_MARKER, "agent-descended-fixture");
        } else {
            command.env_remove(AGENT_MARKER);
        }
        assert!(command.status().unwrap().success());
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
            assert_eq!(outcome, "none", "an agent-descended app must not bind");
        }
    }
}
