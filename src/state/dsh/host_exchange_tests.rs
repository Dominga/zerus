//! Native socket pairs pin the closed-peer receive stage without a race.
use super::*;
use std::net::Shutdown;

fn closed_reply(bytes: &[u8]) -> UnixStream {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.write_all(bytes).unwrap();
    peer.shutdown(Shutdown::Both).unwrap();
    drop(peer);
    // Prove the native stage before the production reader tries it again.
    let option = stream.set_read_timeout(Some(Duration::from_secs(2)));
    #[cfg(target_os = "macos")]
    assert_eq!(option.unwrap_err().raw_os_error(), Some(libc::EINVAL));
    #[cfg(not(target_os = "macos"))]
    option.unwrap();
    stream
}

#[test]
fn a_complete_reply_is_drained_after_the_peer_closes_both_directions() {
    let value = json!({"protocol": 2, "callTokens": true});
    let reply = format!("{}\n", json!({"ok": true, "value": value}));
    let mut stream = closed_reply(reply.as_bytes());
    assert_eq!(
        host_response(&mut stream, Instant::now() + Duration::from_secs(2)).ok(),
        Some(value)
    );
}

#[test]
fn a_closed_peer_with_an_incomplete_or_invalid_reply_is_no_answer() {
    for bytes in [
        b"".as_slice(),
        b"{\"ok\":true,\"value\":{}}",
        b"{\"ok\":true,\n",
        b"{\"ok\":false,\"value\":{}}\n",
    ] {
        let mut stream = closed_reply(bytes);
        assert!(matches!(
            host_response(&mut stream, Instant::now() + Duration::from_secs(2)),
            Err(Unanswered::NoAnswer)
        ));
    }
}

#[test]
fn a_closed_peers_buffered_reply_does_not_extend_the_absolute_deadline() {
    let mut stream = closed_reply(b"{\"ok\":true,\"value\":{}}\n");
    assert!(matches!(
        host_response(&mut stream, Instant::now()),
        Err(Unanswered::NoAnswer)
    ));
}
