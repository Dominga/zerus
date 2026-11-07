//! Read this cache through one held regular native object. SQLite's ordinary
//! pathname open can block on a substituted FIFO before a progress handler
//! exists. The private VFS below permits only that original main-file open;
//! no journal, WAL, SHM, temporary file or pathname fallback is available.
use super::super::repositories::directory::Directory;
use super::*;
use rusqlite::ffi;
use std::{
    ffi::{CStr, CString},
    os::raw::{c_char, c_int},
    ptr,
};

const NAME: &str = "archive-catalog-v1.sqlite3";

struct Pin {
    directory: Directory,
    file: std::sync::Arc<fs::File>,
    stamp: Version,
    until: Instant,
}
impl Pin {
    fn sidecars(&self) -> Result<()> {
        for suffix in ["-journal", "-wal", "-shm"] {
            live(self.until)?;
            match fs::symlink_metadata(self.directory.path.join(format!("{NAME}{suffix}"))) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                _ => return Err(INVALID.into()),
            }
        }
        Ok(())
    }
    fn file(path: &Path) -> Result<fs::File> {
        #[cfg(unix)]
        {
            open(path)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            use windows_sys::Win32::Storage::FileSystem::*;
            // Hold the leaf against deletion/rename while SQLite opens it.
            // In-place writes remain observable through the original version.
            let file = crate::platform::refuse_link(
                fs::OpenOptions::new()
                    .read(true)
                    .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE),
            )
            .open(path)
            .map_err(fail)?;
            let metadata = file.metadata().map_err(fail)?;
            if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            {
                return Err(INVALID.into());
            }
            Ok(file)
        }
    }
    fn new(until: Instant) -> Result<Self> {
        live(until)?;
        // Resolve only the native-configured state root, never an input path.
        let directory = Directory::open(&fs::canonicalize(root()).map_err(fail)?)?;
        let file = Self::file(&directory.path.join(NAME))?;
        let stamp = version(&file)?;
        let pin = Self {
            directory,
            file: std::sync::Arc::new(file),
            stamp,
            until,
        };
        pin.check()?;
        Ok(pin)
    }
    fn check(&self) -> Result<()> {
        live(self.until)?;
        self.directory.verify()?;
        if fs::canonicalize(root()).map_err(fail)? != self.directory.path {
            return Err(INVALID.into());
        }
        self.sidecars()?;
        let current = Self::file(&self.directory.path.join(NAME))?;
        if version(&self.file)? != self.stamp || version(&current)? != self.stamp {
            return Err(INVALID.into());
        }
        live(self.until)
    }
}

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, OnceLock,
};
const DATABASE: &[u8] = b"hgs-archive-reference\0";
struct Source {
    file: Arc<fs::File>,
    stamp: Version,
    until: Instant,
    bytes: AtomicUsize,
}
impl Source {
    fn current(&self) -> bool {
        live(self.until).is_ok() && version(&self.file).ok().as_ref() == Some(&self.stamp)
    }
}
#[repr(C)]
struct NativeFile {
    api: ffi::sqlite3_file,
    source: Arc<Source>,
}
#[repr(C)]
struct Shim {
    api: ffi::sqlite3_vfs,
    source: Arc<Source>,
    name: CString,
}
struct Vfs(Box<Shim>);
impl Vfs {
    fn new(pin: &Pin) -> Result<Self> {
        // The default VFS supplies only platform clock/randomness functions.
        // Our xOpen never invokes a pathname open, including for sidecars.
        unsafe {
            if ffi::sqlite3_initialize() != ffi::SQLITE_OK {
                return Err(INVALID.into());
            }
            let base = ffi::sqlite3_vfs_find(ptr::null());
            if base.is_null() {
                return Err(INVALID.into());
            }
            let source = Arc::new(Source {
                file: pin.file.clone(),
                stamp: pin.stamp.clone(),
                until: pin.until,
                bytes: AtomicUsize::new(0),
            });
            let name =
                CString::new(format!("hgs-archive-read-{}", uuid::Uuid::new_v4())).map_err(fail)?;
            let mut shim = Box::new(Shim {
                api: ptr::read(base),
                source,
                name,
            });
            shim.api.pNext = ptr::null_mut();
            shim.api.zName = shim.name.as_ptr();
            shim.api.szOsFile = std::mem::size_of::<NativeFile>() as c_int;
            shim.api.xFullPathname = Some(full_path);
            shim.api.xOpen = Some(open_main);
            shim.api.xAccess = Some(no_access);
            shim.api.xDelete = Some(no_delete);
            if ffi::sqlite3_vfs_register(&mut shim.api, 0) != ffi::SQLITE_OK {
                return Err(INVALID.into());
            }
            Ok(Self(shim))
        }
    }
}
impl Drop for Vfs {
    fn drop(&mut self) {
        // The Connection and all its sqlite3_file values have already closed.
        unsafe {
            ffi::sqlite3_vfs_unregister(&mut self.0.api);
        }
    }
}
unsafe extern "C" fn full_path(
    _: *mut ffi::sqlite3_vfs,
    input: *const c_char,
    size: c_int,
    output: *mut c_char,
) -> c_int {
    if input.is_null()
        || output.is_null()
        || size < DATABASE.len() as c_int
        || unsafe { CStr::from_ptr(input).to_bytes_with_nul() } != DATABASE
    {
        return ffi::SQLITE_CANTOPEN;
    }
    unsafe {
        ptr::copy_nonoverlapping(DATABASE.as_ptr().cast(), output, DATABASE.len());
    }
    ffi::SQLITE_OK
}
unsafe extern "C" fn open_main(
    vfs: *mut ffi::sqlite3_vfs,
    input: *const c_char,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out: *mut c_int,
) -> c_int {
    if !file.is_null() {
        unsafe { (*file).pMethods = ptr::null() };
    }
    if input.is_null()
        || file.is_null()
        || flags & ffi::SQLITE_OPEN_MAIN_DB == 0
        || flags & ffi::SQLITE_OPEN_READONLY == 0
        || flags
            & (ffi::SQLITE_OPEN_READWRITE
                | ffi::SQLITE_OPEN_CREATE
                | ffi::SQLITE_OPEN_DELETEONCLOSE)
            != 0
        || unsafe { CStr::from_ptr(input).to_bytes_with_nul() } != DATABASE
        || unsafe { ffi::sqlite3_uri_boolean(input, c"immutable".as_ptr(), 0) } != 1
    {
        return ffi::SQLITE_CANTOPEN;
    }
    // SAFETY: api is the first field of our registered, stable Box. SQLite
    // allocated szOsFile bytes with native alignment, and calls xClose only
    // after this complete value and its method table have been installed.
    let shim = unsafe { &*(vfs as *const Shim) };
    if !shim.source.current() {
        return ffi::SQLITE_CANTOPEN;
    }
    unsafe {
        ptr::write(
            file.cast::<NativeFile>(),
            NativeFile {
                api: ffi::sqlite3_file {
                    pMethods: methods(),
                },
                source: shim.source.clone(),
            },
        );
    }
    if !out.is_null() {
        unsafe {
            *out = flags;
        }
    }
    ffi::SQLITE_OK
}
fn methods() -> &'static ffi::sqlite3_io_methods {
    static METHODS: OnceLock<ffi::sqlite3_io_methods> = OnceLock::new();
    METHODS.get_or_init(|| {
        // Zero optional version-2/3 methods: no SHM or mmap can bypass xRead.
        let mut methods: ffi::sqlite3_io_methods = unsafe { std::mem::zeroed() };
        methods.iVersion = 1;
        methods.xClose = Some(close_file);
        methods.xRead = Some(read_file);
        methods.xWrite = Some(no_write);
        methods.xTruncate = Some(no_truncate);
        methods.xSync = Some(no_sync);
        methods.xFileSize = Some(file_size);
        methods.xLock = Some(no_lock);
        methods.xUnlock = Some(no_lock);
        methods.xCheckReservedLock = Some(no_reserved);
        methods.xFileControl = Some(no_control);
        methods.xSectorSize = Some(sector);
        methods.xDeviceCharacteristics = Some(characteristics);
        methods
    })
}
unsafe extern "C" fn close_file(file: *mut ffi::sqlite3_file) -> c_int {
    // SAFETY: successful open_main initialized exactly this NativeFile once.
    unsafe {
        ptr::drop_in_place(file.cast::<NativeFile>());
        (*file).pMethods = ptr::null();
    }
    ffi::SQLITE_OK
}
unsafe extern "C" fn read_file(
    file: *mut ffi::sqlite3_file,
    output: *mut std::ffi::c_void,
    count: c_int,
    offset: i64,
) -> c_int {
    let source = unsafe { &(*file.cast::<NativeFile>()).source };
    if count < 0 || offset < 0 || output.is_null() || count as usize > RECORD || !source.current() {
        return ffi::SQLITE_IOERR_READ;
    }
    let count = count as usize;
    if source
        .bytes
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            n.checked_add(count).filter(|n| *n <= BYTES)
        })
        .is_err()
    {
        return ffi::SQLITE_IOERR_READ;
    }
    // SAFETY: SQLite provides a writable count-byte buffer. Short reads must
    // zero the unread suffix per its I/O contract, never expose old memory.
    let data = unsafe { std::slice::from_raw_parts_mut(output.cast::<u8>(), count) };
    data.fill(0);
    let mut done = 0;
    while done < count {
        if !source.current() {
            return ffi::SQLITE_IOERR_READ;
        }
        let Some(at) = (offset as u64).checked_add(done as u64) else {
            return ffi::SQLITE_IOERR_READ;
        };
        #[cfg(unix)]
        let result = {
            use std::os::unix::fs::FileExt;
            source.file.read_at(&mut data[done..], at)
        };
        #[cfg(windows)]
        let result = {
            use std::os::windows::fs::FileExt;
            source.file.seek_read(&mut data[done..], at)
        };
        match result {
            Ok(0) => return ffi::SQLITE_IOERR_SHORT_READ,
            Ok(read) => done += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => (),
            Err(_) => return ffi::SQLITE_IOERR_READ,
        }
    }
    if source.current() {
        ffi::SQLITE_OK
    } else {
        ffi::SQLITE_IOERR_READ
    }
}
unsafe extern "C" fn file_size(file: *mut ffi::sqlite3_file, out: *mut i64) -> c_int {
    let source = unsafe { &(*file.cast::<NativeFile>()).source };
    if out.is_null() || !source.current() {
        return ffi::SQLITE_IOERR_FSTAT;
    }
    let Ok(size) = i64::try_from(source.stamp.bytes) else {
        return ffi::SQLITE_IOERR_FSTAT;
    };
    unsafe {
        *out = size;
    }
    ffi::SQLITE_OK
}
unsafe extern "C" fn no_write(
    _: *mut ffi::sqlite3_file,
    _: *const std::ffi::c_void,
    _: c_int,
    _: i64,
) -> c_int {
    ffi::SQLITE_READONLY
}
unsafe extern "C" fn no_truncate(_: *mut ffi::sqlite3_file, _: i64) -> c_int {
    ffi::SQLITE_READONLY
}
unsafe extern "C" fn no_sync(_: *mut ffi::sqlite3_file, _: c_int) -> c_int {
    ffi::SQLITE_READONLY
}
unsafe extern "C" fn no_lock(_: *mut ffi::sqlite3_file, _: c_int) -> c_int {
    ffi::SQLITE_OK
}
unsafe extern "C" fn no_reserved(_: *mut ffi::sqlite3_file, out: *mut c_int) -> c_int {
    if out.is_null() {
        return ffi::SQLITE_IOERR;
    }
    unsafe {
        *out = 0;
    }
    ffi::SQLITE_OK
}
unsafe extern "C" fn no_control(
    _: *mut ffi::sqlite3_file,
    _: c_int,
    _: *mut std::ffi::c_void,
) -> c_int {
    ffi::SQLITE_NOTFOUND
}
unsafe extern "C" fn sector(_: *mut ffi::sqlite3_file) -> c_int {
    4096
}
unsafe extern "C" fn characteristics(_: *mut ffi::sqlite3_file) -> c_int {
    // Only tells SQLite to skip sidecar I/O. Native versions are independently
    // checked before/after reads and against the current pathname; this cache
    // is NOT assumed immutable by cleanup's authority checks.
    ffi::SQLITE_IOCAP_IMMUTABLE
}
unsafe extern "C" fn no_access(
    _: *mut ffi::sqlite3_vfs,
    _: *const c_char,
    _: c_int,
    out: *mut c_int,
) -> c_int {
    if out.is_null() {
        return ffi::SQLITE_IOERR;
    }
    unsafe {
        *out = 0;
    }
    ffi::SQLITE_OK
}
unsafe extern "C" fn no_delete(_: *mut ffi::sqlite3_vfs, _: *const c_char, _: c_int) -> c_int {
    ffi::SQLITE_READONLY
}

