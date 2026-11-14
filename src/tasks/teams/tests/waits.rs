//! Test orchestration ceilings: return as soon as the original child advances.
use super::FixtureChild;
use std::{
    fs, io,
    path::Path,
    thread,
    time::{Duration, Instant},
};

impl FixtureChild {
    pub(super) fn wait_for_marker(&self, marker: &Path) -> bool {
        let result = self.wait_until(
            &format!("pause/ready marker {}", marker.display()),
            Duration::from_secs(60),
            true,
            || fs::read(marker).is_ok_and(|bytes| !bytes.is_empty()),
        );
        if let Err(error) = result {
            eprintln!("{error}");
            false
        } else {
            true
        }
    }

    pub(super) fn wait_for_completion(&self) -> io::Result<()> {
        // The helper bounds its own child at 60 seconds. Allow scheduling
        // before that child starts and publication after its pipes settle.
        self.wait_until("child completion", Duration::from_secs(120), false, || {
            self.path.join("status").is_file()
        })
    }

    fn wait_until(
        &self,
        what: &str,
        ceiling: Duration,
        require_live: bool,
        ready: impl Fn() -> bool,
    ) -> io::Result<()> {
        // Like zerus-63rru.21: a generous failure ceiling, never a fixed sleep.
        let until = Instant::now() + ceiling;
        loop {
            let ended = require_live && self.path.join("status").is_file();
            if !ended && ready() {
                return Ok(());
            }
            if ended || Instant::now() >= until {
                let reason = if ended {
                    "child ended before readiness"
                } else {
                    "ceiling reached"
                };
                let mut detail = format!(
                    "real CLI fixture {}: {reason} waiting for {what} (ceiling {ceiling:?})",
                    self.path.display()
                );
                for name in ["status", "stdout", "stderr"] {
                    let value = fs::read_to_string(self.path.join(name))
                        .unwrap_or_else(|error| format!("<not published: {error}>"));
                    detail.push_str(&format!("\n{name}: {value}"));
                }
                return Err(io::Error::new(
                    if ended {
                        io::ErrorKind::BrokenPipe
                    } else {
                        io::ErrorKind::TimedOut
                    },
                    detail,
                ));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

#[test]
fn a_completed_childs_marker_does_not_claim_it_is_still_paused() {
    let folder = tempfile::tempdir().unwrap();
    let marker = folder.path().join("ready");
    fs::write(&marker, "the original child reached its pause").unwrap();
    fs::write(folder.path().join("status"), "0").unwrap();
    let child = FixtureChild {
        path: folder.path().to_owned(),
    };
    assert!(!child.wait_for_marker(&marker));
    child.wait_for_completion().unwrap();
}
