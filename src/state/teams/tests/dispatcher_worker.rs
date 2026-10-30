use super::*;
use super::support::*;

#[test]
fn a_profile_launch_puts_the_mode_after_the_separator() {
    let mut team = team();
    {
        let member = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        member.agent = "claude".into();
        member.permissions_profile = "build".into();
    }
    let args = launch_args(&team, team.member("dev").unwrap(), "C:/work/project");
    let separator = args.iter().position(|arg| arg == "--").unwrap();
    let mode = args.iter().position(|arg| arg == "--permission-mode").unwrap();
    assert!(mode > separator, "{args:?}");
    assert_eq!(args[mode + 1], "acceptEdits");
}

#[test]
fn one_worker_and_one_listing_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("worker.lock");
    let first = worker_lock(&path).unwrap().unwrap();
    assert!(worker_lock(&path).unwrap().is_none());
    drop(first);
    assert!(worker_lock(&path).unwrap().is_some());
    // A tick is skipped while a listing may still run (U2).
    let first = listing(|| {
        assert!(listing(|| 1).is_none());
        2
    });
    assert_eq!(first, Some(2));
    assert!(listing(|| 3).is_some());
}

#[test]
fn the_stop_request_ends_the_worker_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let files = WorkerFiles::new(dir.path().to_path_buf());
    files.start();
    assert_eq!(files.pid(), Some(std::process::id()));
    let beat: Value = serde_json::from_slice(&fs::read(files.heartbeat_file()).unwrap()).unwrap();
    assert_eq!(beat["pid"].as_u64(), Some(u64::from(std::process::id())));
    assert_eq!(beat["teams"].as_u64(), Some(0));
    // A request for another pid is stale and ignored.
    files.request_stop(1);
    assert!(!files.stop_requested());
    // A request for this worker is honoured before the tick runs.
    files.request_stop(std::process::id());
    assert!(files.stop_requested());
    let ran = std::cell::Cell::new(false);
    let pass = worker_pass(&files, || {
        ran.set(true);
        Ok(true)
    });
    assert!(matches!(pass, WorkerPass::Stop), "{pass:?}");
    assert!(!ran.get());
    // A clean exit clears its files and logs the reason (U2).
    files.finish("stop requested");
    assert!(!files.pid_file().exists() && !files.heartbeat_file().exists() && !files.stop_file().exists());
    let log = fs::read_to_string(files.log_file()).unwrap();
    assert!(log.contains("start pid=") && log.contains("stop: stop requested"), "{log}");
}

#[test]
fn a_usage_limit_is_read_from_the_listing_and_named_to_the_lead() {
    let listing = json!([{"sessions":[
        {"name":"claude/x/dev","run_id":"r1","state":"running","activity":"idle","phase":"error",
         "provider_error":{"error_kind":"session_limit","detail":"You've hit your session limit. Resets at 18:00.","at":100.0,"reset_at":200.0}},
        {"name":"codex/x/qa","run_id":"r2","state":"running","activity":"idle","phase":"error",
         "provider_error":{"error_kind":"network","detail":"connection reset","at":100.0}}
    ]}]);
    let seen = seen(&listing);
    let dev = seen.iter().find(|session| session.name == "claude/x/dev").unwrap();
    assert_eq!(dev.limited.as_ref().and_then(|limit| limit.resets_at), Some(200.0));
    assert!(dev.limited.as_ref().is_some_and(|limit| limit.detail.contains("session limit")));
    assert!(seen.iter().find(|session| session.name == "codex/x/qa").unwrap().limited.is_none());
    // The stored member shows the episode in the lead's inbox, with its
    // reset time (H1).
    let mut team = team();
    team.members.iter_mut().find(|member| member.id == "dev").unwrap().limited = Some(MemberLimit {
        at: 100.0,
        resets_at: Some(1_793_050_000.0),
        detail: "You've hit your session limit".into(),
    });
    let items = inbox::items(&team);
    let limited = items.iter().find(|item| item.kind == "member_limited").expect("a member_limited item");
    // H1: the lead reads a local date and time, not a raw epoch.
    assert!(limited.summary.contains("resets at 2026"), "{}", limited.summary);
    assert!(!limited.summary.contains("1793050000"), "{}", limited.summary);
    assert_eq!(limited.member, "dev (Backend)");
    assert!(limited.command.contains("hgs team status --json"), "{}", limited.command);
    // Status keeps the number.
    assert_eq!(team.member("dev").unwrap().limited.as_ref().and_then(|limit| limit.resets_at), Some(1_793_050_000.0));
}

