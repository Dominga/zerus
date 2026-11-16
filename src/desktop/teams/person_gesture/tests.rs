//! Canonical rows come from Teams writers; Validated fixtures explicitly
//! model native admission and do not claim a real window gesture.
use super::*;
use crate::teams::ops::tests::{self as producers, Harness, TEAM};
use std::time::{Duration, Instant};

fn sample() -> (Harness, ops::Request) {
    let mut h = Harness::new();
    let card = h
        .run(
            &producers::service(),
            ops::Operation::OpenCard {
                kind: "launch_refused".into(),
                text: "No account available".into(),
                node: None,
                decisions: None,
                parameters: None,
            },
        )
        .unwrap()
        .item
        .unwrap();
    let revision = h.store.team(TEAM).unwrap().unwrap().revision;
    (
        h,
        producers::request(
            revision,
            ops::Operation::DecideCard {
                card,
                decision: "acknowledged".into(),
            },
        ),
    )
}
fn intent(admission: &Admission) -> wire::Intent {
    admission.authority.team_person_intent().unwrap().clone()
}
fn rewrite(admission: &mut Admission, intent: &wire::Intent) {
    admission.authority = crate::desktop::ui_authority::Validated::fixture(
        &json!({"op":"team_person_decide","intent":intent}),
    );
}
fn apply(h: &mut Harness, request: &ops::Request, admission: Admission) -> Result<ops::Reply> {
    ops::apply_with_gesture(&mut h.store, &actor(), request, 1234, Some(admission))
}
fn unchanged(h: &Harness, request: &ops::Request) {
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().revision,
        request.expected_revision
    );
    let ops::Operation::DecideCard { card, .. } = request.operation else {
        unreachable!()
    };
    assert_eq!(h.store.card(TEAM, card).unwrap().unwrap().state, "open");
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM requests WHERE id=?1",
                [&request.request_id],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn native_model_decides_and_binds_receipt_in_the_same_transaction() {
    let (mut h, request) = sample();
    let admission = Admission::fixture(&h.store, &request);
    let original_intent = intent(&admission);
    let result = apply(&mut h, &request, admission).unwrap();
    assert_eq!(result.revision, request.expected_revision + 1);
    let db = h.store.db.as_ref().unwrap();
    original(db, &original_intent).unwrap();
    assert_eq!(
        ops::replay(db, &actor(), &request)
            .unwrap()
            .unwrap()
            .unwrap(),
        result
    );
    // A new original carrying changed native evidence cannot adopt a UUID.
    let changed = Admission::fixture(&h.store, &request);
    assert_eq!(
        apply(&mut h, &request, changed).unwrap_err().code,
        code::DUPLICATE
    );
    let mut exact = Admission::fixture(&h.store, &request);
    rewrite(&mut exact, &original_intent);
    assert_eq!(apply(&mut h, &request, exact).unwrap(), result);
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().revision,
        result.revision
    );
    assert_eq!(
        ops::apply(&mut h.store, &actor(), &request, 1235)
            .unwrap_err()
            .code,
        "person_gesture_required"
    );
}

#[test]
fn expired_original_is_consumed_after_the_writer_lock_and_changes_nothing() {
    let (mut h, request) = sample();
    let mut admission = Admission::fixture(&h.store, &request);
    let value = json!({"op":"team_person_decide","intent":intent(&admission)});
    admission.authority = crate::desktop::ui_authority::Validated::fixture_expiring(
        &value,
        Instant::now() - Duration::from_secs(1),
    );
    assert_eq!(
        apply(&mut h, &request, admission).unwrap_err().code,
        "person_gesture_required"
    );
    unchanged(&h, &request);
}

