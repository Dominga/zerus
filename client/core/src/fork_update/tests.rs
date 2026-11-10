use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";
const THIRD: &str = "abcabcabcabcabcabcabcabcabcabcabcabcabca";

/// A fresh folder under the system temp folder, removed by the caller.
fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let path = std::env::temp_dir().join(format!(
        "zerus-fork-update-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn sha(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn feed_json(build: u64, commit: &str, size: u64, sha256: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({"schema":1,"build":build,"commit":commit,"size":size,"sha256":sha256,
        "url":format!("https://github.com/WhiteWh/zerus/releases/download/whitewh-{build}/zerus-windows-x86_64.zip")}))
    .unwrap()
}

/// Task #28 (team ede724c4): the Qt updater is gone, so the helper no longer
/// has a Qt copy to match; an installed Qt build's own text is a fixture of
/// tests/fork_handoff.rs.
#[test]
fn the_helper_installs_versioned_and_plain_layouts() {
    assert!(helper_script(true).contains("New-Item -ItemType Junction"));
    assert!(helper_script(false).contains("'.previous'") || helper_script(false).contains("$Previous"));
    // Task 114: both helpers take -Quiet (tests only): no banner, and builds
    // start without a window.
    for versioned in [true, false] {
        let script = helper_script(versioned);
        assert!(script.lines().next().unwrap().ends_with(", [switch]$Quiet)"), "{versioned}");
        assert!(script.contains("if ($Quiet) { $PSDefaultParameterValues['Start-Process:WindowStyle'] = 'Hidden' }"));
        let script = script.replace("\r\n", "\n");
        assert!(script.contains("try {\n    if ($Quiet) { throw 'quiet' }\n    Add-Type -AssemblyName System.Windows.Forms"));
    }
}

#[test]
fn the_feed_has_the_qt_updaters_checks() {
    let digest = hex(&sha(b"zip"));
    let feed = parse_feed(&feed_json(7, COMMIT, 3, &digest)).unwrap();
    assert_eq!((feed.build, feed.commit.as_str(), feed.size, feed.sha256), (7, COMMIT, 3, sha(b"zip")));
    let refused = |value: serde_json::Value| parse_feed(&serde_json::to_vec(&value).unwrap()).unwrap_err().english();
    let good = serde_json::from_slice::<serde_json::Value>(&feed_json(7, COMMIT, 3, &digest)).unwrap();
    for (key, value) in [
        ("schema", serde_json::json!(2)),
        ("build", serde_json::json!(0)),
        ("commit", serde_json::json!("ABC")),
        ("url", serde_json::json!("http://github.com/WhiteWh/zerus/releases/download/x.zip")),
        ("url", serde_json::json!("https://github.com/ufna/zerus/releases/download/x.zip")),
        ("size", serde_json::json!(0)),
        ("size", serde_json::json!(ZIP_LIMIT + 1)),
        ("sha256", serde_json::json!("abcd")),
    ] {
        let mut value_set = good.clone();
        value_set[key] = value;
        assert_eq!(refused(value_set), "The fork update feed is not valid.", "{key}");
    }
    assert_eq!(parse_feed(&vec![b' '; FEED_LIMIT + 1]).unwrap_err().english(), "The fork update feed is too large.");
    assert_eq!(parse_feed(b"not json").unwrap_err().english(), "The fork update feed is not valid.");
}

#[test]
fn the_offer_follows_the_build_number_and_the_running_commit() {
    assert_eq!(offer_for(5, "", 6, COMMIT), Offer::Newer);
    assert_eq!(offer_for(5, "", 5, COMMIT), Offer::Current);
    assert_eq!(offer_for(5, "", 4, COMMIT), Offer::Current);
    assert_eq!(offer_for(5, COMMIT, 9, COMMIT), Offer::Current, "the running commit is never offered again");
    assert_eq!(offer_for(0, OTHER, 9, COMMIT), Offer::Replacement);
    assert_eq!(offer_for(0, COMMIT, 9, COMMIT), Offer::Current);
}

