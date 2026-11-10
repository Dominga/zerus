use super::*;

/// The lead's run of 2026-11-10: the command names the target folder, and
/// cargo names the test program it runs from there.
#[test]
fn the_build_folders_come_from_the_folder_the_command_and_the_programs_it_ran() {
    let cwd = Path::new(if cfg!(windows) { r"D:\work\line" } else { "/work/line" });
    let command = "set TMP=D:/work/line/.tmp-tests&& set CARGO_TARGET_DIR=D:/work/main/target&& cargo test --bin hgs -- one 2>&1";
    let output = "     Running unittests src\\main.rs (D:/work/main/target\\debug\\deps\\hgs-290eb8275e13a9c3.exe)\n\
                  error: failed to run custom build command for `zerus-next`\n\
                  Caused by:\n  process didn't exit successfully: `D:\\work\\main\\target\\debug\\build\\zerus-next-1\\build-script-build` (exit code: 101)";
    let folders = build_folders(cwd, command, output);
    assert_eq!(folders[0], cwd);
    assert_eq!(folders[1], cwd.join("target"));
    assert!(folders.contains(&PathBuf::from("D:/work/main/target")), "{folders:?}");
    assert!(folders.contains(&PathBuf::from("D:/work/main/target\\debug\\deps\\hgs-290eb8275e13a9c3.exe")), "{folders:?}");
    assert!(folders.contains(&PathBuf::from("D:\\work\\main\\target\\debug\\build\\zerus-next-1\\build-script-build")), "{folders:?}");
    let relative = build_folders(cwd, "cargo test --locked --target-dir ../../target/client-core x", "");
    assert!(relative.contains(&cwd.join("../../target/client-core")), "{relative:?}");
    let joined = build_folders(cwd, "cargo test --target-dir=out", "");
    assert!(joined.contains(&cwd.join("out")), "{joined:?}");
}

/// A Low folder with an ordinary TEMP is refused once, naming the folder that
/// carries the label and the owner's step; a Low TEMP (a DeepSeek member's own
/// run folder) is writable at Low and refuses nothing.
#[test]
fn a_low_build_folder_with_an_ordinary_temp_is_one_environment_failure() {
    let dir = tempfile::tempdir().unwrap();
    let checkout = dir.path().join("checkout");
    let temp = dir.path().join("temp");
    std::fs::create_dir_all(checkout.join("target").join("debug")).unwrap();
    std::fs::create_dir_all(&temp).unwrap();
    set_test_low(Some(vec![checkout.clone()]));
    let build = vec![checkout.join("target").join("debug")];
    let text = preflight(&build, Some(&temp)).expect("an environment failure");
    assert!(text.starts_with(MARKER), "{text}");
    assert!(text.contains(&format!("{} carries Windows' Low integrity label", checkout.display())), "{text}");
    assert!(text.contains(&format!("icacls \"{}\" /setintegritylevel \"(OI)(CI)M\" /T /C /Q", checkout.display())), "{text}");
    assert!(text.contains("Nothing ran"), "{text}");
    // The run folder inside the Low checkout is Low as well: no failure.
    assert_eq!(preflight(&build, Some(&checkout.join(".zerus-team").join("runs").join("x").join("temp"))), None);
    // An ordinary build folder: no failure.
    assert_eq!(preflight(&[dir.path().join("other")], Some(&temp)), None);
    // After a run: only output that was refused counts.
    assert_eq!(diagnose("test a ... FAILED\nassertion failed", &build, Some(&temp)), None);
    let after = diagnose("Os { code: 5, kind: PermissionDenied, message: \"Access is denied.\" }", &build, Some(&temp)).unwrap();
    assert!(after.contains("not the tests'"), "{after}");
    set_test_low(None);
}

#[test]
fn the_run_temp_is_the_runs_own_temp_variable() {
    let env = vec![("PATH".to_owned(), "x".to_owned()), ("TEMP".to_owned(), "D:/run/temp".to_owned())];
    assert_eq!(run_temp(&env), PathBuf::from("D:/run/temp"));
    assert_eq!(run_temp(&[]), std::env::temp_dir());
}

