//! Exact production InputOwner copy, pinned by the initial-call source guard.
use std::io;
#[cfg(unix)]
use std::{
    net::Shutdown,
    os::{fd::AsFd, unix::net::UnixStream},
};

pub(super) struct InputOwner {
    #[cfg(unix)]
    socket: Option<UnixStream>,
    #[cfg(unix)]
    owner_pid: u32,
}
impl InputOwner {
    pub(super) fn acquire(writer: &std::process::ChildStdin, has_input: bool) -> io::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                socket: if has_input {
                    Some(UnixStream::from(writer.as_fd().try_clone_to_owned()?))
                } else {
                    None
                },
                owner_pid: std::process::id(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (writer, has_input);
            Ok(Self {})
        }
    }
    fn release(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            let owned = self.owner_pid == std::process::id();
            // A fallible finish is never retried by Drop, even after failure.
            self.owner_pid = 0;
            if owned {
                if let Some(socket) = &self.socket {
                    return socket.shutdown(Shutdown::Write);
                }
            }
        }
        Ok(())
    }
    /// Called after the actual write/flush or stream callback finishes. EOF
    /// does not establish complete input, a valid reply or physical retirement.
    pub(super) fn finish(mut self) -> io::Result<()> {
        self.release()
    }
}
impl Drop for InputOwner {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
