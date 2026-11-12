//! Inactive Stop-capture support. Object custody is not current-name
//! authority. Only a live strict ReadLease authorizes a bounded read/use.
#![cfg(windows)]
#![allow(dead_code)] // The task-control production gate remains off.

use super::Directory;
use crate::{platform, state::task_controls::writer::Writer};
use std::{
    fs::{File, OpenOptions},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::PathBuf,
    time::Instant,
};
use windows_sys::Win32::Storage::FileSystem::*;

type Result<T> = std::result::Result<T, &'static str>;
const PINS: usize = 128;
const PATH_UNITS: usize = 4096;
const FS_UNITS: usize = 32;
const FACTS: &str = "task-stop-facts";
#[cfg(test)]
thread_local! { static OMIT_ROOT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(test)]
pub(in crate::state) fn fault_omit_root(omit: bool) {
    OMIT_ROOT.with(|s| s.set(omit));
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Object {
    volume: u64,
    index: u64,
}
fn object(file: &File) -> Result<Object> {
    let id = platform::FileIdentity::of(file).map_err(|_| "object query")?;
    Ok(Object {
        volume: id.device,
        index: id.inode,
    })
}
#[derive(Clone, PartialEq, Eq)]
struct Domain {
    guid: [u8; 16],
    serial: u32,
}
// The returned final path is PRIVATE metadata, never a path to open. Accept
// only a complete Mount-Manager GUID result and actual NTFS from this handle.
fn parse_domain(fs: &[u16], final_name: &[u16], returned: usize, serial: u32) -> Result<Domain> {
    let end = fs
        .iter()
        .position(|c| *c == 0)
        .ok_or("filesystem result truncated")?;
    if String::from_utf16(&fs[..end]).map_err(|_| "filesystem encoding")? != "NTFS" {
        return Err("unqualified filesystem");
    }
    if returned == 0
        || returned >= final_name.len()
        || final_name[returned] != 0
        || final_name[..returned].contains(&0)
    {
        return Err("volume GUID result incomplete");
    }
    let name = String::from_utf16(&final_name[..returned]).map_err(|_| "volume GUID encoding")?;
    let tail = name
        .strip_prefix(r"\\?\Volume{")
        .ok_or("not a Mount-Manager volume GUID")?;
    if tail.len() < 38 || &tail.as_bytes()[36..38] != b"}\\" {
        return Err("volume GUID shape");
    }
    let guid = tail.get(..36).ok_or("volume GUID shape")?;
    let id = uuid::Uuid::parse_str(guid).map_err(|_| "volume GUID shape")?;
    if id.is_nil() || id.hyphenated().to_string() != guid.to_ascii_lowercase() {
        return Err("volume GUID is not canonical");
    }
    Ok(Domain {
        guid: *id.as_bytes(),
        serial,
    })
}
fn domain(file: &File) -> Result<Domain> {
    let mut fs = [0u16; FS_UNITS];
    let mut serial = 0;
    // SAFETY: borrowed original handle, fixed writable buffers and optional
    // outputs omitted. No volume label or final name is logged or opened.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            fs.as_mut_ptr(),
            FS_UNITS as u32,
        )
    };
    if ok == 0 {
        return Err("filesystem qualification query failed");
    }
    let mut final_name = [0u16; PATH_UNITS];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            final_name.as_mut_ptr(),
            PATH_UNITS as u32,
            VOLUME_NAME_GUID,
        )
    } as usize;
    let result = parse_domain(&fs, &final_name, length, serial)?;
    if object(file)?.volume != u64::from(result.serial) {
        return Err("volume serial binding changed");
    }
    Ok(result)
}
// Pure error/denial policy only. Runtime callers pass the SAME held-object
// privacy query result; a test value never creates a namespace capability.
fn require_private<E>(result: std::result::Result<bool, E>) -> Result<()> {
    match result {
        Ok(true) => Ok(()),
        _ => Err("private object proof unavailable"),
    }
}
fn live(until: Instant) -> Result<()> {
    if Instant::now() >= until {
        Err("original capture deadline expired")
    } else {
        Ok(())
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Shape {
    object: Object,
    length: u64,
    attributes: u32,
}
fn shape(file: &File) -> Result<Shape> {
    let m = file
        .metadata()
        .map_err(|_| "held directory metadata unreadable")?;
    if !m.is_dir() || m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("held directory shape or reparse changed");
    }
    Ok(Shape {
        object: object(file)?,
        length: m.len(),
        attributes: m.file_attributes(),
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Root,
    Facts,
    Ancestor,
}
struct Pin {
    file: File,
    role: Role,
    baseline: Option<Shape>,
}

/// No Clone, public fields, raw handle getter, publication or mutation methods.
/// Construction consumes real Directory objects while their original pin chain
/// and the separately owned native Writer are still held by the caller.
pub(in crate::state) struct Namespace {
    frozen: PathBuf,
    original_root: Object,
    original_facts: Object,
    originals: Vec<Pin>,
    strict_roots: Vec<Pin>,
    keeper: Option<File>,
    domain: Option<Domain>,
    converted: bool,
}
impl Namespace {
    /// Infallible ownership transfer: all input resources remain owned even if
    /// a subsequent qualification fails. No filename-derived replacement.
    pub(in crate::state) fn own(root: Directory, facts: Directory, extra: Vec<Directory>) -> Self {
        let frozen = root.path.clone();
        // Unknown IDs remain impossible sentinels until initialize succeeds.
        let unknown = Object {
            volume: u64::MAX,
            index: u64::MAX,
        };
        let mut this = Self {
            frozen,
            original_root: unknown,
            original_facts: unknown,
            originals: vec![],
            strict_roots: vec![],
            keeper: None,
            domain: None,
            converted: false,
        };
        this.take_directory(root, Role::Root);
        this.take_directory(facts, Role::Facts);
        for directory in extra {
            this.take_directory(directory, Role::Root);
        }
        this
    }
    fn take_directory(&mut self, directory: Directory, primary: Role) {
        let Directory {
            file, _ancestors, ..
        } = directory;
        self.originals.push(Pin {
            file,
            role: primary,
            baseline: None,
        });
        for file in _ancestors {
            self.originals.push(Pin {
                file,
                role: Role::Ancestor,
                baseline: None,
            });
        }
    }
    pub(in crate::state) fn initialize(&mut self, until: Instant) -> Result<()> {
        live(until)?;
        if platform::canonical(&self.frozen).map_err(|_| "canonical root unavailable")?
            != self.frozen
        {
            return Err("selected root is not the frozen canonical root");
        }
        if self.originals.len() > PINS || self.keeper.is_some() || self.converted {
            return Err("unknown or excessive original pin inventory");
        }
        let root = self.originals.first().ok_or("missing original root")?;
        self.original_root = object(&root.file)?;
        let d = self
            .originals
            .iter()
            .find(|p| p.role == Role::Facts)
            .ok_or("missing original facts directory")?;
        self.original_facts = object(&d.file)?;
        let qualified = domain(&root.file)?;
        for pin in &mut self.originals {
            live(until)?;
            let before = shape(&pin.file)?;
            if domain(&pin.file)? != qualified {
                return Err("original pin volume domain changed");
            }
            if before.object == self.original_root {
                pin.role = Role::Root;
            } else if pin.role == Role::Root {
                return Err("extra alias is not original root");
            }
            if matches!(pin.role, Role::Root | Role::Facts) {
                require_private(platform::verify_private_directory(&pin.file))?;
            }
            pin.baseline = Some(before);
            live(until)?;
        }
        self.domain = Some(qualified);
        // Existing verify_private(path) uses exactly these access/share/flags.
        // Open existing independently; std creates non-inheritable handles.
        // Acquisition is under the original strict R/ancestor pins and Writer.
        self.keeper = Some(
            OpenOptions::new()
                .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
                .open(&self.frozen)
                .map_err(|_| "independent root keeper acquisition failed")?,
        );
        self.check_keeper(until)?;
        self.check_originals(until)?;
        self.check_names(until)?;
        Ok(())
    }
    fn check_keeper(&self, until: Instant) -> Result<()> {
        live(until)?;
        let keeper = self.keeper.as_ref().ok_or("no original root keeper")?;
        if shape(keeper)?.object != self.original_root || Some(domain(keeper)?) != self.domain {
            return Err("root keeper binding or privacy changed");
        }
        require_private(platform::verify_private_directory(keeper))?;
        live(until)
    }
    pub(in crate::state) fn qualify_file(&self, file: &File, until: Instant) -> Result<()> {
        live(until)?;
        if Some(domain(file)?) != self.domain {
            return Err("retained file domain changed");
        }
        live(until)
    }
    fn check_originals(&self, until: Instant) -> Result<()> {
        if self.originals.len() + self.strict_roots.len() > PINS {
            return Err("pin inventory bound");
        }
        for pin in self.originals.iter().chain(&self.strict_roots) {
            live(until)?;
            let current = shape(&pin.file)?;
            let expected = pin
                .baseline
                .as_ref()
                .ok_or("incomplete original inventory")?;
            if (pin.role == Role::Root && current.object != expected.object)
                || (pin.role != Role::Root && &current != expected)
                || Some(domain(&pin.file)?) != self.domain
            {
                return Err("retained original directory guard changed");
            }
            if matches!(pin.role, Role::Root | Role::Facts) {
                require_private(platform::verify_private_directory(&pin.file))?;
            }
            live(until)?;
        }
        Ok(())
    }
    fn check_names(&self, until: Instant) -> Result<()> {
        let lease = self.open_names(until)?;
        lease.verify(until)
    }
    fn open_names(&self, until: Instant) -> Result<ReadLease<'_>> {
        self.check_keeper(until)?;
        let root = Directory::open(&self.frozen).map_err(|_| "current root chain unavailable")?;
        let facts = root
            .child(FACTS)
            .map_err(|_| "current facts child unavailable")?;
        if root
            .object()
            .map_err(|_| "current root object unavailable")?
            != (self.original_root.volume, self.original_root.index)
            || facts
                .object()
                .map_err(|_| "current facts object unavailable")?
                != (self.original_facts.volume, self.original_facts.index)
        {
            return Err("current root or facts name changed");
        }
        require_private(root.private())?;
        require_private(facts.private())?;
        for file in std::iter::once(&root.file)
            .chain(&root._ancestors)
            .chain(std::iter::once(&facts.file))
            .chain(&facts._ancestors)
        {
            live(until)?;
            if Some(domain(file)?) != self.domain {
                return Err("current namespace volume changed");
            }
            shape(file)?;
        }
        live(until)?;
        Ok(ReadLease {
            owner: self,
            root,
            facts,
            until,
        })
    }
    /// Consume the actual Pending owner's native Writer by value before
    /// converting its strict R cohort. None is refusal, never release evidence.
    pub(in crate::state) fn convert(
        &mut self,
        native: &mut Option<Writer>,
        until: Instant,
    ) -> Result<()> {
        let owner = native.as_ref().ok_or("no owned original native writer")?;
        owner
            .verify()
            .map_err(|_| "original native writer changed")?;
        if owner.root().map_err(|_| "native root unavailable")? != self.frozen.as_path() {
            return Err("owned native writer belongs to a different root");
        }
        if self.converted {
            return Err("conversion already consumed");
        }
        self.check_keeper(until)?;
        self.check_originals(until)?;
        self.check_names(until)?; // scoped strict walks settle before root drop
        owner
            .verify()
            .map_err(|_| "native owner changed before retirement")?;
        // Consume the actual owner, not a released boolean. This retires both
        // its private FileLock and its otherwise opaque original R Directory.
        drop(native.take().ok_or("native owner already retired")?);
        live(until)?;
        let all = std::mem::take(&mut self.originals);
        let (roots, retained): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|p| p.role == Role::Root);
        self.strict_roots = roots;
        self.originals = retained;
        if self.strict_roots.is_empty() {
            return Err("missing complete original root cohort");
        }
        live(until)?;
        #[cfg(test)]
        if OMIT_ROOT.with(|s| s.get()) {
            return Err("fixture omitted root cohort; no ready handoff");
        }
        self.strict_roots.clear();
        self.converted = true; // physical retirement precedes fallible rechecks
        self.check_keeper(until)?;
        self.check_originals(until)
    }
    pub(in crate::state) fn lease(&self, until: Instant) -> Result<ReadLease<'_>> {
        if !self.converted || !self.strict_roots.is_empty() {
            return Err("original root cohort still held");
        }
        self.check_originals(until)?;
        self.open_names(until)
    }
}