#[test]
fn operation_uuid_revision_and_card_are_bound_to_the_native_original() {
    for field in ["decision", "uuid", "revision", "card", "team"] {
        let (mut h, request) = sample();
        let mut admission = Admission::fixture(&h.store, &request);
        let mut different = intent(&admission);
        match field {
            "uuid" => different.plan.request_id = uuid::Uuid::new_v4().to_string(),
            "revision" => different.plan.expected_revision += 1,
            "card" => different.plan.card += 1,
            "decision" => different.plan.decision = "retry".into(),
            "team" => different.plan.team = uuid::Uuid::new_v4().to_string(),
            _ => unreachable!(),
        }
        rewrite(&mut admission, &different);
        assert_eq!(
            apply(&mut h, &request, admission).unwrap_err().code,
            "person_gesture_required",
            "{field}"
        );
        unchanged(&h, &request);
    }
}

#[test]
fn card_contents_are_rechecked_even_if_the_revision_was_not_updated() {
    let (mut h, request) = sample();
    let admission = Admission::fixture(&h.store, &request);
    // Deliberate store-damage probe. Ordinary product writers advance revision.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE cards SET text='Changed under the dialog' WHERE team=?1",
            [TEAM],
        )
        .unwrap();
    assert_eq!(
        apply(&mut h, &request, admission).unwrap_err().code,
        code::STALE_REVISION
    );
    unchanged(&h, &request);
}

#[test]
fn another_database_object_cannot_receive_the_original_decision() {
    let (source, request) = sample();
    let admission = Admission::fixture(&source.store, &request);
    let (mut destination, _) = sample();
    assert_eq!(
        apply(&mut destination, &request, admission)
            .unwrap_err()
            .code,
        "person_gesture_required"
    );
    unchanged(&destination, &request);
}

#[test]
fn binding_failure_rolls_back_the_card_revision_and_request_receipt() {
    let (mut h, request) = sample();
    let admission = Admission::fixture(&h.store, &request);
    h.store.db.as_ref().unwrap().execute_batch("CREATE TEMP TRIGGER refuse_gesture BEFORE INSERT ON person_gestures BEGIN SELECT RAISE(ABORT,'fixture binding failure'); END;").unwrap();
    assert_eq!(
        apply(&mut h, &request, admission).unwrap_err().code,
        code::STORAGE_FAILED
    );
    unchanged(&h, &request);
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TRIGGER refuse_gesture")
        .unwrap();
    let admission = Admission::fixture(&h.store, &request);
    apply(&mut h, &request, admission).unwrap();
}

