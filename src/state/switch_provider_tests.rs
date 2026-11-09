//! Task 352: moving a session to an account of another agent. The `hgs`
//! children are faked; the state root, the account catalog and the link
//! store are real files in a private folder.
use super::*;
use crate::state::teams::dispatcher::{fake_output, set_fake_hgs};
use std::cell::RefCell;
use std::rc::Rc;

const OLD: &str = "claude/project/work";
const REQUEST: &str = "22222222-3333-4444-8555-666666666666";

#[derive(Default)]
struct World {
    calls: Vec<Vec<String>>,
    sent: Vec<Value>,
    launch_id: String,
    launched: Option<String>,
    fail_launch: bool,
    folder: String,
    old_running: bool,
}

fn config(root: &Path) -> crate::config::Config {
    let config = crate::config::Config {
        home: root.to_string_lossy().into_owned(),
        dir: root.join("config"),
        state: root.join("state"),
        selves: vec![],
        peers: vec![],
        base_peers: vec![],
        peers_from_environment: false,
        machines: std::collections::BTreeMap::new(),
        tab: false,
        tab_colors: String::new(),
    };
    fs::create_dir_all(&config.dir).unwrap();
    fs::create_dir_all(&config.state).unwrap();
    let mut profiles = serde_json::Map::new();
    for (id, label) in [("work", "Work"), ("second", "Second")] {
        let home = config.dir.join("accounts").join(id);
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join(".credentials.json"), "{}").unwrap();
        profiles.insert(id.into(), json!({"provider":"claude","label":label}));
    }
    fs::write(config.dir.join("accounts.json"), json!({"profiles":profiles}).to_string()).unwrap();
    config
}

/// The old session as `hgs inspect` shows it, idle with a conversation.
fn old_row(world: &World) -> Value {
    json!({"name":OLD,"agent":"claude","account_id":"work","run_id":"run-old","conversation_id":"conv-old",
        "cwd":world.folder,"session_state":"cooling","phase":"idle",
        "process_state":if world.old_running {"running"} else {"exited"},"goal":"Ship the account chip",
        "pending_questions":[],"held_inputs":[],"input_queue":[]})
}

fn history() -> Value {
    json!({"events":[
        {"type":"UserPromptSubmit","detail":"Make the chip list every provider"},
        {"type":"AgentMessage","detail":"Listed them, grouped by provider"},
        {"type":"ToolUse","detail":"never carried"},
        {"type":"UserPromptSubmit","detail":"Here is the screenshot",
         "attachments":[{"bytes":10,"index":0,"mime":"image/png","name":"chip.png","request_id":"req-attach"}]},
        {"type":"AgentMessage","detail":"I see the chip in the screenshot"}]})
}

fn tasks() -> Value {
    json!({"tasks":[
        {"ref":"PROJECT/T-aaaaaa","title":"Account chip across providers","execution":{"current":{"executor":{"host":"","session":OLD}}}},
        {"ref":"PROJECT/T-bbbbbb","title":"Someone else's","execution":{"current":{"executor":{"host":"","session":"codex/project/other"}}}}]})
}

fn ok(value: Value) -> anyhow::Result<std::process::Output> {
    Ok(fake_output(0, &value.to_string(), ""))
}

fn refused(text: &str) -> anyhow::Result<std::process::Output> {
    Ok(fake_output(1, "", text))
}

/// The children a move runs, answered from the shared world.
fn install(world: Rc<RefCell<World>>) {
    set_fake_hgs(Some(Box::new(move |_host: &str, args: &[&str], input: Option<&Value>| {
        let mut w = world.borrow_mut();
        w.calls.push(args.iter().map(|arg| (*arg).to_owned()).collect());
        match args {
            ["inspect", name] if *name == OLD => ok(old_row(&w)),
            ["inspect", name] => match &w.launched {
                Some(launched) if launched.as_str() == *name => {
                    ok(json!({"name":name,"launch_id":w.launch_id,"run_id":"run-new","conversation_id":"conv-new"}))
                }
                _ => refused("no such session"),
            },
            ["history", name, "--json"] if *name == OLD => ok(history()),
            ["task", "list", ..] => ok(tasks()),
            ["pause", _] => {
                w.old_running = false;
                ok(Value::Null)
            }
            ["resume", _, "-d"] => {
                w.old_running = true;
                ok(Value::Null)
            }
            ["send", name, "--json"] => {
                assert_eq!(Some(*name), w.launched.as_deref());
                w.sent.push(input.cloned().unwrap_or_default());
                ok(json!({"status":"submitted"}))
            }
            [agent, folder, "--new", "-n", tag, "--launch-id", launch, ..] => {
                assert_eq!(*folder, w.folder);
                if w.fail_launch {
                    return refused("the agent could not start");
                }
                let project = OLD.split('/').nth(1).unwrap();
                w.launch_id = (*launch).to_owned();
                w.launched = Some(format!("{agent}/{project}/{tag}"));
                ok(Value::Null)
            }
            other => panic!("unexpected hgs call {other:?}"),
        }
    })));
}

