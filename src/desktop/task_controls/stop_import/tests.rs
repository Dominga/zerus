use super::*;
use zerus_desktop_ledger::task_controls::Executor;

fn fact() -> Fact {
    Fact {
        id: "00000000-0000-4000-8000-000000000001".into(),
        executor: Executor {
            computer: "00000000-0000-4000-8000-000000000002".into(),
            session_id: "00000000-0000-4000-8000-000000000003".into(),
            run: "run".into(),
            birth: "birth".into(),
            conversation: None,
            child: None,
        },
        source: Source::PersonStop,
        turn: Some("turn".into()),
    }
}
#[test]
fn stop_guard_is_closed_canonical_and_independent_of_the_service_epoch_or_image() {
    let fact = fact();
    let configuration = "a".repeat(64);
    let identity = "b".repeat(64);
    let (request, hold) = entry(&fact, &configuration, &identity).unwrap();
    let guard = serde_json::to_value(Guard {
        schema: 1,
        fact: &fact,
        configuration: &configuration,
        store_identity: &identity,
    })
    .unwrap();
    assert_eq!(guard.as_object().unwrap().len(), 4);
    assert_eq!(request.id, fact.id);
    assert_eq!(hold.request_id, fact.id);
    assert_eq!(request.action, Action::Stop);
    assert_eq!(
        request.admission,
        canonical::tagged_digest(DOMAIN, &guard).unwrap()
    );
    assert_ne!(
        request.admission,
        canonical::tagged_digest(b"zerus-task-control-admission-v1\0", &guard).unwrap()
    );
    assert_eq!(
        entry(&fact, &configuration, &identity).unwrap(),
        (request.clone(), hold)
    );
    let mut changed = fact.clone();
    changed.source = Source::VerifiedExternalInterrupt;
    assert_ne!(
        entry(&changed, &configuration, &identity)
            .unwrap()
            .0
            .admission,
        request.admission
    );
    changed = fact.clone();
    changed.turn = Some("other-turn".into());
    assert_ne!(
        entry(&changed, &configuration, &identity)
            .unwrap()
            .0
            .admission,
        request.admission
    );
    for (config, store) in [
        ("c".repeat(64), identity.clone()),
        (configuration.clone(), "d".repeat(64)),
    ] {
        assert_ne!(
            entry(&fact, &config, &store).unwrap().0.admission,
            request.admission
        );
    }
    for bad in ["", "unknown", &"A".repeat(64)] {
        assert!(entry(&fact, bad, &identity).is_err());
        assert!(entry(&fact, &configuration, bad).is_err());
    }
}
