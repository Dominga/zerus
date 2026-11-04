//! The read-only observation of one original's own CLI input receipt
//! (zerus-5wmh.66.4.4, slice 3, G1 condition 2). It runs off the actor and
//! only after the original's physical drain, while the drained custody lock
//! is still held, so no child of the original can still write it.
//!
//! `hgs send` writes `input_receipts/<request_id>.json` under the CLI's state
//! root, atomically: `in_progress` before the first input byte reaches the
//! terminal, then `submitted` after its Enter. Only an exact `submitted`
//! receipt of this original is a submission; it still proves nothing about
//! whether the agent read the message. Every other observation is Unknown and
//! never authorizes a resend: `in_progress`, a missing, corrupt, oversized,
//! unsafe or mismatched receipt.
use crate::platform;
use std::{
    ffi::CString,
    fs::File,
    io::{self, Read},
    os::fd::{AsRawFd, FromRawFd},
    path::Path,
};
use zerus_client_core::desktop::deferred_send::{Outcome, Plan, RECEIPT_BYTES};

/// No receipt exists for the original's request ID.
pub(super) const MISSING: &str = "deferred_send_receipt_missing";
/// The receipt could not be read safely: an unsafe folder or file, or an
/// I/O error.
pub(super) const UNAVAILABLE: &str = "deferred_send_receipt_unavailable";

/// What the original's own receipt says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Observed {
    /// The exact original's receipt is `submitted`.
    Submitted(Outcome),
    /// Anything else, for the named reason. Never resent.
    Unknown(&'static str),
}

/// Opens `name` in the held `parent` without following a link.
fn open_at(parent: &File, name: &str, flags: libc::c_int) -> io::Result<File> {
    let name = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: the parent is a held descriptor and the name one NUL-free
    // component; the new descriptor is owned by the returned File.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOCTTY,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// Reads the original's receipt from the CLI state root `state`.
pub(super) fn observe(state: &Path, plan: &Plan) -> Observed {
    let Ok(request) = uuid::Uuid::parse_str(&plan.request_id) else {
        return Observed::Unknown(zerus_client_core::desktop::deferred_send::RECEIPT_MISMATCH);
    };
    let read = || -> io::Result<Option<Vec<u8>>> {
        let state = File::open(state)?;
        let folder = match open_at(&state, "input_receipts", libc::O_RDONLY | libc::O_DIRECTORY) {
            Ok(folder) => folder,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !platform::verify_private_directory(&folder)? {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let file = match open_at(&folder, &format!("{request}.json"), libc::O_RDONLY) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        if !platform::verify_private_file(&file)? || file.metadata()?.len() > RECEIPT_BYTES as u64 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let mut bytes = Vec::new();
        file.take(RECEIPT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > RECEIPT_BYTES {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Some(bytes))
    };
    let bytes = match read() {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Observed::Unknown(MISSING),
        Err(_) => return Observed::Unknown(UNAVAILABLE),
    };
    // The wire owns the matching rule: the original's own `submitted`
    // record, or no outcome.
    match Outcome::from_receipt(plan, &bytes) {
        Ok(outcome) => Observed::Submitted(outcome),
        Err(code) => Observed::Unknown(code),
    }
}
