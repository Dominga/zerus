use super::*;
use std::cell::Cell;

struct Fixture {
    temp: tempfile::TempDir,
    location: Location,
    store: Store,
    epoch: String,
    origin: Origin,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let location = Location {
            root: temp.path().join("state/desktop"),
            configuration: "a".repeat(64),
        };
        location.prepare().unwrap();
        let epoch = uuid::Uuid::new_v4().to_string();
        let store = Store::open(&location, &epoch).unwrap();
        let image = temp.path().join("hgs-tray");
        std::fs::write(&image, b"original selected native frontend").unwrap();
        let origin = Origin::capture(
            &image,
            Build {
                version: "1.2.3-dev.7".into(),
                source_commit: "a".repeat(40),
                source_dirty: Some(false),
            },
        )
        .unwrap();
        Self {
            temp,
            location,
            store,
            epoch,
            origin,
        }
    }
    fn request(&self, action: Action) -> Request {
        let s = status(&self.store, &self.location.configuration).unwrap();
        Request::DesktopOwnership {
            request_id: uuid::Uuid::new_v4().to_string(),
            action,
            journal: s.journal,
            revision: s.revision,
        }
    }
    fn apply(&mut self, request: Request) -> Receipt {
        apply(
            &self.location,
            &mut self.store,
            &self.epoch,
            request,
            self.origin.clone(),
            None,
            |_| Ok(()),
        )
        .unwrap()
    }
    fn current(&self) -> Status {
        status(&self.store, &self.location.configuration).unwrap()
    }
    fn count(&self) -> i64 {
        self.store
            .db
            .query_row("SELECT count(*) FROM desktop_ownership_requests", [], |r| {
                r.get(0)
            })
            .unwrap()
    }
    fn query(&self, original: &Request) -> Query {
        Query::for_request(
            uuid::Uuid::new_v4().to_string(),
            self.location.configuration.clone(),
            original,
        )
        .unwrap()
    }
}

