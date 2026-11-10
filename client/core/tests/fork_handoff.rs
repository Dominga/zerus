//! WhiteWh fork, task #63 (owner, 2026-11-06): the move from the installed Qt
//! build to the Tauri app, end to end on a real versioned root
//! (`<root>\current` -> `app-<N>`, `build-info.json`):
//!
//! 1. A Tauri build that exits at once: the Qt updater's own helper (taken
//!    from `tray/src/ForkUpdater.cpp`) points `current` back at the Qt build
//!    and starts it.
//! 2. The Qt helper installs the Tauri zip: `current` -> `app-101`, the Qt
//!    build stays in `app-100`.
//! 3. Rollback to Qt by hand works: `current` -> `app-100` starts the Qt build.
//! 4. The Tauri updater (`zerus_client_core::fork_update`) verifies the next
//!    Tauri zip and its helper installs it: `current` -> `app-102`, `app-101`
//!    stays as the rollback copy and the Qt build goes (two builds are kept).
//!
//! The builds are this test binary copied as `hgs-tray.exe`: started with no
//! arguments it runs this one test, which sees its name and acts as the
//! build: it stays (writing its pid) or exits at once, as `stub-mode` says.
//! The helpers run with `-Quiet` (task 114: never passed by the updaters), so
//! no banner shows and every build starts without a window; the test checks
//! that no window of theirs appeared while they ran (~1.5 minutes).
#![cfg(windows)]

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use zerus_client_core::fork_update::{self, Config, ForkUpdater, TOP};

const QT: &str = "1111111111111111111111111111111111111111";
const TAURI_1: &str = "2222222222222222222222222222222222222222";
const TAURI_2: &str = "3333333333333333333333333333333333333333";

/// As a build: stay (and say so) or exit, as the folder's `stub-mode` says.
fn act_as_build() -> bool {
    let exe = std::env::current_exe().unwrap();
    if !exe.file_name().is_some_and(|name| name.eq_ignore_ascii_case("hgs-tray.exe")) {
        return false;
    }
    let folder = exe.parent().unwrap().to_path_buf();
    let mode = std::fs::read_to_string(folder.join("stub-mode")).unwrap_or_default();
    if mode.trim() == "exit" {
        std::process::exit(3);
    }
    // No console window stays open for the build.
    unsafe { windows_sys::Win32::System::Console::FreeConsole() };
    let _ = std::fs::write(folder.join("stub.pid"), std::process::id().to_string());
    std::thread::sleep(Duration::from_secs(120));
    std::process::exit(0);
}

fn system_root() -> PathBuf {
    std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from)
}

fn build(folder: &Path, commit: &str, desktop: &str, mode: &str) {
    std::fs::create_dir_all(folder).unwrap();
    std::fs::copy(std::env::current_exe().unwrap(), folder.join("hgs-tray.exe")).unwrap();
    std::fs::write(folder.join("hgs.exe"), b"cli").unwrap();
    std::fs::write(folder.join("stub-mode"), mode).unwrap();
    let info = serde_json::json!({"product":"Zerus","platform":"Windows","desktop":desktop,"source_commit":commit});
    std::fs::write(folder.join("build-info.json"), serde_json::to_vec_pretty(&info).unwrap()).unwrap();
}

fn junction(link: &Path, target: &Path) {
    if link.exists() || std::fs::symlink_metadata(link).is_ok() {
        assert!(Command::new("cmd").args(["/c", "rmdir"]).arg(link).status().unwrap().success());
    }
    let status = Command::new("cmd").args(["/c", "mklink", "/J"]).arg(link).arg(target).stdout(Stdio::null()).status().unwrap();
    assert!(status.success(), "mklink /J {link:?}");
}

fn current(root: &Path) -> String {
    let target = std::fs::canonicalize(root.join("current")).unwrap();
    target.file_name().unwrap().to_string_lossy().into_owned()
}

