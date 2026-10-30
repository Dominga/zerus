//! Durable desktop service kernel. Pre-handoff it owns no fleet or effect writer.
mod fanout;
mod handoff;
mod import_evidence;
mod presence_admission;
mod protocol;
mod registration;
mod revoke;
mod store;
mod transport;

use crate::{cli, config::Config, platform};
use anyhow::{ensure, Context, Result};
use fanout::{Fanout, Queue};
pub(crate) use presence_admission::legacy as legacy_presence;
use protocol::{Hello, ABI};
use serde_json::{json, Value};
use std::{
    io::{BufReader, Read, Write},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use store::{Location, Store};
use transport::{Listener, Stream, TIMEOUT};

const USAGE: &str = "usage: hgs desktop serve [--config-dir DIR --state-dir DIR] | status [--json] | watch --json | request --json | registration --json | revoke --json [--request-id UUID] [--reason rollback] | import-evidence --json | import-status --json --import-id UUID";
const HEARTBEAT: Duration = Duration::from_secs(10);
pub const STARTUP_REFUSED: i32 = 78;

#[derive(Debug)]
struct StartupRefused;
impl std::fmt::Display for StartupRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("desktop startup refused")
    }
}
impl std::error::Error for StartupRefused {}

pub fn registered_command(args: &[String]) -> bool {
    matches!(args, [desktop, serve, config_flag, _, state_flag, _]
        if desktop == "desktop" && serve == "serve"
        && config_flag == "--config-dir" && state_flag == "--state-dir")
}

/// launchd's SuccessfulExit=false retries every nonzero status. Only its exact
/// registered invocation maps permanent startup refusals to a clean exit; the
/// diagnostic remains on stderr. Ordinary CLI errors retain a failure status.
pub fn launchd_command(args: &[String]) -> bool {
    args.last().is_some_and(|arg| arg == "--launchd") && registered_command(&args[..args.len() - 1])
}

/// Registrations pass the selected directories as argv, including on Windows
/// where a logon task cannot carry a custom environment block. Only this exact
/// local service form changes configuration before Config::load; ordinary hgs
/// routing and all existing HGS_* selection remain unchanged.
pub fn launch_configuration(args: &[String]) -> cli::Result<()> {
    if let [desktop, serve, config_flag, config, state_flag, state] = args {
        if desktop == "desktop"
            && serve == "serve"
            && config_flag == "--config-dir"
            && state_flag == "--state-dir"
        {
            if !std::path::Path::new(config).is_absolute()
                || !std::path::Path::new(state).is_absolute()
            {
                return Err(cli::Error::new(
                    STARTUP_REFUSED,
                    "desktop registration directories must be absolute",
                ));
            }
            std::env::set_var("HGS_CONFIG_DIR", config);
            std::env::set_var("HGS_STATE_DIR", state);
        }
    }
    Ok(())
}

