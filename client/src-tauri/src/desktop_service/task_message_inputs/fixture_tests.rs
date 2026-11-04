//! Listing and release past the capability probe, against the real service
//! fixture with a fake `hgs` that records every review request.
use super::*;
use crate::desktop_service::{
    task_message::fixture_tests::{connected, private_case},
    tests::Fixture,
};

const PRESENT: &str = "00000000-0000-4000-8000-000000000101";
const ABSENT: &str = "00000000-0000-4000-8000-000000000102";

fn fixture(config: Value) -> Fixture {
    let f = Fixture::new(config);
    std::fs::remove_file(f.0.join("hgs")).unwrap();
    std::fs::write(
        f.0.join("hgs"),
        format!(
            "#!/usr/bin/env python3\n{}\n{}",
            include_str!("fixture.py"),
            include_str!("../fake_hgs.py")
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(f.0.join("hgs"), std::fs::Permissions::from_mode(0o700)).unwrap();
    f
}
/// Every review request the fake `hgs` saw, in order.
fn calls(root: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("review-calls"))
        .unwrap_or_default()
        .lines()
        .filter_map(|s| serde_json::from_str(s).ok())
        .collect()
}
/// Every release the fake saw, each only on the native frontend lane.
fn releases(root: &std::path::Path) -> Vec<Value> {
    let sent: Vec<Value> = calls(root)
        .into_iter()
        .filter(|c| c["value"]["request"]["op"] == "task_message_input_release")
        .collect();
    for call in &sent {
        assert_eq!(
            call["args"][1], "frontend-request",
            "only the native frontend lane"
        );
    }
    sent.into_iter()
        .map(|c| c["value"]["request"]["release"].clone())
        .collect()
}
fn version() -> Value {
    json!({"device":1,"inode":2,"size":70000,"mode":0o100600,"links":1,
        "modified":[1,2],"changed":[3,4]})
}
fn entries() -> Value {
    json!([
        {"request_id":PRESENT,"state":"orphaned","bytes":70000,"input_sha256":"a".repeat(64),
            "created":1700000000000i64,"recorded":null,
            "observed":{"kind":"present","version":version()}},
        {"request_id":ABSENT,"state":"failed","bytes":80000,"input_sha256":"b".repeat(64),
            "created":1700000000001i64,"recorded":[5,6],"observed":{"kind":"absent"}},
    ])
}
async fn list_page(service: &Arc<Service>, after: Option<Cursor>) -> Result<PageView, String> {
    let service = service.clone();
    tauri::async_runtime::spawn_blocking(move || list(Some(service), after))
        .await
        .unwrap()
}
async fn release_one(service: &Arc<Service>, id: &str, listing: u64) -> Answer {
    let (service, id) = (service.clone(), id.to_owned());
    tauri::async_runtime::spawn_blocking(move || release(Some(service), &id, listing))
        .await
        .unwrap()
}

#[test]
fn a_release_names_exactly_the_listed_observation_once() {
    if !private_case("desktop_service::task_message_inputs::fixture_tests::a_release_names_exactly_the_listed_observation_once") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let next = json!({"created":1700000000001i64,"request_id":ABSENT});
        let f = fixture(json!({"review_help": true, "entries": entries(), "next": next}));
        let (service, _cache) = connected(&f).await;
        // Nothing to release before a listing.
        assert_eq!(release_one(&service, PRESENT, 1).await, Answer::NotSent);
        let page = list_page(&service, None).await.unwrap();
        let listing = page.listing;
        assert_eq!(
            page.rows,
            [
                Row {
                    request_id: PRESENT.into(),
                    created: 1700000000000,
                    bytes: 70000,
                    state: "orphaned".into(),
                    file: "present"
                },
                Row {
                    request_id: ABSENT.into(),
                    created: 1700000000001,
                    bytes: 80000,
                    state: "failed".into(),
                    file: "missing"
                },
            ]
        );
        assert_eq!(page.next, serde_json::from_value(next).unwrap());
        let listed = calls(&f.0);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["value"]["op"], "task_message_inputs");
        assert_eq!(listed[0]["value"]["after"], Value::Null);
        assert_eq!(
            listed[0]["args"][1], "request",
            "read-only, not a native request"
        );
        // The present copy: exactly its listed version, once.
        assert_eq!(
            release_one(&service, PRESENT, listing).await,
            Answer::Removed
        );
        assert_eq!(
            release_one(&service, PRESENT, listing).await,
            Answer::NotSent
        );
        // The absent slot: no version; another listing's row sends nothing.
        assert_eq!(
            release_one(&service, ABSENT, listing + 1).await,
            Answer::NotSent
        );
        assert_eq!(
            release_one(&service, ABSENT, listing).await,
            Answer::Removed
        );
        let sent = releases(&f.0);
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert_eq!(sent[0]["request_id"], PRESENT);
        assert_eq!(sent[0]["observed"], version());
        assert_eq!(
            (
                &sent[0]["state"],
                &sent[0]["bytes"],
                &sent[0]["input_sha256"]
            ),
            (&json!("orphaned"), &json!(70000), &json!("a".repeat(64)))
        );
        assert_eq!(sent[1]["request_id"], ABSENT);
        assert_eq!(sent[1]["observed"], Value::Null);
        assert_eq!(sent[1]["state"], "failed");
        assert_ne!(sent[0]["release_id"], sent[1]["release_id"]);
        // A later page names its cursor.
        let cursor: Cursor =
            serde_json::from_value(json!({"created":1,"request_id":PRESENT})).unwrap();
        list_page(&service, Some(cursor)).await.unwrap();
        assert_eq!(calls(&f.0).last().unwrap()["value"]["after"]["created"], 1);
        service.join().await.unwrap();
    });
}