/// A pid that no longer runs: the helper's "Zerus quit" is already true.
fn gone_pid() -> u32 {
    let mut child = Command::new("cmd").args(["/c", "exit", "0"]).stdout(Stdio::null()).spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

/// The build running from `folder` (its pid file), waited for briefly.
fn running(folder: &Path) -> Option<u32> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(pid) = std::fs::read_to_string(folder.join("stub.pid")).ok().and_then(|text| text.trim().parse().ok()) {
            return Some(pid);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

fn stop(folder: &Path) {
    if let Some(pid) = std::fs::read_to_string(folder.join("stub.pid")).ok().and_then(|text| text.trim().parse::<u32>().ok()) {
        let _ = Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        // The folder is free once the process is gone.
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = std::fs::remove_file(folder.join("stub.pid"));
}

/// The zip the fork workflow publishes: one top folder.
fn zip(stage: &Path, out: &Path) {
    let status = Command::new(system_root().join("System32").join("tar.exe"))
        .arg("-a").arg("-c").arg("-f").arg(out).arg("-C").arg(stage).arg(TOP)
        .status()
        .unwrap();
    assert!(status.success(), "tar -a -c {out:?}");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn feed(build: u64, commit: &str, zip: &Path) -> Vec<u8> {
    let bytes = std::fs::read(zip).unwrap();
    serde_json::to_vec(&serde_json::json!({"schema":1,"build":build,"commit":commit,"size":bytes.len(),"sha256":hex(&Sha256::digest(&bytes)),
        "url":format!("https://github.com/WhiteWh/zerus/releases/download/whitewh-{build}/zerus-windows-x86_64.zip")}))
    .unwrap()
}

/// Windows tar.exe, as both updaters unpack.
fn unpack(zip: &Path, folder: &Path) -> Result<(), String> {
    let output = Command::new(system_root().join("System32").join("tar.exe")).arg("-xf").arg(zip).arg("-C").arg(folder).output().unwrap();
    output.status.success().then_some(()).ok_or_else(|| String::from_utf8_lossy(&output.stderr).into_owned())
}

/// Stages `zip` as build `number` with the updater rules (size, SHA-256,
/// tar, hgs-tray.exe, hgs.exe, build-info.json naming the commit).
fn stage(updater: &mut ForkUpdater, number: u64, commit: &str, zip: &Path) {
    updater.begin_check(false).expect("a portable installation checks");
    let download = updater.feed_read(Ok(feed(number, commit, zip))).expect("a newer build downloads");
    let mut file = std::fs::File::create(download.zip()).unwrap();
    let received = fork_update::receive(std::fs::File::open(zip).unwrap(), &mut file, download.feed.size);
    drop(file);
    updater.downloaded(&download, received, unpack);
    assert_eq!(updater.ready_build(), number, "{}", updater.view().status.english());
}

/// Visible top-level windows that the helper or a build under `root` shows:
/// the banner ("Zerus update"), any window of the helper or of a build's
/// image, and a console window titled with a build's path (task 114).
fn helper_windows(helper: u32, root: &Path) -> Vec<String> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible};
    unsafe extern "system" fn each(hwnd: HWND, list: LPARAM) -> windows_sys::core::BOOL {
        let list = unsafe { &mut *(list as *mut Vec<(u32, String)>) };
        if unsafe { IsWindowVisible(hwnd) } != 0 {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            let mut title = [0u16; 512];
            let length = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
            list.push((pid, String::from_utf16_lossy(&title[..length.max(0) as usize])));
        }
        1
    }
    let image = |pid: u32| -> String {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return String::new();
        }
        let mut path = [0u16; 1024];
        let mut size = path.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut size) } != 0;
        unsafe { windows_sys::Win32::Foundation::CloseHandle(process) };
        if ok { String::from_utf16_lossy(&path[..size as usize]) } else { String::new() }
    };
    let mut windows: Vec<(u32, String)> = Vec::new();
    unsafe { EnumWindows(Some(each), &mut windows as *mut _ as LPARAM) };
    let root = root.to_string_lossy().to_lowercase();
    windows
        .into_iter()
        .filter(|(pid, title)| {
            *pid == helper
                || title == "Zerus update"
                || title.to_lowercase().contains(&root)
                || image(*pid).to_lowercase().starts_with(&root)
        })
        .map(|(pid, title)| format!("pid {pid}: {title:?}"))
        .collect()
}

