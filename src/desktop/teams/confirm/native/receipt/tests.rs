use super::*;
use crate::desktop::teams::confirm::{engine, policy::tests::sample};
fn row(kind: Kind) -> Record {
    let (b, _, s) = sample();
    let question = s.questions[0].clone();
    Record {
        plan: engine::candidate(&b, &s, kind, question),
        original: uuid::Uuid::new_v4().to_string(),
        epoch: "epoch-1".into(),
        state: "prepared".into(),
        audit: Some(Audit {
            id: 1,
            request: uuid::Uuid::new_v4().to_string(),
            answers: json!([{"question_id":"trust","selected_option_ids":["trust_1"],"text":""}]),
            fresh: true,
        }),
        audit_done: false,
        result: None,
        detail: String::new(),
        card_request: uuid::Uuid::new_v4().to_string(),
        card_done: false,
        at: 1234,
    }
}
fn answer_receipt(row: &Record, input: &[u8]) -> Value {
    let p = &row.plan;
    json!({"request_id":row.audit.as_ref().unwrap().request,"name":p.snapshot.name,"run_id":p.snapshot.run,"conversation_id":p.snapshot.conversation,"question_id":p.question["question_id"],"question_hash":p.question["question_hash"],"digest":digest(input),"status":"answered","transport":"claude_tui"})
}
#[test]
fn trust_receipt_must_match_every_native_identity_and_exact_input() {
    let row = row(Kind::Trust);
    let (args, input, folder) = invocation(&row).unwrap();
    assert_eq!(args[0], "answer");
    assert_eq!(folder, "question_receipts");
    let receipt = answer_receipt(&row, &input);
    assert!(matches!(
        project(&row, &input, receipt.to_string().as_bytes()),
        Outcome::Approved(None)
    ));
    for field in [
        "request_id",
        "name",
        "run_id",
        "conversation_id",
        "question_id",
        "question_hash",
        "digest",
        "status",
    ] {
        let mut changed = receipt.clone();
        changed[field] = json!("other");
        assert!(
            matches!(
                project(&row, &input, changed.to_string().as_bytes()),
                Outcome::Unknown(_)
            ),
            "{field}"
        );
    }
    let mut changed = receipt;
    changed["status"] = json!("in_progress");
    assert!(matches!(
        project(&row, &input, changed.to_string().as_bytes()),
        Outcome::Unknown(_)
    ));
}
#[test]
fn restore_uses_the_exact_clean_exit_archive_and_only_completed_receipts_publish() {
    let mut row = row(Kind::Resume);
    row.plan.snapshot.conversation = "conversation-1".into();
    row.plan.snapshot.archive = Some(uuid::Uuid::new_v4().to_string());
    let (args, input, folder) = invocation(&row).unwrap();
    let request: Value = serde_json::from_slice(&input).unwrap();
    assert_eq!(args[0], "session-action");
    assert_eq!(folder, "action_receipts");
    assert_eq!(request["action"], "restore");
    assert_eq!(request["archive_id"], json!(row.plan.snapshot.archive));
    let receipt = crate::state::lifecycle_fixture_receipt(
        &row.plan.snapshot.name,
        &input,
        "completed",
        Some("new-run"),
    );
    let Outcome::Approved(Some(target)) = project(&row, &input, receipt.to_string().as_bytes())
    else {
        panic!("Exact completed receipt refused");
    };
    assert_eq!(target.run, "new-run");
    assert_eq!(target.conversation, "conversation-1");
    assert!(target.receipt_at.is_some());
    for field in [
        "request_id",
        "name",
        "run_id",
        "conversation_id",
        "request_hash",
        "archive_id",
        "status",
    ] {
        let mut v = receipt.clone();
        v[field] = json!("other");
        assert!(
            matches!(
                project(&row, &input, v.to_string().as_bytes()),
                Outcome::Unknown(_)
            ),
            "{field}"
        );
    }
    let uncertain =
        crate::state::lifecycle_fixture_receipt(&row.plan.snapshot.name, &input, "uncertain", None);
    assert!(matches!(
        project(&row, &input, uncertain.to_string().as_bytes()),
        Outcome::Unknown(_)
    ));
}
#[test]
fn crash_resume_and_brief_are_separate_requests_bound_to_the_new_run() {
    let mut row = row(Kind::Resume);
    row.plan.snapshot.conversation = "conversation-1".into();
    let (_, input, _) = invocation(&row).unwrap();
    let request: Value = serde_json::from_slice(&input).unwrap();
    assert_eq!(request["action"], "resume");
    assert!(request.get("archive_id").is_none());
    let receipt = crate::state::lifecycle_fixture_receipt(
        &row.plan.snapshot.name,
        &input,
        "completed",
        Some("new-run"),
    );
    let Outcome::Approved(Some(target)) = project(&row, &input, receipt.to_string().as_bytes())
    else {
        panic!("Expected new run");
    };
    let plan = engine::brief(&row, &target);
    row.plan = plan;
    row.audit.as_mut().unwrap().request = uuid::Uuid::new_v4().to_string();
    let (args, input, folder) = invocation(&row).unwrap();
    let brief: Value = serde_json::from_slice(&input).unwrap();
    assert_eq!(args[0], "send");
    assert_eq!(folder, "input_receipts");
    assert_eq!(brief["expected_run_id"], "new-run");
    assert_eq!(brief["expected_conversation_id"], "conversation-1");
    assert_ne!(brief["request_id"], request["request_id"]);
    let receipt = json!({"request_id":brief["request_id"],"name":target.name,"run_id":"new-run","conversation_id":target.conversation,"digest":digest(&input),"status":"submitted","transport":"terminal"});
    assert!(matches!(
        project(&row, &input, receipt.to_string().as_bytes()),
        Outcome::Approved(None)
    ));
    let mut wrong = receipt;
    wrong["run_id"] = request["expected_run_id"].clone();
    assert!(matches!(
        project(&row, &input, wrong.to_string().as_bytes()),
        Outcome::Unknown(_)
    ));
}
#[test]
fn a_missing_or_replaced_original_store_never_finishes_as_approved() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("state");
    crate::platform::private_dir(&state).unwrap();
    let roots = Roots::open(&state, "question_receipts").unwrap();
    let row = row(Kind::Trust);
    let (_, input, _) = invocation(&row).unwrap();
    assert!(matches!(observe(&roots, &row, &input), Outcome::Unknown(_)));
    std::fs::rename(&state, temp.path().join("original")).unwrap();
    crate::platform::private_dir(&state).unwrap();
    assert!(matches!(observe(&roots, &row, &input), Outcome::Unknown(_)));
}

