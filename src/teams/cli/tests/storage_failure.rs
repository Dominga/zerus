use super::harness::*;
use crate::teams::cli::actor::Caller;
use serde_json::json;

pub(super) fn failed_write() -> serde_json::Value {
    let h = Harness::new();
    h.create();
    h.store().db.as_ref().unwrap().execute_batch("CREATE TRIGGER fail_write BEFORE INSERT ON journal BEGIN SELECT RAISE(ABORT,'fixture CLI write failure'); END;").unwrap();
    let input =
        json!({"request_id":uid(0xf001),"team":TEAM,"expected_revision":h.revision()}).to_string();
    let (reply, exit) = h.cli(&Caller::Person, "pause", &input);
    assert_eq!(exit, 1, "{reply}");
    reply
}

#[test]
fn a_command_reports_unknown_storage_outcome_and_can_repeat_the_original_request() {
    let h = Harness::new();
    h.create();
    h.store().db.as_ref().unwrap().execute_batch("CREATE TRIGGER fail_write BEFORE INSERT ON journal BEGIN SELECT RAISE(ABORT,'fixture CLI write failure'); END;").unwrap();
    let id = uid(0xf002);
    let input = json!({"request_id":id,"team":TEAM,"expected_revision":h.revision()}).to_string();
    let (reply, exit) = h.cli(&Caller::Person, "pause", &input);
    assert_eq!(exit, 1);
    assert_eq!(code(&reply), "storage_failed");
    assert_eq!(
        reply["error"]["message"],
        "The result is not known; repeat the same request with the same request ID."
    );
    let store = h.store();
    let db = store.db.as_ref().unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM requests WHERE id=?1", [&id], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0
    );
    db.execute_batch("DROP TRIGGER fail_write").unwrap();
    let (first, exit) = h.cli(&Caller::Person, "pause", &input);
    assert_eq!(exit, 0, "{first}");
    let (replay, exit) = h.cli(&Caller::Person, "pause", &input);
    assert_eq!(exit, 0, "{replay}");
    assert_eq!(first, replay);
    assert_eq!(h.store().journal(TEAM, 200, None).unwrap().rows.len(), 2);
}

#[test]
fn storage_reads_and_cli_conversion_errors_do_not_claim_a_logical_refusal() {
    let h = Harness::new();
    h.create();
    h.store()
        .db
        .as_ref()
        .unwrap()
        .execute_batch("DROP TABLE requests")
        .unwrap();
    let input =
        json!({"request_id":uid(0xf003),"team":TEAM,"expected_revision":h.revision()}).to_string();
    let (reply, exit) = h.cli(&Caller::Person, "pause", &input);
    assert_eq!((code(&reply), exit), ("storage_failed", 1));
    let (reply, exit) =
        crate::teams::cli::refusal(crate::teams::cli::storage("fixture conversion failure"));
    assert_eq!((code(&reply), exit), ("storage_failed", 1));
    assert_eq!(
        reply["error"]["message"],
        "The result is not known; repeat the same request with the same request ID."
    );
}

