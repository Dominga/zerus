//! Isolated native task/audit/code controls. Apple system helpers are real
//! native positives for the primitive, NOT a genuine Zerus bundle/permit.
use super::*;
use objc2::rc::Retained;
use objc2_foundation::{NSDictionary, NSString};
use std::{
    cell::{Cell, RefCell},
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    process::{Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

type Hook = Box<dyn FnOnce(&mut Child)>;
unsafe extern "C" {
    fn mach_port_type(task: u32, name: u32, kind: *mut u32) -> i32;
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecCodeAttributeArchitecture: *const c_void;
    fn SecStaticCodeCreateWithPathAndAttributes(
        path: *const c_void,
        flags: u32,
        attributes: *const c_void,
        code: *mut *mut c_void,
    ) -> i32;
}
thread_local! {
    static AFTER: RefCell<Option<Hook>> = const { RefCell::new(None) };
    static AFTER_REVALIDATION: RefCell<Option<Hook>> = const { RefCell::new(None) };
}
pub(super) fn after_revalidation(child: &mut Child) {
    if let Some(hook) = AFTER_REVALIDATION.with_borrow_mut(Option::take) {
        hook(child);
    }
}
pub(super) fn after_first_check(child: &mut Child) {
    if let Some(hook) = AFTER.with_borrow_mut(Option::take) {
        hook(child);
    }
}
struct HookReset;
impl Drop for HookReset {
    fn drop(&mut self) {
        AFTER.with_borrow_mut(|hook| *hook = None);
        AFTER_REVALIDATION.with_borrow_mut(|hook| *hook = None);
    }
}
fn hook(hook: impl FnOnce(&mut Child) + 'static) -> HookReset {
    AFTER.with_borrow_mut(|slot| assert!(slot.replace(Box::new(hook)).is_none()));
    HookReset
}

struct Owned(Child);
impl Owned {
    fn finish(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "original fixture exit: {status}");
                return;
            }
            assert!(Instant::now() < deadline, "original fixture did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn spawn(root: &Path, executable: &str, args: &[&str]) -> Owned {
    let mut command = Command::new(executable);
    command.args(args).env_clear().env("PATH", "/usr/bin:/bin");
    for key in [
        "HOME",
        "TMPDIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "HGS_CONFIG_DIR",
        "HGS_STATE_DIR",
    ] {
        command.env(key, root);
    }
    let child = Owned(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    // Bounded native readiness reads; no reader thread or process can leak.
    let fd = child.0.stdout.as_ref().unwrap().as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    child
}

fn read_exact(child: &mut Child, expected: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut offset = 0;
    let mut output = vec![0; expected.len()];
    while offset < output.len() {
        match child.stdout.as_mut().unwrap().read(&mut output[offset..]) {
            Ok(0) => panic!("original fixture closed output early"),
            Ok(count) => offset += count,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "original fixture readiness deadline"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("original fixture output: {error}"),
        }
    }
    assert_eq!(output, expected);
}

fn expected(path: &str) -> Signature {
    // Native static CDHash of one fixed Apple fixture, not caller input or a
    // fake positive Selection. The production path cannot call this constructor.
    autoreleasepool(|_| {
        let bytes = path.as_bytes();
        let url = unsafe {
            Cf::checked(
                0,
                CFURLCreateFromFileSystemRepresentation(
                    std::ptr::null(),
                    bytes.as_ptr(),
                    bytes.len() as isize,
                    0,
                ),
                CFURLGetTypeID(),
            )
        }
        .unwrap();
        // A universal Apple tool's unqualified static selection can differ
        // from the native slice actually executed on macOS 27. Pin this
        // fixture's compile-target architecture before obtaining its CDHash;
        // do not derive an expectation from the observed child or relax the
        // exact anchor/hash, path, audit or original-child checks below.
        let architecture = NSString::from_str(match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x86_64",
            other => panic!("unsupported native fixture architecture: {other}"),
        });
        // SAFETY: Security's static CFString is toll-free bridged to NSString.
        let key = unsafe { (kSecCodeAttributeArchitecture as *const NSString).as_ref() }
            .expect("native architecture attribute");
        let attributes = NSDictionary::from_slices(&[key], &[&*architecture]);
        let mut out = std::ptr::null_mut();
        let code = unsafe {
            let status = SecStaticCodeCreateWithPathAndAttributes(
                url.ptr(),
                0,
                Retained::as_ptr(&attributes).cast(),
                &mut out,
            );
            Cf::checked(status, out, SecStaticCodeGetTypeID())
        }
        .unwrap();
        let info = information(code.ptr()).unwrap();
        let unique = info_unique(&info).unwrap();
        let hash = unique
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let requirement = format!("anchor apple and cdhash H\"{hash}\"");
        let parsed = super::super::super::requirement(&requirement).unwrap();
        assert_eq!(
            unsafe { SecStaticCodeCheckValidity(code.ptr(), 0, parsed.as_ptr()) },
            0
        );
        Signature {
            identifier: "fixture only; no genuine selection".into(),
            team: String::new(),
            executable: info_path(&info).unwrap(),
            unique,
            requirement,
        }
    })
}

#[test]
fn native_task_token_matches_only_the_same_owned_child_and_right_is_released() {
    let tree = super::super::tests::Tree::new();
    let mut first = spawn(&tree.0, "/bin/cat", &[]);
    let second = spawn(&tree.0, "/bin/cat", &[]);
    assert!(birth(&mut first.0).is_ok());
    let task = TaskName::acquire(&first.0).unwrap();
    assert!(task.audit(&first.0).is_ok());
    assert!(matches!(task.audit(&second.0), Err(Refusal::Child)));
    let name = task.0;
    drop(task);
    let mut kind = 0;
    assert_ne!(unsafe { mach_port_type(own_task(), name, &mut kind) }, 0);
}

#[test]
fn native_matching_live_code_keeps_input_until_the_caller_sends_and_reaps() {
    let tree = super::super::tests::Tree::new();
    let mut child = spawn(&tree.0, "/bin/cat", &[]);
    let original = child.0.id();
    let mut validations = 0;
    let identity = observe_identity(&mut child.0, &expected("/bin/cat"), || {
        validations += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(validations, 2);
    assert_eq!(identity.pid(), original);
    assert_eq!(identity.parent(), std::process::id());
    assert_eq!(identity.birth(), birth(&mut child.0).unwrap().binding());
    let task = TaskName::acquire(&child.0).unwrap();
    assert_eq!(
        identity.audit_token(),
        &task.audit(&child.0).unwrap().values
    );
    assert_eq!(child.0.id(), original);
    child
        .0
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"public fixture bytes\n")
        .unwrap();
    drop(child.0.stdin.take());
    read_exact(&mut child.0, b"public fixture bytes\n");
    child.finish();
}

#[test]
fn native_wrong_requirement_or_cdhash_closes_input_without_sending_private_bytes() {
    let tree = super::super::tests::Tree::new();
    for wrong_requirement in [true, false] {
        let mut child = spawn(&tree.0, "/bin/cat", &[]);
        let original = child.0.id();
        let mut signature = expected("/bin/cat");
        if wrong_requirement {
            signature.requirement =
                "anchor apple and identifier \"org.example.wrong-helper\"".into();
        } else {
            signature.unique[0] ^= 1;
        }
        let result = observe(&mut child.0, &signature, || Ok(()));
        if wrong_requirement {
            assert_eq!(
                result,
                Err(Refusal::Peer(
                    super::super::super::Refusal::LiveCodeRejected
                ))
            );
        } else {
            assert_eq!(result, Err(Refusal::Child));
        }
        assert!(child.0.stdin.is_none());
        assert_eq!(child.0.id(), original);
        child.finish();
        let mut output = Vec::new();
        child
            .0
            .stdout
            .as_mut()
            .unwrap()
            .read_to_end(&mut output)
            .unwrap();
        assert!(output.is_empty());
    }
}

#[test]
fn native_exec_of_that_same_child_between_live_checks_is_refused() {
    // /bin/sh is a Mac shim which may re-exec before readiness. Keep the
    // interpreter's actual image stable until our explicit public trigger.
    const INTERPRETER: &str = "/bin/zsh";
    let tree = super::super::tests::Tree::new();
    let mut child = spawn(
        &tree.0,
        INTERPRETER,
        &["-f", "-c", "printf 'ready\\n'; read ignored; exec /bin/cat"],
    );
    read_exact(&mut child.0, b"ready\n");
    let original = child.0.id();
    let executed = Rc::new(Cell::new(false));
    let observed = executed.clone();
    let _hook = hook(move |child| {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"public exec trigger\n")
            .unwrap();
        let cat = expected("/bin/cat");
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let matching = autoreleasepool(|_| {
                TaskName::acquire(child)
                    .and_then(|task| task.audit(child))
                    .and_then(|token| verify(&token, &cat))
                    .is_ok()
            });
            if matching {
                break;
            }
            assert!(Instant::now() < deadline, "fixture did not exec cat");
            std::thread::sleep(Duration::from_millis(5));
        }
        observed.set(true);
    });
    let result = observe(&mut child.0, &expected(INTERPRETER), || Ok(()));
    assert!(
        executed.get(),
        "must reach the first real live-code check and observe exec"
    );
    assert_eq!(result, Err(Refusal::Child));
    assert!(child.0.stdin.is_none());
    assert_eq!(child.0.id(), original);
    child.finish();
    let mut output = Vec::new();
    child
        .0
        .stdout
        .as_mut()
        .unwrap()
        .read_to_end(&mut output)
        .unwrap();
    assert!(output.is_empty());
}

#[test]
fn native_generation_change_after_code_check_closes_the_original_input() {
    let tree = super::super::tests::Tree::new();
    let original = generation(&tree.0).unwrap();
    let mut child = spawn(&tree.0, "/bin/cat", &[]);
    let mut checks = 0;
    let result = observe(&mut child.0, &expected("/bin/cat"), || {
        checks += 1;
        if checks == 2 {
            std::fs::write(tree.0.join("Contents/Info.plist"), b"replaced generation").unwrap();
        }
        if generation(&tree.0)? == original {
            Ok(())
        } else {
            Err(Refusal::GenerationChanged)
        }
    });
    assert_eq!(result, Err(Refusal::GenerationChanged));
    assert_eq!(checks, 2);
    assert!(child.0.stdin.is_none());
    child.finish();
}

#[test]
fn native_reaped_child_and_an_extracted_input_never_get_observed() {
    let tree = super::super::tests::Tree::new();
    let mut child = spawn(&tree.0, "/bin/cat", &[]);
    let input = child.0.stdin.take();
    let mut checks = 0;
    assert_eq!(
        observe(&mut child.0, &expected("/bin/cat"), || {
            checks += 1;
            Ok(())
        }),
        Err(Refusal::Child)
    );
    assert_eq!(checks, 0);
    drop(input);
    child.finish();
    assert_eq!(
        observe(&mut child.0, &expected("/bin/cat"), || Ok(())),
        Err(Refusal::Child)
    );
    // Put a different live input into a cached reaped Child: birth still refuses
    // before acquiring a task name. It is never permission to adopt that child.
    let mut other = spawn(&tree.0, "/bin/cat", &[]);
    child.0.stdin = other.0.stdin.take();
    assert_eq!(
        observe(&mut child.0, &expected("/bin/cat"), || Ok(())),
        Err(Refusal::ChildExited)
    );
    assert!(child.0.stdin.is_none());
    other.finish();
}

#[test]
fn native_fast_helper_exit_is_reported_after_revalidation_without_accepting_input() {
    let tree = super::super::tests::Tree::new();
    // Hold the public fixture gate until revalidation starts, then let it
    // finish like --help. Its actual exit before birth is observed through
    // THIS Child, never inferred from elapsed time or a supplied PID.
    let mut child = Owned(
        Command::new("/bin/sh")
            .args(["-c", "IFS= read -r gate; exit 0"])
            .current_dir(&tree.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    assert!(child.0.try_wait().unwrap().is_none());
    assert!(child.0.stdin.is_some());
    // An owned CLOEXEC copy sends only a public test synchronization byte.
    // The original input stays in Child and must still close on refusal.
    let fd = unsafe {
        libc::fcntl(
            child.0.stdin.as_ref().unwrap().as_raw_fd(),
            libc::F_DUPFD_CLOEXEC,
            0,
        )
    };
    assert!(fd >= 0);
    let mut gate = unsafe { std::fs::File::from_raw_fd(fd) };
    let exited = Rc::new(Cell::new(false));
    let seen = exited.clone();
    AFTER_REVALIDATION.with_borrow_mut(|slot| {
        assert!(slot
            .replace(Box::new(move |child| {
                let until = Instant::now() + Duration::from_secs(3);
                loop {
                    if let Some(exit) = child.try_wait().unwrap() {
                        assert!(exit.success());
                        seen.set(true);
                        break;
                    }
                    assert!(Instant::now() < until);
                    std::thread::sleep(Duration::from_millis(5));
                }
            }))
            .is_none());
    });
    let _reset = HookReset;
    let mut revalidated = 0;
    let result = observe(&mut child.0, &expected("/bin/sh"), || {
        revalidated += 1;
        gate.write_all(b"release\n").unwrap();
        Ok(())
    });
    assert_eq!(revalidated, 1);
    assert!(exited.get());
    assert_eq!(result, Err(Refusal::ChildExited));
    assert_eq!(
        crate::desktop::relay::OriginalHelperFailure::from(result.unwrap_err()).code(),
        "original_child_exited"
    );
    assert!(
        child.0.stdin.is_none(),
        "refused original input is still closed"
    );
}

#[test]
fn native_unpaired_connection_cannot_supply_a_positive_child_observation() {
    let tree = super::super::tests::Tree::new();
    let mut child = spawn(&tree.0, "/bin/cat", &[]);
    let connection = crate::desktop::relay::Connection::select(Path::new("/bin/cat")).unwrap();
    assert_eq!(
        connection.observe_original_helper(&mut child.0),
        Err("paired_desktop_cli_unavailable")
    );
    assert!(child.0.stdin.is_none());
    child.finish();
    assert!(select_own().is_err());
}
