//! These are data-format controls. No decoded value grants native admission.
use serde_json::{json, Value};
use zerus_replica::mobile_edit::{Manifest, MAX_EDITS, MAX_MANIFEST};

fn manifest() -> Value {
    json!({"version":1,"project":"project-1",
        "task":"aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "edit_id":"bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        "receipt_nonce":"cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        "edits":[
            {"kind":"content","expected":"a".repeat(64),"title":"A title",
             "text":"A requirement","media":[{"kind":"keep","index":0}]},
            {"kind":"target","expected":{"kind":"any"},
             "target":{"kind":"session","session":{"machine":"computer-1",
             "session":"codex/project/worker","session_id":"launch_id:opaque"}}},
            {"kind":"rank","expected":"V","rank":"W"},
            {"kind":"comment","text":"A comment","attachments":[{
             "name":"fixture.txt","mime":"text/plain","data_base64":"eA=="}]}]})
}

fn decode(value: &Value) -> zerus_replica::Result<Manifest> {
    Manifest::decode(&serde_json::to_vec(value).unwrap())
}

#[test]
fn the_four_allowed_intents_round_trip_without_crdt_or_authority() {
    let draft = decode(&manifest()).expect("the reviewed declarative format must decode");
    assert_eq!(draft.edits.len(), MAX_EDITS);
    assert_eq!(
        Manifest::decode(&serde_json::to_vec(&draft).unwrap()).unwrap(),
        draft
    );
}

#[test]
fn execution_and_raw_crdt_intents_cannot_be_encoded() {
    for kind in [
        "stage", "attempt", "start", "report", "claim", "meta", "project", "task", "crdt",
    ] {
        let mut value = manifest();
        value["edits"] = json!([{"kind":kind}]);
        assert!(decode(&value).is_err(), "accepted {kind}");
    }
}

#[test]
fn unknown_fields_are_refused_at_every_nesting_level() {
    for pointer in [
        "",
        "/edits/0",
        "/edits/0/media/0",
        "/edits/1/target",
        "/edits/1/target/session",
        "/edits/3/attachments/0",
    ] {
        let mut value = manifest();
        value.pointer_mut(pointer).unwrap()["start"] = json!(true);
        assert!(decode(&value).is_err(), "accepted extra field at {pointer}");
    }
}

#[test]
fn identity_and_version_are_explicit_and_canonical() {
    for (key, bad) in [
        ("version", json!(2)),
        ("project", json!("")),
        ("project", json!("bad\nproject")),
        ("project", json!("x".repeat(129))),
        ("task", json!("T-hint")),
        ("task", json!("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA")),
        ("edit_id", json!("")),
        ("receipt_nonce", Value::Null),
    ] {
        let mut value = manifest();
        value[key] = bad;
        assert!(decode(&value).is_err(), "accepted invalid {key}");
    }
}

#[test]
fn empty_duplicate_and_excess_intents_are_refused() {
    let mut value = manifest();
    value["edits"] = json!([]);
    assert!(decode(&value).is_err());
    let comment = manifest()["edits"][3].clone();
    value["edits"] = json!([comment.clone(), comment.clone()]);
    assert!(decode(&value).is_err());
    value["edits"] = json!(vec![comment; MAX_EDITS + 1]);
    assert!(decode(&value).is_err());
}

#[test]
fn text_media_and_retained_indices_keep_the_existing_limits() {
    for (pointer, bad) in [
        ("/edits/0/expected", json!("A".repeat(64))),
        (
            "/edits/0/text",
            json!("x".repeat(zerus_replica::MAX_TEXT + 1)),
        ),
        (
            "/edits/0/media",
            json!([{"kind":"keep","index":0},{"kind":"keep","index":0}]),
        ),
        ("/edits/0/media/0/index", json!(8)),
        ("/edits/1/target/session/session_id", json!("")),
        ("/edits/2/rank", json!("W0")),
        ("/edits/3/attachments/0/name", json!("../fixture.txt")),
        ("/edits/3/attachments/0/data_base64", json!("invalid")),
    ] {
        let mut value = manifest();
        *value.pointer_mut(pointer).unwrap() = bad;
        assert!(decode(&value).is_err(), "accepted invalid {pointer}");
    }
}

#[test]
fn wire_size_is_checked_before_decoding() {
    assert!(Manifest::decode(&vec![b' '; MAX_MANIFEST + 1]).is_err());
    assert!(Manifest::decode(b"{\"version\":1,\"version\":1}").is_err());
}
