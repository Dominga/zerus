//! WhiteWh fork (owner, 2026-11-18): a loopback link an agent on a peer names
//! (`http://localhost:5173/report`) points at that peer, not at this computer.
//! It opens through `hgs @peer tunnel --port N`, which forwards a free local
//! port to the peer's `127.0.0.1:N` for as long as Zerus holds its input; the
//! browser gets the same link on the local port. One tunnel serves each peer
//! port and ends with Zerus.
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Mutex, PoisonError};
use std::time::Duration;

/// How long `hgs tunnel` may take to report its local port.
const OPENING: Duration = Duration::from_secs(30);

/// A loopback `http(s)` link: its scheme, port and what follows the authority.
#[derive(Debug, PartialEq, Eq)]
pub struct Loopback {
    scheme: &'static str,
    pub port: u16,
    rest: String,
}

impl Loopback {
    /// `http(s)://localhost|127.0.0.1|0.0.0.0|[::1][:port]...`, without user
    /// information; anything else is not a loopback link.
    pub fn parse(url: &str) -> Option<Self> {
        let (scheme, after) = if let Some(after) = url.strip_prefix("http://") {
            ("http", after)
        } else if let Some(after) = url.strip_prefix("https://") {
            ("https", after)
        } else {
            return None;
        };
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        let (authority, rest) = after.split_at(end);
        if authority.contains('@') {
            return None;
        }
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let (host, tail) = v6.split_once(']')?;
            (format!("[{host}]"), tail.strip_prefix(':'))
        } else {
            match authority.rsplit_once(':') {
                Some((host, port)) => (host.to_owned(), Some(port)),
                None => (authority.to_owned(), None),
            }
        };
        let host = host.to_ascii_lowercase();
        if !matches!(host.as_str(), "localhost" | "127.0.0.1" | "0.0.0.0" | "[::1]") {
            return None;
        }
        let port = match port {
            Some(port) => port.parse::<u16>().ok().filter(|p| *p > 0)?,
            None if scheme == "https" => 443,
            None => 80,
        };
        Some(Self { scheme, port, rest: rest.to_owned() })
    }
    /// The same link on this computer's port `local`.
    pub fn on(&self, local: u16) -> String {
        format!("{}://localhost:{local}{}", self.scheme, self.rest)
    }
}

struct Tunnel {
    child: Child,
    /// Held: its close (Zerus exits) ends the forward.
    _input: ChildStdin,
    local: u16,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
pub struct Tunnels {
    open: Mutex<HashMap<(String, u16), Tunnel>>,
}

impl Tunnels {
    /// The local port forwarded to `port` on the configured peer `alias`,
    /// opening the tunnel when none is running.
    pub fn local_port(&self, hgs: &Path, alias: &str, port: u16) -> Result<u16, String> {
        let key = (alias.to_owned(), port);
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(tunnel) = open.get_mut(&key) {
            if matches!(tunnel.child.try_wait(), Ok(None)) {
                return Ok(tunnel.local);
            }
            open.remove(&key);
        }
        let mut command = Command::new(hgs);
        command
            .args([format!("@{alias}"), "tunnel".into(), "--port".into(), port.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start the tunnel to {alias}: {error}"))?;
        let input = child.stdin.take().ok_or("The tunnel has no input.")?;
        let (line_tx, line_rx) = mpsc::channel();
        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let mut line = String::new();
                let _ = BufReader::new(stdout).take(4096).read_line(&mut line);
                let _ = line_tx.send(line);
            });
        }
        let (error_tx, error_rx) = mpsc::channel();
        if let Some(mut stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let mut text = Vec::new();
                let _ = stderr.by_ref().take(4096).read_to_end(&mut text);
                let _ = std::io::copy(&mut stderr, &mut std::io::sink());
                let _ = error_tx.send(text);
            });
        }
        let reported = line_rx
            .recv_timeout(OPENING)
            .ok()
            .and_then(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok());
        let Some(reported) = reported else {
            let _ = child.kill();
            let _ = child.wait();
            let detail = error_rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
            let detail = String::from_utf8_lossy(&detail);
            let detail = detail.trim().trim_start_matches("hgs: ");
            return Err(if detail.is_empty() {
                format!("The tunnel to {alias} did not open.")
            } else {
                detail.to_owned()
            });
        };
        let local = reported["local_port"]
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p > 0)
            .ok_or("The tunnel reported no local port.")?;
        if reported["held"] == false {
            // The machine shares this computer's loopback: nothing to hold.
            let _ = child.wait();
            return Ok(local);
        }
        open.insert(key, Tunnel { child, _input: input, local });
        Ok(local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_links_are_tunnelled() {
        let link = Loopback::parse("http://localhost:5173/report/index.html?x=1#top").unwrap();
        assert_eq!(link.port, 5173);
        assert_eq!(link.on(61000), "http://localhost:61000/report/index.html?x=1#top");
        assert_eq!(Loopback::parse("http://127.0.0.1:8000").unwrap().on(1), "http://localhost:1");
        assert_eq!(Loopback::parse("http://0.0.0.0:3000/").unwrap().port, 3000);
        assert_eq!(Loopback::parse("https://LOCALHOST/").unwrap().port, 443);
        assert_eq!(Loopback::parse("http://localhost").unwrap().port, 80);
        assert_eq!(Loopback::parse("http://[::1]:9000/a").unwrap().on(2), "http://localhost:2/a");
        for other in [
            "https://example.com/",
            "http://localhost.example.com:80/",
            "http://user@localhost:80/",
            "http://localhost:0/",
            "http://localhost:99999/",
            "file:///C:/report.html",
            "ftp://localhost/",
        ] {
            assert!(Loopback::parse(other).is_none(), "{other}");
        }
    }
}