#[test]
fn receipt_lookup_does_not_upgrade_an_old_store_or_invent_missing_evidence() {
    let (h, _) = sample();
    h.store
        .db
        .as_ref()
        .unwrap()
        .pragma_update(None, "user_version", 2)
        .unwrap();
    assert!(Store::person_evidence(h.temp.path()).is_err());
    assert_eq!(
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn version_two_migrates_atomically_without_changing_existing_team_or_card_rows() {
    let (mut h, request) = sample();
    let team = h.store.team(TEAM).unwrap();
    let ops::Operation::DecideCard { card, .. } = request.operation else {
        unreachable!()
    };
    let before = h.store.card(TEAM, card).unwrap();
    // Named legacy-layout fixture: rows are product-written, then only the
    // new empty schema leaf is removed to model the pre-cutover version.
    crate::teams::store::person_actions::migration_tests::native_four(&h);
    h.store.db.as_ref().unwrap().execute_batch("DROP TRIGGER person_gesture_store_kept; DROP TRIGGER person_gesture_store_immutable; DROP TRIGGER person_gestures_immutable; DROP TABLE person_gestures; DROP TABLE person_gesture_store; PRAGMA user_version=2;").unwrap();
    assert!(Store::person_evidence(h.temp.path()).is_err());
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(h.store.team(TEAM).unwrap(), team);
    assert_eq!(h.store.card(TEAM, card).unwrap(), before);
    let db = h.store.db.as_ref().unwrap();
    let first = identity(db, h.store.state()).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        crate::teams::store::versions::CORE_NATIVE_FLOOR
    );
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(
        identity(h.store.db.as_ref().unwrap(), h.store.state()).unwrap(),
        first
    );
}

#[test]
fn a_native_person_permit_cannot_be_delegated_to_a_service_actor() {
    let (mut h, request) = sample();
    let admission = Admission::fixture(&h.store, &request);
    assert_eq!(
        ops::apply_with_gesture(
            &mut h.store,
            &producers::service(),
            &request,
            1234,
            Some(admission)
        )
        .unwrap_err()
        .code,
        "person_gesture_required"
    );
    unchanged(&h, &request);
}

#[test]
#[ignore = "exports only an explicitly marked private CLI attack fixture"]
fn export_private_cli_fixture() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::path::PathBuf::from(
        std::env::var_os("ZERUS_TEAM_PERSON_ATTACK_ROOT").expect("private fixture root"),
    );
    assert!(root.join(".zerus-person-gesture-fixture").is_file());
    let destination = root.join("state/teams");
    assert!(!destination.exists(), "never replace existing state");
    let (h, request) = sample();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    crate::platform::private_dir(&destination).unwrap();
    let database = destination.join("teams-v1.sqlite3");
    std::fs::copy(h.temp.path().join("teams/teams-v1.sqlite3"), &database).unwrap();
    std::fs::set_permissions(database, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(destination.join("preview"), "on\n").unwrap();
    let ops::Operation::DecideCard { card, decision } = request.operation else {
        unreachable!()
    };
    std::fs::write(root.join("request.json"), serde_json::to_vec(&json!({"request_id":request.request_id,"team":request.team,"expected_revision":request.expected_revision,"card":card,"decision":decision})).unwrap()).unwrap();
}

fn service(h: &Harness) -> (Config, crate::desktop::store::Store, Scope) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(h.temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let config = Config {
        home: h.temp.path().to_str().unwrap().into(),
        dir: h.temp.path().join("config"),
        state: h.temp.path().into(),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: Default::default(),
        tab: false,
        tab_colors: String::new(),
    };
    crate::platform::private_dir(&config.dir).unwrap();
    crate::swarm::fixture_project(
        &config,
        "22222222-2222-4222-8222-222222222222",
        "example-project",
    );
    let location = crate::desktop::store::Location::new(&config).unwrap();
    location.prepare().unwrap();
    let scope = Scope {
        configuration: location.configuration.clone(),
        service_epoch: uuid::Uuid::new_v4().to_string(),
    };
    let desktop = crate::desktop::store::Store::open(&location, &scope.service_epoch).unwrap();
    (config, desktop, scope)
}
fn review(
    config: &Config,
    desktop: &crate::desktop::store::Store,
    scope: Scope,
    request: &ops::Request,
) -> wire::Review {
    let ops::Operation::DecideCard { card, decision } = &request.operation else {
        unreachable!()
    };
    let plan = wire::Plan {
        request_id: request.request_id.clone(),
        team: request.team.clone(),
        expected_revision: request.expected_revision,
        card: *card,
        decision: decision.clone(),
    };
    let reply = query(
        config,
        desktop,
        &json!({"op":"team_person_prepare","scope":scope,"plan":plan}),
    );
    serde_json::from_value(reply["review"].clone()).unwrap_or_else(|_| {
        panic!(
            "review refused: {reply}; {:?}",
            read(
                config,
                desktop,
                &json!({"op":"team_person_prepare","scope":scope,"plan":plan})
            )
            .err()
        )
    })
}

#[test]
fn service_handler_and_result_reader_preserve_the_exact_original_without_replay() {
    let (h, request) = sample();
    let (config, desktop, scope) = service(&h);
    let review = review(&config, &desktop, scope, &request);
    assert_eq!(review.card_text, "No account available");
    let value = json!({"op":"team_person_decide","intent":review.intent});
    // Generic data cannot create a native permit or decide the card.
    assert_eq!(query(&config, &desktop, &value)["ok"], false);
    unchanged(&h, &request);
    let result = frontend(
        &config,
        &desktop,
        crate::desktop::ui_authority::Validated::fixture(&value),
    );
    assert_eq!(result["reply"]["ok"], true, "{result}");
    let lookup = json!({"op":"team_person_result","intent":review.intent});
    let receipt = query(&config, &desktop, &lookup);
    assert_eq!(receipt, result);
    for _ in 0..3 {
        assert_eq!(query(&config, &desktop, &lookup), result);
    }
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().revision,
        request.expected_revision + 1
    );
    let mut foreign = review.intent;
    foreign.plan.request_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        query(
            &config,
            &desktop,
            &json!({"op":"team_person_result","intent":foreign})
        )["code"],
        "team_person_evidence_unavailable"
    );
}

