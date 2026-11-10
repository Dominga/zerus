//! Test-only same-image native peer, never an owner GUI.
use super::*;
#[path = "../../core/tests/support/windows_frontend_peer.rs"]
mod native_peer;
use std::{
    cell::RefCell,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
};
use zerus_client_core::desktop::windows;
thread_local! {
    static FOREGROUND: RefCell<Option<Vec<u32>>> = const { RefCell::new(None) };
}
struct Foreground;
impl Foreground {
    fn start() -> Self {
        FOREGROUND.with(|v| assert!(v.borrow_mut().replace(Vec::new()).is_none()));
        Self
    }
    fn calls(&self) -> Vec<u32> {
        FOREGROUND.with(|v| v.borrow().as_ref().unwrap().clone())
    }
}
impl Drop for Foreground {
    fn drop(&mut self) {
        FOREGROUND.with(|v| *v.borrow_mut() = None);
    }
}
/// The exact forward call reaches this alias only in tests. Production keeps
/// importing the OS API directly. Record every attempted right, including a
/// wrong PID, before performing the original call.
pub(super) unsafe fn allow_foreground(pid: u32) -> i32 {
    FOREGROUND.with(|v| {
        if let Some(calls) = v.borrow_mut().as_mut() {
            assert!(calls.len() < 16);
            calls.push(pid);
        }
    });
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid) }
}

#[test]
#[ignore = "mandatory hosted native peer; CI invokes only this exact parent"]
fn native_lower_integrity_server_never_forwards_or_claims_a_gui() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    let own = windows::own_facts().unwrap();
    assert!(own.integrity >= 0x2000);
    let image = windows::image::PinnedImage::open(&std::env::current_exe().unwrap()).unwrap();
    let image_id = image.id();
    for (low, mode) in [
        (false, "reply"),
        (true, "reply"),
        (true, "denied"),
        (true, "busy"),
    ] {
        let root = std::env::temp_dir().join(format!("zerus-owned-peer-{}", uuid::Uuid::new_v4()));
        assert!(!root.exists());
        let names = [
            windows::activation_name("hgs-tray", &root),
            format!(
                "com.hgdev.hgs-tray.gui-{}",
                native_peer::sid_text(&own.user).unwrap()
            ),
        ];
        let child = native_peer::Child::start(low, mode, &names).unwrap();
        let foreground = Foreground::start();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let facts = child.facts().unwrap();
            assert_eq!(facts.user, own.user);
            assert_eq!(facts.session, own.session);
            assert_eq!(facts.image, Some(image_id));
            assert_eq!(facts.integrity, if low { 0x1000 } else { own.integrity });
            for name in &names {
                let forwarded = forward(name, &Activation::Show);
                if low {
                    assert_eq!(
                        forwarded,
                        if mode == "reply" {
                            Forward::Untrusted
                        } else {
                            Forward::Unavailable
                        }
                    );
                } else {
                    assert_eq!(forwarded, Forward::Forwarded);
                }
                let claim = claim_at(name, &root, Activation::Sessions);
                if !low {
                    assert!(matches!(claim, Claim::Forwarded));
                } else if mode == "reply" {
                    assert!(matches!(claim, Claim::Untrusted));
                } else {
                    // Task #52 (team ede724c4): since task 110 (4a72905e4) a held
                    // activation pipe (ERROR_ACCESS_DENIED or ERROR_PIPE_BUSY on its
                    // first instance) is a Duplicate the start explains, not a bare
                    // failure; it still claims, forwards and foregrounds nothing.
                    assert!(
                        matches!(claim, Claim::Duplicate("activation_pipe_held")),
                        "{mode}: the held pipe is a duplicate"
                    );
                }
                // An untrusted listener never grants second-instance ownership.
                assert!(!matches!(claim, Claim::Claimed(_)));
            }
            if low {
                assert!(foreground.calls().is_empty());
            } else {
                // Show forwards data only; Sessions lends foreground once per pipe.
                assert_eq!(foreground.calls(), vec![child.pid; 2]);
            }
            if mode == "reply" {
                assert!(!root.exists(), "forward/refusal precedes owner allocation");
            }
        }));
        let capture = child.finish();
        // Only the original physical-exit receipt permits fixture-file cleanup.
        // Pending/failed cleanup retains this private directory for VM disposal.
        if capture.is_ok() && root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let capture = capture.unwrap();
        native_peer::assert_pipes_absent(&names);
        assert!(capture.natural);
        assert_eq!(capture.exit, 0);
        let done = native_peer::marker(&capture.output, native_peer::DONE).unwrap();
        assert_eq!(done["closed"], 2);
        assert_eq!(done["queries"], 0);
        assert_eq!(done["forwards"], if low { 0 } else { 4 });
        assert_eq!(
            done["bytes"],
            if low {
                0
            } else {
                2 * (Activation::Show.line().len() + Activation::Sessions.line().len())
            }
        );
        if let Err(panic) = result {
            resume_unwind(panic);
        }
    }
}
