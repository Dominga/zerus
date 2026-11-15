use serde_json::json;
use std::collections::BTreeMap;
use zerus_replica::mobile_edit::wire::{Command, Frame, Header};

const TASK: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const EDIT: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const NONCE: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const STORE: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";

fn qualify() -> Frame {
    Frame {
        header: Header {
            project: "project-1".into(),
            task: TASK.into(),
            command: Command::Qualify {
                phone_store: STORE.into(),
                edit_id: EDIT.into(),
                seal: "a".repeat(64),
                version: BTreeMap::from([("123".into(), 10)]),
            },
        },
        body: vec![],
    }
}
fn apply() -> Frame {
    let body = serde_json::to_vec(
        &json!({"version":1,"project":"project-1","task":TASK,"edit_id":EDIT,"receipt_nonce":NONCE,
        "edits":[{"kind":"comment","text":"A reviewed comment","attachments":[]}]}),
    )
    .unwrap();
    Frame {
        header: Header {
            project: "project-1".into(),
            task: TASK.into(),
            command: Command::Apply {
                phone_store: STORE.into(),
                edit_id: EDIT.into(),
                receipt_nonce: NONCE.into(),
            },
        },
        body,
    }
}

#[test]
fn qualification_and_inline_edits_round_trip_inside_one_opaque_page() {
    for frame in [qualify(), apply()] {
        let encoded = frame.encode().unwrap();
        let decoded = Frame::decode(&encoded).expect("a valid reviewed frame must decode");
        assert_eq!(decoded.header, frame.header);
        assert_eq!(decoded.body, frame.body);
        assert!(encoded.len() <= zerus_replica::phone::PAGE);
    }
}

#[test]
fn a_frame_cannot_retarget_an_inline_manifest() {
    for part in ["task", "project", "edit_id", "receipt_nonce"] {
        let mut frame = apply();
        let mut body: serde_json::Value = serde_json::from_slice(&frame.body).unwrap();
        body[part] = json!(if part == "project" {
            "project-2"
        } else {
            STORE
        });
        frame.body = serde_json::to_vec(&body).unwrap();
        assert!(
            Frame::decode(&frame.encode().unwrap()).is_err(),
            "accepted changed {part}"
        );
    }
}

#[test]
fn only_the_reviewed_version_and_bounded_exact_header_are_understood() {
    let encoded = qualify().encode().unwrap();
    for end in [0, 7, 8, 11, 12, encoded.len() - 1] {
        assert!(Frame::decode(&encoded[..end]).is_err());
    }
    let mut bad = encoded.clone();
    bad[7] = b'2';
    assert!(Frame::decode(&bad).is_err());
    let mut bad = encoded;
    bad[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(Frame::decode(&bad).is_err());
    assert!(Frame::decode(&vec![0; zerus_replica::phone::PAGE + 1]).is_err());
}

#[test]
fn qualification_cannot_carry_hidden_operations_or_noncanonical_clocks() {
    let mut frame = qualify();
    frame.body = b"hidden mutation".to_vec();
    assert!(Frame::decode(&frame.encode().unwrap()).is_err());
    let mut frame = qualify();
    let Command::Qualify { version, .. } = &mut frame.header.command else {
        panic!()
    };
    *version = BTreeMap::from([("0123".into(), 10)]);
    assert!(Frame::decode(&frame.encode().unwrap()).is_err());
}

#[test]
fn upload_progress_needs_the_exact_frozen_acknowledgement_before_another_part() {
    use zerus_replica::mobile_edit::{wire::Upload, Manifest};
    let manifest = Manifest::decode(&apply().body).unwrap();
    let mut upload = Upload::new(&manifest, STORE).unwrap();
    let frame = upload.next_frame().unwrap().unwrap();
    assert!(upload.next_frame().is_err());
    let exact = upload.expected_ack().unwrap();
    for part in ["project", "task", "sha256", "size", "offset", "complete"] {
        let mut bad = serde_json::to_value(&exact).unwrap();
        match part {
            "project" => bad["source"][part] = json!("project-2"),
            "task" => bad["source"][part] = json!(STORE),
            "sha256" => bad["source"][part] = json!("0".repeat(64)),
            "size" => bad["source"][part] = json!(exact.source.size + 1),
            "offset" => bad[part] = json!(exact.offset - 1),
            "complete" => bad[part] = json!(!exact.complete),
            _ => unreachable!(),
        }
        assert!(
            upload
                .accept(&serde_json::from_value(bad).unwrap())
                .is_err(),
            "accepted changed {part}"
        );
        assert_eq!(upload.expected_ack().as_ref(), Some(&exact));
    }
    assert_eq!(
        Frame::decode(&frame.encode().unwrap())
            .unwrap()
            .upload_ack()
            .unwrap(),
        exact
    );
    upload.accept(&exact).unwrap();
    assert!(upload.next_frame().unwrap().is_none());
    assert!(upload.accept(&exact).is_err());
}
