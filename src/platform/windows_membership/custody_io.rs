//! Bounded storage with genuine EOF, not "read limit reached" completion.
use std::{
    io::{self, Read},
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) struct Drained {
    pub(super) bytes: Vec<u8>,
    pub(super) overflow: bool,
}
/// Borrow the original reader. On error its owner must retain it; no error,
/// output cap or elapsed deadline is an EOF witness. After overflow keep
/// draining without growing storage, while the controller stops its OWN child.
pub(super) fn drain(
    reader: &mut impl Read,
    cap: usize,
    too_large: &AtomicBool,
) -> io::Result<Drained> {
    let mut bytes = Vec::with_capacity(cap.min(4096));
    let mut buffer = [0u8; 8192];
    let mut overflow = false;
    loop {
        let count = match reader.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            other => other?,
        };
        if count > buffer.len() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if count == 0 {
            return Ok(Drained { bytes, overflow });
        }
        let remaining = cap - bytes.len();
        bytes.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            overflow = true;
            too_large.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::{mpsc, Arc},
        thread,
        time::Duration,
    };

    #[test]
    fn the_cap_does_not_become_eof_while_the_original_writer_is_still_held() {
        #[derive(Debug, PartialEq, Eq)]
        enum Event {
            Read,
            Ended,
        }
        struct Observed {
            pipe: std::io::PipeReader,
            events: mpsc::Sender<Event>,
        }
        impl Read for Observed {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                self.events.send(Event::Read).unwrap();
                self.pipe.read(bytes)
            }
        }
        let (input, mut output) = std::io::pipe().unwrap();
        let (events, observed_events) = mpsc::channel();
        let overflow = Arc::new(AtomicBool::new(false));
        let observed = overflow.clone();
        let reader = thread::spawn(move || {
            let mut input = Observed {
                pipe: input,
                events,
            };
            let end = drain(&mut input, 3, &observed);
            input.events.send(Event::Ended).unwrap();
            end
        });
        output.write_all(b"original").unwrap();
        assert_eq!(
            observed_events
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Event::Read
        );
        // The next real read, rather than a sleep, establishes that reaching
        // the storage cap did not return. The original writer is still held.
        assert_eq!(
            observed_events
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Event::Read
        );
        assert!(overflow.load(Ordering::Acquire));
        assert!(!reader.is_finished(), "reaching the cap is not EOF");
        output.write_all(b" still owned").unwrap();
        drop(output);
        let end = reader.join().unwrap().unwrap();
        assert_eq!(end.bytes, b"ori");
        assert!(end.overflow);
    }
    #[test]
    fn a_read_error_is_not_an_empty_completed_stream() {
        struct Failed;
        impl Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::PermissionDenied.into())
            }
        }
        assert!(
            matches!(drain(&mut Failed, 3, &AtomicBool::new(false)), Err(error) if error.kind() == io::ErrorKind::PermissionDenied)
        );
    }
    #[test]
    fn exact_cap_and_zero_cap_still_require_actual_eof() {
        let flag = AtomicBool::new(false);
        let end = drain(&mut b"exact".as_slice(), 5, &flag).unwrap();
        assert_eq!(end.bytes, b"exact");
        assert!(!end.overflow && !flag.load(Ordering::Acquire));
        let end = drain(&mut b"discarded".as_slice(), 0, &flag).unwrap();
        assert!(end.bytes.is_empty() && end.overflow && flag.load(Ordering::Acquire));
    }
}
