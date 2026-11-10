//! Private process controls for the SAME fixed helper runner. No fake source
//! can mint the production Theme Scope or an Enter permit.
use super::*;
use fs2::FileExt;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, Write},
    os::unix::fs::PermissionsExt,
    panic::{catch_unwind, AssertUnwindSafe},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
const FIXTURE: &str = "state::claude_theme::custody::private_helper_fixture";

fn run(case: &str) {
    let temp = tempfile::Builder::new()
        .prefix("zerus-theme-")
        .tempdir()
        .unwrap();
    let mut output = tempfile::tempfile().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", temp.path())
        .env("TMPDIR", temp.path())
        .env("HGS_STATE_DIR", temp.path().join("state"))
        .env("ZERUS_THEME_CUSTODY_FIXTURE", case)
        .args([
            "--exact",
            FIXTURE,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .stdin(Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output.try_clone().unwrap())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Theme helper fixture timed out: {case}");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    output.rewind().unwrap();
    let mut log = String::new();
    output.read_to_string(&mut log).unwrap();
    assert!(status.success(), "{case}: {log}");
    assert!(log.contains("1 passed"), "{case}: {log}");
}

#[test]
fn original_helper_resources_settle_before_resident_backend_reuse() {
    run("success");
}
#[test]
fn source_refusal_and_unwind_cannot_install_a_sticky_global_backend_mode() {
    run("refusal");
    run("panic");
}
#[test]
fn deadline_retains_same_root_and_sid_until_original_descendant_eof() {
    run("deadline");
}

struct Done(mpsc::Sender<()>);
impl Drop for Done {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
struct Source {
    _root: storage::Guard,
    _input: storage::Guard,
    until: Instant,
    checks: AtomicUsize,
    case: String,
    _done: Done,
}
impl helper::Source for Source {
    fn check(&self) -> Result<()> {
        let count = self.checks.fetch_add(1, Ordering::SeqCst) + 1;
        if self.case == "panic" && count == 3 {
            panic!("private post-spawn check unwind");
        }
        if self.case == "refusal" {
            return Err("private original-source refusal".into());
        }
        helper::boundary(self.until)
    }
    fn until(&self) -> Instant {
        self.until
    }
    fn offer(&self, call: &helper::Call) -> Result<()> {
        if matches!(call,helper::Call::Capture(pane) if pane=="%1") {
            Ok(())
        } else {
            Err("Only the private read is supported".into())
        }
    }
}
fn lock(path: &Path) -> storage::Guard {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    storage::Guard::exclusive(file).unwrap()
}
fn held(file: &File) -> bool {
    match file.try_lock_exclusive() {
        Ok(()) => {
            FileExt::unlock(file).unwrap();
            false
        }
        Err(error) if crate::platform::lock_contended(&error) => true,
        Err(error) => panic!("private contention check: {error}"),
    }
}

#[test]
#[ignore = "private subprocess fixture; invoked by the three public controls"]
fn private_helper_fixture() {
    let case = std::env::var("ZERUS_THEME_CUSTODY_FIXTURE").expect("explicit private control");
    assert!(["success", "refusal", "panic", "deadline"].contains(&case.as_str()));
    let home = crate::platform::canonical(home()).unwrap();
    let bin = home.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let script = bin.join("tmux");
    let mut file = File::create(&script).unwrap();
    let body = if case == "deadline" {
        // Parent exits0 while its original stdout/stderr remain held by ONE
        // own, self-terminating descendant. No group/native process is signalled.
        "#!/bin/sh\necho $$ > \"$HOME/helper-pid\"\n(sleep 0.6) &\nexit 0\n"
    } else {
        "#!/bin/sh\ncat >/dev/null\nprintf ordinary\n"
    };
    file.write_all(body.as_bytes()).unwrap();
    drop(file);
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&old_path));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    let root_path = home.join("root-lock");
    let sid_path = home.join("sid-lock");
    let root = lock(&root_path);
    let sid = lock(&sid_path);
    let root_probe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&root_path)
        .unwrap();
    let sid_probe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&sid_path)
        .unwrap();
    let root_id = crate::platform::FileIdentity::of(&root_probe).unwrap();
    let sid_id = crate::platform::FileIdentity::of(&sid_probe).unwrap();
    let (send, done) = mpsc::channel();
    let source = Arc::new(Source {
        _root: root,
        _input: sid,
        until: Instant::now() + Duration::from_millis(if case == "deadline" { 200 } else { 2000 }),
        checks: AtomicUsize::new(0),
        case: case.clone(),
        _done: Done(send),
    });
    let result = catch_unwind(AssertUnwindSafe(|| {
        helper::run(&source, helper::Call::Capture("%1".into()))
    }));
    match case.as_str() {
        "success" => assert_eq!(result.unwrap().unwrap().stdout, b"ordinary"),
        "panic" => assert!(result.is_err()),
        _ => assert!(result.unwrap().is_err()),
    }
    assert!(held(&root_probe) && held(&sid_probe));
    drop(source);
    if case == "deadline" {
        assert!(done.try_recv().is_err());
        assert!(held(&root_probe) && held(&sid_probe));
        #[cfg(target_os = "linux")]
        {
            let pid = std::fs::read_to_string(home.join("helper-pid"))
                .unwrap()
                .trim()
                .parse::<u32>()
                .unwrap();
            // Actual original helper was reaped; descendant stream EOF, not
            // parent exit status, is what still keeps both original locks.
            assert!(!Path::new(&format!("/proc/{pid}")).exists());
        }
    }
    done.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(!held(&root_probe) && !held(&sid_probe));
    assert_eq!(
        crate::platform::FileIdentity::of(&root_probe).unwrap(),
        root_id
    );
    assert_eq!(
        crate::platform::FileIdentity::of(&sid_probe).unwrap(),
        sid_id
    );
    // Replace only this private fixture's shim, then use ordinary backend in
    // the same resident process. No Theme scope or Effort flag can poison it.
    std::fs::write(&script, "#!/bin/sh\nprintf ordinary\n").unwrap();
    assert_eq!(
        backend::current()
            .capture("%1", backend::Capture::Plain)
            .unwrap(),
        b"ordinary"
    );
}