/// zerus-63rru.60: the delivery request takes the conversation from the
/// snapshot, never from the card, and the answer command's own request
/// reader accepts it for a startup card. The card's null would be refused.
#[test]
fn a_startup_cards_request_names_the_snapshots_conversation() {
    let (mut b, _, mut s) = sample();
    let card = crate::state::trust_fixture::startup_card("claude");
    assert!(card["conversation_id"].is_null());
    b.run = card["run_id"].as_str().unwrap().into();
    s.run = b.run.clone();
    s.conversation = String::new();
    s.questions = vec![card.clone()];
    let mut row = row(Kind::Trust);
    row.plan = engine::candidate(&b, &s, Kind::Trust, card.clone());
    let (args, input, folder) = invocation(&row).unwrap();
    assert_eq!((args[0].as_str(), folder), ("answer", "question_receipts"));
    let (run, conversation, question) = crate::state::read_answer_request(&input).unwrap();
    assert_eq!(run, s.run);
    assert_eq!(conversation, "");
    assert_eq!(question, card["question_id"].as_str().unwrap());
    let mut request: Value = serde_json::from_slice(&input).unwrap();
    request["expected_conversation_id"] = card["conversation_id"].clone();
    assert!(crate::state::read_answer_request(request.to_string().as_bytes()).is_err());
}

