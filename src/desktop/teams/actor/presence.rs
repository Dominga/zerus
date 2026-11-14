//! Passive read of the existing service status exchange, with its own bound.
use crate::config::Config;
pub(crate) fn presence(config: &Config) -> Option<bool> {
    use crate::desktop::{protocol, store::Location, transport::TIMEOUT};
    use std::{io::BufReader, time::Instant};
    let location = Location::new(config).ok()?;
    let until = Instant::now() + TIMEOUT;
    let Some(stream) = crate::desktop::connection_until(&location, "status", None, until).ok()?
    else {
        return Some(false);
    };
    let bytes = protocol::line_checked(
        &mut BufReader::new(stream),
        protocol::FRAME_BYTES,
        |reader, phase| {
            let remaining = until.saturating_duration_since(Instant::now());
            anyhow::ensure!(!remaining.is_zero(), "Teams status deadline expired");
            if matches!(phase, protocol::ReadPhase::BeforeRead) {
                reader.get_ref().set_read_timeout(Some(remaining))?;
            }
            Ok(())
        },
    )
    .ok()??;
    let response: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if response["abi"] != crate::desktop::ABI || response["t"] == "error" {
        return None;
    }
    response["value"]["running"].as_bool()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::desktop::{store::Location, transport::Listener};
    use crate::teams::ops::tests::Harness;
    use std::{
        io::{BufRead, BufReader, Write},
        time::{Duration, Instant},
    };

    fn config(h: &Harness) -> Config {
        crate::platform::private_dir(&h.temp.path().join("state")).unwrap();
        crate::platform::private_dir(&h.temp.path().join("config")).unwrap();
        Config {
            home: String::new(),
            dir: h.temp.path().join("config"),
            state: h.temp.path().join("state"),
            selves: vec![],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: Default::default(),
            tab: false,
            tab_colors: String::new(),
        }
    }

    #[test]
    fn a_missing_socket_is_absent_and_the_read_creates_nothing() {
        let h = Harness::new();
        let config = config(&h);
        let desktop = config.state.join("desktop");
        assert_eq!(presence(&config), Some(false));
        assert!(!desktop.exists());
    }

    fn answering(bytes: Vec<u8>, wait: Duration) -> (Option<bool>, Duration) {
        answering_chunks(bytes, wait, None)
    }
    fn answering_chunks(
        bytes: Vec<u8>,
        wait: Duration,
        chunk_delay: Option<Duration>,
    ) -> (Option<bool>, Duration) {
        let h = Harness::new();
        let config = config(&h);
        let location = Location::new(&config).unwrap();
        location.prepare().unwrap();
        let listener = Listener::bind(&location).unwrap();
        let server = std::thread::spawn(move || {
            let until = Instant::now() + Duration::from_secs(2);
            let mut stream = loop {
                match listener.accept() {
                    Ok(stream) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < until =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    other => panic!("fixture accept: {other:?}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["op"], "status");
            assert!(request["request"].is_null());
            std::thread::sleep(wait);
            if let Some(delay) = chunk_delay {
                for part in bytes.chunks(8) {
                    if stream.write_all(part).is_err() {
                        break;
                    }
                    std::thread::sleep(delay);
                }
            } else {
                let _ = stream.write_all(&bytes);
            }
        });
        let at = Instant::now();
        let result = presence(&config);
        let elapsed = at.elapsed();
        server.join().unwrap();
        assert!(!location.root.join("service.sock").exists());
        (result, elapsed)
    }

    #[test]
    fn a_service_reply_reports_presence_and_an_unreadable_exchange_is_unknown() {
        let mut good = serde_json::to_vec(
            &serde_json::json!({"abi":crate::desktop::ABI,"t":"status","value":{"running":true}}),
        )
        .unwrap();
        good.push(b'\n');
        assert_eq!(answering(good, Duration::ZERO).0, Some(true));
        assert_eq!(answering(b"unreadable\n".to_vec(), Duration::ZERO).0, None);
    }

    #[test]
    fn a_slow_service_stays_unknown_with_the_existing_exchange_bound() {
        let (value, elapsed) = answering(vec![], Duration::from_millis(2200));
        assert_eq!(value, None);
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    }
    #[test]
    fn a_dribbling_status_frame_never_renews_the_original_deadline() {
        let mut bytes = serde_json::to_vec(
            &serde_json::json!({"abi":crate::desktop::ABI,"t":"status","value":{"running":true}}),
        )
        .unwrap();
        bytes.push(b'\n');
        let (value, elapsed) =
            answering_chunks(bytes, Duration::ZERO, Some(Duration::from_millis(350)));
        assert_eq!(value, None);
        assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    }
}
