//! Exact-user ownership for objects created by a dedicated desktop writer.
//! This changes creation metadata only, never a caller's user or privileges.
use super::*;
use windows_sys::Win32::Security::{
    SetTokenInformation, TokenStatistics, TOKEN_ADJUST_DEFAULT, TOKEN_STATISTICS,
};

fn refused(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

fn id(token: &Token) -> io::Result<(u32, i32)> {
    let bytes = token.information(TokenStatistics)?;
    // SAFETY: the native query supplied this aligned TOKEN_STATISTICS.
    let value = unsafe { &*bytes.as_ptr().cast::<TOKEN_STATISTICS>() };
    Ok((value.TokenId.LowPart, value.TokenId.HighPart))
}

fn writable_current() -> io::Result<Token> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: only the calling process's own primary token is requested.
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_ADJUST_DEFAULT,
            &mut raw,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcessToken returned a new owned handle.
    Ok(Token(unsafe { OwnedHandle::from_raw_handle(raw) }))
}

fn set_owner(token: &Token, user: &Sid) -> io::Result<()> {
    // A SID's subauthorities need four-byte alignment. Keep the copy alive
    // throughout SetTokenInformation; the kernel copies it before returning.
    let mut aligned = vec![0u32; user.0.len().div_ceil(4)];
    // SAFETY: the valid queried SID fits in the disjoint aligned buffer.
    unsafe {
        std::ptr::copy_nonoverlapping(user.0.as_ptr(), aligned.as_mut_ptr().cast(), user.0.len());
    }
    let owner = TOKEN_OWNER {
        Owner: aligned.as_mut_ptr().cast(),
    };
    // SAFETY: the live queried token and TOKEN_OWNER/SID storage outlive this
    // call. The only changed information class is the new-object owner.
    if unsafe {
        SetTokenInformation(
            token.0.as_raw_handle() as HANDLE,
            TokenOwner,
            (&owner as *const TOKEN_OWNER).cast(),
            std::mem::size_of::<TOKEN_OWNER>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Call in a dedicated desktop writer before workers or private files exist.
/// No supplied SID/PID, impersonation, elevation, ACL rewrite or file repair.
pub(crate) fn prepare() -> io::Result<()> {
    if Token::of_thread()?.is_some() {
        return Err(refused(
            "desktop private creation requires the process token, not impersonation",
        ));
    }
    let original = Token::current()?;
    let original_id = id(&original)?;
    let identity = original.identity()?;
    if original.default_owner()? != identity.user {
        let writable = writable_current()?;
        if id(&writable)? != original_id || writable.identity()? != identity {
            return Err(refused(
                "desktop process token changed before selecting its creation owner",
            ));
        }
        // The User SID comes only from this actual retained primary token.
        // No group or other principal can be selected by a caller.
        set_owner(&writable, &identity.user)?;
    }
    let current = Token::current()?;
    if id(&current)? != original_id
        || current.identity()? != identity
        || current.default_owner()? != identity.user
        || Token::of_thread()?.is_some()
    {
        return Err(refused(
            "desktop private creation owner could not be verified",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
