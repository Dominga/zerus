// This file is included in encrypted_runtime::tests, reusing synthetic Fixture.
mod read_pressure {
    use super::*;
    const LIMIT: i64 = 256 * 1024 * 1024;
    struct Pair {
        id: String,
        digest: String,
        header: String,
        result: String,
    }
    fn pair(
        f: &Fixture,
        profile: &Profile,
        serial: u128,
        class: OperationClass,
        size: usize,
    ) -> Pair {
        let id = uuid::Uuid::from_u128(0x40008000000000000000u128 + serial).to_string();
        let mut request = f.request(profile, &id, &profile.identity.device.device_id, "");
        let private = crypto::open(
            &Identity::from_record(&profile.identity).unwrap(),
            f.phone.public(),
            &request.header,
            &request,
        )
        .unwrap();
        request.header.operation_class = class;
        // Source-style accepted request + full gateway-signed result, no real data.
        request =
            crypto::seal(&f.phone, &profile.identity.device, request.header, &private).unwrap();
        let response = Engine::<NativeFixture>::header(
            profile,
            f.phone.public(),
            &request.header.route_id,
            id.clone(),
            PayloadKind::Result,
            class,
            crate::now() as u64 + 23 * 3600,
        );
        let body=serde_json::to_vec(&json!({"request_id":id,"state":"completed","result":{"synthetic":"x".repeat(size)},"error":null})).unwrap();
        let result = crypto::seal(
            &Identity::from_record(&profile.identity).unwrap(),
            f.phone.public(),
            response,
            &body,
        )
        .unwrap();
        Pair {
            id,
            digest: request.digest().unwrap(),
            header: serde_json::to_string(&request.header).unwrap(),
            result: serde_json::to_string(&result).unwrap(),
        }
    }
    fn seed(j: &Journal, p: &Pair, delivered: bool, at: f64) {
        assert!(j
            .encrypted_reserve(&p.id, &p.digest, &p.header, at)
            .unwrap());
        j.encrypted_result(&p.id, &p.result, at).unwrap();
        if delivered {
            j.encrypted_delivered(&p.id, at).unwrap();
        }
    }
    fn fill_real_limit(f: &Fixture, profile: &Profile, age: f64) -> usize {
        let j = &f.connector.journal;
        for index in 1..=1024 {
            let p = pair(f, profile, index, OperationClass::Read, 256 * 1024);
            if j.pressure_fixture_usage().1 + p.result.len() as i64 > LIMIT {
                return index as usize - 1;
            }
            seed(j, &p, true, crate::now() - age);
        }
        panic!("bounded synthetic cache did not reach the real limit");
    }

    // EXACT same causal method/helpers are copied into baseline CONTROL.
    #[test]
    fn completed_read_cache_pressure_preserves_pending_native_receipt() {
        let f = Fixture::new();
        let profile = workspace::load_active(&f.private_dir).unwrap();
        assert!(fill_real_limit(&f, &profile, 180.0) > 100);
        let j = &f.connector.journal;
        let before = j.pressure_fixture_usage();
        assert_eq!(before.1, before.2);
        assert!(LIMIT - before.1 < 400_000);
        let pending = pair(&f, &profile, 5000, OperationClass::Read, 16);
        assert!(j
            .encrypted_reserve(&pending.id, &pending.digest, &pending.header, crate::now())
            .unwrap());
        assert!(j
            .claim(
                &pending.id,
                "inspect",
                Some("synthetic-native-identity"),
                crate::now()
            )
            .unwrap());
        let receipt =
            crate::request::receipt("completed", json!({"synthetic":"x".repeat(490_455)}), None);
        j.finish(&pending.id, &receipt, crate::now()).unwrap();
        let native = j.encrypted_native_receipt(&pending.id).unwrap();
        let request: Header = serde_json::from_str(&pending.header).unwrap();
        let engine = f.engine();
        let mut saved = None;
        for _ in 0..16 {
            match engine.seal_receipt(&profile, &request, receipt.clone()) {
                Ok(value) => {
                    saved = Some(value);
                    break;
                }
                Err(Failure::CachePressure) => {}
                Err(Failure::Retry) => panic!("local cache pressure is not a network retry"),
                Err(Failure::Fatal(_)) => {
                    panic!("completed Read cache pressure must not stop the publisher")
                }
            }
        }
        assert!(
            saved.is_some(),
            "bounded pressure turns must release result headroom"
        );
        assert!(!f.connector.stopping.is_set());
        assert!(f.connector.native.calls.borrow().is_empty());
        assert_eq!(j.encrypted_native_receipt(&pending.id).unwrap(), native);
        j.encrypted_delivered(&pending.id, crate::now()).unwrap();
        assert!(j.encrypted_pending().unwrap().is_empty());
        assert!(!j
            .encrypted_reserve(&pending.id, &pending.digest, &pending.header, crate::now())
            .unwrap());
        let after = j.pressure_fixture_usage();
        assert_eq!(after.1, after.2);
        assert!(after.1 <= LIMIT);
    }

