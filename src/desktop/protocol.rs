//! Private desktop-v1 framing. Peer watch-v1 is unchanged.
use anyhow::{ensure, Result};
use serde::Deserialize;
use serde_json::Value;
use std::io::BufRead;

pub const ABI: &str = "desktop-v1";
pub const CONTROL_BYTES: usize = zerus_watch_protocol::CONTROL_LINE_BYTES;
pub const FRAME_BYTES: usize = zerus_watch_protocol::MAX_FRAME_BYTES + 4096;
pub const QUEUE_BYTES: usize = 4 << 20;
pub const SUBSCRIBERS: usize = 8;
pub const CONNECTIONS: usize = SUBSCRIBERS + 4 + 1; // one passive producer
pub const PENDING: usize = 16;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub abi: String,
    pub configuration: String,
    pub op: String,
    pub request: Option<Value>,
}

/// Read one complete line without allocating beyond the bound. Truncated and
/// oversize requests close their connection; their payload is never diagnosed.
pub fn line(reader: &mut impl BufRead, limit: usize) -> Result<Option<Vec<u8>>> {
    line_checked(reader, limit, |_| Ok(()))
}

/// The local task-control lane carries its unchanged budget across every
/// partial read. Ordinary callers keep the original no-op check.
pub(super) fn line_checked<R: BufRead>(
    reader: &mut R,
    limit: usize,
    mut check: impl FnMut(&R) -> Result<()>,
) -> Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    loop {
        check(reader)?;
        let part = reader.fill_buf()?;
        if part.is_empty() {
            ensure!(bytes.is_empty(), "incomplete desktop frame");
            return Ok(None);
        }
        let end = part.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(part.len(), |end| end + 1);
        ensure!(
            bytes.len() + count <= limit,
            "desktop frame exceeds its limit"
        );
        bytes.extend_from_slice(&part[..count]);
        reader.consume(count);
        if end.is_some() {
            check(reader)?;
            return Ok(Some(bytes));
        }
    }
}

/// Watch reads may time out while checking the running image. Keep the exact
/// partial frame across those wakes; a timeout never discards its prefix.
#[derive(Default)]
pub(super) struct WatchLine(Vec<u8>);
pub(super) enum ReadTurn {
    Pending,
    End,
    Frame(Vec<u8>),
}
impl WatchLine {
    pub fn read(&mut self, reader: &mut impl BufRead) -> Result<ReadTurn> {
        loop {
            let part = match reader.fill_buf() {
                Ok(part) => part,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    return Ok(ReadTurn::Pending);
                }
                Err(error) => return Err(error.into()),
            };
            if part.is_empty() {
                ensure!(self.0.is_empty(), "incomplete desktop frame");
                return Ok(ReadTurn::End);
            }
            let end = part.iter().position(|byte| *byte == b'\n');
            let count = end.map_or(part.len(), |end| end + 1);
            ensure!(
                self.0.len() + count <= FRAME_BYTES,
                "desktop frame exceeds its limit"
            );
            self.0.extend_from_slice(&part[..count]);
            reader.consume(count);
            if end.is_some() {
                return Ok(ReadTurn::Frame(std::mem::take(&mut self.0)));
            }
        }
    }
}

pub fn encode(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    ensure!(
        bytes.len() <= FRAME_BYTES,
        "desktop frame exceeds its limit"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_refuses_truncation_and_bounds_allocation() {
        assert!(line(&mut &b"abc"[..], 4).is_err());
        assert!(line(&mut &b"abcd\n"[..], 4).is_err());
        assert_eq!(line(&mut &b"abc\nrest"[..], 4).unwrap().unwrap(), b"abc\n");
        assert!(line(&mut &b""[..], 4).unwrap().is_none());
    }

    struct Timed {
        chunks: std::collections::VecDeque<Option<Vec<u8>>>,
        current: Vec<u8>,
    }
    impl std::io::Read for Timed {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            unreachable!()
        }
    }
    impl BufRead for Timed {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            if self.current.is_empty() {
                match self.chunks.pop_front() {
                    Some(Some(bytes)) => self.current = bytes,
                    Some(None) => return Err(std::io::ErrorKind::TimedOut.into()),
                    None => (),
                }
            }
            Ok(&self.current)
        }
        fn consume(&mut self, amount: usize) {
            self.current.drain(..amount);
        }
    }
    #[test]
    fn replacement_wakes_preserve_partial_frames_and_still_refuse_truncation_and_overflow() {
        let mut input = Timed {
            current: Vec::new(),
            chunks: [
                Some(b"hel".to_vec()),
                None,
                Some(b"lo\n".to_vec()),
                None,
                Some(b"partial".to_vec()),
            ]
            .into(),
        };
        let mut line = WatchLine::default();
        assert!(matches!(line.read(&mut input).unwrap(), ReadTurn::Pending));
        assert!(matches!(line.read(&mut input).unwrap(), ReadTurn::Frame(v) if v == b"hello\n"));
        assert!(matches!(line.read(&mut input).unwrap(), ReadTurn::Pending));
        assert!(line.read(&mut input).is_err());
        let mut input = &vec![b'x'; FRAME_BYTES + 1][..];
        assert!(WatchLine::default().read(&mut input).is_err());
        assert!(matches!(
            WatchLine::default().read(&mut &b""[..]).unwrap(),
            ReadTurn::End
        ));
    }
}