fn with_world(installed: &[&str], body: impl FnOnce(&crate::config::Config, Rc<RefCell<World>>)) {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let folder = root.path().join("project");
    fs::create_dir_all(&folder).unwrap();
    let world = Rc::new(RefCell::new(World { folder: folder.to_string_lossy().into_owned(), old_running: true, ..World::default() }));
    crate::state::with_test_state_root(&config.state, || {
        crate::accounts::set_test_installed(Some(installed));
        install(world.clone());
        body(&config, world.clone());
        set_fake_hgs(None);
        crate::accounts::set_test_installed(None);
    });
}

fn launches(world: &Rc<RefCell<World>>) -> Vec<Vec<String>> {
    world.borrow().calls.iter().filter(|call| call.get(2).is_some_and(|arg| arg == "--new")).cloned().collect()
}

#[test]
fn an_account_of_the_same_agent_stays_the_account_switch() {
    with_world(&["claude", "dsh"], |config, world| {
        assert!(!crosses(config, OLD, "second"));
        let preview = preview(config, OLD, "second");
        assert_eq!(preview["kind"], "account", "{preview}");
        assert_eq!(preview["refusal"], "", "{preview}");
        // A same-agent preview runs no child at all.
        assert!(world.borrow().calls.is_empty());
        assert!(crosses(config, OLD, "native-dsh"));
    });
}

#[test]
fn a_preview_names_what_carries_over_and_changes_nothing() {
    with_world(&["claude", "dsh"], |config, world| {
        let preview = preview(config, OLD, "native-dsh");
        assert_eq!(preview["kind"], "move", "{preview}");
        assert_eq!(preview["refusal"], "", "{preview}");
        assert_eq!(preview["new_session"], "dsh/project/work", "{preview}");
        assert_eq!(preview["to"]["agent"], "dsh");
        assert!(preview["drops"].as_array().unwrap().contains(&json!("native_tool_state")));
        assert!(preview["carries"].as_array().unwrap().contains(&json!("recent_turns")));
        let changed = world.borrow().calls.iter().any(|call| matches!(call[0].as_str(), "pause" | "send" | "history") || call.get(2).is_some_and(|arg| arg == "--new"));
        assert!(!changed, "a preview only reads");
    });
}

#[test]
fn a_move_to_deepseek_starts_its_session_with_the_handoff_and_links_both() {
    with_world(&["claude", "dsh"], |config, world| {
        let receipt = run(config, OLD, "native-dsh", REQUEST).unwrap();
        assert_eq!(receipt["status"], "moved", "{receipt}");
        assert_eq!(receipt["new_session"], "dsh/project/work");
        assert_eq!(receipt["to"]["agent"], "dsh");
        assert_eq!(receipt["carried"]["turns"], 4, "{receipt}");
        assert_eq!(receipt["carried"]["attachments"], 1, "{receipt}");
        assert_eq!(receipt["carried"]["tasks"], json!(["PROJECT/T-aaaaaa"]));
        let w = world.borrow();
        // The old session stopped before the new one started.
        let pause = w.calls.iter().position(|call| call[0] == "pause").expect("old session stopped");
        let launch = w.calls.iter().position(|call| call.get(2).is_some_and(|arg| arg == "--new")).unwrap();
        assert!(pause < launch);
        let launch_call = &w.calls[launch];
        assert!(launch_call.windows(2).any(|pair| pair == ["--account", "native-dsh"]), "{launch_call:?}");
        assert!(!launch_call.contains(&"--".to_owned()), "DeepSeek takes no first prompt");
        // DeepSeek gets the handoff as its first message, to the exact conversation.
        assert_eq!(w.sent.len(), 1);
        let sent = &w.sent[0];
        assert_eq!(sent["expected_conversation_id"], "conv-new");
        let text = sent["text"].as_str().unwrap();
        for part in ["moved from Claude Code session claude/project/work", "Make the chip list every provider",
            "I see the chip in the screenshot", "hgs attachment claude/project/work --request req-attach --index 0 --conversation conv-old",
            "PROJECT/T-aaaaaa", "Ship the account chip", "Not carried over"] {
            assert!(text.contains(part), "{part} missing from {text}");
        }
        assert!(!text.contains("never carried"));
        assert!(!text.contains("T-bbbbbb"));
        assert!(text.encode_utf16().count() <= BRIEF_UNITS);
        drop(w);
        // The full handoff is kept beside the receipt.
        assert!(fs::read_to_string(brief_path(REQUEST)).unwrap().contains("Listed them, grouped by provider"));
        // Both rows name the other session; an unrelated row keeps its shape.
        let mut rows = vec![json!({"name":OLD}), json!({"name":"dsh/project/work"}), json!({"name":"codex/project/other"})];
        decorate(&mut rows);
        assert_eq!(rows[0]["moved_to"]["session"], "dsh/project/work");
        assert_eq!(rows[1]["moved_from"]["session"], OLD);
        assert_eq!(rows[1]["moved_from"]["conversation_id"], "conv-old");
        assert_eq!(rows[2], json!({"name":"codex/project/other"}));
    });
}