#[test]
fn the_team_setting_starts_resume_for_a_member_and_the_lead_but_not_a_deepseek_host() {
    let mut base = team();
    let mut dev = session("codex/x/dev", "r1", true);
    dev.free = false;
    dev.phase = "error".into();
    dev.limited = Some(SeenLimit { at: 100.0, resets_at: Some(200.0), detail: "session limit".into() });
    let lead = session("claude/x/lead", "r0", true);
    let stored = MemberLimit { at: 100.0, resets_at: Some(200.0), detail: "session limit".into() };
    // Before the flag, the episode is stored but recovery is not started.
    let found = steps(&base, &[lead.clone(), dev.clone()], 300.0);
    assert!(found.contains(&Step::Limited { member: "dev".into(), limited: Some(stored.clone()) }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::StartLimitResume { .. })));
    // With the setting on, Zerus asks the recovery service to wait for the
    // reset; it does not retry on its own.
    base.settings.resume_after_limits = true;
    base.members.iter_mut().find(|member| member.id == "dev").unwrap().limited = Some(stored.clone());
    assert!(steps(&base, &[lead.clone(), dev.clone()], 300.0).contains(&Step::StartLimitResume { member: "dev".into() }));
    // Once started it is refreshed, not asked again; the end of the
    // episode stops it. N29: the start records the session it names.
    {
        let dev = base.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.limit_resume = true;
        dev.limit_session = dev.session.clone();
    }
    let waiting = steps(&base, &[lead.clone(), dev.clone()], 300.0);
    assert!(!waiting.iter().any(|step| matches!(step, Step::StartLimitResume { .. })));
    assert!(waiting.contains(&Step::RefreshLimitResume { member: "dev".into() }), "{waiting:?}");
    let mut cleared = dev.clone();
    cleared.limited = None;
    cleared.free = true;
    cleared.phase = "idle".into();
    base.members.iter_mut().find(|member| member.id == "dev").unwrap().limited = None;
    assert!(steps(&base, &[lead.clone(), cleared], 300.0).contains(&Step::StopLimitResume { member: "dev".into() }));
    // An unknown reset is the inbox item only: no start.
    let mut unknown = dev.clone();
    unknown.limited = Some(SeenLimit { at: 100.0, resets_at: None, detail: "session limit".into() });
    let mut unknown_team = team();
    unknown_team.settings.resume_after_limits = true;
    unknown_team.members.iter_mut().find(|member| member.id == "dev").unwrap().limited =
        Some(MemberLimit { at: 100.0, resets_at: None, detail: "session limit".into() });
    assert!(!steps(&unknown_team, &[lead.clone(), unknown], 300.0)
        .iter()
        .any(|step| matches!(step, Step::StartLimitResume { .. })));
    // A DeepSeek host is not resumed; the lead's own session follows the
    // setting.
    let mut dsh_team = team();
    dsh_team.settings.resume_after_limits = true;
    dsh_team.members.iter_mut().find(|member| member.id == "dev").unwrap().agent = "dsh".into();
    dsh_team.members.iter_mut().find(|member| member.id == "dev").unwrap().limited = Some(stored.clone());
    assert!(!steps(&dsh_team, &[lead.clone(), dev.clone()], 300.0)
        .iter()
        .any(|step| matches!(step, Step::StartLimitResume { member, .. } if member == "dev")));
    let mut lead_team = team();
    lead_team.settings.resume_after_limits = true;
    lead_team.members.iter_mut().find(|member| member.id == "lead").unwrap().limited = Some(stored);
    let mut lead_limited = lead.clone();
    lead_limited.free = false;
    lead_limited.phase = "error".into();
    lead_limited.limited = Some(SeenLimit { at: 100.0, resets_at: Some(200.0), detail: "session limit".into() });
    assert!(steps(&lead_team, &[lead_limited, dev], 300.0).contains(&Step::StartLimitResume { member: "lead".into() }));
}

#[test]
fn the_hand_off_after_a_swap_carries_the_partial_report_questions_and_git() {
    let mut team = team();
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git").args(args).current_dir(dir.path()).output().unwrap()
    };
    assert!(git(&["init", "-q"]).status.success());
    std::fs::write(dir.path().join("work.txt"), "half").unwrap();
    assert!(git(&["add", "."]).status.success());
    assert!(git(&["-c", "user.name=Zerus", "-c", "user.email=z@example.com", "commit", "-qm", "half way"])
        .status
        .success());
    {
        let task = team.task_mut(1).unwrap();
        task.continuation = Some(Continuation {
            attempt: "a1".into(),
            n: 1,
            since: 0.0,
            report: "Half done: the API is next.".into(),
            questions: vec!["Which database?".into()],
        });
    }
    let extra = continuation_extra(team.task(1).unwrap());
    let attempt = Attempt { n: 2, id: "a2".into(), member: "dev".into(), ..Default::default() };
    let text = hand_off_text_with(&team, team.task(1).unwrap(), &attempt, "", "", &extra);
    assert!(text.contains("after Zerus swapped your session"), "{text}");
    assert!(text.contains("Half done: the API is next."), "{text}");
    assert!(text.contains("Which database?"), "{text}");
    // H6: the git part names the commits since the attempt and the working
    // tree, both bounded.
    // A real attempt start (a minute before the commit) filters by date;
    // a zero start time is no filter at all, as on git 2.55 where
    // `--since @0` means "now".
    let commit_at: f64 = String::from_utf8_lossy(&git(&["log", "-1", "--format=%ct"]).stdout)
        .trim()
        .parse()
        .unwrap();
    for since in [0.0, commit_at - 60.0] {
        let git_text = git_continuation(dir.path().to_str().unwrap(), since);
        assert!(git_text.contains("half way"), "{since}: {git_text}");
        assert!(git_text.contains("Commits since the attempt began") && git_text.contains("Working tree"), "{git_text}");
    }
    let later = git_continuation(dir.path().to_str().unwrap(), commit_at + 3600.0);
    assert!(!later.contains("half way"), "{later}");
    assert!(!git_continuation("", 0.0).contains("Commits"));
    // A swapped lead gets a short brief that names the inbox (H6, U8).
    let mut running = team.clone();
    running.brief = Notice::Pending;
    let brief = brief_text(&running);
    assert!(brief.contains("hgs team inbox --json") && brief.contains(&running.id), "{brief}");
    // U38: the running brief points at the profile for the phase report.
    assert!(brief.contains("hgs team profile --json") && brief.contains("top three"), "{brief}");
}
