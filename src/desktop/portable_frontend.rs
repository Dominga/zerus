//! The portable zip's own app as a maintained frontend on Windows
//! (docs/desktop-windows-mode.md, zerus-5wmh.66.8.1.14).
//!
//! Creation: only the app's own `hgs session-host install` child, outside any
//! agent tree, records the pair in a private record, with the app's digest read
//! through a pin on its loaded image. Use is proven per operation: a process
//! that admits frontends establishes once that its own loaded image is the
//! bound CLI (and pins nothing afterwards); each running GUI is proven through
//! its own process handle and loaded image within the caller's deadline, and a
//! notification launch pins and proves the app itself. No proof pins anything
//! beyond the GUI process it serves or the launch that holds it, so maintained
//! staging can still replace the stable CLI and the app.
use crate::host::staging::Layout;
#[cfg(windows)]
use crate::host::staging::{self, BoundObject, FrontendBinding, FRONTEND_SCHEMA};
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::time::{Duration, Instant};
#[cfg(windows)]
use zerus_client_core::desktop::windows::{
    image::PinnedImage,
    process::{Ancestry, Original},
};

/// The largest app whose digest is read.
#[cfg(windows)]
const DIGEST_BYTES: u64 = 256 * 1024 * 1024;
/// The one original deadline of a record: the app's ancestry, its digest and
/// the last check before publication all count against it.
#[cfg(windows)]
const RECORD_TIME: Duration = Duration::from_secs(10);

/// The largest number of running GUI processes whose proofs are kept.
#[cfg(windows)]
const PROOFS: usize = 4;

/// What a scope knows about the portable app: the private binding, and that
/// this process's own loaded image is the bound CLI. No file stays pinned for
/// the scope's lifetime, so maintained staging can still replace the stable
/// CLI and the app; each use proves the app again (`running`, `closed`).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct PortableScope {
    #[cfg(windows)]
    binding: FrontendBinding,
    #[cfg(windows)]
    cache: std::sync::Arc<Cache>,
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

/// One running GUI process proven to run the bound app: its own process
/// handle (the original object, never a PID), its birth, and its loaded image
/// pinned, with the app's digest read through that pin. Holders keep the pin
/// until they drop the proof; the cache drops its reference when the process
/// exits.
#[cfg(windows)]
pub(crate) struct GuiProof {
    process: std::os::windows::io::OwnedHandle,
    pid: u32,
    birth: u64,
    image: PinnedImage,
    frontend: BoundObject,
}

#[cfg(windows)]
impl GuiProof {
    /// Within `until`, the caller's original deadline: the loaded section of
    /// `original` is the bound app object with the bound digest, and the same
    /// process still runs it.
    fn prove(binding: &FrontendBinding, original: &Original, until: Instant) -> Option<Self> {
        let process = duplicate(&original.handle())?;
        let image = PinnedImage::for_process(&process).ok()?;
        if !is(&image, &binding.frontend) {
            return None;
        }
        let digest = image.sha256(DIGEST_BYTES, until).ok()?;
        let proof = Self {
            process,
            pid: original.pid,
            birth: original.birth,
            image,
            frontend: binding.frontend.clone(),
        };
        (digest == binding.frontend_sha256
            && original.current()
            && proof.current()
            && Instant::now() < until)
            .then_some(proof)
    }
    /// The same process object still runs, from the same pinned loaded image.
    pub fn current(&self) -> bool {
        alive(&self.process)
            && self.image.matches_process(&self.process).is_ok()
            && self.image.revalidate().is_ok()
            && is(&self.image, &self.frontend)
    }
    /// This proof is for `original` (same PID and native birth) and still holds.
    pub fn proves(&self, original: &Original) -> bool {
        self.pid == original.pid
            && self.birth == original.birth
            && original.current()
            && self.current()
    }
}

#[cfg(windows)]
fn duplicate(
    handle: &std::os::windows::io::BorrowedHandle<'_>,
) -> Option<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::{Foundation::*, System::Threading::GetCurrentProcess};
    let mut copy = std::ptr::null_mut();
    // SAFETY: a live borrowed handle duplicated into this process.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle.as_raw_handle(),
            GetCurrentProcess(),
            &mut copy,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    // SAFETY: a new handle owned by this process.
    (ok != 0 && !copy.is_null())
        .then(|| unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(copy) })
}

#[cfg(windows)]
fn alive(process: &impl std::os::windows::io::AsRawHandle) -> bool {
    use windows_sys::Win32::{Foundation::WAIT_TIMEOUT, System::Threading::WaitForSingleObject};
    // SAFETY: a held process handle with SYNCHRONIZE.
    unsafe { WaitForSingleObject(process.as_raw_handle(), 0) == WAIT_TIMEOUT }
}

