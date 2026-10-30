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
    let mut bytes = Vec::new();
    loop {
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
            return Ok(Some(bytes));
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
}