    #[test]
    fn read_cache_authentication_fails_closed() {
        let f = Fixture::new();
        let profile = workspace::load_active(&f.private_dir).unwrap();
        let p = pair(&f, &profile, 6000, OperationClass::Read, 128);
        assert!(accepted_read_cache(
            &profile, &p.id, &p.digest, &p.header, &p.result
        ));
        for malformed in [
            String::new(),
            "a".repeat(64),
            format!("{}=", p.digest),
            "!".repeat(43),
        ] {
            assert!(!accepted_read_cache(
                &profile, &p.id, &malformed, &p.header, &p.result
            ));
        }
        for class in [OperationClass::Mutation, OperationClass::Control] {
            let other = pair(&f, &profile, 6001, class, 128);
            assert!(!accepted_read_cache(
                &profile,
                &other.id,
                &other.digest,
                &other.header,
                &other.result
            ));
        }
        assert!(!accepted_read_cache(
            &profile, &p.id, &p.digest, "{}", &p.result
        ));
        assert!(!accepted_read_cache(
            &profile, &p.id, &p.digest, &p.header, "{}"
        ));
        let duplicate = p.header.replacen('{', "{\"kind\":\"request\",", 1);
        assert!(!accepted_read_cache(
            &profile, &p.id, &p.digest, &duplicate, &p.result
        ));
        let mut result: EncryptedEnvelope = serde_json::from_str(&p.result).unwrap();
        result.header.route_id = "different-route".into();
        assert!(!accepted_read_cache(
            &profile,
            &p.id,
            &p.digest,
            &p.header,
            &serde_json::to_string(&result).unwrap()
        ));
        let mut result: EncryptedEnvelope = serde_json::from_str(&p.result).unwrap();
        result.signature = crypto::encode(&[0; 64]);
        assert!(!accepted_read_cache(
            &profile,
            &p.id,
            &p.digest,
            &p.header,
            &serde_json::to_string(&result).unwrap()
        ));
    }
    fn turn(j: &Journal, profile: &Profile, now: f64, need: usize, limit: i64) {
        j.encrypted_reclaim_read_cache_turn(
            now,
            need,
            limit,
            || false,
            |id, digest, header, result| accepted_read_cache(profile, id, digest, header, result),
        )
        .unwrap();
        let used = j.pressure_fixture_usage();
        assert_eq!(used.1, used.2);
    }
    #[test]
    fn grace_classes_and_undelivered_rows_are_protected() {
        let f = Fixture::new();
        let profile = workspace::load_active(&f.private_dir).unwrap();
        let j = &f.connector.journal;
        let now = crate::now();
        let old = pair(&f, &profile, 7000, OperationClass::Read, 2048);
        seed(j, &old, true, now - 121.0);
        let young = pair(&f, &profile, 7001, OperationClass::Read, 2048);
        seed(j, &young, true, now - 119.0);
        let pending = pair(&f, &profile, 7002, OperationClass::Read, 2048);
        seed(j, &pending, false, now - 1000.0);
        let mutation = pair(&f, &profile, 7003, OperationClass::Mutation, 2048);
        seed(j, &mutation, true, now - 1000.0);
        let control = pair(&f, &profile, 7004, OperationClass::Control, 2048);
        seed(j, &control, true, now - 1000.0);
        let inflight = pair(&f, &profile, 7006, OperationClass::Read, 2048);
        assert!(j
            .encrypted_reserve(
                &inflight.id,
                &inflight.digest,
                &inflight.header,
                now - 1000.0
            )
            .unwrap());
        let mut unknown = pair(&f, &profile, 7005, OperationClass::Read, 2048);
        unknown.header = "{}".into();
        seed(j, &unknown, true, now - 1000.0);
        let protected = [&young, &pending, &mutation, &control, &unknown, &inflight];
        let before: Vec<_> = protected
            .iter()
            .map(|p| j.pressure_fixture_row(&p.id))
            .collect();
        let used = j.pressure_fixture_usage().1;
        for _ in 0..8 {
            turn(j, &profile, now, old.result.len(), used);
        }
        assert!(j.pressure_fixture_row(&old.id).2.is_none());
        assert_eq!(
            before,
            protected
                .iter()
                .map(|p| j.pressure_fixture_row(&p.id))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn insufficient_batch_commits_progress_and_never_replays() {
        let f = Fixture::new();
        let profile = workspace::load_active(&f.private_dir).unwrap();
        let j = &f.connector.journal;
        let now = crate::now();
        let mut pairs = Vec::new();
        for i in 0..192 {
            let p = pair(&f, &profile, 8000 + i, OperationClass::Read, 1024);
            seed(j, &p, true, now - 180.0);
            pairs.push(p);
        }
        let original = j.pressure_fixture_usage();
        let need = (original.1 * 3 / 4) as usize;
        let mut partial = false;
        for _ in 0..16 {
            turn(j, &profile, now, need, original.1);
            let used = j.pressure_fixture_usage().1;
            partial |= used < original.1 && used > original.1 - need as i64;
            if used <= original.1 - need as i64 {
                break;
            }
        }
        assert!(
            partial,
            "one insufficient bounded batch must commit refunds"
        );
        assert!(j.pressure_fixture_usage().1 <= original.1 - need as i64);
        assert_eq!(j.pressure_fixture_usage().0, original.0);
        for p in &pairs {
            assert!(!j
                .encrypted_reserve(&p.id, &p.digest, &p.header, now)
                .unwrap());
        }
        assert!(j.encrypted_pending().unwrap().is_empty());
    }
    #[test]
    fn restart_and_duplicate_maintenance_refund_once() {
        let f = Fixture::new();
        let profile = workspace::load_active(&f.private_dir).unwrap();
        let now = crate::now();
        let p = pair(&f, &profile, 9000, OperationClass::Read, 4096);
        seed(&f.connector.journal, &p, true, now - 180.0);
        let old = f.connector.journal.pressure_fixture_row(&p.id);
        let state = profile.state_dir.clone();
        let used = f.connector.journal.pressure_fixture_usage().1;
        assert!(
            Journal::open(&state, "encrypted-test", now).is_err(),
            "second worker must be fenced by the existing journal lock"
        );
        // Model cancellation immediately after the pure verifier returned but
        // before its row-CAS. No row or refund may be partially committed.
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            f.connector
                .journal
                .encrypted_reclaim_read_cache_bound(now, p.result.len(), used, |_, _, _, _| {
                    panic!("synthetic cancellation between rows")
                })
                .unwrap();
        }));
        assert!(interrupted.is_err());
        assert_eq!(f.connector.journal.pressure_fixture_row(&p.id), old);
        drop(f.connector);
        let reopened = Journal::open(&state, "encrypted-test", now).unwrap();
        reopened.encrypted_init(now).unwrap();
        for _ in 0..8 {
            turn(&reopened, &profile, now, p.result.len(), used);
        }
        let row = reopened.pressure_fixture_row(&p.id);
        assert_eq!(
            (&row.0, &row.1, row.3, row.4, row.5),
            (&old.0, &old.1, old.3, old.4, old.5)
        );
        assert!(row.2.is_none());
        assert_eq!(reopened.pressure_fixture_usage(), (1, 0, 0));
        assert!(!reopened
            .encrypted_reserve(&p.id, &p.digest, &p.header, now)
            .unwrap());
        assert!(reopened.encrypted_pending().unwrap().is_empty());
    }
    #[tokio::test(start_paused = true)]
    async fn quota_retry_keeps_heartbeat_wait_alive_but_storage_is_fatal() {
        let f = Fixture::new();
        for error in [crate::error::CONFLICT, crate::error::JOURNAL_UNAVAILABLE] {
            assert!(matches!(
                journal::<()>(Err(JournalError::Refused(
                    crate::error::ConnectorError::new(error)
                ))),
                Err(Failure::Fatal(_))
            ));
        }
        assert!(matches!(
            journal::<()>(Err(JournalError::Storage(rusqlite::Error::InvalidQuery))),
            Err(Failure::Fatal(_))
        ));
        assert!(matches!(
            journal::<()>(Err(JournalError::Io(std::io::Error::other("synthetic")))),
            Err(Failure::Fatal(_))
        ));
        let mut network_delay = 1;
        for seconds in [2, 4, 8, 16, 32, 60, 60] {
            assert_eq!(
                Failure::Retry.request_retry_wait(&mut network_delay),
                Some(Duration::from_secs(seconds))
            );
        }
        let start = tokio::time::Instant::now();
        for _ in 0..32 {
            let error = journal::<()>(Err(JournalError::Refused(
                crate::error::ConnectorError::new(crate::error::RETAINED_LIMIT),
            )))
            .unwrap_err();
            assert!(matches!(error, Failure::CachePressure));
            let wait = error.request_retry_wait(&mut network_delay).unwrap();
            assert_eq!(wait, Duration::from_millis(250));
            assert!(
                runtime::until_stopped(&f.connector.stopping, tokio::time::sleep(wait))
                    .await
                    .is_some()
            );
        }
        assert_eq!(tokio::time::Instant::now() - start, Duration::from_secs(8));
        assert_eq!(network_delay, 1);
        assert_eq!(
            Failure::Retry.request_retry_wait(&mut network_delay),
            Some(Duration::from_secs(2))
        );
        assert!(!f.connector.stopping.is_set());
        assert!(f.connector.native.calls.borrow().is_empty());
    }
    #[test]
    fn expired_deadline_still_advances_both_bounded_frontiers() {
        let f = Fixture::new();
        let profile = workspace::load_active(&f.private_dir).unwrap();
        let j = &f.connector.journal;
        let now = crate::now();
        let p = pair(&f, &profile, 10000, OperationClass::Read, 1024);
        seed(j, &p, true, now - 180.0);
        let used = j.pressure_fixture_usage().1;
        // Pretend ALL initial SQL/index/scheduling work already exhausted20ms.
        j.encrypted_reclaim_read_cache_turn(
            now,
            p.result.len(),
            used,
            || true,
            |id, digest, header, result| accepted_read_cache(&profile, id, digest, header, result),
        )
        .unwrap();
        assert!(j.pressure_fixture_row(&p.id).2.is_none());
        assert_eq!(j.pressure_fixture_usage(), (1, 0, 0));
        // Previously classified but too-young cache must also progress later,
        // even when every future turn begins with an expired deadline.
        let young = pair(&f, &profile, 10001, OperationClass::Read, 1024);
        seed(j, &young, true, now);
        let used = j.pressure_fixture_usage().1;
        j.encrypted_reclaim_read_cache_turn(
            now,
            young.result.len(),
            used,
            || true,
            |id, digest, header, result| accepted_read_cache(&profile, id, digest, header, result),
        )
        .unwrap();
        assert!(j.pressure_fixture_row(&young.id).2.is_some());
        j.encrypted_reclaim_read_cache_turn(
            now + 120.0,
            young.result.len(),
            used,
            || true,
            |_, _, _, _| panic!("classified trim must precede new adoption"),
        )
        .unwrap();
        assert_eq!(j.pressure_fixture_usage(), (2, 0, 0));
    }
    #[test]
    fn nonascii_oversized_candidate_is_metadata_only_and_conservative() {
        let f = Fixture::new();
        let profile = workspace::load_active(&f.private_dir).unwrap();
        let j = &f.connector.journal;
        let now = crate::now();
        let mut bad = pair(&f, &profile, 11000, OperationClass::Read, 16);
        bad.result = "é".repeat(1_048_577); // <=2MiB characters, >2MiB UTF8 bytes.
        seed(j, &bad, true, now - 181.0);
        let prior = j.pressure_fixture_row(&bad.id);
        let good = pair(&f, &profile, 11001, OperationClass::Read, 1024);
        seed(j, &good, true, now - 180.0);
        let used = j.pressure_fixture_usage().1;
        j.encrypted_reclaim_read_cache_turn(
            now,
            good.result.len(),
            used,
            || true,
            |_, _, _, _| panic!("oversized candidate must never be loaded for validation"),
        )
        .unwrap();
        assert_eq!(j.pressure_fixture_row(&bad.id), prior);
        // Class2 persists the rejected frontier, so the next bounded turn
        // reaches the good pair without reloading the malformed big string.
        j.encrypted_reclaim_read_cache_turn(
            now,
            good.result.len(),
            used,
            || true,
            |id, digest, header, result| accepted_read_cache(&profile, id, digest, header, result),
        )
        .unwrap();
        assert!(j.pressure_fixture_row(&good.id).2.is_none());
        assert_eq!(j.pressure_fixture_row(&bad.id), prior);
        let counts = j.pressure_fixture_usage();
        assert_eq!(counts.1, counts.2);
    }
}