#[test]
fn the_build_embeds_its_number_and_only_a_fork_feed() {
    let config = Config::from_build(Some("12"), Some(LATEST_FEED));
    assert_eq!((config.installed_build, config.feed_url.as_str()), (12, LATEST_FEED));
    let elsewhere = Config::from_build(Some(""), Some("https://example.com/feed.json"));
    assert_eq!((elsewhere.installed_build, elsewhere.feed_url.as_str()), (0, ""));
    assert_eq!(Config::from_build(None, None), Config::default());
}

#[test]
fn a_download_is_bounded_and_hashed() {
    let mut out = Vec::new();
    let (size, hash) = receive(&b"abc"[..], &mut out, 3).unwrap();
    assert_eq!((size, hash, out.as_slice()), (3, sha(b"abc"), &b"abc"[..]));
    assert!(receive(&b"abcd"[..], &mut Vec::new(), 3).is_err(), "more than the feed names is refused");
}

/// A portable build folder with its manifest.
fn build_folder(folder: &Path, commit: &str) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::write(folder.join("hgs-tray.exe"), b"gui").unwrap();
    std::fs::write(folder.join("hgs.exe"), b"cli").unwrap();
    std::fs::write(folder.join("build-info.json"), serde_json::to_vec(&serde_json::json!({"source_commit":commit,"desktop":"tauri"})).unwrap()).unwrap();
}

#[test]
fn a_plain_portable_folder_stages_beside_itself() {
    let dir = scratch("plain");
    let install = dir.join("Zerus");
    build_folder(&install, COMMIT);
    let layout = layout_for(&install.join("hgs-tray.exe"));
    assert!(!layout.versioned && !layout.superseded, "{layout:?}");
    assert!(same(&layout.install, &resolved(&install)));
    assert!(same(&layout.stage, &with_suffix(&resolved(&install), ".update")));
    assert!(portable(&layout));
    assert_eq!(source_commit(&install), COMMIT);
    assert!(verify_unpacked(&install, COMMIT) && !verify_unpacked(&install, OTHER));
    std::fs::remove_file(install.join("build-info.json")).unwrap();
    assert!(!portable(&layout_for(&install.join("hgs-tray.exe"))), "only the portable zip has build-info.json");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "mklink /J {link:?}");
}

