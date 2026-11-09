use super::*;
use std::os::fd::AsRawFd;

fn until() -> Instant {
    Instant::now() + Duration::from_secs(3)
}
fn close() -> Control {
    Control::Close {
        pipeline: "11111111-1111-4111-8111-111111111111".into(),
    }
}
#[test]
fn the_single_control_reader_keeps_frame_boundaries_and_never_needs_eof_to_reply() {
    let (reader, mut writer) = std::io::pipe().unwrap();
    let bytes = close().encode().unwrap();
    writer.write_all(&bytes).unwrap();
    writer.write_all(&bytes).unwrap();
    let mut frames = Reader::default();
    assert_eq!(frames.next(reader.as_raw_fd(), until()).unwrap(), close());
    assert_eq!(frames.next(reader.as_raw_fd(), until()).unwrap(), close());
    assert_eq!(frames.used, 2);
    drop(writer);
    assert!(
        frames.next(reader.as_raw_fd(), until()).is_err(),
        "EOF cannot invent another control"
    );
}
#[test]
fn incomplete_invalid_oversized_expired_or_excess_controls_never_become_a_loan() {
    for bytes in [
        b"{\"op\":\"force\"}\n".as_slice(),
        b"{\"op\":\"close\"}",
        b"\n",
    ] {
        let (reader, mut writer) = std::io::pipe().unwrap();
        writer.write_all(bytes).unwrap();
        drop(writer);
        assert!(Reader::default().next(reader.as_raw_fd(), until()).is_err());
    }
    let (reader, _held_writer) = std::io::pipe().unwrap();
    assert!(Reader::default()
        .next(reader.as_raw_fd(), Instant::now())
        .is_err());
    let (reader, mut writer) = std::io::pipe().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = writer.write_all(&vec![b'x'; FRAME_BYTES + 1]);
    });
    assert!(Reader::default().next(reader.as_raw_fd(), until()).is_err());
    drop(reader);
    writer.join().unwrap();
    let (reader, mut writer) = std::io::pipe().unwrap();
    writer.write_all(&close().encode().unwrap()).unwrap();
    let mut exhausted = Reader {
        used: MAX_CONTROLS,
        ..Reader::default()
    };
    assert!(exhausted.next(reader.as_raw_fd(), until()).is_err());
}
