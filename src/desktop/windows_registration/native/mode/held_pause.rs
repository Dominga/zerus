//! Debug builds only: this module and its environment read are never compiled
//! into a release image (`scripts/check-windows-portable.py` proves the
//! published executables omit its markers). The hosted fixture holds ONE exact
//! first choice at Held, after its durable reservation and after the Stop
//! phase deadline has started, for a bounded time (zerus-yfbd.1).
//!
//! It only waits. Nothing is skipped, forced, injected or rewritten: the
//! registration lock, the retained Grant, caller and resources stay held, and
//! no budget is renewed. A release, the timeout or any unsafe or mismatched
//! handshake continues into the same ordinary code: the StopOffered save's
//! fence refuses a wait that outlived the Stop deadline, still at Held.
use super::*;
use std::io::Write;

/// Data only: the exact canonical original UUID to hold, nothing else.
pub(super) const ENV: &str = "HGS_TEST_MODE_HELD_PAUSE";
/// One monotonic bound for the wait itself, well inside the CLI's exchange
/// timeout. It never extends the transition's own deadline.
const LIMIT: Duration = Duration::from_secs(12);
const RELEASE_BYTES: usize = 64;

fn canonical(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|u| !u.is_nil() && u.to_string() == value)
}

/// The helper environment is rebuilt from fixed values; only a canonical UUID
/// passes through.
pub(super) fn forward(values: &mut Vec<(String, String)>) {
    if let Some(value) = std::env::var(ENV).ok().filter(|value| canonical(value)) {
        values.push((ENV.into(), value));
    }
}

/// Holds `request` when, and only when, the fixture named exactly it and its
/// private config folder takes an owner-only marker. The marker announces this
/// helper's PID; a private release file naming the same UUID ends the wait.
pub(super) fn wait(config: &Config, request: &str) {
    hold(
        &config.dir,
        request,
        std::env::var(ENV).ok().as_deref(),
        LIMIT,
    );
}

fn hold(folder: &Path, request: &str, named: Option<&str>, limit: Duration) -> bool {
    let until = Instant::now() + limit;
    if !canonical(request) || named != Some(request) {
        return false;
    }
    if !platform::verify_private(folder, platform::Private::Directory).unwrap_or(false) {
        return false;
    }
    let paused = folder.join(format!("held-pause-{request}.json"));
    let release = folder.join(format!("held-release-{request}"));
    if publish(&paused, request).is_err() {
        return false;
    }
    while Instant::now() < until && !released(&release, request) {
        std::thread::sleep(Duration::from_millis(25));
    }
    true
}

/// A new owner-only regular file; an existing entry, link or junction refuses.
fn publish(path: &Path, request: &str) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    ensure!(
        platform::verify_private_file(&file)?,
        "Held pause marker is not private."
    );
    file.write_all(
        json!({"request":request,"pid":std::process::id()})
            .to_string()
            .as_bytes(),
    )?;
    file.sync_all()?;
    Ok(())
}

/// No-follow, owner-only, single-link and bounded; the exact UUID or nothing.
fn released(path: &Path, request: &str) -> bool {
    crate::desktop::handoff::read_private(path, RELEASE_BYTES)
        .is_ok_and(|bytes| bytes == request.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_held_pause_only_waits_for_its_exact_private_release_within_its_bound() {
        // Task #23 (team ede724c4): `hold` waits only in a private folder; a
        // TEMP that inherits Authenticated Users Modify is not one.
        let folder = crate::host::staging::private_tempdir();
        let folder = zerus_desktop_migration::canonical(folder.path()).unwrap();
        let request = uuid::Uuid::new_v4().to_string();
        let other = uuid::Uuid::new_v4().to_string();
        let marker = folder.join(format!("held-pause-{request}.json"));
        // Unnamed, another UUID or a non-canonical value: no marker, no wait.
        for named in [None, Some(other.as_str()), Some("not-a-uuid")] {
            let started = Instant::now();
            assert!(!hold(&folder, &request, named, Duration::from_secs(5)));
            assert!(started.elapsed() < Duration::from_secs(1));
            assert!(!marker.exists());
        }
        // The bound always ends it; the marker names this process.
        let started = Instant::now();
        assert!(hold(
            &folder,
            &request,
            Some(&request),
            Duration::from_millis(300)
        ));
        assert!(started.elapsed() >= Duration::from_millis(300));
        let value: Value = serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
        assert_eq!(value, json!({"request":request,"pid":std::process::id()}));
        // An existing marker (here the earlier one) is never reused: no wait.
        let started = Instant::now();
        assert!(!hold(
            &folder,
            &request,
            Some(&request),
            Duration::from_secs(5)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
        std::fs::remove_file(&marker).unwrap();
        // A release naming another UUID keeps waiting; the exact one ends it.
        let release = folder.join(format!("held-release-{request}"));
        std::fs::write(&release, &other).unwrap();
        let writer = {
            let (release, request) = (release.clone(), request.clone());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(700));
                std::fs::write(release, request).unwrap();
            })
        };
        let started = Instant::now();
        assert!(hold(
            &folder,
            &request,
            Some(&request),
            Duration::from_secs(10)
        ));
        let waited = started.elapsed();
        writer.join().unwrap();
        assert!(
            waited >= Duration::from_millis(700) && waited < Duration::from_secs(5),
            "{waited:?}"
        );
    }

    #[test]
    fn only_a_canonical_uuid_passes_into_the_helper_environment() {
        assert!(canonical("11111111-1111-4111-8111-111111111111"));
        for value in [
            "",
            "not-a-uuid",
            "00000000-0000-0000-0000-000000000000",
            "11111111-1111-4111-8111-11111111111",
            "11111111-1111-4111-8111-111111111111 & calc",
            "{11111111-1111-4111-8111-111111111111}",
            "11111111-1111-4111-8111-11111111111A",
        ] {
            assert!(!canonical(value), "{value}");
        }
    }
}
