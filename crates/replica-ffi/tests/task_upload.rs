//! Explicit FFI opt-in is separate from the unchanged downlevel exchange.
use sha2::{Digest, Sha256};
use zerus_replica_ffi::{TaskMediaInput, TaskPosition, TaskStage, TaskStore, TaskTarget};

#[test]
fn the_opted_in_ffi_upload_keeps_all_bytes_and_reopens_its_host_each_page() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = TaskStore::new(
        a.path().to_string_lossy().into(),
        "project-1".into(),
        "Phone".into(),
    );
    let host = TaskStore::new(
        b.path().to_string_lossy().into(),
        "project-1".into(),
        "Host".into(),
    );
    let bytes: Vec<u8> = (0u64..25600)
        .flat_map(|i| Sha256::digest(i.to_le_bytes()))
        .collect();
    let task = phone
        .add_with_media(
            "Inspect the original photo".into(),
            TaskStage::Todo,
            TaskTarget::AnySession,
            TaskPosition::End,
            vec![TaskMediaInput {
                name: "fixture.png".into(),
                mime: "image/png".into(),
                data: bytes.clone(),
                reference: "image-1".into(),
            }],
        )
        .unwrap();
    let old = phone.clone().exchange();
    let mut skipped = false;
    while let Some(payload) = old.next_request().unwrap() {
        let result = host.answer_exchange(payload).unwrap();
        skipped |= old.accept(result).unwrap().unsent.contains(&task.uuid);
    }
    assert!(skipped);
    assert!(host.get(task.uuid.clone()).is_err());
    let exchange = phone.clone().exchange_uploading(vec![]);
    let mut requests = 0;
    while let Some(payload) = exchange.next_request().unwrap() {
        assert!(payload.len() <= 1 << 20);
        let json: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert!(json.as_object().unwrap().keys().all(|key| [
            "schema",
            "project",
            "inventory",
            "range",
            "updates",
            "continue"
        ]
        .contains(&key.as_str())));
        let progress = exchange
            .accept(host.answer_exchange(payload).unwrap())
            .unwrap();
        assert!(progress.unsent.is_empty());
        assert!(progress.refused_there.is_empty());
        assert!(progress.refused_here.is_empty());
        requests += 1;
    }
    assert!(requests > 2);
    let media = host.media(task.uuid.clone(), None).unwrap();
    assert_eq!(media.len(), 1);
    assert_eq!(
        host.read_media(task.uuid.clone(), None, 0, media[0].token.clone())
            .unwrap()
            .data,
        bytes
    );
    // The same constructor still offers bounded Done-task fetches.
    let fetch = phone.clone().exchange_uploading(vec![task.uuid]);
    while let Some(payload) = fetch.next_request().unwrap() {
        fetch
            .accept(host.answer_exchange(payload).unwrap())
            .unwrap();
    }
}
