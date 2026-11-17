//! The new path is explicit; source, offsets, custody and legacy behavior stay bound.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};
use zerus_replica::{
    phone::{answer, Answer, Budget, Exchange, Request},
    Actor, NewTask, ProjectStore, Stage, Target, TaskAttachment, TaskDoc,
};

fn store(root: &Path) -> ProjectStore {
    ProjectStore::open(root, "project-1").unwrap()
}
fn image(store: &ProjectStore) -> (String, Vec<u8>) {
    let bytes: Vec<u8> = (0u64..25600)
        .flat_map(|i| Sha256::digest(i.to_le_bytes()))
        .collect();
    let mut task = TaskDoc::create(NewTask {
        project: "project-1",
        id: &store.unused_short_id().unwrap(),
        text: "Original requirement",
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
fn upload(exchange: &mut Exchange, phone: &ProjectStore, host: &ProjectStore) -> Request {
    for _ in 0..10 {
        let request = exchange.next_request(phone).unwrap().unwrap();
        if request
            .updates
            .iter()
            .any(|u| STANDARD.decode(&u.bytes).unwrap().starts_with(b"ZERUSUP1"))
        {
            return request;
        }
        exchange
            .accept(phone, answer(host, request).unwrap())
            .unwrap();
    }
    panic!("no upload frame");
}
fn split(request: &Request) -> (Value, Vec<u8>) {
    assert_eq!(request.updates.len(), 1);
    let bytes = STANDARD.decode(&request.updates[0].bytes).unwrap();
    assert_eq!(&bytes[..8], b"ZERUSUP1");
    let size = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;
    (
        serde_json::from_slice(&bytes[12..12 + size]).unwrap(),
        bytes[12 + size..].to_vec(),
    )
}
fn replace(request: &mut Request, header: Value, chunk: Vec<u8>) {
    let header = serde_json::to_vec(&header).unwrap();
    let mut bytes = b"ZERUSUP1".to_vec();
    bytes.extend_from_slice(&(header.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&header);
    bytes.extend(chunk);
    request.updates[0].bytes = STANDARD.encode(bytes);
}
fn finish(exchange: &mut Exchange, phone: &ProjectStore, host: &ProjectStore) {
    while let Some(request) = exchange.next_request(phone).unwrap() {
        assert!(serde_json::to_vec(&request).unwrap().len() <= 1 << 20);
        let progress = exchange
            .accept(phone, answer(host, request).unwrap())
            .unwrap();
        assert!(progress.unsent.is_empty());
        assert!(progress.refused_here.is_empty());
        assert!(progress.refused_there.is_empty());
    }
}

#[test]
fn ordinary_exchange_keeps_the_downlevel_large_update_refusal() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = store(b.path());
    let (uuid, _) = image(&phone);
    let mut old = Exchange::new();
    let mut unsent = Vec::new();
    while let Some(request) = old.next_request(&phone).unwrap() {
        assert!(request
            .updates
            .iter()
            .all(|u| !STANDARD.decode(&u.bytes).unwrap().starts_with(b"ZERUSUP1")));
        unsent.extend(
            old.accept(&phone, answer(&host, request).unwrap())
                .unwrap()
                .unsent,
        );
    }
    assert!(unsent.contains(&uuid));
    assert!(host.load(&uuid).is_err());
}

#[test]
fn upload_survives_host_restarts_and_freezes_its_source_while_rows_arrive() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let (uuid, bytes) = image(&phone);
    let mut host = store(b.path());
    let mut exchange = Exchange::with_uploads();
    let first = upload(&mut exchange, &phone, &host);
    let source = split(&first).0["source"].clone();
    let answered = answer(&host, first).unwrap();
    assert!(host.load(&uuid).is_err());
    assert_eq!(answered.imported, 0);
    assert!(
        answered.refused.is_empty(),
        "a prefix must not be submitted to canonical import"
    );
    exchange.accept(&phone, answered).unwrap();
    let mut edited = phone.load(&uuid).unwrap();
    edited
        .edit_text(
            "Edited while the original bytes are in flight",
            &Actor::Person,
            3,
        )
        .unwrap();
    phone.save(&mut edited).unwrap();
    let (other, _) = image(&phone);
    let mut frames = 1;
    while let Some(request) = exchange.next_request(&phone).unwrap() {
        if request
            .updates
            .iter()
            .any(|u| STANDARD.decode(&u.bytes).unwrap().starts_with(b"ZERUSUP1"))
        {
            let header = split(&request).0;
            if header["source"]["task"] == uuid && frames < 2 {
                assert_eq!(header["source"], source);
                frames += 1;
            }
        }
        drop(host);
        host = store(b.path());
        let result = answer(&host, request).unwrap();
        exchange.accept(&phone, result).unwrap();
    }
    assert!(frames >= 2);
    assert_eq!(
        host.load(&uuid).unwrap().view().unwrap().text,
        "Edited while the original bytes are in flight"
    );
    for id in [&uuid, &other] {
        assert_eq!(
            host.load(id).unwrap().attachments().unwrap()[0]
                .bytes()
                .unwrap(),
            bytes
        );
    }
    let slot = host.dir().join(".phone-upload");
    assert!(fs::metadata(slot).unwrap().len() <= zerus_replica::MAX_TASK_DOCUMENT as u64 + 1024);
    assert!(!host.dir().read_dir().unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("upload-tmp")));
}

#[test]
fn a_chunk_and_its_acknowledgement_are_exact_and_repeating_it_only_repeats_replication() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = store(b.path());
    let (uuid, _) = image(&phone);
    let mut exchange = Exchange::with_uploads();
    let first = upload(&mut exchange, &phone, &host);
    let accepted = answer(&host, first.clone()).unwrap();
    let slot = fs::read(host.dir().join(".phone-upload")).unwrap();
    assert_eq!(answer(&host, first).unwrap().upload, accepted.upload);
    assert_eq!(fs::read(host.dir().join(".phone-upload")).unwrap(), slot);
    assert!(exchange.next_request(&phone).is_err());
    let value = serde_json::to_value(&accepted).unwrap();
    for (field, replacement) in [
        ("project", json!("other")),
        ("task", json!("11111111-1111-4111-8111-111111111111")),
        ("sha256", json!("00".repeat(32))),
        ("size", json!(1)),
    ] {
        let mut wrong = value.clone();
        wrong["upload"]["source"][field] = replacement;
        assert!(
            exchange
                .accept(&phone, serde_json::from_value(wrong).unwrap())
                .is_err(),
            "{field}"
        );
    }
    for (field, replacement) in [("offset", json!(0)), ("complete", json!(true))] {
        let mut wrong = value.clone();
        wrong["upload"][field] = replacement;
        assert!(
            exchange
                .accept(&phone, serde_json::from_value(wrong).unwrap())
                .is_err(),
            "{field}"
        );
    }
    let mut missing = value;
    missing.as_object_mut().unwrap().remove("upload");
    assert!(exchange
        .accept(&phone, serde_json::from_value(missing).unwrap())
        .is_err());
    exchange.accept(&phone, accepted.clone()).unwrap();
    assert!(exchange.accept(&phone, accepted).is_err());
    finish(&mut exchange, &phone, &host);
    assert!(host.load(&uuid).is_ok());
}