/// Proofs of running GUI processes, at most `PROOFS`. One watcher thread per
/// scope waits on their processes and drops a proof as soon as its process
/// exits; holders that still borrow it keep its pin until they drop it.
#[cfg(windows)]
struct Cache {
    state: std::sync::Mutex<CacheState>,
    wake: std::os::windows::io::OwnedHandle,
    /// Tests: make the watcher's next wake a wait failure.
    #[cfg(test)]
    fail: std::sync::atomic::AtomicBool,
}

#[cfg(windows)]
struct CacheState {
    proofs: Vec<std::sync::Arc<GuiProof>>,
    watching: bool,
    closed: bool,
}

#[cfg(windows)]
impl Cache {
    fn new() -> Option<std::sync::Arc<Self>> {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::System::Threading::CreateEventW;
        // SAFETY: an unnamed auto-reset event.
        let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
        if event.is_null() {
            return None;
        }
        Some(std::sync::Arc::new(Self {
            state: std::sync::Mutex::new(CacheState {
                proofs: Vec::new(),
                watching: false,
                closed: false,
            }),
            // SAFETY: the new event is owned here.
            wake: unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(event) },
            #[cfg(test)]
            fail: std::sync::atomic::AtomicBool::new(false),
        }))
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, CacheState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
    fn signal(&self) {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: the owned event handle.
        unsafe { windows_sys::Win32::System::Threading::SetEvent(self.wake.as_raw_handle()) };
    }
    fn find(&self, original: &Original) -> Option<std::sync::Arc<GuiProof>> {
        self.lock()
            .proofs
            .iter()
            .find(|proof| proof.pid == original.pid && proof.birth == original.birth)
            .cloned()
    }
    fn insert(self: &std::sync::Arc<Self>, proof: std::sync::Arc<GuiProof>) {
        let start = {
            let mut state = self.lock();
            if state.closed
                || state
                    .proofs
                    .iter()
                    .any(|held| held.pid == proof.pid && held.birth == proof.birth)
            {
                return;
            }
            if state.proofs.len() >= PROOFS {
                state.proofs.remove(0);
            }
            state.proofs.push(proof);
            !std::mem::replace(&mut state.watching, true)
        };
        if start {
            let weak = std::sync::Arc::downgrade(self);
            let started = std::thread::Builder::new()
                .name("portable-frontend-exit".into())
                .spawn(move || watch(weak));
            if started.is_err() {
                // Without a watcher nothing is kept past its process.
                let mut state = self.lock();
                state.proofs.clear();
                state.watching = false;
            }
        }
        self.signal();
    }
    fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        state.proofs.clear();
        drop(state);
        self.signal();
    }
    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().proofs.len()
    }
}

/// Wait for the cached GUI processes and the wake event; drop the proof of a
/// process the moment it exits. Ends when its scope closes.
#[cfg(windows)]
fn watch(cache: std::sync::Weak<Cache>) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::WAIT_FAILED,
        System::Threading::{WaitForMultipleObjects, INFINITE},
    };
    loop {
        let Some(strong) = cache.upgrade() else {
            return;
        };
        let handles = {
            let state = strong.lock();
            if state.closed {
                return;
            }
            let mut handles = vec![duplicate(&std::os::windows::io::AsHandle::as_handle(
                &strong.wake,
            ))];
            handles.extend(state.proofs.iter().map(|proof| {
                duplicate(&std::os::windows::io::AsHandle::as_handle(&proof.process))
            }));
            handles
        };
        drop(strong);
        let Some(handles) = handles.into_iter().collect::<Option<Vec<_>>>() else {
            return fail(&cache);
        };
        let raw: Vec<_> = handles
            .iter()
            .map(|handle| handle.as_raw_handle())
            .collect();
        // SAFETY: at most 1 + PROOFS live handles, all held through the wait.
        let woke = unsafe { WaitForMultipleObjects(raw.len() as u32, raw.as_ptr(), 0, INFINITE) };
        let Some(strong) = cache.upgrade() else {
            return;
        };
        #[cfg(test)]
        let woke = if strong.fail.swap(false, std::sync::atomic::Ordering::SeqCst) {
            WAIT_FAILED
        } else {
            woke
        };
        if woke == WAIT_FAILED {
            drop(strong);
            return fail(&cache);
        }
        strong.lock().proofs.retain(|proof| alive(&proof.process));
    }
}