#[test]
#[cfg(unix)]
fn u1_privacy_refusals_keep_their_cause_for_reads_and_writes() {
    use std::os::unix::fs::PermissionsExt;
    for (name, mode) in [("teams", 0o755), ("teams/teams-v1.sqlite3", 0o644)] {
        let h = Harness::new();
        h.create();
        let path = h.state().join(name);
        let original = std::fs::metadata(&path).unwrap().permissions();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        for command in [
            "list".to_owned(),
            format!("status --team {TEAM}"),
            "pause".to_owned(),
        ] {
            let input =
                json!({"request_id":uid(0xf020),"team":TEAM,"expected_revision":1}).to_string();
            let (reply, exit) = h.cli(&Caller::Person, &command, &input);
            assert_eq!(exit, 1, "{reply}");
            assert_eq!(code(&reply), "invalid_state", "{reply}");
            assert!(
                reply["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("not private"),
                "{reply}"
            );
            assert!(!reply["error"]["message"]
                .as_str()
                .unwrap()
                .contains("repeat"));
        }
        std::fs::set_permissions(path, original).unwrap();
    }
}

#[test]
fn a_storage_read_reports_its_cause_without_a_request_id_instruction() {
    let h = Harness::new();
    h.create();
    h.store()
        .db
        .as_ref()
        .unwrap()
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE members")
        .unwrap();
    let (reply, exit) = h.cli(&Caller::Person, "list", "");
    assert_eq!(exit, 1);
    assert_eq!(code(&reply), "storage_failed");
    assert_eq!(
        reply["error"]["message"],
        "The read failed and may be repeated."
    );
    assert!(
        reply["error"]["detail"]
            .as_str()
            .unwrap()
            .contains("no such table: ")
            && reply["error"]["detail"]
                .as_str()
                .unwrap()
                .contains("members"),
        "{reply}"
    );
}

#[test]
fn storage_diagnostic_detail_is_kept_and_bounded_on_a_character_boundary() {
    let original = "cause: ".to_owned() + &"⚙".repeat(5000);
    let (reply, exit) = crate::teams::cli::refusal(crate::teams::cli::storage(&original));
    assert_eq!(exit, 1);
    let detail = reply["error"]["detail"].as_str().expect("underlying cause");
    assert!(detail.starts_with("Teams storage is unavailable: cause: "));
    assert!(detail.len() <= 4096);
    assert!(detail.len() >= 4093);
    assert!(original.contains("cause: "));
}

#[test]
fn a_writer_that_cannot_begin_is_certainly_busy_and_the_same_request_can_repeat() {
    let h = Harness::new();
    h.create();
    let id = uid(0xf030);
    let input = json!({"request_id":id,"team":TEAM,"expected_revision":h.revision()}).to_string();
    let writer = rusqlite::Connection::open(h.state().join("teams/teams-v1.sqlite3")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (reply, exit) = h.cli(&Caller::Person, "pause", &input);
    assert_eq!(exit, 1);
    assert_eq!(code(&reply), "store_busy", "{reply}");
    assert_eq!(
        reply["error"]["message"],
        "The team store is busy. Nothing was written; try again."
    );
    assert!(reply["error"].get("detail").is_none());
    let store = h.store();
    assert_eq!(
        store
            .db
            .as_ref()
            .unwrap()
            .query_row("SELECT count(*) FROM requests WHERE id=?1", [&id], |r| r
                .get::<_, u32>(0))
            .unwrap(),
        0
    );
    writer.execute_batch("ROLLBACK").unwrap();
    let (first, exit) = h.cli(&Caller::Person, "pause", &input);
    assert_eq!(exit, 0, "{first}");
    let (replay, exit) = h.cli(&Caller::Person, "pause", &input);
    assert_eq!(exit, 0, "{replay}");
    assert_eq!(first, replay);
    assert_eq!(h.store().journal(TEAM, 200, None).unwrap().rows.len(), 2);
}

/// zerus-63rru.35 (6a): the 4096-byte detail cut can fall inside a
/// multi-byte character. The bounded detail keeps whole characters, never a
/// cut byte sequence.
#[test]
fn a_storage_detail_cut_inside_a_multibyte_character_keeps_the_text_whole() {
    // The storage prefix is 30 bytes; 4059 ASCII bytes put the 4096-byte
    // cut seven bytes into the gear run, inside its third character.
    let original = "a".repeat(4059) + &"⚙".repeat(100);
    let (reply, exit) = crate::teams::cli::refusal(crate::teams::cli::storage(&original));
    assert_eq!(exit, 1);
    let detail = reply["error"]["detail"].as_str().expect("underlying cause");
    assert_eq!(detail.len(), 4095, "the cut drops the partial character");
    assert!(detail.ends_with('⚙'));
    assert!(std::str::from_utf8(detail.as_bytes()).is_ok());
}
