//! A bounded input barrier for an original helper that must stay alive while its
//! parent observes its loaded code. Synchronization only: the release bytes do
//! not select a command or grant authority. Ordinary argv dispatch and every
//! native/context/registration fence still apply after the barrier.
use crate::Invocation;
use std::{
    io,
    os::fd::AsRawFd,
    time::{Duration, Instant},
};

pub const ARG: &str = "--desktop-helper-rendezvous-v1";
const RELEASE: &[u8] = b"HGS-ORIGINAL-CHILD-V1\n";
const WAIT: Duration = Duration::from_secs(5);

/// Keep the original command, input, request identity and timeout unchanged
/// behind a fixed public prefix. The owned runner writes it only AFTER its
/// original-child spawn/observation/registration callback returns successfully.
pub fn prepare(original: &Invocation) -> Invocation {
    let mut invocation = original.clone();
    invocation.args.insert(0, ARG.into());
    let mut input = RELEASE.to_vec();
    input.extend_from_slice(original.stdin.as_deref().unwrap_or_default());
    invocation.stdin = Some(input);
    invocation
}

/// CLI entry before any ordinary dispatch (including fast --help). No config,
/// native operation or requested command is inspected/executed while waiting.
/// Read only the fixed prefix from the raw pipe, leaving ALL original stdin
/// bytes for the existing handler. Do not buffer ahead or inherit an env opt-in.
pub fn dispatch_args(mut args: Vec<String>) -> io::Result<Vec<String>> {
    if args.first().is_some_and(|value| value == ARG) {
        if args.len() < 2 || args[1] == ARG {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        wait(std::io::stdin().as_raw_fd(), Instant::now() + WAIT)?;
        args.remove(0);
    }
    Ok(args)
}

fn wait(fd: libc::c_int, until: Instant) -> io::Result<()> {
    let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: borrowed input descriptor and writable stat output. A terminal or
    // regular file is not the original owned helper pipe used by this transport.
    if unsafe { libc::fstat(fd, info.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat initialized the entire value on success.
    if unsafe { info.assume_init() }.st_mode & libc::S_IFMT != libc::S_IFIFO {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut offset = 0;
    while offset < RELEASE.len() {
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout = remaining.as_millis().saturating_add(1).min(i32::MAX as _) as i32;
        // SAFETY: one initialized poll descriptor, no borrowed memory retained.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if ready == 0 {
            continue;
        }
        if poll.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let mut input = [0u8; RELEASE.len()];
        // SAFETY: read at most the missing prefix bytes, never any original
        // private request data. EOF on parent refusal ends without dispatch.
        let count = unsafe { libc::read(fd, input.as_mut_ptr().cast(), RELEASE.len() - offset) };
        if count < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let count = count as usize;
        if input[..count] != RELEASE[offset..offset + count] {
            return Err(io::ErrorKind::InvalidData.into());
        }
        offset += count;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::{Read, Write},
        os::fd::{FromRawFd, OwnedFd},
    };
    fn pipe() -> (File, File) {
        let mut fds = [0; 2];
        // SAFETY: exact two-descriptor output; each result is owned once.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        unsafe {
            (
                File::from(OwnedFd::from_raw_fd(fds[0])),
                File::from(OwnedFd::from_raw_fd(fds[1])),
            )
        }
    }
    #[test]
    fn release_preserves_the_exact_original_input_and_identity() {
        let original = Invocation {
            args: vec!["desktop".into(), "frontend-request".into()],
            stdin: Some(b"private-fixture-frame\n".to_vec()),
            timeout: Duration::from_millis(123),
            request_id: Some("original-request".into()),
        };
        let prepared = prepare(&original);
        assert_eq!(&prepared.args[1..], original.args);
        assert_eq!(prepared.timeout, original.timeout);
        assert_eq!(prepared.request_id, original.request_id);
        let (mut input, mut output) = pipe();
        output.write_all(prepared.stdin.as_ref().unwrap()).unwrap();
        drop(output);
        wait(input.as_raw_fd(), Instant::now() + WAIT).unwrap();
        let mut rest = Vec::new();
        input.read_to_end(&mut rest).unwrap();
        assert_eq!(Some(rest), original.stdin);
    }
    #[test]
    fn eof_wrong_prefix_timeout_and_nonpipe_refuse() {
        for bytes in [b"".as_slice(), &RELEASE[..3], b"untrusted-prefix"] {
            let (input, mut output) = pipe();
            output.write_all(bytes).unwrap();
            drop(output);
            assert!(wait(input.as_raw_fd(), Instant::now() + WAIT).is_err());
        }
        let (input, _output) = pipe();
        assert_eq!(
            wait(input.as_raw_fd(), Instant::now()).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        let file = File::open("/dev/null").unwrap();
        assert_eq!(
            wait(file.as_raw_fd(), Instant::now() + WAIT)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
