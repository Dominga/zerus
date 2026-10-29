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
    let list = invocation(Operation::List, &list_request(" desktop-a ", "", 0, 0));
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

/// A fake `hgs` logging its arguments and answering per case.
#[cfg(unix)]
fn fake(dir: &std::path::Path, help: &str, list: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join("hgs");
    let staged = dir.join("hgs.new");
    std::fs::write(
        &staged,
        format!(
            "#!/bin/sh\nhere=$(dirname \"$0\")\necho \"$*\" >> \"$here/calls\"\ncat >/dev/null\ncase \"$*\" in\n  --help) {help};;\n  'approval-log list') {list};;\n  'approval-log detail') echo '{{\"status\":\"approved\",\"detail\":\"\"}}';;\n  *) exit 9;;\nesac\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&staged, &script).unwrap();
    script
}

#[cfg(unix)]
#[test]
fn only_the_journal_s_reads_run_and_oversized_output_fails_as_intended() {
    let dir = std::env::temp_dir().join(format!("zerus-approvals-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    struct Gone(std::path::PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _gone = Gone(dir.clone());
    let calls = || std::fs::read_to_string(dir.join("calls")).unwrap_or_default();
    let help = "echo 'hgs approval-log begin|finish|list|detail (JSON stdin)'";
    let hgs = fake(
        &dir,
        help,
        "echo '{\"entries\":[],\"next\":null,\"previous\":null}'",
    );
    let mut supported = false;
    let request = list_request("", "", 0, 0);
    assert!(call(&hgs, Operation::List, &request, &mut supported).is_ok());
    assert!(call(&hgs, Operation::Detail, &json!({"id": 9}), &mut supported).is_ok());
    // The probe runs once, then only list and detail: never begin or finish.
    assert_eq!(calls(), "--help\napproval-log list\napproval-log detail\n");
    // A reply past the limit: the CLI's own error when it wrote one.
    let hgs = fake(
        &dir,
        help,
        "echo 'journal busy' >&2; head -c 300000 /dev/zero | tr '\\0' x; exit 3",
    );
    let failed = call(&hgs, Operation::List, &request, &mut supported).unwrap_err();
    assert!(
        failed == "journal busy" || failed == "Invalid automatic approval journal response.",
        "{failed}"
    );
    let hgs = fake(&dir, help, "head -c 300000 /dev/zero | tr '\\0' x");
    assert_eq!(
        call(&hgs, Operation::List, &request, &mut supported).unwrap_err(),
        "Invalid automatic approval journal response."
    );
    // A help past its limit is its own error, not a failed exit.
    let hgs = fake(&dir, "head -c 300000 /dev/zero | tr '\\0' x", "true");
    let mut fresh = false;
    assert_eq!(
        call(&hgs, Operation::List, &request, &mut fresh).unwrap_err(),
        "help exceeds 262144 bytes"
    );
    assert!(!fresh);
}

#[test]
fn a_reply_cut_off_at_the_limit_says_the_cli_s_error_when_it_wrote_one() {
    assert_eq!(past_limit("  journal busy\n").unwrap_err(), "journal busy");
    assert_eq!(
        past_limit("").unwrap_err(),
        "Invalid automatic approval journal response."
    );
    assert_eq!(
        past_limit(&"e".repeat(3000)).unwrap_err().chars().count(),
        2000
    );
}
