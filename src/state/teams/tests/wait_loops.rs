use super::*;

#[test]
fn a_lead_wait_instruction_is_recognised_and_ordinary_direction_is_not() {
    assert!(lead_wait_instruction("Keep 138 and wait for the 1.58.0 install"));
    assert!(lead_wait_instruction("Poll the CI run until it is green, then merge"));
    assert!(lead_wait_instruction("Подожди, пока не установится новая сборка"));
    assert!(!lead_wait_instruction("Rebase on the lead branch and fix the two failing tests"));
    assert!(!lead_wait_instruction("Wait for QA's review comments before the rename"), "a teammate is not an outside event");
}

#[test]
fn a_waiting_job_is_named_or_loops_around_a_pause() {
    assert!(waiting_job("wait for 1.58.0", "hgs --version").unwrap().contains("its name says it waits"));
    assert!(waiting_job("poll ci", "gh run view 5").is_some());
    let loop_cmd = ":loop\r\nhgs --version | findstr 1.58 && exit /b 0\r\ntimeout /t 120\r\ngoto loop";
    assert!(waiting_job("check", loop_cmd).unwrap().contains("around a pause"));
    assert!(waiting_job("check", "while ! hgs --version | grep -q 1.58; do sleep 120; done").is_some());
    assert!(waiting_job("check", "while ($true) { hgs --version; Start-Sleep 120 }").is_some());
    assert!(waiting_job("cargo test", "cargo test --locked -- state::").is_none());
    assert!(waiting_job("flaky", "for /l %i in (1,1,5) do cargo test x").is_none(), "a repeat with no pause is work");
    assert!(waiting_job("settle", "sleep 2 && cargo build").is_none(), "a pause with no loop is work");
}

#[test]
fn a_waiting_reply_names_an_outside_event() {
    assert!(waiting_reply("I'll wait for the CI run to finish before merging."));
    assert!(waiting_reply("Blocked on the owner's approval card for the token."));
    assert!(waiting_reply("Жду установки новой сборки."));
    assert!(!waiting_reply("Waiting for the build lock held by QA."), "a teammate's lock is not outside");
    assert!(!waiting_reply("Done: the parser is fixed, tests pass."));
}

fn tool(command: &str, at: f64) -> Value {
    json!({"type":"PreToolUse","tool":"Bash","detail":command,"at":at.to_string()})
}

#[test]
fn the_same_read_only_check_repeated_at_intervals_is_a_loop() {
    let events = vec![
        tool("hgs --version", 100.0),
        json!({"type":"PostToolUse","tool":"Bash","detail":"hgs --version","at":"101"}),
        tool("hgs --version", 220.0),
        tool("cargo build", 230.0),
        tool("hgs --version", 345.0),
    ];
    assert_eq!(repeated_check(&events, 10.0), Some(("hgs --version".to_owned(), 3)));
    assert_eq!(repeated_check(&events, 200.0), None, "before the attempt does not count");
    // Quick re-runs (a retry, a typo) are not intervals; work commands never count.
    let quick = vec![tool("git status", 100.0), tool("git status", 110.0), tool("git status", 120.0)];
    assert_eq!(repeated_check(&quick, 0.0), None);
    let work = vec![tool("cargo test x", 100.0), tool("cargo test x", 300.0), tool("cargo test x", 500.0)];
    assert_eq!(repeated_check(&work, 0.0), None);
}

#[test]
fn the_guessed_condition_is_a_ci_run_a_version_or_a_time() {
    let (ci, what) = guess_condition("gh api repos/WhiteWh/zerus/actions/runs/123456", 0.0);
    assert_eq!(ci, json!({"kind":"ci","repo":"WhiteWh/zerus","run":123456}));
    assert_eq!(what, "CI run 123456 of WhiteWh/zerus");
    let (build, what) = guess_condition("wait for 1.58.0\nhgs --version", 0.0);
    assert_eq!(build, json!({"kind":"build","version":"1.58.0"}));
    assert_eq!(what, "hgs 1.58.0 installed");
    let (time, what) = guess_condition("hgs --version", 1000.0);
    assert_eq!(time, json!({"kind":"time","at":2800.0}));
    assert!(what.contains("guessed"));
}

#[test]
fn refusals_name_the_park_command() {
    let lead = lead_refusal("T", Some(138), "A1");
    assert!(lead.starts_with("Waiting is a park, never an instruction"), "{lead}");
    assert!(lead.contains("\"task\":138") && lead.contains("\"attempt\":\"A1\"") && lead.contains("hgs team park --json"), "{lead}");
    let job = job_refusal("its name says it waits (\"wait\")", "T", None, "");
    assert!(job.contains("a wait is a park, never a job") && job.contains("\"task\":N") && job.contains("hgs team park --json"), "{job}");
}

#[test]
fn a_named_script_is_read_for_its_loop() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("poll.cmd"), ":again\r\nhgs --version\r\ntimeout /t 120 /nobreak\r\ngoto again\r\n").unwrap();
    let text = script_text("poll.cmd", dir.path());
    assert!(text.contains("goto again"));
    assert!(waiting_job("install check", &format!("poll.cmd\n{text}")).is_some());
    assert_eq!(script_text("cmd /c poll.cmd", dir.path()), text);
    assert!(script_text("cargo build", dir.path()).is_empty());
}
