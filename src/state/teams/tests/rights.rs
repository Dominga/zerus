use super::*;

fn member(agent: &str, profile: &str, permissions: &str) -> Member {
    Member { id: "m".into(), agent: agent.into(), permissions_profile: profile.into(), permissions: permissions.into(), ..Default::default() }
}

fn has(member: &Member, capability: &str) -> bool {
    capabilities(member).contains(capability)
}

/// Task 154 (a, b): every profile maps to the agent's own launch mode.
#[test]
fn every_profile_maps_to_each_agents_real_mode() {
    // DeepSeek: its presets.
    assert_eq!(profile_selection("dsh", "read").as_deref(), Some("native:{\"preset\":\"read-only\"}"));
    for profile in ["write", "build", "commit"] {
        assert_eq!(profile_selection("dsh", profile).as_deref(), Some("native:{\"preset\":\"workspace-write\"}"), "{profile}");
    }
    for profile in ["network", "operate"] {
        assert_eq!(profile_selection("dsh", profile).as_deref(), Some("native:{\"preset\":\"danger-full-access\"}"), "{profile}");
    }
    // Codex: its sandbox, asking for approval as before.
    assert_eq!(profile_selection("codex", "read").as_deref(), Some("native:{\"approval\":\"on-request\",\"sandbox\":\"read-only\"}"));
    assert_eq!(profile_selection("codex", "commit").as_deref(), Some("native:{\"approval\":\"on-request\",\"sandbox\":\"workspace-write\"}"));
    assert_eq!(profile_selection("codex", "operate").as_deref(), Some("native:{\"approval\":\"on-request\",\"sandbox\":\"danger-full-access\"}"));
    // Kimi: plan mode reads; it has no sandbox to widen.
    assert_eq!(profile_selection("kimi", "read").as_deref(), Some("native:{\"plan\":\"on\"}"));
    assert_eq!(profile_selection("kimi", "operate"), None);
    // Claude keeps its permission mode after `--`; every profile has one.
    for (profile, mode) in [("read", "plan"), ("write", "default"), ("build", "acceptEdits"), ("commit", "acceptEdits"), ("network", "acceptEdits"), ("operate", "acceptEdits")] {
        assert_eq!(claude_mode(profile), Some(mode), "{profile}");
        assert_eq!(profile_selection("claude", profile), None);
    }
    // No profile keeps the account's own mode.
    for agent in ["claude", "codex", "kimi", "dsh"] {
        assert_eq!(profile_selection(agent, ""), None, "{agent}");
        assert_eq!(launch_selection(&member(agent, "", "")), "provider", "{agent}");
    }
    // A relaunch resumes Claude with the mode as its native selection.
    assert_eq!(launch_selection(&member("claude", "write", "")), "native:{\"mode\":\"manual\"}");
    assert_eq!(launch_selection(&member("claude", "operate", "")), "native:{\"mode\":\"acceptEdits\"}");
    // An explicit selection wins over the profile.
    assert_eq!(launch_selection(&member("dsh", "read", "bypass")), "bypass");
}

/// Task 154 (a): a member takes the agent's native selection as the solo
/// launch does; DeepSeek has no auto mode.
#[test]
fn a_member_takes_the_agents_native_selection_as_the_solo_launch_does() {
    for agent in ["claude", "codex", "kimi", "dsh"] {
        assert_eq!(check_selection(agent, "").unwrap(), "");
        assert_eq!(check_selection(agent, "provider").unwrap(), "provider");
        assert_eq!(check_selection(agent, "bypass").unwrap(), "bypass");
    }
    for agent in ["claude", "codex", "kimi"] {
        assert_eq!(check_selection(agent, "auto").unwrap(), "auto");
    }
    let error = check_selection("dsh", "auto").unwrap_err().to_string();
    assert!(error.contains("DeepSeek has no auto mode") && error.contains("workspace-write"), "{error}");
    assert_eq!(check_selection("dsh", "native:{\"preset\":\"danger-full-access\"}").unwrap(), "native:{\"preset\":\"danger-full-access\"}");
    // DeepSeek names presets of its own; the launch checks them.
    assert_eq!(check_selection("dsh", "native:{\"preset\":\"team-qa\"}").unwrap(), "native:{\"preset\":\"team-qa\"}");
    assert_eq!(
        check_selection("codex", "native:{ \"sandbox\":\"read-only\", \"approval\":\"never\" }").unwrap(),
        "native:{\"approval\":\"never\",\"sandbox\":\"read-only\"}"
    );
    assert_eq!(check_selection("claude", "native:{\"mode\":\"plan\"}").unwrap(), "native:{\"mode\":\"plan\"}");
    assert_eq!(check_selection("kimi", "native:{\"plan\":\"on\"}").unwrap(), "native:{\"plan\":\"on\"}");
    let error = check_selection("codex", "native:{\"preset\":\"read-only\"}").unwrap_err().to_string();
    assert!(error.contains("codex has no native control \"preset\"") && error.contains("sandbox"), "{error}");
    let error = check_selection("codex", "native:{\"sandbox\":\"wide-open\"}").unwrap_err().to_string();
    assert!(error.contains("read-only, workspace-write, danger-full-access"), "{error}");
    assert!(check_selection("claude", "native:[1]").is_err());
    assert!(check_selection("claude", "native:{\"mode\":3}").is_err());
    assert!(check_selection("opencode", "native:{\"mode\":\"plan\"}").is_err());
    let error = check_selection("codex", "root").unwrap_err().to_string();
    assert!(error.contains("\"root\" is not a launch selection"), "{error}");
}