/// The real labels (Windows): a folder labeled Low the way a DeepSeek sandbox
/// labels its workspace is found from below, a program copied into it starts
/// at Low and is refused in an ordinary TEMP - the failure of 2026-11-10 - and
/// the owner's step (the icacls line the message names) makes it pass.
#[cfg(windows)]
#[test]
fn a_real_low_label_is_found_and_the_owners_step_clears_it() {
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    // Setting a label needs WRITE_OWNER, which a folder on a second drive
    // (a member's run TEMP inside its worktree) inherits only as Modify: the
    // test's own folder gets this user's full control first, as Zerus gives
    // the folders it creates.
    crate::platform::grant_full_control(dir.path()).unwrap();
    let checkout = dir.path().join("checkout");
    let temp = dir.path().join("temp");
    std::fs::create_dir_all(&checkout).unwrap();
    std::fs::create_dir_all(&temp).unwrap();
    let icacls = |path: &Path, level: &str| {
        Command::new("icacls")
            .arg(path)
            .args(["/setintegritylevel", level, "/T", "/C", "/Q"])
            .stdin(Stdio::null())
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(icacls(&checkout, "(OI)(CI)L"), "the test labels its own folder");
    // icacls /C answers 0 even when it labeled nothing.
    assert_eq!(crate::platform::integrity_label(&checkout).unwrap(), Some(0x1000), "the test's folder is labeled Low");
    let deps = checkout.join("target").join("debug").join("deps");
    std::fs::create_dir_all(&deps).unwrap();
    assert_eq!(crate::platform::integrity_label(&deps).unwrap(), Some(0x1000), "new folders inherit the Low label");
    assert_eq!(crate::platform::integrity_label(&temp).unwrap(), None, "an ordinary folder has no label");
    assert_eq!(labeled_root(&deps.join("hgs-1.exe")), Some(checkout.clone()));
    let text = preflight(std::slice::from_ref(&deps), Some(&temp)).expect("an environment failure");
    assert!(text.contains(&format!("icacls \"{}\"", checkout.display())), "{text}");
    // A program copied into the Low folder starts at Low and cannot create a
    // folder in the ordinary TEMP.
    let system = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let program = deps.join("probe.exe");
    std::fs::copy(system.join("System32").join("cmd.exe"), &program).unwrap();
    let mkdir = |name: &str| {
        Command::new(&program)
            .args(["/d", "/c", "mkdir"])
            .arg(temp.join(name))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        temp.join(name).is_dir()
    };
    // Task #58 (team ede724c4): Windows starts a Low-labeled program at Low only
    // where integrity levels are enforced for its user. On the hosted runner
    // the job runs as the built-in Administrator (SID ...-500, High) with UAC
    // off, so the copy ran at High and made the folder. The program says which
    // level it runs at (whoami /groups names the Low label S-1-16-4096); the
    // refusal is checked only where it can happen.
    let runs_low = Command::new(&program)
        .args(["/d", "/c", "whoami", "/groups"])
        .stdin(Stdio::null())
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains("S-1-16-4096"))
        .unwrap_or(false);
    if runs_low {
        assert!(!mkdir("refused"), "a program labeled Low runs at Low and is refused in a Medium folder");
    } else {
        eprintln!("this computer does not start a Low-labeled program at Low (integrity levels not enforced for this user): the refusal is not checked");
    }
    // Task #48: the owner's step exactly as the message prints it, run in
    // Windows PowerShell. The old unquoted line was refused there ("The term
    // 'OI' is not recognized") and changed nothing.
    let step = owner_step(&checkout.display().to_string());
    assert!(text.contains(&format!("run\n  {step}\n")), "{text}");
    let unquoted = step.replace("\"(OI)(CI)M\"", "(OI)(CI)M");
    let (ran, said) = run_in_powershell(&unquoted);
    assert!(!ran && said.contains("OI"), "the unquoted step fails in PowerShell: {said}");
    assert_eq!(crate::platform::integrity_label(&checkout).unwrap(), Some(0x1000), "and labels nothing");
    let (ran, said) = run_in_powershell(&step);
    assert!(ran, "the printed step runs in PowerShell: {said}");
    assert_eq!(preflight(std::slice::from_ref(&deps), Some(&temp)), None);
    assert!(!runs_low || mkdir("allowed"), "after the step the same program runs at Medium");
}
