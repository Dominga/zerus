//! WhiteWh fork (owner, 2026-11-18): a site an agent serves on its peer's own
//! loopback (`http://localhost:5173`) opens on this computer through an SSH
//! local forward. `hgs @host tunnel --port N` binds a free loopback port here,
//! forwards it to the peer's `127.0.0.1:N`, prints
//! `{"host","port","local_port","held"}` once the forward accepts, and holds it
//! until its standard input closes (its owner stopped or exited) or ssh ends.
//! A WSL machine shares this computer's loopback: its port is printed as is.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use serde_json::json;
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// How long the forward may take to accept its first connection.
const READY: Duration = Duration::from_secs(20);

pub fn dispatch(config: &Config, host: &str, args: &[String], dry: bool) -> Result<i32> {
    if host.is_empty()
        || config.is_self(host)
        || !config.peers.iter().any(|p| p == host)
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        return Err(Error::new(1, "Choose a configured remote machine for this tunnel."));
    }
    let port = port(args)?;
    if crate::wsl::profile(config, host).is_some() {
        if dry {
            println!("WSL shares this computer's loopback: port {port}");
            return Ok(0);
        }
        println!(
            "{}",
            json!({"host":host,"port":port,"local_port":port,"held":false})
        );
        return Ok(0);
    }
    let local = free_port()?;
    let mut command = platform::ssh();
    command
        .args(crate::machines::ssh_options(config, host))
        .args([
            "-N",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=8",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "ControlPath=none",
            "-L",
        ])
        .arg(format!("127.0.0.1:{local}:127.0.0.1:{port}"))
        .arg(host);
    if dry {
        println!("{command:?}");
        return Ok(0);
    }
    command.stdout(std::process::Stdio::null());
    let mut ssh = platform::spawn_ssh(&mut command, false, false, true).map_err(|e| {
        Error::new(1, format!("Cannot start the tunnel (OpenSSH ssh is required): {e}"))
    })?;
    let errors = drain(ssh.stderr.take());
    let deadline = Instant::now() + READY;
    let address = SocketAddr::from(([127, 0, 0, 1], local));
    loop {
        if let Some(status) = ssh.child.try_wait()? {
            let text = errors.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
            let detail = String::from_utf8_lossy(&text).trim().to_owned();
            return Err(Error::new(
                1,
                if detail.is_empty() {
                    format!("The tunnel to {host} ended ({status}).")
                } else {
                    format!("The tunnel to {host} ended: {detail}")
                },
            ));
        }
        if TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = ssh.child.kill();
            let _ = ssh.child.wait();
            return Err(Error::new(1, format!("The tunnel to {host} did not open in time.")));
        }
        thread::sleep(Duration::from_millis(200));
    }
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "{}",
        json!({"host":host,"port":port,"local_port":local,"held":true})
    )?;
    out.flush()?;
    drop(out);
    // The owner holds this process's input: its end (a stop, or the owner's
    // exit) ends the forward. Without an input the forward lasts until ssh
    // ends.
    let (closed, input_ended) = mpsc::channel();
    thread::spawn(move || {
        let mut sink = [0u8; 256];
        let mut input = io::stdin();
        while matches!(input.read(&mut sink), Ok(n) if n > 0) {}
        let _ = closed.send(());
    });
    loop {
        if let Some(status) = ssh.child.try_wait()? {
            return Ok(status.code().unwrap_or(1));
        }
        if input_ended.recv_timeout(Duration::from_millis(500)).is_ok() {
            let _ = ssh.child.kill();
            let _ = ssh.child.wait();
            return Ok(0);
        }
    }
}

/// The one `--port N` argument, 1 to 65535.
fn port(args: &[String]) -> Result<u16> {
    match args {
        [flag, value] if flag == "--port" => value.parse::<u16>().ok().filter(|p| *p > 0),
        _ => None,
    }
    .ok_or_else(usage)
}

fn free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| Error::new(1, format!("Cannot reserve a local port: {e}")))?;
    Ok(listener.local_addr()?.port())
}

/// Read stderr to its end so ssh never blocks on it; the first 4 KiB return.
fn drain(stderr: Option<Box<dyn Read + Send>>) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut text = Vec::new();
        if let Some(mut errors) = stderr {
            let _ = errors.by_ref().take(4096).read_to_end(&mut text);
            let _ = io::copy(&mut errors, &mut io::sink());
        }
        let _ = sender.send(text);
    });
    receiver
}

fn usage() -> Error {
    Error::new(2, "usage: hgs @host tunnel --port PORT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tunnel_takes_exactly_one_port() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(port(&args(&["--port", "5173"])).unwrap(), 5173);
        for bad in [&["--port", "0"][..], &["--port", "65536"], &["--port", "x"], &["--port"], &[], &["--port", "1", "--port", "2"], &["--path", "1"]] {
            assert!(port(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_free_port_is_a_loopback_port() {
        let port = free_port().unwrap();
        assert!(port > 0);
        TcpListener::bind(("127.0.0.1", port)).unwrap();
    }
}