/// Task 154 (d): a sandboxed launch cannot reach the session host; QA in
/// DeepSeek's default workspace preset could not launch sessions.
#[test]
fn capabilities_follow_each_agents_sandbox() {
    // DeepSeek: the account's default and the workspace preset are sandboxed.
    for dsh in [member("dsh", "", ""), member("dsh", "write", ""), member("dsh", "commit", "")] {
        assert!(has(&dsh, "edit") && has(&dsh, "build") && has(&dsh, "commit"));
        assert!(!has(&dsh, "sessions") && !has(&dsh, "terminals") && !has(&dsh, "worker") && !has(&dsh, "network"));
    }
    assert!(!has(&member("dsh", "read", ""), "edit"));
    for dsh in [member("dsh", "operate", ""), member("dsh", "network", ""), member("dsh", "", "bypass")] {
        assert!(has(&dsh, "sessions") && has(&dsh, "terminals") && has(&dsh, "worker") && has(&dsh, "network"));
    }
    // Codex likewise.
    assert!(!has(&member("codex", "", ""), "sessions"));
    assert!(!has(&member("codex", "", "auto"), "terminals"));
    assert!(has(&member("codex", "operate", ""), "worker"));
    assert!(has(&member("codex", "", "native:{\"sandbox\":\"danger-full-access\"}"), "sessions"));
    assert!(!has(&member("codex", "read", ""), "edit"));
    // Claude and Kimi run in no sandbox of their own; plan mode only reads.
    for agent in ["claude", "kimi"] {
        assert!(has(&member(agent, "", ""), "sessions"), "{agent}");
        assert!(!has(&member(agent, "read", ""), "edit"), "{agent}");
    }
    assert!(has(&member("claude", "build", ""), "terminals"));
}

#[test]
fn a_task_needs_what_it_names_or_what_its_words_ask() {
    let mut task = Task {
        id: 140,
        title: "Status accuracy matrix".into(),
        description: "Launch scratch sessions of each agent, read the terminal snapshot with hgs terminal, and start a worker as the team worker.".into(),
        ..Default::default()
    };
    assert_eq!(needs(&task).into_iter().collect::<Vec<_>>(), ["sessions", "terminals", "worker"]);
    // An explicit list wins, an empty one too.
    task.needs = Some(vec![]);
    assert!(needs(&task).is_empty());
    task.needs = Some(vec!["network".into()]);
    assert_eq!(needs(&task).into_iter().collect::<Vec<_>>(), ["network"]);
    // Ordinary work needs nothing beyond editing and building.
    let plain = Task { id: 1, title: "Fix the parser".into(), description: "Edit src/parse.rs and run its tests through hgs team run.".into(), ..Default::default() };
    assert!(needs(&plain).is_empty());
    assert!(check_needs(&["sessions".into(), "terminals".into()]).is_ok());
    let error = check_needs(&["root".into()]).unwrap_err().to_string();
    assert!(error.contains("not \"root\"") && error.contains("sessions"), "{error}");
    // The narrowest profile that does it; sessions take operate.
    let wants: BTreeSet<String> = ["sessions".to_owned()].into();
    for agent in ["claude", "codex", "kimi", "dsh"] {
        assert_eq!(profile_for(agent, &wants), Some("operate"), "{agent}");
    }
    let build: BTreeSet<String> = ["build".to_owned()].into();
    assert_eq!(profile_for("dsh", &build), Some("write"));
}

/// Task 154 (4): QA and a merger get the rights their job needs.
#[test]
fn role_templates_get_the_rights_their_job_needs() {
    assert_eq!(role_profile("qa", "QA tester"), Some("operate"));
    assert_eq!(role_profile("", "QA tester (Windows, DeepSeek)"), Some("operate"));
    assert_eq!(role_profile("review-merge", "Reviewer & Merger"), Some("commit"));
    assert_eq!(role_profile("", "Merger"), Some("commit"));
    for (id, name) in [("backend", "Backend"), ("tester", "Tester"), ("", "Quality"), ("lead", "Lead"), ("integrator", "Integrator")] {
        assert_eq!(role_profile(id, name), None, "{name}");
    }
}