#[test]
fn grant_is_exact_idempotent_and_never_imports_approval_consent() {
    let mut f = Fixture::new();
    let request = f.request(Action::Grant);
    let receipt = f.apply(request.clone());
    assert!(receipt.matches(&request, &f.location.configuration));
    assert!(matches!(receipt.outcome, Outcome::Granted { .. }));
    assert!(!receipt.native_ready());
    assert!(receipt.service_granted());
    assert_eq!(f.apply(request.clone()), receipt);
    assert_eq!(f.count(), 1);
    let policy: (bool, i64) = f
        .store
        .db
        .query_row(
            "SELECT enabled,revision FROM desktop_delivery_policy WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(policy, (false, 0));
    assert!(
        f.store.effect_status()["revoked"] == true,
        "a durable mode grant alone never authorizes an effect writer"
    );
    let mut different = request;
    if let Request::DesktopOwnership { action, .. } = &mut different {
        *action = Action::Rearm;
    }
    assert!(apply(
        &f.location,
        &mut f.store,
        &f.epoch,
        different,
        f.origin.clone(),
        None,
        |_| Ok(())
    )
    .is_err());
    assert_eq!(f.count(), 1);
}

#[test]
fn revoke_requires_explicit_rearm_even_after_restart_and_preserves_history() {
    let mut f = Fixture::new();
    let grant = f.request(Action::Grant);
    f.apply(grant);
    let id = uuid::Uuid::new_v4().to_string();
    let result = super::super::revoke::request(
        &f.location,
        &mut f.store,
        &serde_json::json!({"op":"revoke","request_id":id}),
    );
    assert_eq!(result["revoked"], true);
    assert!(!super::super::revoke::background_permitted(&f.location, &f.store).unwrap());
    let request = f.request(Action::Grant);
    let refused = f.apply(request.clone());
    assert!(refused.matches(&request, &f.location.configuration));
    assert!(
        matches!(refused.outcome,Outcome::Refused{ref code} if code == "ownership_rearm_required")
    );
    let next_epoch = uuid::Uuid::new_v4().to_string();
    // Reopen under a new process epoch without dropping the original temp root.
    let old = std::mem::replace(&mut f.store, Store::open(&f.location, &next_epoch).unwrap());
    drop(old);
    assert!(f.current().rearm_required);
    let rearm = f.request(Action::Rearm);
    let receipt = f.apply(rearm);
    assert!(matches!(receipt.outcome, Outcome::Granted { .. }));
    assert!(super::super::revoke::background_permitted(&f.location, &f.store).unwrap());
    let history: i64 = f
        .store
        .db
        .query_row(
            "SELECT count(*) FROM desktop_commands WHERE request_id=?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        history, 1,
        "rearm supersedes the fence without deleting the original revoke"
    );
}

#[test]
fn pending_takeover_is_not_ready_until_actual_drain_and_stale_settlement_never_releases_native() {
    let mut f = Fixture::new();
    let grant = f.request(Action::Grant);
    f.apply(grant);
    let request = f.request(Action::NativeTakeover);
    let calls = Cell::new(0);
    let pending = apply(
        &f.location,
        &mut f.store,
        &f.epoch,
        request.clone(),
        f.origin.clone(),
        None,
        |store| {
            calls.set(calls.get() + 1);
            assert_eq!(store.effect_status()["revoked"], true);
            anyhow::bail!("original worker still retained")
        },
    )
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(matches!(pending.outcome, Outcome::Pending));
    assert!(!pending.native_ready());
    assert!(f.current().rearm_required);
    assert!(matches!(
        f.apply(f.request(Action::Rearm)).outcome,
        Outcome::Refused { .. }
    ));
    let found = lookup(&f.store, &f.location.configuration, f.query(&request)).unwrap();
    assert!(found.found && !found.definitive);
    assert!(!found.receipt.unwrap().native_ready());
    settle(&f.location, &mut f.store, None, |_| {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    let settled = lookup(&f.store, &f.location.configuration, f.query(&request))
        .unwrap()
        .receipt
        .unwrap();
    assert!(settled.matches(&request, &f.location.configuration));
    assert!(settled.native_ready());
    assert_eq!(calls.get(), 2);
    let query = f.query(&request);
    let next = f.request(Action::Rearm);
    f.apply(next);
    let old = lookup(&f.store, &f.location.configuration, query)
        .unwrap()
        .receipt
        .unwrap();
    assert!(matches!(old.outcome, Outcome::NativeReady { .. }));
    assert!(
        !old.native_ready(),
        "the immutable old receipt cannot settle a new frontend after a grant"
    );
}

#[test]
fn replacement_image_invalidates_launch_grant_and_requires_new_explicit_rearm() {
    let mut f = Fixture::new();
    let request = f.request(Action::Grant);
    f.apply(request);
    let other = f.temp.path().join("replacement");
    std::fs::write(&other, b"new selected native frontend").unwrap();
    std::fs::rename(&other, &f.origin.image).unwrap();
    assert_eq!(f.current().state, State::IdentityChanged);
    assert!(!service_granted(&f.store, &f.location.configuration).unwrap());
    f.origin = Origin::capture(&f.origin.image, f.origin.build.clone()).unwrap();
    let request = f.request(Action::Grant);
    assert!(matches!(f.apply(request).outcome, Outcome::Refused { .. }));
    let request = f.request(Action::Rearm);
    f.apply(request);
    assert!(service_granted(&f.store, &f.location.configuration).unwrap());
}

#[test]
fn negative_lookup_is_definitive_only_for_the_unchanged_original_scope() {
    let mut f = Fixture::new();
    let absent = f.request(Action::Grant);
    let query = f.query(&absent);
    let result = lookup(&f.store, &f.location.configuration, query.clone()).unwrap();
    assert!(result.matches(&query));
    assert!(!result.found && result.definitive);
    let another = f.request(Action::Grant);
    f.apply(another);
    let result = lookup(&f.store, &f.location.configuration, query.clone()).unwrap();
    assert!(result.matches(&query));
    assert!(!result.found && !result.definitive);
    let stale = f.apply(absent);
    assert!(matches!(stale.outcome, Outcome::Refused { .. }));
    assert_eq!(f.count(), 1);
}

#[test]
fn read_only_absence_never_creates_state_and_incomplete_state_refuses() {
    let temp = tempfile::tempdir().unwrap();
    let location = Location {
        root: temp.path().join("absent/desktop"),
        configuration: "a".repeat(64),
    };
    let current: Status = serde_json::from_value(offline_read(&location, None).unwrap()).unwrap();
    assert!(current.matches(&location.configuration));
    assert_eq!(current.state, State::Passive);
    assert!(!location.root.exists());
    location.prepare().unwrap();
    std::fs::write(location.root.join("state.sqlite3-wal"), b"uncertain").unwrap();
    assert!(offline_read(&location, None).is_err());
}

#[test]
fn takeover_recovery_after_restart_only_finishes_the_original_reduction() {
    let mut f = Fixture::new();
    let request = f.request(Action::NativeTakeover);
    let receipt = apply(
        &f.location,
        &mut f.store,
        &f.epoch,
        request.clone(),
        f.origin.clone(),
        None,
        |_| anyhow::bail!("pending"),
    )
    .unwrap();
    assert!(!receipt.native_ready());
    let next_epoch = uuid::Uuid::new_v4().to_string();
    let old = std::mem::replace(&mut f.store, Store::open(&f.location, &next_epoch).unwrap());
    drop(old);
    assert_eq!(f.current().state, State::Draining);
    settle(&f.location, &mut f.store, None, |_| Ok(())).unwrap();
    let receipt = lookup(&f.store, &f.location.configuration, f.query(&request))
        .unwrap()
        .receipt
        .unwrap();
    assert!(receipt.native_ready());
    assert_eq!(f.count(), 1);
    assert_eq!(f.store.effect_status()["revoked"], true);
}

#[test]
fn settled_receipts_are_bounded_and_evicted_ids_cannot_regain_authority() {
    let mut f = Fixture::new();
    let oldest = f.request(Action::Grant);
    f.apply(oldest.clone());
    for _ in 0..RECEIPTS {
        let request = f.request(Action::Grant);
        f.apply(request);
    }
    assert_eq!(f.count(), RECEIPTS);
    let revision = f.current().revision;
    let old = lookup(&f.store, &f.location.configuration, f.query(&oldest)).unwrap();
    assert!(!old.found && !old.definitive);
    let refused = f.apply(oldest);
    assert!(
        matches!(refused.outcome,Outcome::Refused{ref code} if code == "ownership_scope_changed")
    );
    assert_eq!(f.current().revision, revision);
    assert_eq!(f.count(), RECEIPTS);
    let takeover = f.request(Action::NativeTakeover);
    assert!(f.apply(takeover).native_ready());
    assert_eq!(
        f.count(),
        RECEIPTS,
        "settlement also applies the completed-receipt bound"
    );
}

#[test]
fn revoking_a_pending_takeover_closes_its_receipt_without_ever_claiming_native_ready() {
    let mut f = Fixture::new();
    let request = f.request(Action::NativeTakeover);
    apply(
        &f.location,
        &mut f.store,
        &f.epoch,
        request.clone(),
        f.origin.clone(),
        None,
        |_| anyhow::bail!("pending"),
    )
    .unwrap();
    let tx = f.store.db.transaction().unwrap();
    invalidate(&tx).unwrap();
    zerus_desktop_ledger::revoke(&tx, "manual_revoke").unwrap();
    tx.commit().unwrap();
    let receipt = lookup(&f.store, &f.location.configuration, f.query(&request))
        .unwrap()
        .receipt
        .unwrap();
    assert!(receipt.matches(&request, &f.location.configuration));
    assert!(!receipt.native_ready());
    assert!(matches!(receipt.outcome,Outcome::Refused{ref code} if code == "ownership_revoked"));
    let count: i64 = f
        .store
        .db
        .query_row(
            "SELECT count(*) FROM desktop_ownership_requests WHERE final=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 0,
        "a superseded reduction cannot consume a permanent pending slot"
    );
}

#[test]
fn rearm_cannot_replace_an_original_runtime_that_has_not_drained() {
    let mut f = Fixture::new();
    let grant = f.request(Action::Grant);
    f.apply(grant);
    let tx = f.store.db.transaction().unwrap();
    invalidate(&tx).unwrap();
    tx.commit().unwrap();
    let request = f.request(Action::Rearm);
    let count = f.count();
    let revision = f.current().revision;
    let refused = apply(
        &f.location,
        &mut f.store,
        &f.epoch,
        request.clone(),
        f.origin.clone(),
        None,
        |_| anyhow::bail!("original runtime retained"),
    )
    .unwrap();
    assert!(refused.matches(&request, &f.location.configuration));
    assert!(
        matches!(refused.outcome,Outcome::Refused{ref code} if code == "ownership_drain_pending")
    );
    assert_eq!(f.current().revision, revision);
    assert_eq!(f.count(), count);
    assert!(!service_granted(&f.store, &f.location.configuration).unwrap());
}

#[test]
fn final_native_proof_refusal_rolls_back_before_any_durable_intent() {
    let mut f = Fixture::new();
    let request = f.request(Action::Grant);
    let before = f.current();
    let result = apply_guarded(
        &f.location,
        &mut f.store,
        &f.epoch,
        (request, f.origin.clone()),
        None,
        |_| Ok(()),
        || anyhow::bail!("original native deadline expired after SQLite lock wait"),
    );
    assert!(result.is_err());
    assert_eq!(f.current(), before);
    assert_eq!(f.count(), 0);
}

#[test]
fn old_grant_receipt_cannot_hide_a_later_manual_revoke() {
    let mut f = Fixture::new();
    let grant = f.request(Action::Grant);
    assert!(f.apply(grant.clone()).service_granted());
    let tx = f.store.db.transaction().unwrap();
    invalidate(&tx).unwrap();
    tx.commit().unwrap();
    let receipt = lookup(&f.store, &f.location.configuration, f.query(&grant))
        .unwrap()
        .receipt
        .unwrap();
    assert!(receipt.matches(&grant, &f.location.configuration));
    assert!(matches!(receipt.outcome, Outcome::Granted { .. }));
    assert!(!receipt.service_granted());
}
