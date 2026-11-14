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
    assert!(page.entries[0].categories.is_empty());
    let mut categorized = entry(5, "approved");
    categorized["categories"] = json!(["paid", "future", "shell", 3]);
    let page = super::page(json!({"entries": [categorized]}).as_object().unwrap()).unwrap();
    assert_eq!(page.entries[0].categories, ["shell", "paid"]);
    let many: Vec<Value> = (0..51).map(|n| entry(n, "approved")).collect();
    assert_eq!(
        super::page(json!({"entries": many}).as_object().unwrap()).unwrap_err(),
        "Invalid approval journal page."
    );
}

/// zerus-63rru.66 round 5: the standing folder-trust kind survives the
/// journal page projection, so the row sentences distinguish the five
/// trust outcomes; a resume row shows none.
#[test]
fn the_page_carries_the_standing_trust_kind_for_each_outcome() {
    let cases = [
        ("approved", "Approved once", true),
        ("failed", "Failed", false),
        ("unknown", "Unconfirmed", false),
        ("cancelled", "Cancelled", false),
        ("unconfirmed", "Unconfirmed", false),
    ];
    let rows: Vec<Value> = cases
        .iter()
        .map(|(status, _, _)| {
            json!({"id": 1, "at": 1, "host": "h", "session": "s", "prompt": "Trust this folder?",
                "status": status, "categories": ["team_folder_trust"], "scope": "/fixture/main"})
        })
        .chain([
            json!({"id": 2, "at": 2, "host": "h", "session": "s", "prompt": "Resume?",
            "status": "approved", "categories": ["team_resume"], "scope": "/fixture/main"}),
        ])
        .collect();
    let reply = json!({"entries": rows, "next": null, "previous": null});
    let page = page(reply.as_object().unwrap()).unwrap();
    assert_eq!(page.entries.len(), 6);
    for (entry, (_, result, confirmed)) in page.entries.iter().zip(cases.iter()) {
        assert_eq!(entry.result, *result, "{entry:?}");
        assert_eq!(
            entry.standing.as_deref(),
            Some("team_folder_trust"),
            "{entry:?}"
        );
        assert_eq!(
            trust_sentence(
                entry.scope.as_deref(),
                &json!(["team_folder_trust"]),
                Some(if *confirmed { "approved" } else { "unknown" })
            ),
            Some(if *confirmed {
                "Trusted for /fixture/main and its subfolders on this account".to_owned()
            } else {
                "Trust was requested for /fixture/main and its subfolders; it was not confirmed"
                    .to_owned()
            }),
            "{entry:?}"
        );
    }
    let resume = page.entries.last().unwrap();
    assert_eq!(
        resume.standing, None,
        "a resume row carries no standing kind"
    );
    assert_eq!(
        trust_sentence(
            resume.scope.as_deref(),
            &json!(["team_resume"]),
            Some("approved")
        ),
        None
    );
}