/// Without a watcher nothing may stay cached past its process: drop every
/// proof the cache owns (holders keep their own) and let the next insertion
/// start a new watcher.
#[cfg(windows)]
fn fail(cache: &std::sync::Weak<Cache>) {
    if let Some(cache) = cache.upgrade() {
        let mut state = cache.lock();
        state.proofs.clear();
        state.watching = false;
    }
}

#[cfg(windows)]
impl Drop for PortableScope {
    fn drop(&mut self) {
        self.cache.close();
    }
}

#[cfg(windows)]
impl PortableScope {
    /// For a process that admits frontends: the private binding, when this
    /// process's own loaded image is the bound CLI object. The CLI is pinned
    /// only while that is established.
    pub fn materialize(home: &str) -> Option<Self> {
        Self::from_binding(staging::read_frontend(&staging::images_below(home))?)
    }
    fn from_binding(binding: FrontendBinding) -> Option<Self> {
        let own = PinnedImage::for_process(&own_process()).ok()?;
        let cli = is(&own, &binding.cli);
        drop(own);
        cli.then(|| {
            Some(Self {
                binding,
                cache: Cache::new()?,
            })
        })
        .flatten()
    }
    /// A running GUI (an authenticated frontend, a mode caller's ancestor):
    /// the proof for that exact process within the caller's own deadline. A
    /// cached proof is image evidence only, rechecked for the same process.
    pub fn running(&self, original: &Original, until: Instant) -> Option<std::sync::Arc<GuiProof>> {
        if let Some(proof) = self.cache.find(original) {
            return proof.proves(original).then_some(proof);
        }
        let proof = std::sync::Arc::new(GuiProof::prove(&self.binding, original, until)?);
        self.cache.insert(proof.clone());
        Some(proof)
    }
    /// With the UI closed (a notification launch): the bound app pinned, and
    /// its digest read through that same pin, which the caller keeps through
    /// the launch. Never taken from a running GUI's proof.
    pub fn closed(&self, image: &Path, until: Instant) -> Option<PinnedImage> {
        let pin = PinnedImage::open(image).ok()?;
        if !is(&pin, &self.binding.frontend) {
            return None;
        }
        let digest = pin.sha256(DIGEST_BYTES, until).ok()?;
        (digest == self.binding.frontend_sha256
            && pin.revalidate().is_ok()
            && Instant::now() < until)
            .then_some(pin)
    }
}

/// Under the install lock, after staging completed: when this process is the
/// direct child of the native app beside its own loaded image, outside any
/// agent tree, pin that app's loaded image and the staged CLI and record both,
/// with the app's digest read through its pin. One original deadline covers
/// the whole proof. `publish` prepares and verifies the record first and then
/// runs this proof's last check (the same ancestry, parent and pins against
/// that deadline) immediately before committing the name, while all of them
/// are still held. Anything else records nothing.
#[cfg(windows)]
pub(crate) fn record(
    layout: &Layout,
    publish: &mut staging::Publish<'_>,
) -> Result<Option<FrontendBinding>, staging::StageError> {
    let until = Instant::now() + RECORD_TIME;
    let Some((ancestry, gui, cli)) = prove_creation(layout, until) else {
        return Ok(None);
    };
    let Ok(digest) = gui.sha256(DIGEST_BYTES, until) else {
        return Ok(None);
    };
    let binding = FrontendBinding {
        schema: FRONTEND_SCHEMA,
        frontend: object(&gui),
        frontend_sha256: digest,
        cli: object(&cli),
    };
    // The same original evidence, checked against the same deadline as the
    // last step before the record's name is committed; held until then.
    let commit = || {
        #[cfg(test)]
        tests::barrier();
        ancestry.validate(until).is_ok()
            && ancestry
                .ancestors()
                .next()
                .is_some_and(|parent| parent.current())
            && gui.revalidate().is_ok()
            && cli.revalidate().is_ok()
            && Instant::now() < until
    };
    let published = publish(&binding, &commit)?;
    Ok(published.then_some(binding))
}

/// The app that started this process and the staged CLI, both pinned, with the
/// ancestry that admitted them.
#[cfg(windows)]
fn prove_creation(layout: &Layout, until: Instant) -> Option<(Ancestry, PinnedImage, PinnedImage)> {
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
    Some((ancestry, gui, cli))
}

#[cfg(windows)]
pub(crate) fn materialize(home: &str) -> Option<PortableScope> {
    PortableScope::materialize(home)
}

#[cfg(not(windows))]
pub(crate) fn materialize(_: &str) -> Option<PortableScope> {
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