/// Runs a helper to its end, as it runs once Zerus quits, but with `-Quiet`:
/// no window of the helper or of the builds it starts may appear.
fn run_helper(program: &Path, args: &[String], root: &Path) -> std::process::Output {
    let started = Instant::now();
    let child = Command::new(program)
        .args(args)
        .arg("-Quiet")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let helper = child.id();
    let waiter = std::thread::spawn(move || child.wait_with_output().unwrap());
    let mut shown = Vec::new();
    while !waiter.is_finished() {
        for window in helper_windows(helper, root) {
            if !shown.contains(&window) {
                shown.push(window);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = waiter.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(240), "the helper finished");
    assert!(shown.is_empty(), "a quiet helper showed windows: {shown:?}");
    output
}

/// The Qt updater's helper text, as `ForkUpdater::startInstall` wrote it.
/// Task #28 (team ede724c4): the Qt sources are gone; an installed Qt build
/// still hands over with this text, kept as fixtures cut from the last
/// tray/src/ForkUpdater.cpp (its versionedHead/installHead, bannerScript and
/// versionedBody/installBody raw strings).
fn qt_helper(versioned: bool) -> String {
    if versioned {
        include_str!("fixtures/qt-fork-helper/versioned.ps1").to_owned()
    } else {
        include_str!("fixtures/qt-fork-helper/plain.ps1").to_owned()
    }
}

/// The Qt build's updater: the same staging rules, its own helper text.
fn qt_install(root: &Path, number: u64, commit: &str, zip: &Path) -> std::process::Output {
    let layout = fork_update::layout_for(&root.join("current").join("hgs-tray.exe"));
    assert!(layout.versioned, "the Qt build runs from a versioned root: {layout:?}");
    let config = Config { installed_build: 100, feed_url: fork_update::LATEST_FEED.to_owned() };
    let mut qt = ForkUpdater::new(config, layout.clone(), fork_update::portable(&layout), fork_update::source_commit(&layout.install));
    stage(&mut qt, number, commit, zip);
    let script = root.join(format!("qt-helper-{number}.ps1"));
    std::fs::write(&script, qt_helper(true)).unwrap();
    let command = fork_update::helper_command(&layout, &script, gone_pid(), &qt.unpacked(), number, &system_root(), &std::env::temp_dir());
    run_helper(&command.program, &command.args, root)
}

#[test]
fn qt_installed_layout_hands_over_to_tauri_and_back() {
    if act_as_build() {
        return;
    }
    let root = std::env::temp_dir().join(format!("zerus-fork-handoff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let work = root.join("work");
    // The installed Qt build: <root>\app-100 with current pointing at it.
    build(&root.join("app-100"), QT, "qt", "stay");
    junction(&root.join("current"), &root.join("app-100"));
    // The Tauri zips: one that exits at once, the next build, the one after.
    let zips = [("bad", TAURI_1, "exit"), ("101", TAURI_1, "stay"), ("102", TAURI_2, "stay")].map(|(name, commit, mode)| {
        let folder = work.join(name);
        build(&folder.join(TOP), commit, "tauri", mode);
        let out = work.join(format!("{name}.zip"));
        zip(&folder, &out);
        out
    });

    // 1. A Tauri build that exits within 15 seconds: current goes back to Qt.
    let output = qt_install(&root, 101, TAURI_1, &zips[0]);
    assert!(!output.status.success(), "the helper reports the failure");
    assert_eq!(current(&root), "app-100", "{}", std::fs::read_to_string(root.join("update.log")).unwrap_or_default());
    assert!(running(&root.join("app-100")).is_some(), "the Qt build runs again");
    stop(&root.join("app-100"));

    // 2. The Qt updater installs the Tauri zip as its next fork build.
    let output = qt_install(&root, 101, TAURI_1, &zips[1]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(current(&root), "app-101");
    assert_eq!(fork_update::source_commit(&root.join("current")), TAURI_1);
    let info: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("current").join("build-info.json")).unwrap()).unwrap();
    assert_eq!(info["desktop"], "tauri");
    assert!(root.join("app-100").join("hgs-tray.exe").is_file(), "the Qt build stays as the rollback copy");
    assert!(!root.join(".update").exists(), "the stage is cleared");
    assert!(running(&root.join("app-101")).is_some(), "the Tauri build runs");

    // The Tauri build finds the layout Qt left: versioned, app-101 current.
    let layout = fork_update::layout_for(&root.join("current").join("hgs-tray.exe"));
    assert!(layout.versioned && fork_update::portable(&layout), "{layout:?}");
    assert!(layout.install.ends_with("app-101"), "{layout:?}");
    stop(&root.join("app-101"));

    // 3. Rollback to the Qt build by hand: current back to app-100 starts it.
    junction(&root.join("current"), &root.join("app-100"));
    let mut rollback_child = {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: the stand-in build never shows a console.
        Command::new(root.join("current").join("hgs-tray.exe")).creation_flags(0x0800_0000)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap()
    };
    assert!(running(&root.join("app-100")).is_some(), "the Qt build runs after the rollback");
    stop(&root.join("app-100"));
    rollback_child.wait().expect("the stopped rollback stand-in can be reaped");
    junction(&root.join("current"), &root.join("app-101"));

    // 4. The Tauri build updates itself to the following fork build.
    let layout = fork_update::layout_for(&root.join("current").join("hgs-tray.exe"));
    let config = Config { installed_build: 101, feed_url: fork_update::LATEST_FEED.to_owned() };
    let mut tauri = ForkUpdater::new(config, layout.clone(), fork_update::portable(&layout), fork_update::source_commit(&layout.install));
    assert_eq!(tauri.view().running_commit, TAURI_1);
    stage(&mut tauri, 102, TAURI_2, &zips[2]);
    let script = root.join("tauri-helper-102.ps1");
    let command = tauri.start_install(&script, gone_pid(), &system_root(), &std::env::temp_dir()).unwrap();
    assert_eq!(std::fs::read_to_string(&script).unwrap(), qt_helper(true), "the Tauri app writes the Qt helper");
    assert!(!command.args.iter().any(|arg| arg.eq_ignore_ascii_case("-Quiet")), "the updater never asks for quiet");
    let output = run_helper(&command.program, &command.args, &root);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(current(&root), "app-102");
    assert_eq!(fork_update::source_commit(&root.join("current")), TAURI_2);
    assert!(root.join("app-101").join("hgs-tray.exe").is_file(), "the previous Tauri build is the rollback copy");
    assert!(!root.join("app-100").exists(), "two builds are kept: the Qt build goes after the second Tauri update");
    assert!(running(&root.join("app-102")).is_some());
    stop(&root.join("app-102"));

    let _ = Command::new("cmd").args(["/c", "rmdir"]).arg(root.join("current")).status();
    let _ = std::fs::remove_dir_all(&root);
}
