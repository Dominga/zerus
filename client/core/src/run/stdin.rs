//! Input transport for noninteractive helpers. This is not a spawn policy or
//! an operation receipt: only the original writer may end its socket input.
use std::{
    io,
    process::{Child, Command},
};
#[cfg(unix)]
use std::{
    net::Shutdown,
    os::{
        fd::{AsFd, OwnedFd},
        unix::net::UnixStream,
    },
    process::{ChildStdin, Stdio},
};

/// The runner's one prepared stdin. Spawn through this adapter so the guard
/// sees the original writer in `Child.stdin` and can withhold it as before.
/// Dropping/closing that field is never followed by a replacement writer.
pub struct InputSpawn {
    #[cfg(unix)]
    child: OwnedFd,
    #[cfg(unix)]
    writer: ChildStdin,
}
impl InputSpawn {
    pub(super) fn new(has_input: bool) -> io::Result<Self> {
        #[cfg(unix)]
        {
            let turn = crate::unix_spawn::turn();
            Self::new_in_turn(has_input, &turn)
        }
        #[cfg(not(unix))]
        {
            let _ = has_input;
            Ok(Self {})
        }
    }
    #[cfg(unix)]
    pub(super) fn new_in_turn(has_input: bool, turn: &crate::unix_spawn::Turn) -> io::Result<Self> {
        let (child, writer) = if has_input {
            let (writer, child) = crate::unix_spawn::socket_pair_in_turn(turn)?;
            (OwnedFd::from(child), OwnedFd::from(writer))
        } else {
            crate::unix_spawn::null_pair_in_turn(turn)?
        };
        Ok(Self {
            child,
            writer: ChildStdin::from(writer),
        })
    }
    #[cfg(all(test, unix))]
    pub(super) fn observe_input(&self) -> io::Result<std::fs::File> {
        Ok(std::fs::File::from(
            self.child.as_fd().try_clone_to_owned()?,
        ))
    }
    /// Configure stdin and attach its original writer BEFORE the host applies
    /// any post-spawn image/context/registration checks.
    pub fn spawn(self, command: &mut Command) -> io::Result<Child> {
        self.spawn_with(command, crate::child_environment::spawn)
    }
    /// Keep the host's existing spawn turn or Windows flags. The supplied
    /// function only starts the child; post-spawn guards run on our returned
    /// Child, after the original writer has been attached.
    pub fn spawn_with(
        self,
        command: &mut Command,
        spawn: impl FnOnce(&mut Command) -> io::Result<Child>,
    ) -> io::Result<Child> {
        #[cfg(unix)]
        {
            command.stdin(Stdio::from(self.child));
            let spawned = spawn(command);
            // Command retains custom stdio after spawn. Retire our copy of
            // the child's read end now, including on failure, so an early
            // child close can actually end/fail the original writer.
            command.stdin(Stdio::null());
            let mut child = spawned?;
            child.stdin = Some(self.writer);
            Ok(child)
        }
        #[cfg(not(unix))]
        spawn(command)
    }
}

/// Holds a shutdown descriptor only AFTER the guarded spawn accepts its
/// original writer. A refused guard can close stdin immediately, without a
/// hidden duplicate keeping it open while that same guard observes the child.
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn fork_stdin_child_owner_drop_cannot_half_close_the_parent_input() {
        let input = InputSpawn::new(true).unwrap();
        let mut peer = UnixStream::from(input.child);
        let mut writer = input.writer;
        let mut owner = InputOwner::acquire(&writer, true).unwrap();
        // Only the fork child destroys this copy and then calls _exit.
        let _held = crate::run::input_tests::HeldFork::after(|| unsafe {
            std::ptr::drop_in_place(&mut owner)
        });
        peer.set_read_timeout(Some(std::time::Duration::from_millis(100)))
            .unwrap();
        let mut byte = [0];
        assert!(matches!(
            peer.read(&mut byte).unwrap_err().kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ));
        writer.write_all(b"parent still owns input").unwrap();
        drop(writer);
        owner.finish().unwrap();
        let mut bytes = Vec::new();
        peer.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"parent still owns input");
    }
}