#[test]
fn bad_frame_bindings_lengths_and_offsets_never_change_the_slot_or_import_a_prefix() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = store(b.path());
    let (uuid, _) = image(&phone);
    let mut exchange = Exchange::with_uploads();
    let first = upload(&mut exchange, &phone, &host);
    let (header, chunk) = split(&first);
    for mut bad in [
        first.clone(),
        first.clone(),
        first.clone(),
        first.clone(),
        first.clone(),
        first.clone(),
    ]
    .into_iter()
    .enumerate()
    {
        let mut h = header.clone();
        let mut data = chunk.clone();
        match bad.0 {
            0 => h["source"]["project"] = json!("other"),
            1 => h["source"]["task"] = json!("11111111-1111-4111-8111-111111111111"),
            2 => h["source"]["size"] = json!(zerus_replica::MAX_TASK_DOCUMENT + 1),
            3 => h["offset"] = json!(usize::MAX),
            4 => data.clear(),
            _ => data.resize(zerus_replica::phone::PAGE + 1, 0),
        }
        replace(&mut bad.1, h, data);
        assert!(answer(&host, bad.1).is_err(), "case {}", bad.0);
        assert!(!host.dir().join(".phone-upload").exists());
        assert!(host.load(&uuid).is_err());
    }
    let mut mixed = first.clone();
    mixed.updates.push(first.updates[0].clone());
    assert!(answer(&host, mixed).is_err());
    let duplicate = first.clone();
    let ack = answer(&host, first).unwrap();
    exchange.accept(&phone, ack).unwrap();
    let (h, mut changed) = split(&duplicate);
    changed[0] ^= 1;
    let mut bad_duplicate = duplicate;
    replace(&mut bad_duplicate, h, changed);
    assert!(answer(&host, bad_duplicate).is_err());
    let next = exchange.next_request(&phone).unwrap().unwrap();
    let original = fs::read(host.dir().join(".phone-upload")).unwrap();
    let (mut header, chunk) = split(&next);
    header["offset"] = json!(header["offset"].as_u64().unwrap() + 1);
    let mut bad = next.clone();
    replace(&mut bad, header, chunk);
    assert!(answer(&host, bad).is_err());
    assert_eq!(
        fs::read(host.dir().join(".phone-upload")).unwrap(),
        original
    );
    assert!(host.load(&uuid).is_err());
    exchange
        .accept(&phone, answer(&host, next).unwrap())
        .unwrap();
    finish(&mut exchange, &phone, &host);
}

