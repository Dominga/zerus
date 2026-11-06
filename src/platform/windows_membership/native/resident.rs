//! Dormant, one-original helper transport. Bytes are DATA, never coverage or
//! deletion permission. There is no production entry/caller in this leaf.
use super::{
    super::custody_input::Writer,
    child::{Attempt, Pipe},
};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, PoisonError,
    },
    time::Instant,
};

const INPUT_CAP: usize = 16 << 20;
const OUTPUT_CAP: usize = 16 << 20;
const ERROR_CAP: usize = 64 << 10;
static CLAIMED: AtomicBool = AtomicBool::new(false);
static RETIRING: Mutex<Option<Original>> = Mutex::new(None);

struct Lease;
impl Lease {
    fn acquire() -> io::Result<Self> {
        CLAIMED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| io::ErrorKind::WouldBlock.into())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        CLAIMED.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failure {
    Admission,
    Input,
    Output,
    Overflow,
    Deadline,
    Exit,
    Abandoned,
}
pub(super) struct Bytes {
    pub(super) request: uuid::Uuid,
    /// Still untrusted. The later CLOSED protocol must bind and decode this;
    /// neither an empty reply nor successful transport establishes clear.
    pub(super) stdout: Vec<u8>,
}
pub(super) enum Progress {
    Pending { retiring: bool },
    Settled(std::result::Result<Bytes, Failure>),
    Consumed,
}

type Stream = super::super::custody_output::Reader<Pipe>;

struct Original {
    attempt: Attempt,
    request: uuid::Uuid,
    until: Instant,
    streams: [Stream; 2],
    writer: Option<Writer<Pipe>>,
    offered: bool,
    fault: Option<Failure>,
    stop_sent: bool,
    _lease: Lease,
}
impl Original {
    fn refuse(&mut self, failure: Failure) {
        self.fault.get_or_insert(failure);
    }
    fn stop(&mut self) {
        if !self.stop_sent {
            self.stop_sent = true;
            self.attempt.close_input();
            // Only the retained ORIGINAL process/job. No observed PID or
            // process group is accepted; this does not claim exit or EOF.
            self.attempt.stop();
        }
    }
    /// Return Some only after ALL original resources physically settle. A
    /// failed/read-error stream remains owned until actual EOF is observed.
    fn poll(&mut self) -> Option<u32> {
        if Instant::now() >= self.until {
            self.refuse(Failure::Deadline);
        }
        if self.attempt.refusal().is_some() {
            self.refuse(Failure::Admission);
        }
        for stream in &mut self.streams {
            stream.poll();
        }
        if self.streams.iter().any(|s| s.overflow()) {
            self.refuse(Failure::Overflow);
        }
        if self.streams.iter().any(|s| s.failed) {
            self.refuse(Failure::Output);
        }
        if let Some(writer) = self.writer.as_mut() {
            writer.poll();
            if writer.joined && !writer.ok {
                self.refuse(Failure::Input);
            }
        }
        if self.fault.is_some() {
            self.stop();
        }
        let exit = match self.attempt.exit() {
            Ok(exit) => exit,
            Err(_) => {
                self.refuse(Failure::Exit);
                None
            }
        };
        if exit.is_some() && self.writer.is_none() {
            self.refuse(Failure::Admission);
        }
        let job_empty = match self.attempt.job_empty() {
            Ok(empty) => empty,
            Err(_) => {
                self.refuse(Failure::Exit);
                false
            }
        };
        let input_joined = self.writer.as_ref().map_or(self.stop_sent, |w| w.joined);
        if Instant::now() >= self.until {
            self.refuse(Failure::Deadline);
        }
        if self.fault.is_some() {
            self.stop();
        }
        exit.filter(|_| job_empty && input_joined && self.streams.iter().all(|s| s.joined && s.eof))
    }
    fn result(mut self, exit: u32) -> std::result::Result<Bytes, Failure> {
        if exit != 0 {
            self.refuse(Failure::Exit);
        }
        if !self.streams[1].bytes.is_empty() {
            self.refuse(Failure::Output);
        }
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        Ok(Bytes {
            request: self.request,
            stdout: std::mem::take(&mut self.streams[0].bytes),
        })
    }
}

#[must_use]
pub(super) struct Resident(Option<Original>);
impl Resident {
    pub(super) fn start(request: uuid::Uuid, until: Instant) -> io::Result<Self> {
        if request.is_nil() || Instant::now() >= until {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        reap();
        let lease = Lease::acquire()?;
        let mut attempt = Attempt::start(until)?;
        // This newly created Attempt has never loaned its outputs. Taking them
        // cannot fail without an internal programming error; panic leaves the
        // same Attempt/job under its failure backstop, never a successful drain.
        let [stdout, stderr] = attempt.outputs().expect("original unloaned outputs");
        let streams = [
            Stream::new(stdout, OUTPUT_CAP),
            Stream::new(stderr, ERROR_CAP),
        ];
        let fault = if attempt.refusal().is_some() {
            Some(Failure::Admission)
        } else if streams.iter().any(|s| s.failed) {
            Some(Failure::Output)
        } else {
            None
        };
        let mut original = Original {
            attempt,
            request,
            until,
            streams,
            writer: None,
            offered: false,
            fault,
            stop_sent: false,
            _lease: lease,
        };
        if original.fault.is_some() {
            original.stop();
        }
        Ok(Self(Some(original)))
    }
    /// Low-level ONE input handoff only. The later fixed protocol/caller must
    /// provide its already bound frame; this method grants no operation rights.
    pub(super) fn offer(&mut self, frame: Vec<u8>) -> bool {
        let Some(original) = self.0.as_mut() else {
            return false;
        };
        // Observe any startup/read/admission failure before the ONE handoff.
        // This poll cannot lock a live input/output worker or replace a child.
        original.poll();
        if std::mem::replace(&mut original.offered, true)
            || frame.is_empty()
            || frame.len() > INPUT_CAP
            || Instant::now() >= original.until
            || original.fault.is_some()
        {
            original.refuse(Failure::Input);
            original.stop();
            return false;
        }
        let Some(pipe) = original.attempt.input() else {
            original.refuse(Failure::Admission);
            original.stop();
            return false;
        };
        let writer = Writer::start(pipe, frame);
        let started = writer.started();
        original.writer = Some(writer);
        if !started {
            original.refuse(Failure::Input);
            original.stop();
        }
        started
    }
    pub(super) fn poll(&mut self) -> Progress {
        let Some(original) = self.0.as_mut() else {
            return Progress::Consumed;
        };
        if let Some(exit) = original.poll() {
            return Progress::Settled(self.0.take().unwrap().result(exit));
        }
        Progress::Pending {
            retiring: original.fault.is_some(),
        }
    }
}
impl Drop for Resident {
    fn drop(&mut self) {
        let Some(mut original) = self.0.take() else {
            return;
        };
        original.refuse(Failure::Abandoned);
        original.stop();
        if original.poll().is_some() {
            return;
        }
        let mut slot = RETIRING.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(original);
        } else {
            // Impossible with the one-original lease. Keep exact resources on
            // invariant failure; freeing a job/pipe is NEVER a drain witness.
            std::mem::forget(original);
        }
    }
}
fn reap() {
    let mut slot = RETIRING.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.as_mut().is_some_and(|o| o.poll().is_some()) {
        // Discard original data; no replay, re-offer or later PID adoption.
        slot.take();
    }
}
pub(super) fn retirement_pending() -> bool {
    reap();
    RETIRING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};

