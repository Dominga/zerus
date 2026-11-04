//! Native process/image component controls. No actor authentication, Terminate,
//! installed bundle, private owner state or complete controller qualification.
use super::*;
#[cfg(windows)]
use std::io::Read;
use std::{
    process::Stdio,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use zerus_client_core::{run, Exit, Invocation};
#[cfg(windows)]
const CHILD: &str = "desktop_service::termination_outcome::image::tests::child";
#[cfg(windows)]
const MARKER: &str = "ZERUS_TERMINATION_IMAGE_FIXTURE";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("zerus-image-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("bin")).unwrap();
        #[cfg(windows)]
        let source = std::env::current_exe().unwrap();
        // The hosted debug Tauri test binary exceeds the production image cap.
        // cat is a small native ELF and also proves exact input through EOF.
        #[cfg(target_os = "linux")]
        let source = PathBuf::from("/bin/cat");
        std::fs::copy(source, root.join("bin/fixture.exe")).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("bin/fixture.exe")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[cfg(windows)]
struct Owned(Child);
#[cfg(windows)]
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[cfg(windows)]
#[test]
fn child() {
    if std::env::var_os(MARKER).is_none() {
        return;
    }
    let mut input = Vec::new();
    std::io::stdin().take(1024).read_to_end(&mut input).unwrap();
    assert_eq!(input, b"fixed image fixture input");
    println!("original image fixture completed");
}
fn arguments() -> Vec<String> {
    #[cfg(windows)]
    {
        ["--exact", CHILD, "--nocapture", "--test-threads=1"]
            .map(String::from)
            .to_vec()
    }
    #[cfg(target_os = "linux")]
    {
        Vec::new()
    }
}
fn completed(exit: Exit) -> bool {
    matches!(exit, Exit::Code {code:0, stdout, ..} if {
        #[cfg(windows)]
        { String::from_utf8_lossy(&stdout).contains("original image fixture completed") }
        #[cfg(target_os = "linux")]
        { stdout == b"fixed image fixture input" }
    })
}
fn command(image: &Image) -> Command {
    let mut command = Command::new(image.executable());
    command
        .args(arguments())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.env(MARKER, "1");
    command
}
fn run_after(image: Arc<Image>, after_check: impl FnOnce()) -> Exit {
    let mut command = command(&image);
    image.before_spawn(&mut command).unwrap();
    after_check(); // The reviewer's race: AFTER the final check, before spawn.
    let mut child = command.spawn().unwrap();
    let verified = image.matches_child(&child);
    // Use the actual original Child, never adopt its PID for custody.
    use std::io::Write;
    let mut input = child.stdin.take();
    let written = verified
        && input
            .as_mut()
            .is_some_and(|pipe| pipe.write_all(b"fixed image fixture input").is_ok());
    drop(input);
    let output = child.wait_with_output().unwrap();
    // All assertions follow the actual wait, including on guard mutations.
    assert!(verified && written);
    Exit::Code {
        code: output.status.code().unwrap_or(-1),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}
#[cfg(target_os = "linux")]
#[test]
fn leaf_or_ancestor_replacement_after_the_last_check_cannot_execute_the_replacement() {
    #[cfg(unix)]
    let _process = crate::PROCESS_TREE.read().unwrap();
    for ancestor in [false, true] {
        let f = Fixture::new();
        let image = Arc::new(Image::capture(&f.path()).unwrap());
        let path = f.path();
        let exit = run_after(image, || {
            if ancestor {
                std::fs::rename(f.0.join("bin"), f.0.join("original-bin")).unwrap();
                std::fs::create_dir(f.0.join("bin")).unwrap();
            } else {
                std::fs::rename(&path, f.0.join("bin/original.exe")).unwrap();
            }
            std::fs::copy("/bin/false", &path).unwrap();
        });
        assert!(completed(exit));
    }
}
#[cfg(windows)]
#[test]
fn execution_and_ancestor_pins_exclude_replacement_through_the_original_child() {
    #[cfg(unix)]
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new();
    let image = Arc::new(Image::capture(&f.path()).unwrap());
    let path = f.path();
    let exit = run_after(image, || {
        assert!(std::fs::rename(&path, f.0.join("bin/moved.exe")).is_err());
        assert!(std::fs::rename(f.0.join("bin"), f.0.join("moved-bin")).is_err());
        assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
    });
    assert!(completed(exit));
}
#[cfg(windows)]
#[test]
fn an_unpaired_surrogate_is_never_the_held_loaded_image() {
    #[cfg(unix)]
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new();
    let image = Image::capture(&f.path()).unwrap();
    let surrogate = f.0.join("surrogate.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &surrogate).unwrap();
    let mut untrusted = Command::new(&surrogate);
    untrusted
        .args(arguments())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    assert!(
        image.before_spawn(&mut untrusted).is_err(),
        "wrong program refuses before creation"
    );
    untrusted.env(MARKER, "1");
    let mut child = Owned(untrusted.spawn().unwrap());
    assert!(
        !image.matches_child(&child.0),
        "identical bytes at a different file ID are not the original image"
    );
    drop(child.0.stdin.take());
}
#[test]
fn the_frozen_accounted_runner_executes_the_retained_image_and_keeps_its_input() {
    #[cfg(unix)]
    let _process = crate::PROCESS_TREE.read().unwrap();
    let f = Fixture::new();
    let image = Arc::new(Image::capture(&f.path()).unwrap());
    let environment = run::FrozenEnvironment::capture().unwrap();
    let invocation = Invocation {
        args: arguments(),
        stdin: Some(b"fixed image fixture input".to_vec()),
        timeout: Duration::from_secs(45),
        request_id: None,
    };
    let checked = image.clone();
    let exit = run::run_accounted_frozen(
        image.executable(),
        &invocation,
        &environment,
        64 << 10,
        &AtomicBool::new(false),
        image.clone(),
        move |command| {
            #[cfg(windows)]
            command.env(MARKER, "1");
            checked.before_spawn(command)?;
            let mut child = command.spawn()?;
            if !checked.matches_child(&child) {
                drop(child.stdin.take());
            }
            Ok(child)
        },
    )
    .unwrap();
    assert!(completed(exit));
}
