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
#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::time::{Duration, Instant};
#[cfg(windows)]
use zerus_client_core::desktop::windows::{
    image::PinnedImage,
    process::{AgentDescended, Ancestry, Original},
};

/// The largest app whose digest is read.
#[cfg(windows)]
const DIGEST_BYTES: u64 = 256 * 1024 * 1024;
/// Reading the bound app's digest for `session-host status`.
#[cfg(windows)]
const STATUS_TIME: Duration = Duration::from_secs(10);
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
    /// The host images folder holding the private record.
    #[cfg(windows)]
    images: PathBuf,
    /// This process's own loaded image, established once when the scope was made.
    #[cfg(windows)]
    cli: BoundObject,
    /// The last record that passed every check and binds `cli`.
    #[cfg(windows)]
    snapshot: std::sync::Mutex<Option<std::sync::Arc<FrontendBinding>>>,
    /// One record read in flight per scope.
    #[cfg(windows)]
    reread: std::sync::Mutex<()>,
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
    /// The record this proof was made against.
    frontend: BoundObject,
    digest: String,
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
            digest: binding.frontend_sha256.clone(),
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
    /// Whether this proof was made against `binding`'s app.
    fn bound_to(&self, binding: &FrontendBinding) -> bool {
        self.frontend == binding.frontend && self.digest == binding.frontend_sha256
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
    fn remove(&self, proof: &std::sync::Arc<GuiProof>) {
        self.lock()
            .proofs
            .retain(|held| !std::sync::Arc::ptr_eq(held, proof));
    }
    /// Drop the proofs the cache owns that were made against another record.
    fn retain_bound(&self, binding: &FrontendBinding) {
        self.lock().proofs.retain(|held| held.bound_to(binding));
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
    /// For a process that admits frontends: this process's own loaded image,
    /// established once (the CLI is pinned only while that is done). Only
    /// when that image is exactly `<dir>\.local\bin\hgs.exe` does a portable
    /// scope exist, reading `<dir>\.local\share\hgs\host\frontend.json`,
    /// where the binding install writes it; the private chain checks cover
    /// every folder from `<dir>\.local` down. Any other CLI has none.
    pub fn materialize() -> Option<Self> {
        let own = PinnedImage::for_process(&own_process()).ok()?;
        let cli = object(&own);
        drop(own);
        // Beside the bound CLI itself, never from HOME or another setting.
        let images = images_beside(&cli.path)?;
        Self::with_images(images, cli)
    }
    fn with_images(images: PathBuf, cli: BoundObject) -> Option<Self> {
        let scope = Self {
            images,
            cli,
            snapshot: std::sync::Mutex::new(None),
            reread: std::sync::Mutex::new(()),
            cache: Cache::new()?,
        };
        let _ = scope.refresh();
        Some(scope)
    }
    #[cfg(test)]
    fn snapshot(&self) -> Option<std::sync::Arc<FrontendBinding>> {
        self.snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
    /// Read the private record for one admitting operation. Reads are
    /// serialized, one in flight per scope, and each operation decides on its
    /// own read, started after it arrived. Only a record that passes every
    /// check and binds this process's own CLI is returned, and only such a
    /// record replaces the snapshot. When it names another app, the cache
    /// drops its proofs made against the old record. A failed read returns
    /// nothing: the operation is refused and never answered from the snapshot.
    fn refresh(&self) -> Option<std::sync::Arc<FrontendBinding>> {
        let _one = self
            .reread
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let binding = staging::read_frontend(&self.images)?;
        if binding.cli != self.cli {
            return None;
        }
        let binding = std::sync::Arc::new(binding);
        let changed = {
            let mut snapshot = self
                .snapshot
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let changed = snapshot.as_deref() != Some(&*binding);
            if changed {
                *snapshot = Some(binding.clone());
            }
            changed
        };
        if changed {
            self.cache.retain_bound(&binding);
        }
        Some(binding)
    }
    /// A running GUI (an authenticated frontend, a mode caller's ancestor):
    /// the proof for that exact process within the caller's own deadline.
    /// Every call reads and checks the current private record first, with a
    /// read of its own; a missing, unsafe or other-CLI record refuses even when
    /// a proof is cached. A cached proof is reused only when that record names
    /// exactly its app object and digest, and it is rechecked for the same
    /// process; otherwise the process is proven against the record.
    /// Task 180: why `running` gave no proof, in fixed words the desktop
    /// shows: the binding's folders, the binding itself, a binding of another
    /// hgs, or a running app that is not the bound one.
    pub fn refusal(&self) -> &'static str {
        match staging::frontend_binding(&self.images) {
            Err(reason) => reason,
            Ok(binding) if binding.cli != self.cli => "frontend binding names another hgs",
            Ok(_) => "frontend is not the bound application",
        }
    }
    pub fn running(&self, original: &Original, until: Instant) -> Option<std::sync::Arc<GuiProof>> {
        let binding = self.refresh()?;
        if let Some(proof) = self.cache.find(original) {
            if proof.bound_to(&binding) && proof.proves(original) {
                return Some(proof);
            }
            self.cache.remove(&proof);
        }
        let proof = std::sync::Arc::new(GuiProof::prove(&binding, original, until)?);
        self.cache.insert(proof.clone());
        Some(proof)
    }
    /// With the UI closed (a notification launch): the record read again, the
    /// bound app pinned, and its digest read through that same pin, which the
    /// caller keeps through the launch. Never taken from a running GUI's proof.
    pub fn closed(&self, image: &Path, until: Instant) -> Option<PinnedImage> {
        let binding = self.refresh()?;
        let pin = PinnedImage::open(image).ok()?;
        if !is(&pin, &binding.frontend) {
            return None;
        }
        let digest = pin.sha256(DIGEST_BYTES, until).ok()?;
        (digest == binding.frontend_sha256 && pin.revalidate().is_ok() && Instant::now() < until)
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
    set_refusal(None);
    let until = Instant::now() + RECORD_TIME;
    let (ancestry, gui, cli) = match prove_creation(layout, until) {
        Ok(proof) => proof,
        Err(reason) => {
            set_refusal(Some(reason));
            return Ok(None);
        }
    };
    let digest = match gui.sha256(DIGEST_BYTES, until) {
        Ok(digest) => digest,
        Err(error) => {
            set_refusal(Some(format!("the app's digest: {error}")));
            return Ok(None);
        }
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
        let refused = if let Err(error) = ancestry.validate(until) {
            Some(format!("the last check: the ancestry: {error}"))
        } else if !ancestry.ancestors().next().is_some_and(|parent| parent.current()) {
            Some("the last check: the app that started this hgs is gone".to_owned())
        } else if let Err(error) = gui.revalidate() {
            Some(format!("the last check: the app's pin: {error}"))
        } else if let Err(error) = cli.revalidate() {
            Some(format!("the last check: the staged CLI's pin: {error}"))
        } else if Instant::now() >= until {
            Some(format!("the last check: past the record's {} s deadline", RECORD_TIME.as_secs()))
        } else {
            None
        };
        let ok = refused.is_none();
        set_refusal(refused);
        ok
    };
    let published = publish(&binding, &commit)?;
    Ok(published.then_some(binding))
}

/// Task #43 (team ede724c4, 2026-11-10): why the last `record` of this
/// process bound no app (the proof or its last check), or None when it bound
/// one or was not asked. `session-host install --json` shows it as
/// `frontend_refusal`; the tests print it.
#[cfg(windows)]
static REFUSAL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(windows)]
fn set_refusal(reason: Option<String>) {
    *REFUSAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = reason;
}

#[cfg(windows)]
pub(crate) fn last_refusal() -> Option<String> {
    REFUSAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

/// The app that started this process and the staged CLI, both pinned, with the
/// ancestry that admitted them.
/// A refusal names what failed: this CLI's image, the ancestry (with the hop
/// and marker that refused it), the parent's image, or the staged CLI's pin.
#[cfg(windows)]
fn prove_creation(layout: &Layout, until: Instant) -> Result<(Ancestry, PinnedImage, PinnedImage), String> {
    let own = PinnedImage::for_process(&own_process()).map_err(|error| format!("this hgs's own image: {error}"))?;
    let ancestry = Ancestry::inspect(std::process::id(), until).map_err(|error| match AgentDescended::detail(&error) {
        Some(detail) => format!("the ancestry: {error}: {detail}"),
        None => format!("the ancestry: {error}"),
    })?;
    let parent = ancestry.ancestors().next().ok_or("the ancestry: no parent process")?;
    let gui = PinnedImage::for_process(&parent.handle()).map_err(|error| format!("the parent's image: {error}"))?;
    let beside = own.path().parent().ok_or("this hgs's image has no folder")?.join(staging::FRONTEND_IMAGE);
    if !gui
        .path()
        .as_os_str()
        .eq_ignore_ascii_case(beside.as_os_str())
    {
        return Err(format!("the parent's image {} is not the app beside this hgs ({})", gui.path().display(), beside.display()));
    }
    ancestry.validate(until).map_err(|error| format!("the ancestry: {error}"))?;
    let cli = PinnedImage::open(&layout.cli).map_err(|error| format!("the staged CLI's pin ({}): {error}", layout.cli.display()))?;
    Ok((ancestry, gui, cli))
}

#[cfg(windows)]
pub(crate) fn materialize() -> Option<PortableScope> {
    PortableScope::materialize()
}

#[cfg(not(windows))]
pub(crate) fn materialize() -> Option<PortableScope> {
    None
}

/// The host images folder that belongs to the stable CLI at `cli`: for exactly
/// `<dir>\.local\bin\hgs.exe`, `<dir>\.local\share\hgs\host` (the Layout of
/// `staging::Layout`, so the binding install writes there). Anything else has
/// none.
#[cfg(any(windows, test))]
fn images_beside(cli: &Path) -> Option<PathBuf> {
    let named = |path: &Path, name: &str| {
        path.file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(name))
    };
    let bin = cli.parent()?;
    let local = bin.parent()?;
    (named(cli, crate::host::staging::EXECUTABLE) && named(bin, "bin") && named(local, ".local"))
        .then(|| local.join("share").join("hgs").join("host"))
}

/// For `session-host status`: the bound app's path, whether the binding still
/// names the staged CLI, and whether the file at that path is still the bound
/// app (the same object with the recorded digest, read through a pin). The app
/// stages again at start when either is not so. Informational; admission
/// proves the pair itself and the record alone admits nothing.
pub(crate) fn status(layout: &Layout) -> Option<(PathBuf, bool, bool)> {
    let binding = crate::host::staging::read_frontend(&layout.images)?;
    #[cfg(windows)]
    let (cli, frontend) = (
        PinnedImage::open(&layout.cli).is_ok_and(|cli| is(&cli, &binding.cli)),
        PinnedImage::open(&binding.frontend.path).is_ok_and(|app| {
            is(&app, &binding.frontend)
                && app
                    .sha256(DIGEST_BYTES, Instant::now() + STATUS_TIME)
                    .is_ok_and(|digest| digest == binding.frontend_sha256)
        }),
    );
    #[cfg(not(windows))]
    let (cli, frontend) = (false, false);
    Some((binding.frontend.path, cli, frontend))
}

#[cfg(all(test, windows))]
mod tests;

#[cfg(test)]
mod layout_tests {
    use super::*;
    use crate::host::staging::EXECUTABLE;

    #[test]
    fn the_service_reads_the_record_exactly_where_the_install_writes_it() {
        let dir = tempfile::tempdir().unwrap();
        let fixture = dir
            .path()
            .join("fixture home")
            .to_string_lossy()
            .into_owned();
        let homes = [
            fixture.as_str(),
            if cfg!(windows) {
                r"C:\Users\Example"
            } else {
                "/home/example"
            },
        ];
        for home in homes {
            let layout =
                crate::host::staging::Layout::new(home, Path::new(home).join("config").as_path());
            assert_eq!(
                images_beside(&layout.cli),
                Some(layout.images.clone()),
                "{home}"
            );
        }
    }

    #[test]
    fn a_cli_outside_dot_local_bin_has_no_record_location() {
        let root = Path::new(if cfg!(windows) {
            r"C:\Users\Example"
        } else {
            "/home/example"
        });
        for cli in [
            root.join("AppData/Local/Programs/Zerus").join(EXECUTABLE),
            root.join("tools/bin").join(EXECUTABLE),
            root.join(".local/lib").join(EXECUTABLE),
            root.join(".local/bin/hgs-tray.exe"),
            PathBuf::from(EXECUTABLE),
        ] {
            assert_eq!(images_beside(&cli), None, "{}", cli.display());
        }
        // Names compare without regard to ASCII case, as Windows does.
        assert_eq!(
            images_beside(&root.join(".LOCAL/Bin/HGS.EXE")),
            Some(root.join(".LOCAL").join("share").join("hgs").join("host"))
        );
    }
}
