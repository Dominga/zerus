//! Seed only the explicitly isolated real tmux/PTY qualification tree. Actual
//! requests then run the production CLI/controller, never a synthetic driver.
use super::*;

#[test]
#[ignore = "private ID seed entry for qualify_effort_custody.py"]
fn seed_actual_effort_record() {
    assert_eq!(
        std::env::var("ZERUS_PICKER_EFFORT_SEED").as_deref(),
        Ok("1")
    );
    let source = PathBuf::from(std::env::var_os("ZERUS_PICKER_EFFORT_RECORD").unwrap());
    let home = crate::platform::canonical(crate::platform::home().unwrap()).unwrap();
    assert_eq!(
        crate::platform::canonical(source.parent().unwrap()).unwrap(),
        home
    );
    assert_eq!(
        crate::platform::canonical(root().parent().unwrap()).unwrap(),
        home
    );
    let bytes = fs::read(&source).unwrap();
    assert!(bytes.len() <= 64 << 10);
    let mut record: Value = serde_json::from_slice(&bytes).unwrap();
    let agent = string(&record, "agent").to_owned();
    assert!(["claude", "codex", "kimi"].contains(&agent.as_str()));
    assert!(string(&record, "name").starts_with(&format!("{agent}/")));
    let writer = lock(None).unwrap();
    private_dir(&root().join("effort_receipts")).unwrap();
    let legacy = crate::state::storage::legacy_record_path(string(&record, "name"));
    // Read the SAME actual raw saved record under Root, never an inspect DTO
    // or rewritten native PID/run/conversation supplied by a test callback.
    let current: Value = serde_json::from_slice(&fs::read(&legacy).unwrap()).unwrap();
    assert!(
        same_target(&record, &current),
        "native seed selector/binding changed"
    );
    record = current;
    let kind = std::env::var("ZERUS_PICKER_EFFORT_KIND").unwrap();
    match kind.as_str() {
        "fresh" => {
            // Initial-publication fixtures only. Never manufacture Fresh by
            // deleting a real supervised native session's published identity.
            assert!(string(&record, "session_id").is_empty() && !record["supervisor"].is_object());
            fs::remove_file(&legacy).unwrap();
            record["session_id"] = json!(uuid::Uuid::new_v4().to_string());
            record["terminal_name"] = record["name"].clone();
            record["layout"] = json!("id");
            publish_new_id(&writer, &mut record).unwrap();
        }
        "moved" => {
            if string(&record, "session_id").is_empty() {
                assert!(
                    !record["supervisor"].is_object(),
                    "a supervised native run must keep its exact session UUID"
                );
                record["session_id"] = json!(uuid::Uuid::new_v4().to_string());
            } else {
                assert!(crate::state::session_id::canonical(string(
                    &record,
                    "session_id"
                )));
            }
            atomic(&legacy, &record.to_string()).unwrap();
            record =
                owner_transfer::begin(&writer, &record, Instant::now() + Duration::from_secs(5))
                    .unwrap();
        }
        _ => panic!("unknown private input-kind fixture"),
    }
    // The physical root is private and carries one exact original input kind.
    drop(
        Scope::open(
            &writer,
            &record,
            Receipts::Effort,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap(),
    );
    fs::write(source, serde_json::to_vec(&record).unwrap()).unwrap();
}