#[cfg(windows)]
#[test]
fn a_versioned_root_is_found_through_current_or_app_n() {
    let dir = scratch("versioned");
    let app = dir.join("app-100");
    build_folder(&app, COMMIT);
    junction(&dir.join("current"), &app);
    for executable in [dir.join("current").join("hgs-tray.exe"), app.join("hgs-tray.exe")] {
        let layout = layout_for(&executable);
        assert!(layout.versioned && !layout.superseded, "{layout:?}");
        assert!(same(&layout.install, &resolved(&app)), "{layout:?}");
        assert!(same(&layout.root, &resolved(&dir)));
        assert!(same(&layout.stage, &resolved(&dir).join(".update")));
        assert!(portable(&layout));
    }
    // Another build is current: this one stays put and never updates.
    let newer = dir.join("app-101");
    build_folder(&newer, OTHER);
    std::process::Command::new("cmd").args(["/c", "rmdir"]).arg(dir.join("current")).status().unwrap();
    junction(&dir.join("current"), &newer);
    let old = layout_for(&app.join("hgs-tray.exe"));
    assert!(old.superseded && !old.versioned && !portable(&old), "{old:?}");
    std::process::Command::new("cmd").args(["/c", "rmdir"]).arg(dir.join("current")).status().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The unpack step: writes the build a zip would hold.
fn unpack_as(commit: &'static str) -> impl FnOnce(&Path, &Path) -> Result<(), String> {
    move |zip: &Path, folder: &Path| {
        assert!(zip.is_file(), "the zip waits to be unpacked");
        build_folder(&folder.join(TOP), commit);
        Ok(())
    }
}

fn updater(dir: &Path, installed: u64, running: &str) -> ForkUpdater {
    let install = dir.join("Zerus");
    build_folder(&install, running);
    let layout = layout_for(&install.join("hgs-tray.exe"));
    let config = Config { installed_build: installed, feed_url: LATEST_FEED.to_owned() };
    ForkUpdater::new(config, layout.clone(), portable(&layout), source_commit(&install))
}

/// Writes the zip bytes as the app's download does and reports them.
fn fetch(download: &Download, bytes: &[u8]) -> Result<(u64, [u8; 32]), String> {
    let mut file = std::fs::File::create(download.zip()).unwrap();
    receive(bytes, &mut file, download.feed.size)
}

#[test]
fn a_newer_numbered_build_downloads_verifies_and_waits_for_restart() {
    let dir = scratch("newer");
    let mut fork = updater(&dir, 5, OTHER);
    assert!(fork.automatic() && fork.fork_build());
    assert_eq!(fork.view().status.english(), "Fork updates have not been checked.");
    assert_eq!(fork.begin_check(true).as_deref(), Some(LATEST_FEED));
    assert!(fork.begin_check(false).is_none(), "one check at a time");
    let zip = b"zip of build 6";
    let download = fork.feed_read(Ok(feed_json(6, COMMIT, zip.len() as u64, &hex(&sha(zip))))).expect("a newer build downloads");
    assert!(same(&download.folder, &fork.layout().stage));
    assert_eq!(fork.view().status.english(), "Downloading fork build 6…");
    fork.downloaded(&download, fetch(&download, zip), unpack_as(COMMIT));
    assert_eq!((fork.ready_build(), fork.view().busy), (6, false));
    assert_eq!(fork.view().status.english(), "Fork build 6 is ready. Restart Zerus to finish the update.");
    assert!(!download.zip().exists(), "the zip goes once unpacked");
    // A newer build verifies beside the waiting one and only then replaces it.
    fork.begin_check(true).unwrap();
    let next = fork.feed_read(Ok(feed_json(7, THIRD, zip.len() as u64, &hex(&sha(zip))))).unwrap();
    assert!(next.folder.to_string_lossy().ends_with(".update-next"), "{:?}", next.folder);
    // A corrupt download leaves the waiting build as it was.
    fork.downloaded(&next, fetch(&next, b"zip of build X"), unpack_as(THIRD));
    assert_eq!(fork.ready_build(), 6);
    assert_eq!(fork.view().status.english(), "Fork build 7 does not match its published size and SHA-256.");
    assert!(fork.unpacked().join("hgs-tray.exe").is_file());
    fork.begin_check(true).unwrap();
    let next = fork.feed_read(Ok(feed_json(7, THIRD, zip.len() as u64, &hex(&sha(zip))))).unwrap();
    fork.downloaded(&next, fetch(&next, zip), unpack_as(THIRD));
    assert_eq!(fork.ready_build(), 7);
    assert_eq!(source_commit(&fork.unpacked()), THIRD);
    assert!(!next.folder.exists());
    // The same build again is not fetched twice.
    fork.begin_check(true).unwrap();
    assert!(fork.feed_read(Ok(feed_json(7, THIRD, 3, &hex(&sha(b"zip"))))).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_build_that_does_not_match_its_feed_is_refused() {
    let dir = scratch("refused");
    let mut fork = updater(&dir, 5, OTHER);
    let zip = b"zip";
    fork.begin_check(true).unwrap();
    let download = fork.feed_read(Ok(feed_json(6, COMMIT, 3, &hex(&sha(zip))))).unwrap();
    // The zip names another commit: incomplete, nothing waits.
    fork.downloaded(&download, fetch(&download, zip), unpack_as(OTHER));
    assert_eq!((fork.ready_build(), fork.view().status.english().as_str()), (0, "Fork build 6 is incomplete."));
    assert!(!download.folder.exists());
    fork.begin_check(true).unwrap();
    let download = fork.feed_read(Ok(feed_json(6, COMMIT, 3, &hex(&sha(zip))))).unwrap();
    fork.downloaded(&download, fetch(&download, zip), |_: &Path, _: &Path| Err("tar: bad archive".to_owned()));
    assert_eq!(fork.view().status.english(), "Could not unpack fork build 6: tar: bad archive");
    fork.begin_check(true).unwrap();
    let download = fork.feed_read(Ok(feed_json(6, COMMIT, 3, &hex(&sha(zip))))).unwrap();
    fork.downloaded(&download, Err("timed out".into()), unpack_as(COMMIT));
    assert_eq!(fork.view().status.english(), "Fork build 6 did not download: timed out");
    fork.begin_check(true).unwrap();
    assert!(fork.feed_read(Err("HTTP 404".into())).is_none());
    assert_eq!(fork.view().status.english(), "Fork update check failed: HTTP 404");
    // The current build is the newest.
    fork.begin_check(true).unwrap();
    assert!(fork.feed_read(Ok(feed_json(5, COMMIT, 3, &hex(&sha(zip))))).is_none());
    assert_eq!(fork.view().status.english(), "Fork build 5 is the newest.");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unnumbered_build_is_replaced_only_after_the_owner_confirms() {
    let dir = scratch("replacement");
    let mut fork = updater(&dir, 0, OTHER);
    assert!(!fork.fork_build() || fork.view().installed_build == 0);
    fork.begin_check(false).unwrap();
    let zip = b"zip";
    assert!(fork.feed_read(Ok(feed_json(9, COMMIT, 3, &hex(&sha(zip))))).is_none(), "offered, never fetched");
    let view = fork.view();
    assert_eq!((view.offered_build, view.offered_commit.as_str()), (9, COMMIT));
    assert!(view.status.english().starts_with("This build has no fork build number (commit fedcba9876)."), "{}", view.status.english());
    let download = fork.confirm_replacement().expect("the confirmation downloads it");
    fork.downloaded(&download, fetch(&download, zip), unpack_as(COMMIT));
    assert_eq!(fork.view().status.english(), "Fork build 9 is ready to replace this build. Restart Zerus to replace it.");
    assert!(fork.confirm_replacement().is_none(), "a confirmation applies once");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn restart_writes_the_helper_with_the_qt_updaters_arguments() {
    let dir = scratch("restart");
    let mut fork = updater(&dir, 5, OTHER);
    let script = dir.join("helper.ps1");
    assert!(fork.start_install(&script, 1, Path::new("C:/Windows"), &dir).is_err(), "nothing is ready");
    fork.begin_check(true).unwrap();
    let download = fork.feed_read(Ok(feed_json(6, COMMIT, 3, &hex(&sha(b"zip"))))).unwrap();
    fork.downloaded(&download, fetch(&download, b"zip"), unpack_as(COMMIT));
    let command = fork.start_install(&script, 4242, Path::new("C:/Windows"), &dir).unwrap();
    assert!(fork.installing());
    assert_eq!(std::fs::read_to_string(&script).unwrap(), helper_script(false));
    assert!(command.program.ends_with("System32/WindowsPowerShell/v1.0/powershell.exe")
        || command.program.ends_with(r"System32\WindowsPowerShell\v1.0\powershell.exe"));
    let args = command.args.join(" ");
    for part in ["-NoProfile", "-ExecutionPolicy Bypass", "-WindowStyle Hidden", "-ParentPid 4242", "-Build 6", "-Install", "-Previous"] {
        assert!(args.contains(part), "{part}: {args}");
    }
    assert!(args.contains(".previous") && args.contains(TOP), "{args}");
    // The real update shows its banner: the updater never asks for quiet.
    assert!(!command.args.iter().any(|arg| arg.eq_ignore_ascii_case("-Quiet")), "{args}");
    assert!(fork.start_install(&script, 1, Path::new("C:/Windows"), &dir).is_err(), "one install at a time");
    // A versioned root targets app-<N> and its current junction.
    let versioned = Layout { install: dir.join("app-5"), versioned: true, root: dir.clone(), link: dir.join("current"), superseded: false, stage: dir.join(".update") };
    let args = helper_command(&versioned, &script, 1, &dir.join(".update").join(TOP), 6, Path::new("C:/Windows"), &dir).args.join(" ");
    assert!(args.contains("-Root") && args.contains("-Link") && args.contains("app-6") && !args.contains("-Previous"), "{args}");
    let _ = std::fs::remove_dir_all(&dir);
}