/// zerus-63rru.66 round 6: the real service list reply through the real
/// page, committed as the fixture the Approvals render test consumes; the
/// guard fails when the core's page output drifts from the committed file
/// (regenerate with HGS_R6_WRITE_FIXTURE set).
#[test]
fn the_real_service_reply_projects_into_the_committed_page_fixture() {
    let reply = json!({"entries": [
        {"id": 1, "at": 1, "host": "a.example", "session": "agent/project/fixture", "provider": "codex",
            "prompt": "Trust this folder?", "status": "approved", "categories": ["team_folder_trust"], "scope": "/fixture/main"},
        {"id": 2, "at": 2, "host": "a.example", "session": "agent/project/fixture", "provider": "codex",
            "prompt": "Trust this folder?", "status": "failed", "categories": ["team_folder_trust"], "scope": "/fixture/main"},
        {"id": 3, "at": 3, "host": "a.example", "session": "agent/project/fixture", "provider": "codex",
            "prompt": "Trust this folder?", "status": "unknown", "categories": ["team_folder_trust"], "scope": "/fixture/main"},
        {"id": 4, "at": 4, "host": "a.example", "session": "agent/project/fixture", "provider": "codex",
            "prompt": "Trust this folder?", "status": "cancelled", "categories": ["team_folder_trust"], "scope": "/fixture/main"},
        {"id": 5, "at": 5, "host": "a.example", "session": "agent/project/fixture", "provider": "codex",
            "prompt": "Trust this folder?", "status": "unconfirmed", "categories": ["team_folder_trust"], "scope": "/fixture/main"},
        {"id": 6, "at": 6, "host": "a.example", "session": "agent/project/fixture", "provider": "claude",
            "prompt": "Resume the member?", "status": "approved", "categories": ["team_resume"], "scope": "/fixture/main"},
        {"id": 7, "at": 7, "host": "a.example", "session": "agent/project/fixture", "provider": "claude",
            "prompt": "Resume the member?", "status": "approved", "categories": [], "scope": "/fixture/main"},
        {"id": 8, "at": 8, "host": "a.example", "session": "agent/project/fixture", "provider": "claude",
            "prompt": "May I run this command?", "status": "approved", "categories": ["shell"], "scope": null},
    ], "next": null, "previous": null});
    let page = page(reply.as_object().unwrap()).unwrap();
    let mut projected = serde_json::to_value(&page).unwrap();
    // Each row's exact detail sentence, from the same core formatter the
    // detail reader feeds, on the detail reply's full shape with the raw
    // categories (zerus-63rru.66 round 7).
    for (i, entry) in reply["entries"].as_array().unwrap().iter().enumerate() {
        let mut detail_reply = entry.clone();
        detail_reply["categories"] = entry["categories"].clone();
        detail_reply["question"] = json!({"questions": [{"id": "trust", "question": "Trust this folder?", "body": "/fixture/main"}]});
        detail_reply["answers"] = json!([]);
        let text = detail(detail_reply.as_object().unwrap());
        projected["entries"][i]["detail"] = json!(text);
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/ui/screens/settings/approvals-page.r6.fixture.json"
    );
    if std::env::var_os("HGS_R6_WRITE_FIXTURE").is_some() {
        std::fs::write(
            path,
            serde_json::to_string_pretty(&projected).unwrap() + "\n",
        )
        .unwrap();
    }
    let committed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        projected, committed,
        "the core page output drifted; regenerate with HGS_R6_WRITE_FIXTURE"
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
    // zerus-63rru.66 round 4: the sentence distinguishes the record's kind
    // and outcome — a confirmed folder-trust grant claims the trust, an
    // unconfirmed, unknown or refused one only says trust was requested,
    // and a non-trust record shows nothing even with an old row's scope.
    for status in [Some("failed"), Some("unknown"), Some("cancelled"), None] {
        assert_eq!(
            trust_sentence(Some("/fixture/main"), &json!(["team_folder_trust"]), status),
            Some(
                "Trust was requested for /fixture/main and its subfolders; it was not confirmed"
                    .to_owned()
            ),
            "{status:?}"
        );
    }
    assert_eq!(
        trust_sentence(
            Some("/fixture/main"),
            &json!(["team_folder_trust"]),
            Some("approved")
        ),
        Some("Trusted for /fixture/main and its subfolders on this account".to_owned())
    );
    assert_eq!(
        trust_sentence(
            Some("/fixture/main"),
            &json!(["team_resume"]),
            Some("approved")
        ),
        None,
        "a resume record shows no trust sentence"
    );
    assert_eq!(
        trust_sentence(Some("/fixture/main"), &Value::Null, Some("approved")),
        None,
        "a record without the trust category shows no trust sentence"
    );
    // The granted scope's sentence (zerus-63rru.66 round 3): present when the
    // reply carries it, absent for a row without one.
    let scoped = json!({"status": "approved", "detail": "", "scope": "/fixture/main",
        "categories": ["team_folder_trust"]});
    assert_eq!(
        detail(scoped.as_object().unwrap()),
        "Approved once\n\nTrusted for /fixture/main and its subfolders on this account"
    );
    assert_eq!(
        detail(
            json!({"status": "approved", "detail": ""})
                .as_object()
                .unwrap()
        ),
        "Approved once"
    );
    let categorized = json!({"status": "approved", "detail": "", "categories": ["shell", "paid"]});
    assert_eq!(
        detail(categorized.as_object().unwrap()),
        "Approved once\n\nCategories: Shell commands, Paid usage"
    );
}

/// A fake `hgs` logging its arguments and answering per case.
#[cfg(unix)]
fn fake(
    dir: &std::path::Path,
    help: &[u8],
    reply: &[u8],
    failure: Option<&[u8]>,
) -> std::path::PathBuf {
    std::fs::write(dir.join("help"), help).unwrap();
    std::fs::write(dir.join("reply"), reply).unwrap();
    if let Some(error) = failure {
        std::fs::write(dir.join("fail"), error).unwrap();
    } else {
        let _ = std::fs::remove_file(dir.join("fail"));
    }
    let script = dir.join("hgs");
    let _ = std::fs::remove_file(&script);
    std::os::unix::fs::symlink(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/support/fake_hgs_approvals.sh"),
        &script,
    )
    .unwrap();
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
    let help = b"hgs approval-log begin|finish|list|detail (JSON stdin)\n";
    let entries = b"{\"entries\":[],\"next\":null,\"previous\":null}\n";
    let hgs = fake(&dir, help, entries, None);
    let mut supported = false;
    let request = list_request("", "", 0, 0);
    assert!(call(&hgs, Operation::List, &request, &mut supported).is_ok());
    assert!(call(&hgs, Operation::Detail, &json!({"id": 9}), &mut supported).is_ok());
    // The probe runs once, then only list and detail: never begin or finish.
    assert_eq!(calls(), "--help\napproval-log list\napproval-log detail\n");
    // A reply past the limit: the CLI's own error when it wrote one.
    let oversized = vec![b'x'; 300000];
    let hgs = fake(&dir, help, &oversized, Some(b"journal busy\n"));
    let failed = call(&hgs, Operation::List, &request, &mut supported).unwrap_err();
    assert!(
        failed == "journal busy" || failed == "Invalid automatic approval journal response.",
        "{failed}"
    );
    let hgs = fake(&dir, help, &oversized, None);
    assert_eq!(
        call(&hgs, Operation::List, &request, &mut supported).unwrap_err(),
        "Invalid automatic approval journal response."
    );
    // A help past its limit is its own error, not a failed exit.
    let hgs = fake(&dir, &oversized, entries, None);
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