#[test]
fn service_scope_epoch_machine_and_store_are_not_person_authority() {
    for field in ["configuration", "epoch", "machine", "store"] {
        let (h, request) = sample();
        let (config, desktop, scope) = service(&h);
        let mut review = review(&config, &desktop, scope, &request);
        match field {
            "configuration" => review.intent.scope.configuration = "f".repeat(64),
            "epoch" => review.intent.scope.service_epoch = uuid::Uuid::new_v4().to_string(),
            "machine" => review.intent.machine = uuid::Uuid::new_v4().to_string(),
            "store" => review.intent.store = "f".repeat(64),
            _ => unreachable!(),
        }
        let value = json!({"op":"team_person_decide","intent":review.intent});
        let result = frontend(
            &config,
            &desktop,
            crate::desktop::ui_authority::Validated::fixture(&value),
        );
        assert_eq!(
            result["reply"]["code"], "person_gesture_required",
            "{field}: {result}"
        );
        unchanged(&h, &request);
    }
}

#[test]
fn receipt_lookup_never_repairs_a_missing_or_oversized_original() {
    for damage in ["binding", "receipt", "oversized"] {
        let (h, request) = sample();
        let (config, desktop, scope) = service(&h);
        let review = review(&config, &desktop, scope, &request);
        let value = json!({"op":"team_person_decide","intent":review.intent});
        assert_eq!(
            frontend(
                &config,
                &desktop,
                crate::desktop::ui_authority::Validated::fixture(&value)
            )["reply"]["ok"],
            true
        );
        let db = h.store.db.as_ref().unwrap();
        // Named deliberate store-damage cases, never fixture row producers.
        match damage {
            "binding" => {
                db.execute(
                    "DELETE FROM person_gestures WHERE request_id=?1",
                    [&request.request_id],
                )
                .unwrap();
            }
            "receipt" => {
                db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
                db.execute("DELETE FROM requests WHERE id=?1", [&request.request_id])
                    .unwrap();
            }
            "oversized" => {
                db.execute(
                    "UPDATE requests SET result=?2 WHERE id=?1",
                    // Valid JSON would decode successfully without the size
                    // fence; a syntax error must not make this control pass.
                    params![
                        request.request_id,
                        json!({"Refused":{"code":"invalid_state","message":"x".repeat(600000)}})
                            .to_string()
                    ],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = h.store.team(TEAM).unwrap().unwrap().revision;
        assert_eq!(
            query(
                &config,
                &desktop,
                &json!({"op":"team_person_result","intent":review.intent})
            )["code"],
            "team_person_evidence_unavailable"
        );
        assert_eq!(h.store.team(TEAM).unwrap().unwrap().revision, before);
    }
}

#[test]
fn store_nonce_and_original_native_evidence_are_immutable() {
    let (mut h, request) = sample();
    let admission = Admission::fixture(&h.store, &request);
    let original_intent = intent(&admission);
    apply(&mut h, &request, admission).unwrap();
    let db = h.store.db.as_ref().unwrap();
    let before = identity(db, h.store.state()).unwrap();
    assert!(db
        .execute(
            "UPDATE person_gesture_store SET nonce=?1 WHERE id=1",
            [uuid::Uuid::new_v4().to_string()]
        )
        .is_err());
    assert!(db
        .execute("DELETE FROM person_gesture_store WHERE id=1", [])
        .is_err());
    assert!(db
        .execute(
            "UPDATE person_gestures SET intent='{}' WHERE request_id=?1",
            [&request.request_id]
        )
        .is_err());
    assert_eq!(identity(db, h.store.state()).unwrap(), before);
    original(db, &original_intent).unwrap();
}

mod merge;