#[test]
fn a_move_to_a_terminal_agent_gives_the_handoff_as_its_first_prompt() {
    with_world(&["claude", "codex", "dsh"], |config, world| {
        let mut catalog: Value = serde_json::from_str(&fs::read_to_string(config.dir.join("accounts.json")).unwrap()).unwrap();
        // A Codex account: a terminal agent, so the brief goes after `--`.
        catalog["profiles"]["codex-one"] = json!({"provider":"codex","label":"Codex one"});
        fs::write(config.dir.join("accounts.json"), catalog.to_string()).unwrap();
        let home = config.dir.join("accounts").join("codex-one");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("auth.json"), "{}").unwrap();
        let receipt = run(config, OLD, "codex-one", REQUEST).unwrap();
        assert_eq!(receipt["status"], "moved", "{receipt}");
        let all = launches(&world);
        assert_eq!(all.len(), 1);
        let codex = &all[0];
        assert_eq!(codex[0], "codex");
        assert!(codex.contains(&"-d".to_owned()));
        let at = codex.iter().position(|arg| arg == "--").expect("first prompt");
        assert!(codex[at + 1].contains("Make the chip list every provider"));
    });
}

#[test]
fn a_move_is_refused_before_anything_changes_when_the_agent_is_missing() {
    with_world(&["claude"], |config, world| {
        let error = run(config, OLD, "native-dsh", REQUEST).unwrap_err();
        assert!(error.contains("DeepSeek is not installed"), "{error}");
        let preview = preview(config, OLD, "native-dsh");
        assert!(preview["refusal"].as_str().unwrap().contains("not installed"), "{preview}");
        let w = world.borrow();
        assert!(w.calls.iter().all(|call| call[0] == "inspect"), "{:?}", w.calls);
        assert!(!receipt_path(REQUEST).exists(), "a refusal keeps no receipt");
    });
}

#[test]
fn a_busy_or_asking_session_is_refused() {
    with_world(&["claude", "dsh"], |config, world| {
        set_fake_hgs(Some(Box::new(|_host: &str, args: &[&str], _input: Option<&Value>| match args {
            ["inspect", _] => ok(json!({"name":OLD,"run_id":"r","conversation_id":"c","cwd":".","session_state":"working","phase":"tool"})),
            other => panic!("unexpected {other:?}"),
        })));
        let error = run(config, OLD, "native-dsh", REQUEST).unwrap_err();
        assert!(error.contains("not idle"), "{error}");
        set_fake_hgs(Some(Box::new(|_host: &str, args: &[&str], _input: Option<&Value>| match args {
            ["inspect", _] => ok(json!({"name":OLD,"run_id":"r","conversation_id":"c","cwd":".","session_state":"user","phase":"input",
                "pending_questions":[{"id":"q"}]})),
            other => panic!("unexpected {other:?}"),
        })));
        let error = run(config, OLD, "native-dsh", REQUEST).unwrap_err();
        assert!(error.contains("question"), "{error}");
        install(world);
    });
}

#[test]
fn a_retry_with_the_same_request_returns_the_kept_receipt() {
    with_world(&["claude", "dsh"], |config, world| {
        let first = run(config, OLD, "native-dsh", REQUEST).unwrap();
        let again = run(config, OLD, "native-dsh", REQUEST).unwrap();
        assert_eq!(first, again);
        assert_eq!(launches(&world).len(), 1, "never a second session");
        let error = run(config, OLD, "second", REQUEST).unwrap_err();
        assert!(error.contains("another move"), "{error}");
    });
}

#[test]
fn a_failed_start_resumes_the_old_session() {
    with_world(&["claude", "dsh"], |config, world| {
        world.borrow_mut().fail_launch = true;
        let receipt = run(config, OLD, "native-dsh", REQUEST).unwrap();
        assert_eq!(receipt["status"], "failed", "{receipt}");
        assert_eq!(receipt["relaunched"], true);
        assert!(receipt["reason"].as_str().unwrap().contains("could not start"));
        assert!(world.borrow().old_running);
        let mut rows = vec![json!({"name":OLD})];
        decorate(&mut rows);
        assert!(rows[0].get("moved_to").is_none(), "a failed move links nothing");
    });
}

#[test]
fn the_new_name_skips_a_taken_tag() {
    with_world(&["claude", "dsh"], |_config, world| {
        world.borrow_mut().launched = Some("dsh/project/work".into());
        assert_eq!(free_name("dsh", "project", "work").unwrap(), ("work-2".into(), "dsh/project/work-2".into()));
    });
}
