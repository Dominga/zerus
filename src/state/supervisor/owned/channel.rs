//! One bounded read-only observation at a time. There is deliberately no effect
//! verb, attach token, reconnect permit or producer on the installed CLI.
use super::native::Source;
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    time::{Duration, Instant},
};
const FRAME: usize = 8192;
const QUERY: Duration = Duration::from_millis(25);
pub(super) struct Channel {
    directory: tempfile::TempDir,
    listener: UnixListener,
}
impl Channel {
    pub(super) fn new() -> io::Result<Self> {
        let directory = tempfile::Builder::new().prefix("hgs-owner-").tempdir()?;
        let listener = UnixListener::bind(directory.path().join("observe"))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            directory,
            listener,
        })
    }
    pub(super) fn path(&self) -> std::path::PathBuf {
        self.directory.path().join("observe")
    }
    pub(super) fn poll(&self, source: &Source, binding: &Value, data: &Value) {
        // Never loop across queued peers or renew a deadline after any phase.
        let until = Instant::now() + QUERY;
        let Ok((mut stream, _)) = self.listener.accept() else {
            return;
        };
        if stream.set_nonblocking(true).is_err() || !source.current() || !source.peer(&stream) {
            return;
        }
        let Ok(request) = read(&mut stream, until) else {
            return;
        };
        let Ok(request) = serde_json::from_slice::<Value>(&request) else {
            return;
        };
        if request != serde_json::json!({"observe":binding})
            || !source.current()
            || !source.peer(&stream)
        {
            return;
        }
        let Ok(mut bytes) = serde_json::to_vec(data) else {
            return;
        };
        if bytes.len() >= FRAME {
            return;
        }
        bytes.push(b'\n');
        if write(&mut stream, &bytes, until).is_ok() {
            // Darwin may stop exposing LOCAL_PEERPID after close. Keep this
            // same native connection alive through the observer's final check.
            // A single completion byte has no effect or admission semantics.
            let mut byte = [0];
            while Instant::now() < until {
                match stream.read(&mut byte) {
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1))
                    }
                    _ => break,
                }
            }
        }
    }
}
fn read(stream: &mut UnixStream, until: Instant) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        if Instant::now() >= until {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut part = [0; 512];
        match stream.read(&mut part) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => {
                if bytes.len() + n > FRAME {
                    return Err(io::Error::other("observation frame too large"));
                }
                bytes.extend_from_slice(&part[..n]);
                if let Some(end) = bytes.iter().position(|b| *b == b'\n') {
                    if end + 1 != bytes.len() {
                        return Err(io::Error::other("multiple observation frames"));
                    }
                    bytes.truncate(end);
                    return Ok(bytes);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(e) => return Err(e),
        }
    }
}
fn write(stream: &mut UnixStream, mut bytes: &[u8], until: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= until {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match stream.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(1))
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// No installed consumer. A future effect controller must NOT use this data as
// permission; original writer/IO, marker, stops and native closure remain gates.
#[cfg(test)]
pub(super) fn query(
    path: &std::path::Path,
    binding: &Value,
    owner: u32,
    birth: &str,
) -> io::Result<Value> {
    let until = Instant::now() + Duration::from_millis(250);
    let selected = crate::platform::image_identity()
        .ok_or_else(|| io::Error::other("selected observer image unavailable"))?;
    let mut stream = connect(path, until)?;
    stream.set_nonblocking(true)?;
    if super::native::image_of(owner) != Some(selected)
        || super::native::peer_pid(&stream) != Some(owner)
        || !super::native::birth(owner).is_some_and(|actual| actual == birth)
    {
        return Err(io::Error::other("original native peer changed"));
    }
    let mut bytes = serde_json::to_vec(&serde_json::json!({"observe":binding}))?;
    if bytes.len() >= FRAME {
        return Err(io::Error::other("observation frame too large"));
    }
    bytes.push(b'\n');
    write(&mut stream, &bytes, until)?;
    let data = read(&mut stream, until)?;
    let reply: Value = serde_json::from_slice(&data)?;
    if reply["binding"] != *binding
        || super::native::image_of(owner) != Some(selected)
        || super::native::peer_pid(&stream) != Some(owner)
        || !super::native::birth(owner).is_some_and(|actual| actual == birth)
    {
        return Err(io::Error::other("original observation binding changed"));
    }
    write(&mut stream, b"!", until)?;
    Ok(reply)
}

#[cfg(test)]
fn connect(path: &std::path::Path, until: Instant) -> io::Result<UnixStream> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    };
    let bytes = path.as_os_str().as_bytes();
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() >= address.sun_path.len() || bytes.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    address.sun_family = libc::AF_UNIX as _;
    for (out, input) in address.sun_path.iter_mut().zip(bytes) {
        *out = *input as _;
    }
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1;
    #[cfg(target_os = "macos")]
    {
        address.sun_len = length as u8;
    }
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    if unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    stream.set_nonblocking(true)?;
    loop {
        if Instant::now() >= until {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let result = unsafe {
            libc::connect(
                stream.as_raw_fd(),
                (&address as *const libc::sockaddr_un).cast(),
                length as libc::socklen_t,
            )
        };
        if result == 0 {
            return Ok(stream);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EISCONN) => return Ok(stream),
            Some(libc::EINPROGRESS) | Some(libc::EALREADY) | Some(libc::EAGAIN) => {
                std::thread::sleep(Duration::from_millis(1))
            }
            _ => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_observation_is_rejected_before_json_parsing() {
        use std::os::fd::AsRawFd;
        let (mut sender, mut receiver) = UnixStream::pair().unwrap();
        let size = (FRAME * 4) as libc::c_int;
        assert_eq!(
            unsafe {
                libc::setsockopt(
                    sender.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_SNDBUF,
                    (&size as *const libc::c_int).cast(),
                    std::mem::size_of_val(&size) as libc::socklen_t,
                )
            },
            0
        );
        sender
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        receiver.set_nonblocking(true).unwrap();
        let mut bytes = vec![b'x'; FRAME + 1];
        bytes.push(b'\n');
        sender.write_all(&bytes).unwrap();
        assert!(read(&mut receiver, Instant::now() + Duration::from_secs(1)).is_err());
    }
}