/// Runs one ignored fixture of this module in its own process, with a private
/// state and configuration under `temp` and `stdin` as its input.
fn isolated(temp: &std::path::Path, fixture: &str, stdin: &[u8]) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("desktop::teams::confirm::native::receipt::tests::{fixture}"),
        ])
        .args(["--ignored", "--test-threads=1"])
        .env("HGS_STATE_DIR", temp.join("state"))
        .env("HGS_CONFIG_DIR", temp.join("config"))
        .env_remove("HGS_SESSION")
        .env_remove("HGS_SESSION_ID")
        .env_remove("HGS_RUN_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let output = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && text.contains("1 passed"),
        "{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// zerus-63rru.62: the service's restore of an exited member is refused
/// before launch, and the card says why. The member is a Claude session that
/// exited before its first message: its archive is real (the state's own
/// writer) and names a transcript Claude Code never wrote. The confirmation's
/// own restore request runs through the real `hgs session-action`, whose
/// writer records the receipt; the confirmation reads that receipt.
#[test]
fn a_restore_refused_before_launch_carries_the_commands_reason() {
    let temp = tempfile::tempdir().unwrap();
    isolated(temp.path(), "restore_refusal_prepare", b"");
    let input = std::fs::read(temp.path().join("input")).unwrap();
    isolated(temp.path(), "restore_refusal_command", &input);
    let row: Record =
        serde_json::from_slice(&std::fs::read(temp.path().join("row.json")).unwrap()).unwrap();
    let request = &row.audit.as_ref().unwrap().request;
    let bytes = std::fs::read(
        temp.path()
            .join(format!("state/action_receipts/{request}.json")),
    )
    .unwrap();
    let receipt: Value = serde_json::from_slice(&bytes).unwrap();
    let name = &row.plan.snapshot.name;
    assert_eq!(receipt["status"], "failed", "{receipt}");
    let error = format!("{name}: persisted conversation is missing or empty");
    assert_eq!(receipt["error"], json!(error), "{receipt}");
    assert!(receipt.get("result_target").is_none(), "{receipt}");
    let shown = match project(&row, &input, &bytes) {
        Outcome::Refused(text) => text,
        Outcome::Approved(_) => "approved".into(),
        Outcome::Unknown(why) => format!("unknown: {why}"),
        Outcome::NotStarted(why) => format!("not started: {why}"),
        Outcome::Retry => "retry".into(),
    };
    eprintln!("the receipt: {receipt}; the card's reason: {shown}");
    assert!(shown.contains(&error), "{shown}");
    assert!(
        shown.ends_with(crate::desktop::teams::confirm::policy::SEAT),
        "{shown}"
    );
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn restore_refusal_prepare() {
    let state = std::env::var("HGS_STATE_DIR").unwrap();
    let temp = std::path::Path::new(&state).parent().unwrap().to_owned();
    let conversation = uuid::Uuid::new_v4().to_string();
    let mut record = json!({"name":"claude/example/member","agent":"claude","version":1,
        "run_id":uuid::Uuid::new_v4().to_string(),"conversation_id":conversation,
        "launch_id":uuid::Uuid::new_v4().to_string(),"account_id":"account-1",
        "cwd":state,"launch_dir":state,"base":["claude"],"worktree_owner":{"id":"owner-1"},
        "created":1.0,"transcript":format!("{state}/claude-home/projects/-member/{conversation}.jsonl"),
        "completion_source":"supervisor","completion_reason":"clean_exit","exit_code":0,
        "exited_at":1000.0});
    let _ = crate::state::inspection_for_test(&mut record);
    let id = crate::state::archive_for_test(&record).unwrap();
    let mut row = row(Kind::Resume);
    row.plan.snapshot.name = "claude/example/member".into();
    row.plan.snapshot.run = record["run_id"].as_str().unwrap().into();
    row.plan.snapshot.conversation = conversation;
    row.plan.snapshot.archive = Some(id);
    let (args, input, folder) = invocation(&row).unwrap();
    assert_eq!(folder, "action_receipts");
    std::fs::write(temp.join("args.json"), serde_json::to_vec(&args).unwrap()).unwrap();
    std::fs::write(temp.join("input"), input).unwrap();
    std::fs::write(temp.join("row.json"), serde_json::to_vec(&row).unwrap()).unwrap();
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn restore_refusal_command() {
    let state = std::env::var("HGS_STATE_DIR").unwrap();
    let temp = std::path::Path::new(&state).parent().unwrap().to_owned();
    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(temp.join("args.json")).unwrap()).unwrap();
    // The real command: its writer records the receipt, refused or not.
    let result = crate::cli::dispatch_at(args, std::time::Instant::now());
    assert!(result.is_ok(), "{:?}", result.err().map(|e| e.message));
}

/// The card's reason is the command's own, cleaned and bounded as the
/// doorbell's refusal is: control characters as spaces, whitespace collapsed,
/// at most 512 characters; an empty error says so.
#[test]
fn a_refused_restore_reason_is_cleaned_and_bounded() {
    let mut row = row(Kind::Resume);
    row.plan.snapshot.archive = Some(uuid::Uuid::new_v4().to_string());
    let long = format!("first\tline\nsecond\u{1b}[0m {}", "x".repeat(600));
    let text = refused_before_launch(&row.plan, &long);
    let reason = text
        .strip_prefix("The restore of its exited session was refused before launch: ")
        .and_then(|rest| rest.strip_suffix(&format!(". {}", super::super::super::policy::SEAT)))
        .unwrap();
    assert!(reason.starts_with("first line second [0m xxx"), "{reason}");
    assert_eq!(reason.chars().count(), 512);
    let empty = refused_before_launch(&row.plan, " \n ");
    assert!(empty.contains("the command gave no reason"), "{empty}");
}
