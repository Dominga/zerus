//! Original-stop ordering and the existing atomic metadata write. Non-Windows
//! tests compile this SAME code as a model; they construct no native authority.
//! Windows file publication runs in the admitted dedicated helper, whose entry
//! selects its exact-user creation owner. Models must not change the shared
//! Windows unit process token; the hosted task fixture exercises that helper.
use crate::{
    desktop::windows_registration_policy::mode::{Phase, Record},
    platform,
};
use anyhow::{ensure, Result};
use std::{io::Write, path::Path};

#[cfg(all(test, not(windows)))]
std::thread_local! { static FAIL_DIRECTORY_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

pub(in crate::desktop) const RECORD: &str = "windows-registration-mode.json";

pub(in crate::desktop) fn offer<O, F: FnMut() -> Result<()>>(
    record: &mut Record,
    original: &O,
    fence: &mut F,
    mut check: impl FnMut(&O, &mut F) -> Result<()>,
    publish: impl FnOnce(&Record, &mut dyn FnMut() -> Result<()>) -> Result<()>,
) -> Result<()> {
    ensure!(
        record
            .pending
            .as_ref()
            .is_some_and(|p| p.phase == Phase::Held && p.review.is_none()),
        "Original transition phase changed."
    );
    check(original, fence)?;
    record.advance(Phase::Held, Phase::StopOffered)?;
    // Err does not imply that publication failed. Never roll the in-memory or
    // durable phase back: a post-persist check/sync/lost reply stays Unknown.
    publish(record, &mut || check(original, fence))
}

pub(in crate::desktop) fn checked<O, F: FnMut() -> Result<()>>(
    original: &mut O,
    fence: &mut F,
    check: impl FnOnce(&O, &mut F) -> Result<()>,
    stop: impl FnOnce(&mut O, &mut F) -> Result<()>,
) -> Result<()> {
    check(original, fence)?;
    stop(original, fence)
}

pub(in crate::desktop) fn publish(
    root: &Path,
    fence: &mut dyn FnMut() -> Result<()>,
    prepare: impl FnOnce() -> Result<Vec<u8>>,
) -> Result<()> {
    fence()?;
    let bytes = prepare()?;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    platform::make_private(file.as_file())?;
    ensure!(
        platform::verify_private_file(file.as_file())?,
        "Mode record is not private."
    );
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    fence()?;
    platform::persist(file, root.join(RECORD)).map_err(|error| error.error)?;
    #[cfg(all(test, not(windows)))]
    ensure!(
        !FAIL_DIRECTORY_SYNC.get(),
        "injected directory sync uncertainty"
    );
    platform::sync_dir(root)?;
    fence()
}

#[cfg(all(test, not(windows)))]
#[path = "stop/tests.rs"]
mod tests;
