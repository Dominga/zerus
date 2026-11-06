//! A current requester-bound READ, with no ownership reservation or effect.
use super::*;
use zerus_client_core::desktop::ownership::observation::{self as wire, Observation};

struct Database {
    #[cfg(unix)]
    identity: crate::sqlite_file::Identity,
    #[cfg(windows)]
    original: std::fs::File,
    #[cfg(windows)]
    path: super::super::revoke::hold::Verification,
}
impl Database {
    fn capture(location: &Location, store: &Store) -> Result<Self> {
        location.check_existing()?;
        super::super::store::database_files(location)?;
        #[cfg(unix)]
        let original = Self {
            identity: crate::sqlite_file::private(&location.root.join("state.sqlite3"))?,
        };
        #[cfg(windows)]
        let original = {
            use std::os::windows::io::BorrowedHandle;
            let mut handle: *mut std::ffi::c_void = std::ptr::null_mut();
            // SAFETY: SQLite owns this live connection and writes one HANDLE
            // into the provided pointer. Borrowing never closes its handle;
            // our duplicate has its own handle-scoped lifetime on Windows.
            let rc = unsafe {
                rusqlite::ffi::sqlite3_file_control(
                    store.db.handle(),
                    c"main".as_ptr(),
                    rusqlite::ffi::SQLITE_FCNTL_WIN32_GET_HANDLE,
                    (&mut handle as *mut *mut std::ffi::c_void).cast(),
                )
            };
            ensure!(
                rc == rusqlite::ffi::SQLITE_OK && !handle.is_null(),
                "original ownership database handle unavailable"
            );
            // SAFETY: the handle belongs to store.db, which remains borrowed
            // until this read and its duplicate are finished.
            let file = unsafe { BorrowedHandle::borrow_raw(handle) }.try_clone_to_owned()?;
            let (checked, path) = super::super::revoke::hold::database(location)?;
            drop(checked);
            Self {
                original: file.into(),
                path,
            }
        };
        original.check(location, store)?;
        Ok(original)
    }
    fn check(&self, location: &Location, _store: &Store) -> Result<()> {
        location.check_existing()?;
        super::super::store::database_files(location)?;
        let path = location.root.join("state.sqlite3");
        #[cfg(unix)]
        crate::sqlite_file::bind_private(&_store.db, &path, self.identity)?;
        #[cfg(windows)]
        {
            let original = platform::FileIdentity::of(&self.original)?;
            let current =
                platform::file_identity(&path).context("ownership database unavailable")?;
            ensure!(
                self.path.identity()? == (original.device, original.inode)
                    && (current.device, current.inode) == (original.device, original.inode),
                "original ownership database changed"
            );
        }
        Ok(())
    }
}

pub(in crate::desktop) fn frontend(
    location: &Location,
    store: &Store,
    epoch: &str,
    permit: ui_authority::Validated,
) -> Result<Observation> {
    let (query, origin) = permit.ownership_observation()?;
    ensure!(
        query.valid() && query.configuration == location.configuration,
        "invalid ownership observation"
    );
    let database = Database::capture(location, store)?;
    // Deferred read transaction: no initialization, refresh, reservation or
    // mutation. The service and ownership rows describe one coherent snapshot.
    let tx = store.db.unchecked_transaction()?;
    let service: (String, String, u64) = tx.query_row(
        "SELECT substr(configuration,1,65),substr(epoch,1,37),generation FROM service WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    ensure!(
        service
            == (
                location.configuration.clone(),
                epoch.into(),
                store.generation
            ),
        "current service database generation changed"
    );
    let current = row(&tx, &location.configuration)?;
    ensure!(
        current.origin.as_ref() == Some(&origin)
            && query.matches(&current.status, epoch, store.generation),
        "ownership does not belong to this native requester"
    );
    database.check(location, store)?;
    let (same_query, same_origin) = permit.ownership_observation()?;
    ensure!(
        same_query == query && same_origin == origin,
        "ownership requester changed"
    );
    tx.commit()?;
    database.check(location, store)?;
    Ok(Observation {
        abi: wire::ABI.into(),
        query,
        current: current.status,
    })
}
