//! One private, read-only native checkout holder. Its original stdin is the
//! sole control stream; reconnects cannot adopt it. Peer sockets can request
//! ONLY an already-armed DATA loan, never arm/retire a phase or send signals.
use super::*;
use crate::platform::worktree_membership::objects::loan::{holder::Holder, peer::Peer};
use std::{
    io::Write,
    os::{
        fd::{AsRawFd, RawFd},
        unix::net::UnixListener,
    },
};
use zerus_client_core::worktree_programs::custody::{
    self, Binding, Control, NativeChild, Reply, FRAME_BYTES, MAX_PHASES,
};

const SETUP_WAIT: Duration = Duration::from_secs(5);
const MAX_CONTROLS: usize = 3 * MAX_PHASES + 2;

fn failed(error: impl std::fmt::Display) -> String {
    error.to_string()
}
fn ready(fd: RawFd, until: Instant) -> Result<bool> {
    loop {
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("The original checkout holder expired".into());
        }
        let mut item = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let used = unsafe {
            libc::poll(
                &mut item,
                1,
                remaining
                    .as_millis()
                    .saturating_add(1)
                    .min(i32::MAX as u128) as i32,
            )
        };
        if used > 0 {
            if item.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err(INVALID.into());
            }
            return Ok(item.revents & (libc::POLLIN | libc::POLLHUP) != 0);
        }
        if used < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
            return Err(failed(io::Error::last_os_error()));
        }
    }
}
#[derive(Default)]
struct Reader {
    pending: Vec<u8>,
    used: usize,
}
impl Reader {
    fn next(&mut self, fd: RawFd, until: Instant) -> Result<Control> {
        if self.used >= MAX_CONTROLS {
            return Err(INVALID.into());
        }
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let remaining = self.pending.split_off(end + 1);
                let bytes = std::mem::replace(&mut self.pending, remaining);
                self.used += 1;
                return Control::decode(&bytes).map_err(failed);
            }
            if self.pending.len() >= FRAME_BYTES {
                return Err(INVALID.into());
            }
            ready(fd, until)?;
            let mut buffer = [0; 8192];
            let cap = buffer.len().min(FRAME_BYTES - self.pending.len());
            let n = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), cap) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(failed(e));
            }
            if n == 0 {
                return Err("The original checkout holder control stream closed".into());
            }
            self.pending.extend_from_slice(&buffer[..n as usize]);
        }
    }
}
fn emit(out: &mut impl Write, reply: &Reply) -> Result<()> {
    out.write_all(&reply.encode(now() as u64).map_err(failed)?)
        .and_then(|_| out.flush())
        .map_err(failed)
}

/// Only custody DATA is implemented here. This command neither schedules an
/// effect nor advertises a stop/Force capability. Native callers must separately
/// retain Source/Actions and supply their actual-join control provenance.
pub(super) fn run(args: &[String]) -> Result<i32> {
    if args != ["--json"] {
        return Err(
            "usage: hgs worktrees program-holder --json (private custody stream on stdin)".into(),
        );
    }
    let config = selected_config()?;
    serve(&config, io::stdin().as_raw_fd(), &mut io::stdout().lock())?;
    Ok(0)
}