#[test]
fn the_whole_hash_and_canonical_task_identity_are_checked_before_import() {
    for wrong_task in [false, true] {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
        let host = store(b.path());
        let (uuid, _) = image(&phone);
        let mut exchange = Exchange::with_uploads();
        let mut request = upload(&mut exchange, &phone, &host);
        loop {
            let (mut header, chunk) = split(&request);
            if wrong_task {
                header["source"]["task"] = json!("11111111-1111-4111-8111-111111111111");
                request.updates[0].task = Some("11111111-1111-4111-8111-111111111111".into());
            } else {
                header["source"]["sha256"] = json!("00".repeat(32));
            }
            let end = header["offset"].as_u64().unwrap() + chunk.len() as u64;
            let total = header["source"]["size"].as_u64().unwrap();
            replace(&mut request, header, chunk);
            match answer(&host, request.clone()) {
                Ok(result) if end == total => {
                    assert!(!result.refused.is_empty());
                    break;
                }
                Ok(_) => {
                    // Advance the source phone using the honest source-bound ack, not the tampered identity.
                    let (h, _) = split(&request);
                    let offset = end;
                    let honest_source = serde_json::json!({"project":"project-1","task":uuid,"sha256":sha256_of_frozen(&phone,&host,&uuid),"size":h["source"]["size"]});
                    let result:Answer=serde_json::from_value(json!({"schema":1,"imported":0,"refused":[],"updates":[],"more":false,"digest":"","upload":{"source":honest_source,"offset":offset,"complete":false}})).unwrap();
                    exchange.accept(&phone, result).unwrap();
                    request = exchange.next_request(&phone).unwrap().unwrap();
                }
                Err(_) => {
                    assert_eq!(end, total);
                    break;
                }
            }
        }
        assert!(host.load(&uuid).is_err());
        assert!(host.load("11111111-1111-4111-8111-111111111111").is_err());
    }
}
fn sha256_of_frozen(phone: &ProjectStore, host: &ProjectStore, uuid: &str) -> String {
    let ours = phone.inventory().unwrap().0;
    let theirs = host.inventory().unwrap().0;
    let (updates, _) = phone.updates_for(&ours, &theirs, usize::MAX).unwrap();
    format!(
        "{:x}",
        Sha256::digest(
            &updates
                .iter()
                .find(|u| u.document == zerus_replica::Document::Task(uuid.into()))
                .unwrap()
                .bytes
        )
    )
}

#[test]
fn a_busy_private_slot_is_bounded_and_expired_document_sync_can_restart() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let c = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = store(b.path());
    let other = ProjectStore::open_phone(c.path(), "project-1").unwrap();
    image(&phone);
    image(&other);
    let mut first = Exchange::with_uploads();
    let first_request = upload(&mut first, &phone, &host);
    answer(&host, first_request).unwrap();
    let mut second = Exchange::with_uploads();
    let second_request = upload(&mut second, &other, &host);
    assert!(answer(&host, second_request.clone())
        .unwrap_err()
        .to_string()
        .contains("busy"));
    let file = fs::OpenOptions::new()
        .write(true)
        .open(host.dir().join(".phone-upload"))
        .unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(601))
        .unwrap();
    drop(file);
    let ack = answer(&host, second_request).unwrap();
    second.accept(&other, ack).unwrap();
    finish(&mut second, &other, &host);
}