pub fn dispatch(config: &Config, target: &str, args: &[String], dry: bool) -> cli::Result<i32> {
    let run = || -> Result<()> {
        ensure!(
            !dry && (target.is_empty() || config.is_self(target)),
            "desktop manages only this computer; remote desktop IPC is refused"
        );
        let location = Location::new(config).map_err(|error| {
            if args.first().is_some_and(|arg| arg == "serve") {
                error.context(StartupRefused)
            } else {
                error
            }
        })?;
        match args
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["serve"] => serve(location),
            ["serve", "--config-dir", dir, "--state-dir", state] => {
                if std::path::Path::new(dir) != config.dir
                    || std::path::Path::new(state) != config.state
                {
                    return Err(anyhow::anyhow!("desktop registration configuration was not selected; invoke its exact local command").context(StartupRefused));
                }
                serve(location)
            }
            ["registration", "--json"] => {
                println!("{}", registration::preview(config, &location)?);
                Ok(())
            }
            ["import-evidence", "--json"] => {
                println!("{}", import_evidence::cli(&location)?);
                Ok(())
            }
            ["import-status", "--json", "--import-id", id] => {
                println!("{}", import_evidence::status(&location, id)?);
                Ok(())
            }
            ["status", "--json"] => {
                println!("{}", status(&location)?);
                Ok(())
            }
            ["status"] => {
                let status = status(&location)?;
                println!(
                    "Desktop service: {}",
                    if status["running"] == true {
                        "running (passive, awaiting handoff)"
                    } else {
                        "not running"
                    }
                );
                Ok(())
            }
            ["watch", "--json"] => watch(&location),
            ["revoke", rest @ ..] => {
                let command = revoke::cli(rest)?;
                let result = match exchange(&location, "request", Some(revoke::payload(&command)?))
                    .with_context(|| {
                        format!(
                            "revoke request {} has no settled receipt; keep Qt stopped",
                            command.id()
                        )
                    })? {
                    Some(result) => result,
                    None => revoke::offline(&location, &command).with_context(|| {
                        format!(
                            "revoke request {} was not settled; keep Qt stopped",
                            command.id()
                        )
                    })?,
                };
                println!("{result}");
                let settled = result["accepted"] == true
                    && result["durable"] == true
                    && result["revoked"] == true
                    && result["checkpointed"] == true;
                let absent = result["applicable"] == false
                    && result["handoff"] == "none"
                    && result["revoked"] == true
                    && result["nothing_written"] == true;
                ensure!((settled || absent) && result["request_id"] == command.id()
                        && result["configuration"] == location.configuration,
                    "desktop revocation is not settled; keep Qt stopped and inspect the same request_id");
                Ok(())
            }
            ["request", "--json"] => {
                let mut bytes = Vec::new();
                std::io::stdin()
                    .take(protocol::CONTROL_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                ensure!(
                    bytes.len() <= protocol::CONTROL_BYTES,
                    "desktop request exceeds its limit"
                );
                let request: Value =
                    serde_json::from_slice(&bytes).context("invalid desktop request")?;
                println!(
                    "{}",
                    exchange(&location, "request", Some(request))?
                        .context("desktop service is not running")?
                );
                Ok(())
            }
            _ => anyhow::bail!(USAGE),
        }
    };
    run().map(|()| 0).map_err(|error| {
        cli::Error::new(
            if error.is::<StartupRefused>() {
                STARTUP_REFUSED
            } else {
                1
            },
            format!("{error:#}"),
        )
    })
}

fn frame(epoch: &str, seq: u64, kind: &str, value: Value) -> Value {
    json!({"abi":ABI,"epoch":epoch,"seq":seq,"t":kind,"value":value})
}
fn projection(
    location: &Location,
    store: &Store,
    epoch: &str,
    subscribers: usize,
    migration: &Value,
) -> Value {
    json!({"abi":ABI,"version":env!("CARGO_PKG_VERSION"),"schema":store::SCHEMA,
        "configuration":location.configuration,"generation":store.generation,"epoch":epoch,
        "running":true,"ownership":"passive","writers_enabled":false,"handoff":"required",
        "upstreams":[],"complete":false,"subscribers":subscribers,"automatic_approvals":false,
        "pending":{"reads":store.read_status()["pending"],"posting":0,"approvals":0},"errors":[],"migration":migration,"effect_journal":store.effect_status(),"read_queue":store.read_status(),
        "catalog_projection":store.catalog_status(),
        "limits":{"subscribers":protocol::SUBSCRIBERS,"queue_bytes":protocol::QUEUE_BYTES,
            "frame_bytes":protocol::FRAME_BYTES,"control_bytes":protocol::CONTROL_BYTES,"ingress_bytes":16<<20},
        "components":{"fleet":"awaiting_handoff","presence":"awaiting_handoff","reads":"awaiting_handoff","attention":"awaiting_handoff","approvals":"awaiting_handoff"}})
}

