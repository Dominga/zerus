//! Phone-created media crosses the existing bounded, opaque update channel.
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};
use zerus_replica::{
    phone::{answer, Exchange},
    Actor, NewTask, ProjectStore, Stage, Target, TaskAttachment, TaskDoc,
};

fn image(store: &ProjectStore) -> (String, Vec<u8>) {
    let bytes: Vec<u8> = (0u64..25600)
        .flat_map(|i| Sha256::digest(i.to_le_bytes()))
        .collect();
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: &store.unused_short_id().unwrap(),
        text: "Inspect the photo",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap();
    task.set_attachments(
        &[TaskAttachment {
            name: "fixture.png".into(),
            mime: "image/png".into(),
            data_base64: STANDARD.encode(&bytes),
            reference: "image-1".into(),
        }],
        &Actor::Person,
        2,
    )
    .unwrap();
    store.save(&mut task).unwrap();
    (task.uuid().to_owned(), bytes)
}

#[test]
fn a_phone_created_large_image_arrives_complete() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = ProjectStore::open(b.path(), "project-1").unwrap();
    let (uuid, bytes) = image(&phone);
    let mut exchange = Exchange::with_uploads();
    let mut requests = 0;
    while let Some(request) = exchange.next_request(&phone).unwrap() {
        assert!(serde_json::to_vec(&request).unwrap().len() <= 1 << 20);
        let progress = exchange
            .accept(&phone, answer(&host, request).unwrap())
            .unwrap();
        assert!(
            progress.unsent.is_empty(),
            "large phone media was not uploaded: {:?}",
            progress.unsent
        );
        assert!(progress.refused_here.is_empty());
        assert!(progress.refused_there.is_empty());
        requests += 1;
    }
    assert!(requests > 2);
    assert_eq!(
        host.load(&uuid).unwrap().attachments().unwrap()[0]
            .bytes()
            .unwrap(),
        bytes
    );
}
