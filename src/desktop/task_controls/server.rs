//! The ordinary authenticated transport and Hello ABI, gated before routing.
//! The kernel peer is retained, never reconstructed from a renderer PID.
use super::{authority, wire::Envelope};
use crate::desktop::{
    protocol,
    transport::{self, Stream},
    ui_authority, Event,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    io::Write,
    sync::mpsc,
    time::{Duration, Instant},
};

pub(super) const TOTAL: Duration = Duration::from_secs(10);
pub(in crate::desktop) fn line(
    reader: &mut std::io::BufReader<Stream>,
    limit: usize,
    until: Instant,
) -> Result<Option<Vec<u8>>> {
    protocol::line_checked(reader, limit, |reader| {
        let remaining = until.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "task-control request deadline expired"
        );
        reader.get_ref().set_read_timeout(Some(remaining))?;
        Ok(())
    })
}
pub(in crate::desktop) fn write(stream: &mut Stream, bytes: &[u8], until: Instant) -> Result<()> {
    let mut written = 0;
    while written < bytes.len() {
        let remaining = until.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "task-control request deadline expired"
        );
        stream.set_write_timeout(Some(remaining))?;
        match stream.write(&bytes[written..]) {
            Ok(0) => anyhow::bail!("task-control connection closed"),
            Ok(count) => written += count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.into()),
        }
    }
    ensure!(
        Instant::now() < until,
        "task-control request deadline expired"
    );
    Ok(())
}
pub(in crate::desktop) struct Incoming {
    pub(super) envelope: Envelope,
    pub(super) peer: authority::Peer,
    pub(super) until: Instant,
    pub(super) reply: mpsc::SyncSender<Value>,
}

pub(in crate::desktop) fn client(
    mut stream: Stream,
    original_hello: &[u8],
    send: mpsc::SyncSender<Event>,
    scope: &ui_authority::Scope,
    until: Instant,
) -> Result<()> {
    ensure!(super::enabled(), "task_controls_inactive");
    let envelope =
        Envelope::hello(original_hello, &scope.configuration).map_err(anyhow::Error::msg)?;
    let peer = authority::Peer::capture(&stream, until)?;
    peer.command(&envelope.command, until)?;
    let (reply, receive) = mpsc::sync_channel(1);
    let id = envelope.request_id.clone();
    send.try_send(Event::TaskControl(Box::new(Incoming {
        envelope,
        peer,
        until,
        reply,
    })))
    .map_err(|_| anyhow::anyhow!("task-control admission is full"))?;
    let remaining = until.saturating_duration_since(Instant::now());
    ensure!(!remaining.is_zero(), "task-control deadline expired");
    let value = receive
        .recv_timeout(remaining)
        .context("task-control original outcome is unknown; use status")?;
    ensure!(
        value["value"]["request_id"] == id,
        "task-control original reply changed"
    );
    let remaining = until.saturating_duration_since(Instant::now());
    ensure!(!remaining.is_zero(), "task-control deadline expired");
    stream.set_write_timeout(Some(remaining))?;
    write(&mut stream, &protocol::encode(&value)?, until)?;
    transport::shutdown(&stream);
    Ok(())
}

pub(super) fn refused(id: &str, code: &str, message: &str) -> Value {
    json!({"schema":1,"request_id":id,"ok":false,"attempted":false,
        "code":code,"error":message})
}
pub(super) fn response(reply: &mpsc::SyncSender<Value>, epoch: &str, seq: u64, value: Value) {
    let _ = reply.try_send(crate::desktop::frame(epoch, seq, "response", value));
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn a_real_same_uid_held_peer_does_not_prove_the_requested_cli_command() {
        let (stream, _other) = std::os::unix::net::UnixStream::pair().unwrap();
        let until = Instant::now() + TOTAL;
        let peer = authority::Peer::capture(&stream, until).unwrap();
        let envelope = Envelope::read(
            &serde_json::to_vec(&json!({"schema":1,
            "request_id":"00000000-0000-4000-8000-000000000001",
            "command":{"operation":"status","input":{"schema":1,"target":{"scope":"request",
                "request_id":"00000000-0000-4000-8000-000000000001"}}}}))
            .unwrap(),
        )
        .unwrap();
        peer.current(until).unwrap();
        // The real kernel peer is this test harness, not `hgs task control`.
        // Neither a truthful UID nor JSON operation supplies that command proof.
        assert!(peer.command(&envelope.command, until).is_err());
        assert!(peer.command(&envelope.command, Instant::now()).is_err());
    }
}

#[cfg(all(test, unix))]
mod deadline_tests {
    use super::*;
    use std::{
        io::{BufReader, Read},
        os::unix::net::UnixStream,
    };
    #[test]
    fn partial_frames_keep_the_original_deadline_and_buffered_following_bytes() {
        let (mut out, input) = UnixStream::pair().unwrap();
        let writer = std::thread::spawn(move || {
            let _ = out.write_all(b"a");
            std::thread::sleep(Duration::from_millis(40));
            let _ = out.write_all(b"b");
            std::thread::sleep(Duration::from_millis(80));
            let _ = out.write_all(b"c\n");
        });
        let mut reader = BufReader::new(input);
        assert!(line(
            &mut reader,
            1024,
            Instant::now() + Duration::from_millis(90)
        )
        .is_err());
        writer.join().unwrap();
        let (mut out, input) = UnixStream::pair().unwrap();
        out.write_all(b"abc\nrest").unwrap();
        let mut reader = BufReader::new(input);
        assert_eq!(
            line(&mut reader, 1024, Instant::now() + TOTAL)
                .unwrap()
                .unwrap(),
            b"abc\n"
        );
        assert!(line(&mut reader, 1024, Instant::now()).is_err());
        let mut rest = [0; 4];
        reader.read_exact(&mut rest).unwrap();
        assert_eq!(&rest, b"rest");
    }
}