enum Event {
    Command(Hello, Arc<Queue>),
    ListenerFailed,
}
struct Permit(Arc<AtomicUsize>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn serve(location: Location) -> Result<()> {
    let (_signals, _lock, listener, epoch, mut store) = (|| -> Result<_> {
        let signals = platform::ShutdownGuard::install()?;
        // This dedicated service process has no workers yet. Make socket creation
        // private too, so a concurrent status probe never sees the pre-chmod mode.
        #[cfg(unix)]
        unsafe {
            libc::umask(0o077);
        }
        location.prepare()?;
        let lock = location.lock()?;
        let listener = Arc::new(Listener::bind(&location)?);
        let epoch = uuid::Uuid::new_v4().to_string();
        let store = Store::open(&location, &epoch)?;
        Ok((signals, lock, listener, epoch, store))
    })()
    .context(StartupRefused)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    // At most 16 bounded control lines, plus at most 12 bounded parsers. No
    // upstream ingress exists before handoff; later sources share this actor.
    let (send, receive) = mpsc::sync_channel(protocol::PENDING);
    let acceptor = {
        let (listener, stopped, active) = (listener.clone(), stopped.clone(), active.clone());
        thread::Builder::new()
            .name("desktop-listener".into())
            .spawn(move || {
                while !stopped.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok(stream) => {
                            if active
                                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                                    (n < protocol::CONNECTIONS).then_some(n + 1)
                                })
                                .is_err()
                            {
                                transport::shutdown(&stream);
                                continue;
                            }
                            let permit = Permit(active.clone());
                            let (send, stopped) = (send.clone(), stopped.clone());
                            let _ = thread::Builder::new().name("desktop-client".into()).spawn(
                                move || {
                                    let _permit = permit;
                                    let _ = client(stream, send, stopped);
                                },
                            );
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(50))
                        }
                        Err(_) if stopped.load(Ordering::SeqCst) => break,
                        Err(_) => {
                            let _ = send.try_send(Event::ListenerFailed);
                            break;
                        }
                    }
                }
            })?
    };
    let mut fanout = Fanout::default();
    let mut migration = handoff::inspect(&location);
    let mut seq = 0;
    let mut heartbeat = Instant::now() + HEARTBEAT;
    let mut collect_at = Instant::now();
    let mut failed = false;
    while platform::shutdown_signal() == 0 {
        match receive.recv_timeout(Duration::from_millis(200)) {
            Ok(Event::Command(hello, queue)) => {
                let value = projection(&location, &store, &epoch, fanout.len(), &migration);
                let mut streaming = false;
                let reply = if hello.abi != ABI || hello.configuration != location.configuration {
                    frame(
                        &epoch,
                        seq,
                        "error",
                        json!({"code":"incompatible_configuration"}),
                    )
                } else if hello.op == "watch" && hello.request.is_none() {
                    if fanout.subscribe(queue.clone()) {
                        streaming = true;
                        frame(&epoch, seq, "hello", value)
                    } else {
                        frame(&epoch, seq, "error", json!({"code":"subscriber_limit"}))
                    }
                } else if hello.op == "status" && hello.request.is_none() {
                    frame(&epoch, seq, "status", value)
                } else if hello.op == "request" {
                    let result = request(&location, &mut store, hello.request.as_ref());
                    migration = handoff::inspect(&location);
                    frame(&epoch, seq, "response", result)
                } else {
                    frame(
                        &epoch,
                        seq,
                        "error",
                        json!({"code":"unsupported_operation"}),
                    )
                };
                queue.offer(protocol::encode(&reply)?.into());
                if !streaming {
                    queue.close();
                }
            }
            Ok(Event::ListenerFailed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                failed = true;
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => (),
        }
        if Instant::now() >= collect_at {
            let delay = if store.collect_catalog() {
                Duration::from_millis(250)
            } else {
                HEARTBEAT
            };
            collect_at = Instant::now() + delay;
        }
        if Instant::now() >= heartbeat {
            store.recover_effects(&epoch);
            seq += 1;
            migration = handoff::inspect(&location);
            let value = projection(&location, &store, &epoch, fanout.len(), &migration);
            fanout.publish(protocol::encode(&frame(&epoch, seq, "heartbeat", value))?.into());
            heartbeat = Instant::now() + HEARTBEAT;
        }
    }
    stopped.store(true, Ordering::SeqCst);
    listener.close();
    let _ = acceptor.join();
    drop(fanout);
    for event in receive.try_iter() {
        if let Event::Command(_, queue) = event {
            queue.close();
        }
    }
    let deadline = Instant::now() + TIMEOUT + Duration::from_secs(1);
    while active.load(Ordering::SeqCst) != 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    store.checkpoint()?;
    ensure!(!failed, "desktop listener failed");
    ensure!(
        active.load(Ordering::SeqCst) == 0,
        "desktop connections did not drain"
    );
    Ok(())
}