pub(super) struct Reader {
    // Close SQLite, unregister the private VFS, then release original objects.
    db: Connection,
    vfs: Vfs,
    pin: Pin,
}
impl std::ops::Deref for Reader {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.db
    }
}
impl Reader {
    pub(super) fn open(until: Instant) -> Result<Self> {
        let pin = Pin::new(until)?;
        #[cfg(test)]
        AFTER_PIN.with(|hook| {
            if let Some(hook) = hook.take() {
                hook();
            }
        });
        let vfs = Vfs::new(&pin)?;
        live(until)?;
        let db = Connection::open_with_flags_and_vfs(
            "file:hgs-archive-reference?immutable=1",
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            vfs.0.name.to_str().map_err(fail)?,
        )
        .map_err(fail)?;
        // No schema text or result cell from this rebuildable cache needs an
        // unbounded SQLite allocation, including before the first query VM.
        unsafe {
            ffi::sqlite3_limit(db.handle(), ffi::SQLITE_LIMIT_SQL_LENGTH, 64 * 1024);
            ffi::sqlite3_limit(db.handle(), ffi::SQLITE_LIMIT_LENGTH, (META * 4) as c_int);
        }
        db.busy_timeout(Duration::ZERO).map_err(fail)?;
        db.progress_handler(1000, Some(move || Instant::now() >= until));
        let reader = Self { db, vfs, pin };
        reader.check()?;
        reader.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA temp_store=MEMORY; PRAGMA cache_size=-2048;").map_err(fail)?;
        Ok(reader)
    }
    pub(super) fn check(&self) -> Result<()> {
        self.pin.check()
    }
    pub(super) fn bytes(&self) -> usize {
        self.vfs.0.source.bytes.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
pub(super) fn fixture_settles() {
    let default = unsafe { ffi::sqlite3_vfs_find(ptr::null()) };
    let reader = Reader::open(Instant::now() + Duration::from_secs(2)).unwrap();
    let name = reader.vfs.0.name.clone();
    let source = Arc::downgrade(&reader.vfs.0.source);
    let file = Arc::downgrade(&reader.pin.file);
    drop(reader);
    assert!(source.upgrade().is_none());
    assert!(file.upgrade().is_none());
    assert!(unsafe { ffi::sqlite3_vfs_find(name.as_ptr()) }.is_null());
    assert_eq!(unsafe { ffi::sqlite3_vfs_find(ptr::null()) }, default);
}
#[cfg(test)]
thread_local! {pub(super) static AFTER_PIN:std::cell::Cell<Option<fn()>>=const {std::cell::Cell::new(None)};}
