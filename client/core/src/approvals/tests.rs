// The journal reads of tray/src/AutomaticApprovalSettings.cpp and
// HgsClient::requestApprovalLog, against src/approval_log.rs's replies.
use super::*;

fn code(code: i32, stdout: &str, stderr: &str) -> Option<Exit> {
    Some(Exit::Code {
        code,
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.into(),
    })
}

#[test]
fn the_journal_is_read_only_after_its_command_is_found() {
    let hgs = Path::new("/srv/bin/hgs");
    assert_eq!(probe().args, ["--help"]);
    assert!(probed(
        hgs,
        code(
            0,
            "  hgs approval-log begin|finish|list|detail (JSON stdin)\n",
            ""
        )
    )
    .is_ok());
    assert_eq!(
        probed(hgs, code(0, "hgs [@host] ls", "")).unwrap_err(),
        "Update the local hgs CLI to use automatic approvals and its journal."
    );
    assert_eq!(probed(hgs, Some(Exit::TimedOut)).unwrap_err(), "timed out");
    assert_eq!(probed(hgs, code(3, "", "")).unwrap_err(), "exit code 3");
    assert_eq!(
        probed(hgs, code(1, "", " broken \n")).unwrap_err(),
        "broken"
    );
    assert_eq!(
        probed(hgs, Some(Exit::StartFailed("No such file".into()))).unwrap_err(),
        "cannot start '/srv/bin/hgs': No such file"
    );
    let list = invocation("list", &list_request(" desktop-a ", "", 0, 0));
    assert_eq!(list.args, ["approval-log", "list"]);
    assert_eq!(list.timeout, Duration::from_secs(10));
    let sent: Value = serde_json::from_slice(&list.stdin.unwrap()).unwrap();
    assert_eq!(sent, json!({"host": "desktop-a", "session": ""}));
    assert_eq!(
        list_request("", "s", 120, 0),
        json!({"host": "", "session": "s", "before": 120})
    );
    assert_eq!(
        list_request("", "", 0, 70),
        json!({"host": "", "session": "", "after": 70})
    );
}

#[test]
fn replies_and_failures_read_as_qt_words_them() {
    assert!(answer(code(0, r#"{"entries":[],"next":null,"previous":null}"#, "")).is_ok());
    let invalid = "Invalid automatic approval journal response.";
    assert_eq!(answer(code(0, "{}", "")).unwrap_err(), invalid);
    assert_eq!(answer(code(0, "[]", "")).unwrap_err(), invalid);
    assert_eq!(answer(None).unwrap_err(), invalid);
    assert_eq!(
        answer(code(1, "", "  Approval journal entry is unavailable\n")).unwrap_err(),
        "Approval journal entry is unavailable"
    );
    let unavailable =
        "The automatic approval journal is unavailable. No new approval was authorized.";
    assert_eq!(answer(code(2, "", "")).unwrap_err(), unavailable);
    assert_eq!(answer(Some(Exit::TimedOut)).unwrap_err(), unavailable);
    assert_eq!(
        answer(code(1, "", &"e".repeat(3000)))
            .unwrap_err()
            .chars()
            .count(),
        2000
    );
}

#[test]
fn a_page_holds_at_most_50_entries_and_names_each_outcome() {
    let entry = |id: i64, status: &str| {
        json!({"id": id, "at": 1_800_000_000_000_i64, "host": "desktop-a", "session": "claude/sample/review",
            "provider": "claude", "prompt": "May I run this command?", "status": status})
    };
    let reply = json!({"entries": [entry(9, "approved"), entry(8, "failed"), entry(7, "cancelled"), entry(6, "pending")],
        "next": 6, "previous": null});
    let page = page(reply.as_object().unwrap()).unwrap();
    let results: Vec<&str> = page.entries.iter().map(|e| e.result.as_str()).collect();
    assert_eq!(
        results,
        ["Approved once", "Failed", "Cancelled", "Unconfirmed"]
    );
    assert_eq!((page.next, page.previous), (6, 0));
    assert_eq!(page.entries[0].at, 1_800_000_000_000);
    let many: Vec<Value> = (0..51).map(|n| entry(n, "approved")).collect();
    assert_eq!(
        super::page(json!({"entries": many}).as_object().unwrap()).unwrap_err(),
        "Invalid approval journal page."
    );
}

#[test]
fn a_detail_shows_the_question_and_the_answer_chosen() {
    let reply = json!({"id": 9, "status": "approved", "detail": "Granted once.",
        "question": {"questions": [{"id": "approval", "question": "May I run this command?", "body": "printf done",
            "options": [{"id": "1", "label": "Yes"}, {"id": "2", "label": "Always allow"}, {"id": "3", "label": "No"}]}]},
        "answers": [{"question_id": "approval", "selected_option_ids": ["1"], "text": ""}]});
    assert_eq!(
        detail(reply.as_object().unwrap()),
        "Approved once\n\nMay I run this command?\n\nprintf done\n\nSelected answer: Yes\n\nGranted once."
    );
    let bare = json!({"status": "failed", "detail": ""});
    assert_eq!(detail(bare.as_object().unwrap()), "Failed");
}
