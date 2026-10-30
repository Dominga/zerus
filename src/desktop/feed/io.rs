use super::*;
use crate::desktop::{
    self,
    protocol::Hello,
    store::Location,
    transport::{self, Stream},
};
use std::{
    io::{self, BufReader, Read, Write},
    sync::atomic::AtomicBool,
};

struct Closed(Arc<Producer>);
impl Drop for Closed {
    fn drop(&mut self) {
        self.0.close();
    }
}
struct Readable {
    reader: BufReader<Stream>,
    stopped: Arc<AtomicBool>,
    producer: Arc<Producer>,
    until: Instant,
}
impl Read for Readable {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.stopped.load(Ordering::Acquire)
                || !self.producer.alive()
                || Instant::now() >= self.until
            {
                return Err(io::Error::other("desktop feed read stopped"));
            }
            match self.reader.read(bytes) {
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) => {}
                other => return other,
            }
        }
    }
}
fn receive<T>(rx: &mpsc::Receiver<T>, stopped: &AtomicBool) -> Result<T> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        ensure!(
            !stopped.load(Ordering::Acquire) && Instant::now() < until,
            "desktop feed receipt unavailable"
        );
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(value) => return Ok(value),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => anyhow::bail!("desktop feed receipt unavailable"),
        }
    }
}
/// Authentication and the bounded desktop hello have already completed in the
/// common client path. This one thread reads one item and waits for its receipt.
pub(in crate::desktop) fn client(
    mut stream: Stream,
    mut reader: BufReader<Stream>,
    hello: Hello,
    send: mpsc::SyncSender<desktop::Event>,
    stopped: Arc<AtomicBool>,
    ingress: Arc<Ingress>,
) -> Result<()> {
    let (reply, rx) = mpsc::sync_channel(1);
    send.try_send(desktop::Event::FeedOpen(hello, reply))
        .map_err(|_| anyhow::anyhow!("desktop feed admission full"))?;
    let mut opened = receive(&rx, &stopped)?;
    let producer = opened.producer.take();
    let guard = producer.map(Closed);
    stream.write_all(&protocol::encode(&opened.reply)?)?;
    let Some(guard) = guard else {
        return Ok(());
    };
    // Reuse the reader that parsed hello: bytes already buffered after that
    // line belong to this producer too, rather than being silently discarded.
    reader
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(200)))?;
    let mut reader = BufReader::new(Readable {
        reader,
        stopped: stopped.clone(),
        producer: guard.0.clone(),
        until: Instant::now() + Duration::from_secs(30),
    });
    while !stopped.load(Ordering::Acquire) && guard.0.alive() {
        reader.get_mut().until = Instant::now() + Duration::from_secs(30);
        let Some(bytes) = protocol::line(&mut reader, LINE_BYTES)? else {
            break;
        };
        let packet = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid desktop feed item"))?;
        let Some(permit) = ingress.reserve(bytes.len(), &guard.0, &stopped) else {
            break;
        };
        let (reply, rx) = mpsc::sync_channel(1);
        let mut event = desktop::Event::FeedData(Data {
            producer: guard.0.clone(),
            packet,
            reply,
            _permit: permit,
        });
        loop {
            if stopped.load(Ordering::Acquire) || !guard.0.alive() {
                return Ok(());
            }
            match send.try_send(event) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Full(back)) => {
                    event = back;
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return Ok(()),
            }
        }
        let receipt = receive(&rx, &stopped)?;
        stream.write_all(&protocol::encode(&receipt)?)?;
        if receipt["value"]["accepted"] != true {
            break;
        }
    }
    transport::shutdown(&stream);
    Ok(())
}

/// A selected local CLI proxy, never a source launcher. Callers probe the exact
/// help capability before invoking an older selected executable.
pub(in crate::desktop) fn proxy(location: &Location) -> Result<()> {
    let request =
        json!({"abi":ABI,"producer":uuid::Uuid::new_v4().to_string(),"mode":"display_only"});
    let mut stream = desktop::connection(location, "feed", Some(request))?
        .context("desktop service is not running")?;
    stream.set_read_timeout(Some(Duration::from_secs(12)))?;
    let mut replies = BufReader::new(stream.try_clone()?);
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut identity = None;
    loop {
        let bytes = protocol::line(&mut replies, protocol::CONTROL_BYTES)?
            .context("desktop feed disconnected")?;
        let reply: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid desktop feed receipt"))?;
        ensure!(
            reply["abi"] == protocol::ABI && reply["value"]["feed_abi"] == ABI,
            "incompatible desktop feed receipt"
        );
        let current = (
            reply["epoch"].clone(),
            reply["value"]["incarnation"].clone(),
        );
        if let Some(expected) = &identity {
            ensure!(*expected == current, "desktop feed identity changed");
        } else {
            identity = Some(current);
        }
        output.write_all(&bytes)?;
        output.flush()?;
        ensure!(reply["value"]["accepted"] == true, "desktop feed refused");
        let Some(bytes) = protocol::line(&mut input, LINE_BYTES)? else {
            break;
        };
        stream.write_all(&bytes)?;
    }
    transport::shutdown(&stream);
    Ok(())
}
