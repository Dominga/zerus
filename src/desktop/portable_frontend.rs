//! The portable zip's own app as a maintained frontend on Windows
//! (docs/desktop-windows-mode.md, zerus-5wmh.66.8.1.14).
//!
//! Creation: only the app's own `hgs session-host install` child, outside any
//! agent tree, records the pair, both pinned, with the app's digest read through
//! its pin. Use: each process that admits frontends pins the recorded pair
//! again, proves that the CLI is its own loaded image, checks the app's digest
//! once and keeps both pins for as long as its scope lives. Admission itself
//! never hashes, and neither the record's identities nor its digest admit
//! anything without that proof.
use crate::host::staging::Layout;
#[cfg(windows)]
use crate::host::staging::{self, BoundObject, FrontendBinding, FRONTEND_SCHEMA};
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::time::{Duration, Instant};
#[cfg(windows)]
use zerus_client_core::desktop::windows::{image::PinnedImage, process::Ancestry};

/// The largest app whose digest is read.
#[cfg(windows)]
const DIGEST_BYTES: u64 = 256 * 1024 * 1024;
/// The one digest read per binding lifetime.
#[cfg(windows)]
const DIGEST_TIME: Duration = Duration::from_secs(10);
/// Proving the installing app and its ancestry.
#[cfg(windows)]
const ANCESTRY_TIME: Duration = Duration::from_secs(2);

/// The exact pinned pair: neither file nor any folder above it can be written,
/// deleted or renamed while this lives.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct Portable {
    #[cfg(windows)]
    gui: PinnedImage,
    #[cfg(windows)]
    cli: PinnedImage,
}

#[cfg(windows)]
impl Portable {
    /// The pinned app's final path.
    pub fn gui(&self) -> &Path {
        self.gui.path()
    }
    /// Whether both retained objects still have their paths and lengths.
    pub fn current(&self) -> bool {
        self.gui.revalidate().is_ok() && self.cli.revalidate().is_ok()
    }
}

#[cfg(windows)]
fn own_process() -> std::os::windows::io::BorrowedHandle<'static> {
    // SAFETY: the current-process pseudo handle is always valid.
    unsafe {
        std::os::windows::io::BorrowedHandle::borrow_raw(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
        )
    }
}

#[cfg(windows)]
fn object(image: &PinnedImage) -> BoundObject {
    let (volume, id) = image.id();
    BoundObject {
        path: image.path().to_owned(),
        volume,
        file_id: format!("{id:032x}"),
        length: image.bytes(),
    }
}

#[cfg(windows)]
fn is(image: &PinnedImage, object: &BoundObject) -> bool {
    let (volume, id) = image.id();
    volume == object.volume
        && format!("{id:032x}") == object.file_id
        && image.bytes() == object.length
        && image
            .path()
            .as_os_str()
            .eq_ignore_ascii_case(object.path.as_os_str())
}

/// Under the install lock, after staging completed: when this process is the
/// direct child of the native app beside its own loaded image, outside any
/// agent tree, pin that app's loaded image and the staged CLI and record both,
/// with the app's digest read through its pin. Anything else records nothing.
#[cfg(windows)]
pub(crate) fn record(layout: &Layout) -> Option<FrontendBinding> {
    let until = Instant::now() + ANCESTRY_TIME;
    let own = PinnedImage::for_process(&own_process()).ok()?;
    let ancestry = Ancestry::inspect(std::process::id(), until).ok()?;
    let parent = ancestry.ancestors().next()?;
    let gui = PinnedImage::for_process(&parent.handle()).ok()?;
    let beside = own.path().parent()?.join(staging::FRONTEND_IMAGE);
    if !gui
        .path()
        .as_os_str()
        .eq_ignore_ascii_case(beside.as_os_str())
    {
        return None;
    }
    ancestry.validate(until).ok()?;
    let cli = PinnedImage::open(&layout.cli).ok()?;
    let digest = gui
        .sha256(DIGEST_BYTES, Instant::now() + DIGEST_TIME)
        .ok()?;
    if !parent.current() || cli.revalidate().is_err() {
        return None;
    }
    Some(FrontendBinding {
        schema: FRONTEND_SCHEMA,
        frontend: object(&gui),
        frontend_sha256: digest,
        cli: object(&cli),
    })
}

/// For a process that admits frontends: the recorded pair pinned again, when
/// the CLI is this process's own loaded image and the app has the recorded
/// digest. Done once per scope, before any request is admitted.
#[cfg(windows)]
pub(crate) fn materialize(home: &str) -> Option<Portable> {
    prove(&staging::read_frontend(&staging::images_below(home))?)
}

#[cfg(windows)]
fn prove(binding: &FrontendBinding) -> Option<Portable> {
    let cli = PinnedImage::for_process(&own_process()).ok()?;
    if !is(&cli, &binding.cli) {
        return None;
    }
    let gui = PinnedImage::open(&binding.frontend.path).ok()?;
    if !is(&gui, &binding.frontend) {
        return None;
    }
    let digest = gui
        .sha256(DIGEST_BYTES, Instant::now() + DIGEST_TIME)
        .ok()?;
    (digest == binding.frontend_sha256).then_some(Portable { gui, cli })
}

#[cfg(not(windows))]
pub(crate) fn materialize(_: &str) -> Option<Portable> {
    None
}

/// For `session-host status`: the bound app's path, and whether the binding
/// still names the staged CLI. Informational; it proves nothing.
pub(crate) fn status(layout: &Layout) -> Option<(PathBuf, bool)> {
    let binding = crate::host::staging::read_frontend(&layout.images)?;
    #[cfg(windows)]
    let current = PinnedImage::open(&layout.cli).is_ok_and(|cli| is(&cli, &binding.cli));
    #[cfg(not(windows))]
    let current = false;
    Some((binding.frontend.path, current))
}

#[cfg(all(test, windows))]
mod tests;
