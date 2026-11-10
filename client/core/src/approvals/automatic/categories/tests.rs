use super::*;
use serde_json::json;

/// The card `src/state/claude_approval.rs` builds: `header` is the hook's tool.
fn claude_card(tool: &str) -> Value {
    json!({"approval":true,"can_answer":true,"approval_choices":true,"source":"claude_tool_approval",
        "answer_transport":"claude_tui","question_id":"claude-approval:x","question_hash":"a".repeat(64),
        "run_id":"run-one","conversation_id":"conversation-one",
        "questions":[{"id":"approval","question":"Do you want to proceed?","header":tool,"body":"","allow_other":false,
            "options":[{"id":"1","label":"Yes","scope":"once"},{"id":"2","label":"No","scope":"once"}]}]})
}
/// The card `src/state/opencode/forms.rs` builds for a permission request.
fn opencode_card(action: &str, resources: &str) -> Value {
    let prompt = format!("{action}\n{resources}");
    json!({"approval":true,"can_answer":true,"approval_choices":true,"source":"opencode_native",
        "answer_transport":"native_opencode","kind":"opencode_permission","permission_action":action,
        "question_id":"per_1","question_hash":"b".repeat(64),"run_id":"run-one","conversation_id":"conversation-one",
        "questions":[{"id":"permission","header":"Permission","prompt":prompt,"question":prompt,"allow_other":false,
            "options":[{"id":"once","label":"Allow once","scope":"once"},{"id":"always","label":"Always allow","scope":"broader"},
                {"id":"reject","label":"Reject","scope":"once"}]}]})
}
/// The card `src/state/dsh.rs` builds from `src/dsh/bridge.mjs`'s approval.
fn dsh_card(tool: &str) -> Value {
    json!({"approval":true,"can_answer":true,"source":"dsh_native","answer_transport":"native_dsh",
        "question_id":"q1","question_hash":"c".repeat(64),"run_id":"run-one","conversation_id":"conversation-one",
        "questions":[{"id":"decision","question":format!("Allow {tool}?"),"header":"Approval required","body":"",
            "allow_other":false,"options":[{"id":"allow","label":"Allow once"},{"id":"deny","label":"Reject"}]}]})
}

fn category(provider: &str, question: &Value) -> Category {
    tool(provider, question).category
}

#[test]
fn every_table_row_gets_its_category() {
    use Category::*;
    for (name, expected) in [
        ("Read", Read),
        ("Glob", Read),
        ("Grep", Read),
        ("LS", Read),
        ("LSP", Read),
        ("NotebookRead", Read),
        ("Edit", Edit),
        ("Write", Edit),
        ("MultiEdit", Edit),
        ("NotebookEdit", Edit),
        ("EnterWorktree", Edit),
        ("Bash", Shell),
        ("PowerShell", Shell),
        ("Monitor", Shell),
        ("WebFetch", Web),
        ("mcp__github__create_issue", Mcp),
        ("mcp__a-b__c", Mcp),
        ("ListMcpResourcesTool", Mcp),
        ("ReadMcpResourceTool", Mcp),
        ("Agent", Agents),
        ("Task", Agents),
        ("Skill", Agents),
        ("Workflow", Agents),
        ("WebSearch", Paid),
    ] {
        assert_eq!(
            category("claude", &claude_card(name)),
            expected,
            "claude {name}"
        );
    }
    for (action, expected) in [
        ("read", Read),
        ("glob", Read),
        ("grep", Read),
        ("list", Read),
        ("lsp", Read),
        ("edit", Edit),
        ("shell", Shell),
        ("execute", Shell),
        ("webfetch", Web),
        ("subagent", Agents),
        ("skill", Agents),
        ("websearch", Paid),
    ] {
        assert_eq!(
            category("opencode", &opencode_card(action, "*")),
            expected,
            "opencode {action}"
        );
    }
    for (name, expected) in [
        ("read", Read),
        ("read_image", Read),
        ("glob", Read),
        ("grep", Read),
        ("terminal_list", Read),
        ("terminal_read", Read),
        ("job_list", Read),
        ("job_output", Read),
        ("edit", Edit),
        ("write", Edit),
        ("apply_patch", Edit),
        ("str_replace_editor", Edit),
        ("bash", Shell),
        ("pwsh", Shell),
        ("terminal_open", Shell),
        ("terminal_send", Shell),
        ("terminal_signal", Shell),
        ("terminal_close", Shell),
        ("job_kill", Shell),
        ("web_fetch", Web),
        ("mcp__docs__search", Mcp),
        ("skill", Agents),
        ("subagent", Agents),
        ("workflow", Agents),
        ("spawn_teammate", Agents),
        ("send_message", Agents),
        ("interrupt_agent", Agents),
        ("wait_agent", Agents),
        ("web_search", Paid),
    ] {
        assert_eq!(category("dsh", &dsh_card(name)), expected, "dsh {name}");
    }
}