fn client(
    mut stream: Stream,
    send: mpsc::SyncSender<Event>,
    stopped: Arc<AtomicBool>,
) -> Result<()> {
    // Unix peer credentials require no input. Windows pipe impersonation can
    // require a completed read, so that authentication remains after the hello.
    #[cfg(unix)]
    transport::authenticate(&stream)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let bytes =
        protocol::line(&mut reader, protocol::CONTROL_BYTES)?.context("missing desktop hello")?;
    #[cfg(windows)]
    transport::authenticate(&stream)?;
    let hello: Hello = serde_json::from_slice(&bytes).context("invalid desktop hello")?;
    ensure!(!stopped.load(Ordering::SeqCst), "desktop is stopping");
    let watching = hello.op == "watch";
    let queue = Arc::new(Queue::default());
    send.try_send(Event::Command(hello, queue.clone()))
        .map_err(|_| anyhow::anyhow!("desktop admission is full"))?;
    // Close even if a timeout change or thread creation fails after admission.
    struct CloseQueue(Arc<Queue>);
    impl Drop for CloseQueue {
        fn drop(&mut self) {
            self.0.close();
        }
    }
    let _close = CloseQueue(queue.clone());
    let reader = if watching {
        stream.set_read_timeout(None)?;
        let queue = queue.clone();
        Some(
            thread::Builder::new()
                .name("desktop-detach".into())
                .spawn(move || {
                    // No control operations on the display stream. EOF or any input
                    // detaches only this subscriber; commands use the request lane.
                    let _ = reader.read(&mut [0]);
                    queue.close();
                })?,
        )
    } else {
        None
    };
    let written = (|| -> Result<()> {
        while let Some(frame) = queue.next() {
            stream.write_all(&frame)?;
        }
        Ok(())
    })();
    queue.close();
    transport::shutdown(&stream);
    if let Some(reader) = reader {
        let _ = reader.join();
    }
    written
}

fn request(location: &Location, store: &mut Store, value: Option<&Value>) -> Value {
    let id = value
        .and_then(|v| v["request_id"].as_str())
        .filter(|id| uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == *id));
    let Some(id) = id else {
        return json!({"accepted":false,"code":"invalid_request_id"});
    };
    let op = value.and_then(|v| v["op"].as_str()).unwrap_or("");
    // Revocation only reduces authority. Every positive effect remains fenced.
    if op == "ping" {
        json!({"request_id":id,"accepted":true,"durable":false,"op":"pong"})
    } else if matches!(op, "revoke" | "revoke_status") {
        revoke::request(location, store, value.expect("validated request"))
    } else {
        json!({"request_id":id,"accepted":false,"code":"handoff_required"})
    }
}

fn connection(location: &Location, op: &str, request: Option<Value>) -> Result<Option<Stream>> {
    location.check_existing()?;
    let Some(mut stream) = transport::connect(location)? else {
        return Ok(None);
    };
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    let hello = json!({"abi":ABI,"configuration":location.configuration,"op":op,"request":request});
    let bytes = protocol::encode(&hello)?;
    ensure!(
        bytes.len() <= protocol::CONTROL_BYTES,
        "desktop request exceeds its limit"
    );
    stream.write_all(&bytes)?;
    Ok(Some(stream))
}
fn exchange(location: &Location, op: &str, request: Option<Value>) -> Result<Option<Value>> {
    let revoking = request
        .as_ref()
        .is_some_and(|value| matches!(value["op"].as_str(), Some("revoke" | "revoke_status")));
    let Some(stream) = connection(location, op, request)? else {
        return Ok(None);
    };
    // Revocation includes a bounded SQLite busy wait and durable filesystem I/O.
    // A timeout remains an unknown result, never permission to start Qt.
    if revoking {
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    }
    let bytes = protocol::line(&mut BufReader::new(stream), protocol::FRAME_BYTES)?
        .context("desktop service disconnected")?;
    let response: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        response["abi"] == ABI && response["t"] != "error",
        "desktop service refused the request"
    );
    Ok(Some(response["value"].clone()))
}
fn status(location: &Location) -> Result<Value> {
    Ok(exchange(location, "status", None)?.unwrap_or_else(|| json!({"abi":ABI,"running":false,"ownership":"unavailable","configuration":location.configuration,"writers_enabled":false})))
}
fn watch(location: &Location) -> Result<()> {
    let stream = connection(location, "watch", None)?.context("desktop service is not running")?;
    stream.set_read_timeout(None)?;
    let detached = stream.try_clone()?;
    thread::Builder::new()
        .name("desktop-stdin".into())
        .spawn(move || {
            let _ = std::io::stdin().read(&mut [0]);
            transport::shutdown(&detached);
        })?;
    let mut reader = BufReader::new(stream);
    let mut output = std::io::stdout().lock();
    while let Some(bytes) = protocol::line(&mut reader, protocol::FRAME_BYTES)? {
        output.write_all(&bytes)?;
        output.flush()?;
    }
    Ok(())
}