#[test]
fn losing_the_final_answer_recovers_only_by_document_sync() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = store(b.path());
    let (uuid, bytes) = image(&phone);
    let mut exchange = Exchange::with_uploads();
    let first = upload(&mut exchange, &phone, &host);
    let mut next = Some(first);
    while let Some(request) = next.take() {
        let ack = answer(&host, request).unwrap();
        if ack.upload.as_ref().is_some_and(|a| a.complete) {
            break;
        }
        exchange.accept(&phone, ack).unwrap();
        next = exchange.next_request(&phone).unwrap();
    }
    assert!(exchange.next_request(&phone).is_err());
    assert_eq!(
        host.load(&uuid).unwrap().attachments().unwrap()[0]
            .bytes()
            .unwrap(),
        bytes
    );
    let before = host.load(&uuid).unwrap().snapshot().unwrap();
    let mut recovery = Exchange::with_uploads();
    finish(&mut recovery, &phone, &host);
    assert_eq!(host.load(&uuid).unwrap().snapshot().unwrap(), before);
}

#[cfg(unix)]
#[test]
fn private_namespace_replacement_links_and_special_files_are_refused() {
    use std::os::unix::fs::symlink;
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = store(b.path());
    image(&phone);
    let mut exchange = Exchange::with_uploads();
    let request = upload(&mut exchange, &phone, &host);
    let slot = host.dir().join(".phone-upload");
    let external = b.path().join("unrelated");
    fs::write(&external, b"unchanged").unwrap();
    fs::set_permissions(
        &external,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    symlink(&external, &slot).unwrap();
    assert!(answer(&host, request.clone()).is_err());
    fs::remove_file(&slot).unwrap();
    assert_eq!(fs::read(&external).unwrap(), b"unchanged");
    fs::hard_link(&external, &slot).unwrap();
    assert!(answer(&host, request.clone()).is_err());
    fs::remove_file(&slot).unwrap();
    assert_eq!(fs::read(&external).unwrap(), b"unchanged");
    fs::create_dir(&slot).unwrap();
    assert!(answer(&host, request.clone()).is_err());
    fs::remove_dir(&slot).unwrap();
    let moved = host.dir().with_extension("held");
    fs::rename(host.dir(), &moved).unwrap();
    fs::create_dir(host.dir()).unwrap();
    assert!(answer(&host, request).is_err());
    assert!(!slot.exists());
}

#[test]
fn a_caller_cannot_make_the_chunk_budget_unbounded_or_too_small_for_framing() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = store(b.path());
    image(&phone);
    let mut large = Exchange::with_upload_budget(Budget {
        push_bytes: usize::MAX,
        ..Budget::default()
    });
    let request = upload(&mut large, &phone, &host);
    assert!(
        STANDARD.decode(&request.updates[0].bytes).unwrap().len() <= zerus_replica::phone::PAGE
    );
    large
        .accept(&phone, answer(&host, request).unwrap())
        .unwrap();
    finish(&mut large, &phone, &host);
    // A second host has no copy: the small-budget path must still need to push.
    let c = tempfile::tempdir().unwrap();
    let host = store(c.path());
    let mut exchange = Exchange::with_upload_budget(Budget {
        push_bytes: 16,
        ..Budget::default()
    });
    for _ in 0..10 {
        match exchange.next_request(&phone) {
            Err(e) => {
                assert!(e.to_string().contains("budget"));
                return;
            }
            Ok(Some(r)) => {
                if let Err(e) = exchange.accept(&phone, answer(&host, r).unwrap()) {
                    assert!(e.to_string().contains("budget"));
                    return;
                }
            }
            _ => panic!("large source was silently skipped"),
        }
    }
    panic!("framing did not reject a tiny budget");
}

#[test]
fn an_upload_keeps_the_original_request_budget_and_an_incomplete_slot_is_not_delivery() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let phone = ProjectStore::open_phone(a.path(), "project-1").unwrap();
    let host = store(b.path());
    let (uuid, _) = image(&phone);
    let mut exchange = Exchange::with_upload_budget(Budget {
        requests: 2,
        ..Budget::default()
    });
    for _ in 0..2 {
        let request = exchange.next_request(&phone).unwrap().unwrap();
        exchange
            .accept(&phone, answer(&host, request).unwrap())
            .unwrap();
    }
    let error = exchange.next_request(&phone).unwrap_err().to_string();
    assert!(error.contains("within 2 requests"));
    assert!(host.load(&uuid).is_err());
}