#[test]
fn unlisted_ambiguous_or_unreadable_tools_are_other() {
    use Category::Other;
    for name in [
        "Artifact",
        "ShareOnboardingGuide",
        "CronCreate",
        "RemoteTrigger",
        "SendUserFile",
        "NewTool",
        "mcp__",
        "mcp__server",
        "mcp____tool",
        "Bash command",
        "Read\nBash",
        "",
        "bash",
    ] {
        assert_eq!(
            category("claude", &claude_card(name)),
            Other,
            "claude {name:?}"
        );
    }
    let mut missing = claude_card("Bash");
    missing["questions"][0]
        .as_object_mut()
        .unwrap()
        .remove("header");
    assert_eq!(category("claude", &missing), Other);
    // Underscored OpenCode actions can be MCP tools named server_tool.
    for action in [
        "external_directory",
        "opencode_read_mcp_resource",
        "opencode_list_mcp_resources",
        "github_create_issue",
        "question",
        "todowrite",
        "browser",
        "doom_loop",
        "Shell",
        "shell x",
    ] {
        assert_eq!(
            category("opencode", &opencode_card(action, "*")),
            Other,
            "opencode {action:?}"
        );
    }
    // An older peer's card has no exact action; a disagreeing prompt is not trusted.
    let mut old = opencode_card("shell", "ls");
    old.as_object_mut().unwrap().remove("permission_action");
    assert_eq!(category("opencode", &old), Other);
    let mut disagree = opencode_card("shell", "ls");
    disagree["permission_action"] = json!("read");
    assert_eq!(category("opencode", &disagree), Other);
    for name in [
        "browser",
        "create_worktree",
        "Bash",
        "web search",
        "x?y",
        "",
    ] {
        assert_eq!(category("dsh", &dsh_card(name)), Other, "dsh {name:?}");
    }
    for change in [
        json!({"header":"Other"}),
        json!({"id":"other"}),
        json!({"question":"Run bash?"}),
        json!({"question":"Allow bash"}),
        json!({"question":null}),
    ] {
        let mut card = dsh_card("bash");
        for (key, value) in change.as_object().unwrap() {
            card["questions"][0][key] = value.clone();
        }
        assert_eq!(category("dsh", &card), Other, "{change}");
    }
    assert_eq!(category("codex", &claude_card("Bash")), Other);
}

#[test]
fn paid_wins_for_paid_tools_paid_hints_and_a_cold_cache() {
    let warm = json!({});
    let set = |provider, question: &Value, details: &Value| {
        required(provider, question, details, 1_000.0)
    };
    assert_eq!(
        set("claude", &claude_card("WebSearch"), &warm),
        Set::EMPTY.with(Category::Paid)
    );
    assert_eq!(
        set("claude", &claude_card("Bash"), &warm),
        Set::EMPTY.with(Category::Shell)
    );
    // OpenCode image generation is Other (underscored) but its name is paid.
    let image = opencode_card("experimental_image_generate", "openai/gpt-image-1");
    assert_eq!(
        set("opencode", &image, &warm),
        Set::EMPTY.with(Category::Other).with(Category::Paid)
    );
    // A paid first line still requires Paid when the exact action is missing.
    let mut old = opencode_card("websearch", "query");
    old.as_object_mut().unwrap().remove("permission_action");
    assert_eq!(
        set("opencode", &old, &warm),
        Set::EMPTY.with(Category::Other).with(Category::Paid)
    );
    assert_eq!(
        set("dsh", &dsh_card("web_search"), &warm),
        Set::EMPTY.with(Category::Paid)
    );
    // Cold cache: the agent reports it, or a warm cache's expiry has passed.
    let cold = json!({"cache_hint":{"status":"cold"}});
    let expired = json!({"session_usage":{"prompt_cache":{"status":"warm","expires_at":999.0}}});
    let fresh = json!({"session_usage":{"prompt_cache":{"status":"warm","expires_at":2_000.0}}});
    for details in [&cold, &expired] {
        assert_eq!(
            set("claude", &claude_card("Bash"), details),
            Set::EMPTY.with(Category::Shell).with(Category::Paid)
        );
        assert_eq!(
            set("dsh", &dsh_card("whatever"), details),
            Set::EMPTY.with(Category::Other).with(Category::Paid)
        );
    }
    assert_eq!(
        set("claude", &claude_card("Bash"), &fresh),
        Set::EMPTY.with(Category::Shell)
    );
}

#[test]
fn a_set_permits_only_when_every_required_category_is_on() {
    let defaults = Set::defaults();
    assert_eq!(
        defaults.ids(),
        ["read", "edit", "shell", "web", "mcp", "agents", "other"]
    );
    let shell = Set::EMPTY.with(Category::Shell);
    assert!(defaults.permits(shell));
    assert!(!defaults.permits(shell.with(Category::Paid)));
    assert!(defaults
        .with(Category::Paid)
        .permits(shell.with(Category::Paid)));
    assert!(!Set::EMPTY
        .with(Category::Paid)
        .permits(shell.with(Category::Paid)));
    assert!(!defaults.permits(Set::EMPTY));
    for category in Category::ALL {
        assert_eq!(Category::from_id(category.id()), Some(category));
        assert!(category.setting().ends_with(category.id()));
    }
    assert_eq!(Category::from_id("unknown"), None);
    assert_eq!(
        labels(shell.with(Category::Paid)),
        "Shell commands, Paid usage"
    );
}

#[test]
fn category_settings_are_per_computer_flags() {
    for category in Category::ALL {
        let key = crate::settings::find(category.setting()).unwrap();
        assert_eq!(key.kind, crate::settings::Kind::Flag(category.default_on()));
    }
}