    fn finish(original: &mut Resident, until: Instant) -> std::result::Result<Bytes, Failure> {
        loop {
            match original.poll() {
                Progress::Settled(result) => return result,
                Progress::Pending { .. } => {
                    assert!(Instant::now() < until, "private physical-settlement bound");
                    thread::sleep(Duration::from_millis(2));
                }
                Progress::Consumed => panic!("original result was already consumed"),
            }
        }
    }

    #[test]
    fn native_original_exit_without_eof_cannot_release_admission_or_adopt_old_data() {
        assert!(!retirement_pending());
        let until = Instant::now() + Duration::from_secs(20);
        let request = uuid::Uuid::new_v4();
        let mut original = Resident::start(request, until).unwrap();
        // Only the fixture holds this additional OWN writer. It cannot keep
        // the child alive, but must prevent a physical stdout-EOF witness.
        let hold = original
            .0
            .as_ref()
            .unwrap()
            .attempt
            .hold_original_stdout()
            .unwrap();
        assert!(original.offer(b"original-pipe-frame\n".to_vec()));
        loop {
            assert!(matches!(original.poll(), Progress::Pending { .. }));
            let retained = original.0.as_ref().unwrap();
            if retained.attempt.exit().unwrap().is_some() && retained.attempt.job_empty().unwrap() {
                assert!(retained.writer.as_ref().unwrap().joined);
                assert!(!retained.streams[0].eof);
                break;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(2));
        }
        let replacement = Resident::start(uuid::Uuid::new_v4(), until);
        assert_eq!(replacement.err().unwrap().kind(), io::ErrorKind::WouldBlock);
        drop(hold);
        let reply = finish(&mut original, until).unwrap();
        assert_eq!(reply.request, request);
        assert!(String::from_utf8(reply.stdout)
            .unwrap()
            .contains("original-pipe-ack"));
        assert!(matches!(original.poll(), Progress::Consumed));
        assert!(!CLAIMED.load(Ordering::Acquire));

        // Drop is a refusal, not a release witness. Keep the SAME process/job,
        // input/read endpoints and request in the one-original retirement slot.
        let request = uuid::Uuid::new_v4();
        let until = Instant::now() + Duration::from_secs(20);
        let original = Resident::start(request, until).unwrap();
        let hold = original
            .0
            .as_ref()
            .unwrap()
            .attempt
            .hold_original_stdout()
            .unwrap();
        drop(original);
        assert!(retirement_pending());
        assert_eq!(RETIRING.lock().unwrap().as_ref().unwrap().request, request);
        assert!(CLAIMED.load(Ordering::Acquire));
        assert_eq!(
            Resident::start(uuid::Uuid::new_v4(), until)
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        drop(hold);
        while retirement_pending() {
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(2));
        }
        assert!(!CLAIMED.load(Ordering::Acquire));

        // A NEW request is possible only after that original truly settled;
        // its old refused data is not returned or used by the new request.
        let request = uuid::Uuid::new_v4();
        let until = Instant::now() + Duration::from_secs(20);
        let mut original = Resident::start(request, until).unwrap();
        assert!(original.offer(b"original-pipe-frame\n".to_vec()));
        let reply = finish(&mut original, until).unwrap();
        assert_eq!(reply.request, request);
        assert!(!retirement_pending());

        // One original deadline and one offer; neither can be refreshed by the
        // input handoff. This mutates only private fixture time, not production.
        let until = Instant::now() + Duration::from_secs(20);
        let mut original = Resident::start(uuid::Uuid::new_v4(), until).unwrap();
        original.0.as_mut().unwrap().until = Instant::now();
        assert!(!original.offer(b"original-pipe-frame\n".to_vec()));
        assert!(original.0.as_ref().unwrap().writer.is_none());
        assert!(finish(&mut original, until).is_err());
        assert!(!CLAIMED.load(Ordering::Acquire));
    }
}