#[test]
fn a_service_without_the_review_or_a_refused_page_lists_and_sends_nothing() {
    if !private_case("desktop_service::task_message_inputs::fixture_tests::a_service_without_the_review_or_a_refused_page_lists_and_sends_nothing") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"entries": entries()}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            list_page(&service, None).await,
            Err("task_message_inputs_unsupported".into())
        );
        assert!(calls(&f.0).is_empty(), "no listing without the marker");
        service.join().await.unwrap();

        let f = fixture(json!({"review_help": true, "entries": entries(),
            "page_code": "task_message_input_review_busy"}));
        let (service, _cache) = connected(&f).await;
        assert_eq!(
            list_page(&service, None).await,
            Err("task_message_input_review_busy".into())
        );
        assert_eq!(release_one(&service, PRESENT, 1).await, Answer::NotSent);
        assert!(
            releases(&f.0).is_empty(),
            "a refused page remembers nothing"
        );
        service.join().await.unwrap();
    });
}

#[test]
fn a_lost_or_kept_release_is_reported_and_never_sent_again() {
    if !private_case("desktop_service::task_message_inputs::fixture_tests::a_lost_or_kept_release_is_reported_and_never_sent_again") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"review_help": true, "entries": entries(), "release_lost": true}));
        let (service, _cache) = connected(&f).await;
        let listing = list_page(&service, None).await.unwrap().listing;
        assert_eq!(
            release_one(&service, PRESENT, listing).await,
            Answer::Unknown
        );
        assert_eq!(
            release_one(&service, PRESENT, listing).await,
            Answer::NotSent
        );
        assert_eq!(releases(&f.0).len(), 1, "one request, never retried");
        service.join().await.unwrap();

        let f = fixture(json!({"review_help": true, "entries": entries(),
            "outcome": "kept", "code": review::CHANGED}));
        let (service, _cache) = connected(&f).await;
        let listing = list_page(&service, None).await.unwrap().listing;
        assert_eq!(
            release_one(&service, PRESENT, listing).await,
            Answer::Changed
        );
        // A fresh review may confirm again: its new observation is sent.
        let again = list_page(&service, None).await.unwrap().listing;
        assert!(again > listing);
        assert_eq!(release_one(&service, PRESENT, again).await, Answer::Changed);
        assert_eq!(releases(&f.0).len(), 2);
        service.join().await.unwrap();
    });
}

#[test]
fn a_review_from_another_service_scope_is_never_sent() {
    if !private_case("desktop_service::task_message_inputs::fixture_tests::a_review_from_another_service_scope_is_never_sent") {
        return;
    }
    tauri::async_runtime::block_on(async {
        let f = fixture(json!({"review_help": true, "entries": entries()}));
        let (service, _cache) = connected(&f).await;
        let page = list_page(&service, None).await.unwrap();
        // The same record as an earlier service epoch observed it.
        let mut scope = calls(&f.0)[0]["value"]["scope"].clone();
        scope["service_epoch"] = json!("00000000-0000-4000-8000-0000000000ff");
        let foreign: Scope = serde_json::from_value(scope).unwrap();
        let listed: Vec<Entry> = serde_json::from_value(entries()).unwrap();
        let listing = service.shared.held_inputs.begin();
        service
            .shared
            .held_inputs
            .remember(listing, &foreign, &listed[..1]);
        assert_eq!(
            release_one(&service, PRESENT, listing).await,
            Answer::NotSent
        );
        assert_eq!(
            release_one(&service, PRESENT, page.listing).await,
            Answer::NotSent
        );
        assert!(releases(&f.0).is_empty(), "nothing reached the service");
        service.join().await.unwrap();
    });
}
