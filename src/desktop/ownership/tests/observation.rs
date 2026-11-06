//! Actor/Origin policy controls, not a native frontend authentication positive.
use super::*;
use zerus_client_core::desktop::ownership::observation::Query as Read;

fn query(f: &Fixture) -> Read {
    let current = f.current();
    Read {
        request_id: uuid::Uuid::new_v4().to_string(),
        configuration: f.location.configuration.clone(),
        journal: current.journal.unwrap(),
        owner_epoch: current.owner_epoch.unwrap(),
        service_epoch: f.epoch.clone(),
        generation: f.store.generation,
    }
}
fn permit(query: &Read, origin: Origin) -> ui_authority::Validated {
    ui_authority::Validated::fixture_observation(
        &serde_json::to_value(Request::DesktopOwnershipObserve {
            query: Box::new(query.clone()),
        })
        .unwrap(),
        origin,
    )
}
fn granted() -> Fixture {
    let mut f = Fixture::new();
    let request = f.request(Action::Grant);
    assert!(f.apply(request).service_granted());
    f
}
fn read(f: &Fixture, query: &Read, origin: Origin) -> Result<wire::observation::Observation> {
    super::super::observation::frontend(&f.location, &f.store, &f.epoch, permit(query, origin))
}

#[test]
fn current_requester_read_preserves_the_grant_and_all_reservations() {
    let f = granted();
    let query = query(&f);
    let before = (
        f.current(),
        f.writer(),
        f.count(),
        f.store.db.total_changes(),
    );
    let result = read(&f, &query, f.origin.clone()).unwrap();
    assert!(result.matches(&query));
    assert_eq!(
        (
            f.current(),
            f.writer(),
            f.count(),
            f.store.db.total_changes()
        ),
        before
    );
    let request = permit(&query, f.origin.clone());
    assert!(
        request.ownership_parts().is_err(),
        "a read is not ownership mutation authority"
    );
    assert!(
        permit(&query, f.origin.clone()).into_request().is_err(),
        "no generic effect dispatch"
    );
}

#[test]
fn a_different_requester_is_refused_even_when_the_display_status_is_granted_and_current() {
    let f = granted();
    let query = query(&f);
    assert!(query.matches(&f.current(), &f.epoch, f.store.generation));
    let other_image = f.base.join("other-frontend");
    std::fs::write(&other_image, b"another original frontend").unwrap();
    let other = Origin::capture(&other_image, f.origin.build.clone()).unwrap();
    assert!(other.current());
    assert!(read(&f, &query, other).is_err());
    let mut other_build = f.origin.clone();
    other_build.build.source_commit = "b".repeat(40);
    assert!(read(&f, &query, other_build).is_err());
    assert_eq!(f.count(), 1);
}

#[test]
fn stale_configuration_incarnation_and_current_endpoint_are_refused_without_writes() {
    let f = granted();
    let before = f.store.db.total_changes();
    for field in [
        "configuration",
        "journal",
        "owner_epoch",
        "service_epoch",
        "generation",
    ] {
        let mut q = query(&f);
        match field {
            "configuration" => q.configuration = "b".repeat(64),
            "journal" => q.journal = uuid::Uuid::new_v4().to_string(),
            "owner_epoch" => q.owner_epoch = uuid::Uuid::new_v4().to_string(),
            "service_epoch" => q.service_epoch = uuid::Uuid::new_v4().to_string(),
            _ => q.generation += 1,
        }
        assert!(read(&f, &q, f.origin.clone()).is_err(), "{field}");
    }
    assert_eq!(f.store.db.total_changes(), before);
    assert_eq!(f.count(), 1);
}

#[test]
fn revoke_preparing_drain_and_replaced_frontend_cannot_be_recovered_by_reading() {
    let f = granted();
    let query = query(&f);
    for state in ["preparing", "draining", "revoked", "native", "passive"] {
        f.store
            .db
            .execute("UPDATE desktop_ownership SET state=?1 WHERE id=1", [state])
            .unwrap();
        assert!(read(&f, &query, f.origin.clone()).is_err(), "{state}");
    }
    f.store
        .db
        .execute(
            "UPDATE desktop_ownership SET state='service',rearm_required=1 WHERE id=1",
            [],
        )
        .unwrap();
    assert!(read(&f, &query, f.origin.clone()).is_err());
    f.store
        .db
        .execute(
            "UPDATE desktop_ownership SET rearm_required=0 WHERE id=1",
            [],
        )
        .unwrap();
    std::fs::write(&f.origin.image, b"changed original image").unwrap();
    assert!(read(&f, &query, f.origin.clone()).is_err());
    assert_eq!(f.count(), 1);
}

#[cfg(unix)]
#[test]
fn a_database_replacement_cannot_supply_the_live_actors_observation() {
    let f = granted();
    let query = query(&f);
    let path = f.location.root.join("state.sqlite3");
    std::fs::rename(&path, f.location.root.join("original.sqlite3")).unwrap();
    std::fs::write(&path, b"a different database object").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(read(&f, &query, f.origin.clone()).is_err());
}