/// Typed current-namespace capability. No raw File/Directory escapes. The
/// whole logical use must retain this value through its final check/commit.
pub(in crate::state) struct ReadLease<'a> {
    owner: &'a Namespace,
    root: Directory,
    facts: Directory,
    until: Instant,
}
impl ReadLease<'_> {
    pub(in crate::state) fn verify(&self, until: Instant) -> Result<()> {
        if until != self.until {
            return Err("namespace lease deadline changed");
        }
        live(until)?;
        self.owner.check_keeper(until)?;
        self.owner.check_originals(until)?;
        self.root
            .verify()
            .map_err(|_| "current root chain changed")?;
        self.facts
            .verify()
            .map_err(|_| "current facts chain changed")?;
        if self
            .root
            .object()
            .map_err(|_| "root identity unavailable")?
            != (
                self.owner.original_root.volume,
                self.owner.original_root.index,
            )
            || self
                .facts
                .object()
                .map_err(|_| "facts identity unavailable")?
                != (
                    self.owner.original_facts.volume,
                    self.owner.original_facts.index,
                )
            || !self
                .root
                .private()
                .map_err(|_| "root privacy unavailable")?
            || !self
                .facts
                .private()
                .map_err(|_| "facts privacy unavailable")?
        {
            return Err("current lease binding changed");
        }
        for file in std::iter::once(&self.root.file)
            .chain(&self.root._ancestors)
            .chain(std::iter::once(&self.facts.file))
            .chain(&self.facts._ancestors)
        {
            if Some(domain(file)?) != self.owner.domain {
                return Err("lease volume qualification changed");
            }
            shape(file)?;
            live(until)?;
        }
        live(until)
    }
    /// Scoped readers cannot escape as namespace guards. The callback sees
    /// borrowed read-only files, never the mutable Directory/current-chain pins.
    pub(in crate::state) fn with_files<T>(
        &self,
        name: &str,
        until: Instant,
        read: impl FnOnce(&File, &File) -> Result<T>,
    ) -> Result<T> {
        self.verify(until)?;
        let writer = self
            .root
            .read_private(".lock")
            .map_err(|_| "current writer unavailable")?;
        let fact = self
            .facts
            .read_private(name)
            .map_err(|_| "current fact unavailable")?;
        let writer_id =
            platform::FileIdentity::of(&writer).map_err(|_| "current writer identity")?;
        let fact_id = platform::FileIdentity::of(&fact).map_err(|_| "current fact identity")?;
        let data = read(&writer, &fact)?;
        let writer_again = self
            .root
            .read_private(".lock")
            .map_err(|_| "final writer name unavailable")?;
        let fact_again = self
            .facts
            .read_private(name)
            .map_err(|_| "final fact name unavailable")?;
        for f in [&writer, &fact, &writer_again, &fact_again] {
            self.owner.qualify_file(f, until)?;
            if !platform::verify_private_file(f).map_err(|_| "final file privacy unreadable")? {
                return Err("final file privacy changed");
            }
        }
        if platform::FileIdentity::of(&writer).map_err(|_| "final writer identity")? != writer_id
            || platform::FileIdentity::of(&writer_again)
                .map_err(|_| "final writer name identity")?
                != writer_id
            || platform::FileIdentity::of(&fact).map_err(|_| "final fact identity")? != fact_id
            || platform::FileIdentity::of(&fact_again).map_err(|_| "final fact name identity")?
                != fact_id
        {
            return Err("current names or file identities changed during read use");
        }
        self.verify(until)?;
        Ok(data)
    }
}