fn serve(config: &crate::config::Config, input: RawFd, out: &mut impl Write) -> Result<()> {
    let mut reader = Reader::default();
    let Control::Open {
        pipeline,
        discovery,
        presentation,
    } = reader.next(input, Instant::now() + SETUP_WAIT)?
    else {
        return Err(INVALID.into());
    };
    let bytes = serde_json::to_vec(&discovery).map_err(failed)?;
    let (manifest, scope, presentation) = discover_presented(config, &bytes, presentation)?;
    let until = Instant::now() + Duration::from_secs(manifest.expires.saturating_sub(now() as u64));
    // Custody setup never writes inside the selected checkout, even when the
    // caller's TMPDIR points there. Paths are data; native object checks decide.
    let temp = std::env::temp_dir();
    if scope.protects(&[&temp], until).map_err(failed)? {
        return Err(INVALID.into());
    }
    let socket_dir = tempfile::Builder::new()
        .prefix("hgs-wt-")
        .tempdir_in(temp)
        .map_err(failed)?;
    let socket = socket_dir.path().join("pins");
    if socket.as_os_str().len() > 100 {
        return Err(INVALID.into());
    }
    let listener = UnixListener::bind(&socket).map_err(failed)?;
    listener.set_nonblocking(true).map_err(failed)?;
    let original = crate::platform::process_identity(std::process::id()).ok_or(INVALID)?;
    let mut holder =
        Holder::new(scope, manifest.clone(), pipeline.clone(), now() as u64).map_err(failed)?;
    emit(
        out,
        &Reply::Ready {
            abi: custody::ABI.into(),
            pipeline: pipeline.clone(),
            manifest: Box::new(manifest.clone()),
            presentation,
            holder: NativeChild {
                pid: std::process::id(),
                parent: original.parent,
                birth: original.birth,
                audit: crate::platform::worktree_membership::objects::loan::peer::own_audit()
                    .map_err(failed)?,
            },
            socket: socket.to_str().ok_or(INVALID)?.into(),
        },
    )?;
    let mut sockets = 0;
    loop {
        if Instant::now() >= until {
            holder.close();
            return Err(INVALID.into());
        }
        // The sole control reader and local socket listener share one bounded
        // loop; no detached reader may outlive the original holder's custody.
        let mut fds = [
            libc::pollfd {
                fd: input,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, 50) };
        if n < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(failed(io::Error::last_os_error()));
        }
        if !reader.pending.is_empty() || fds[0].revents != 0 {
            let frame = reader.next(input, until)?;
            if frame.pipeline() != pipeline {
                return Err(INVALID.into());
            }
            match frame {
                Control::Arm {
                    input,
                    borrower,
                    deadline_millis,
                    ..
                } => {
                    if owner(&manifest.capture)?["id"] != manifest.capture.owner_id {
                        return Err(INVALID.into());
                    }
                    let deadline =
                        until.min(custody::clock::restore(deadline_millis).map_err(failed)?);
                    let original = Peer::pin(
                        borrower.pid,
                        &borrower.birth,
                        borrower.parent,
                        borrower.audit.as_ref(),
                        deadline,
                    )
                    .map_err(failed)?;
                    holder
                        .arm(&input, original, now() as u64, deadline)
                        .map_err(failed)?;
                    emit(
                        out,
                        &Reply::Armed {
                            binding: Binding::for_input(&pipeline, &input),
                        },
                    )?;
                }
                Control::ArmCleanup {
                    input,
                    borrower,
                    deadline_millis,
                    ..
                } => {
                    if owner(&manifest.capture)?["id"] != manifest.capture.owner_id {
                        return Err(INVALID.into());
                    }
                    let deadline =
                        until.min(custody::clock::restore(deadline_millis).map_err(failed)?);
                    let original = Peer::pin(
                        borrower.pid,
                        &borrower.birth,
                        borrower.parent,
                        borrower.audit.as_ref(),
                        deadline,
                    )
                    .map_err(failed)?;
                    holder
                        .arm_cleanup(&input, original, now() as u64, deadline)
                        .map_err(failed)?;
                    emit(
                        out,
                        &Reply::Armed {
                            binding: Binding::for_cleanup(&pipeline, &input),
                        },
                    )?;
                }
                Control::Joined { binding } => {
                    holder
                        .joined(&binding.bytes().map_err(failed)?)
                        .map_err(failed)?;
                    emit(out, &Reply::Retired { binding })?;
                }
                Control::Observe { observation, .. } => {
                    let ownership = owner(&manifest.capture)?;
                    let observed = holder
                        .observe(now() as u64, until.min(Instant::now() + SETUP_WAIT))
                        .map_err(failed)?;
                    if owner(&manifest.capture)? != ownership {
                        return Err(INVALID.into());
                    }
                    emit(
                        out,
                        &Reply::Observed {
                            pipeline: pipeline.clone(),
                            observation,
                            manifest: Box::new(observed),
                        },
                    )?;
                }
                Control::Close { .. } => {
                    holder.close();
                    if !holder.settled() {
                        return Err(INVALID.into());
                    }
                    emit(out, &Reply::Closed { pipeline })?;
                    return Ok(());
                }
                Control::Open { .. } => return Err(INVALID.into()),
            }
        }
        if fds[1].revents & libc::POLLIN != 0 {
            sockets += 1;
            if sockets > 4 * MAX_PHASES {
                return Err(INVALID.into());
            }
            let (socket, _) = listener.accept().map_err(failed)?;
            // Invalid peer/frame is refused before delivery. It cannot retire
            // or replace the original phase; the owner must still physically join.
            let _refused = holder.lend_next(&socket);
        }
    }
}

#[cfg(test)]
mod tests;