#[cfg(test)]
mod domain_controls {
    use super::*;
    fn fs(name: &str) -> Vec<u16> {
        name.encode_utf16().chain([0]).collect()
    }
    fn name(text: &str) -> (Vec<u16>, usize) {
        let mut v: Vec<_> = text.encode_utf16().collect();
        let n = v.len();
        v.push(0);
        (v, n)
    }
    #[test]
    fn only_complete_ntfs_mount_manager_guid_is_qualified() {
        let (n, c) = name(r"\\?\Volume{01234567-89ab-4cde-8fab-0123456789ab}\fixture");
        assert!(parse_domain(&fs("NTFS"), &n, c, 7).is_ok());
        for other in ["ReFS", "FAT32", "ntfs", ""] {
            assert!(parse_domain(&fs(other), &n, c, 7).is_err());
        }
        for other in [
            r"\\server\share\fixture",
            r"C:\fixture",
            r"\Device\HarddiskVolume1\fixture",
            r"\\?\Volume{00000000-0000-0000-0000-000000000000}\fixture",
            r"\\?\Volume{bad}\fixture",
        ] {
            let (v, count) = name(other);
            assert!(parse_domain(&fs("NTFS"), &v, count, 7).is_err());
        }
    }
    #[test]
    fn zero_truncated_unterminated_or_embedded_nul_results_refuse() {
        let (mut n, c) = name(r"\\?\Volume{01234567-89ab-4cde-8fab-0123456789ab}\fixture");
        assert!(parse_domain(&fs("NTFS"), &n, 0, 7).is_err());
        assert!(parse_domain(&fs("NTFS"), &n, n.len(), 7).is_err());
        n[c] = 1;
        assert!(parse_domain(&fs("NTFS"), &n, c, 7).is_err());
        n[c] = 0;
        n[20] = 0;
        assert!(parse_domain(&fs("NTFS"), &n, c, 7).is_err());
        assert!(parse_domain(&[78, 84, 70, 83], &n, c, 7).is_err());
    }
    #[test]
    fn different_volume_or_serial_never_matches_the_same_domain() {
        let (n, c) = name(r"\\?\Volume{01234567-89ab-4cde-8fab-0123456789ab}\fixture");
        let a = parse_domain(&fs("NTFS"), &n, c, 7).unwrap();
        assert!(a != parse_domain(&fs("NTFS"), &n, c, 8).unwrap());
        let (other, k) = name(r"\\?\Volume{11234567-89ab-4cde-8fab-0123456789ab}\fixture");
        assert!(a != parse_domain(&fs("NTFS"), &other, k, 7).unwrap());
    }
    #[test]
    fn malformed_unicode_guid_is_refusal_not_a_panic_or_path_fallback() {
        for text in [
            r"\\?\Volume{é1234567-89ab-4cde-8fab-0123456789ab}\x",
            r"\\?\Volume{01234567-89ab-4cde-8fab-0123456789aé}\x",
        ] {
            let (n, c) = name(text);
            assert!(parse_domain(&fs("NTFS"), &n, c, 7).is_err());
        }
    }
}

#[cfg(test)]
mod private_policy_controls {
    use super::*;
    #[test]
    fn denied_and_unreadable_privacy_never_become_an_affirmative_guard() {
        assert!(require_private::<()>(Ok(true)).is_ok());
        assert!(require_private::<()>(Ok(false)).is_err());
        assert!(require_private::<()>(Err(())).is_err());
    }
    #[test]
    fn expired_budget_is_never_renewed_by_a_guard() {
        let past = Instant::now() - std::time::Duration::from_secs(1);
        for _ in 0..3 {
            assert!(live(past).is_err());
        }
    }
}
