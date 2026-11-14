//! Compare token consumption with the production parsers, independent of arm
//! braces, index variable names, guards, or helpers used to consume a value.
use super::*;
use std::collections::BTreeSet;

const PROBE: &str = "cli::parent_scan_tests::parser_fixture";
const NAME: &str = "claude/fixture/parent";
const SID: &str = "11111111-1111-4111-8111-111111111111";
const RUN: &str = "fixture-parent-run";

fn option_literals(source: &str, begin: &str, end: &str) -> BTreeSet<String> {
    let body = source
        .split_once(begin)
        .unwrap()
        .1
        .split_once(end)
        .unwrap()
        .0;
    regex::Regex::new(r#""(--?[a-z][a-z-]*|--)""#)
        .unwrap()
        .captures_iter(body)
        .map(|v| v[1].to_owned())
        .collect()
}

#[test]
fn the_parent_scan_follows_the_actual_launch_and_fork_parsers() {
    let source = include_str!("../cli.rs");
    for (parser, begin, end, required) in [
        (
            "launch",
            "impl LaunchOptions {",
            "/// Whether Claude has saved",
            "--account",
        ),
        (
            "fork",
            "impl<'a> ForkOptions<'a> {",
            "\nfn fork_session(",
            "--archive",
        ),
    ] {
        let options = option_literals(source, begin, end);
        assert!(options.contains(required), "empty {parser} inventory");
        // An empty option is the direct --parent control. Every case gets a
        // new process: successful parses set parent_link's real OnceLock.
        for option in std::iter::once(String::new()).chain(options) {
            let home = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", PROBE, "--ignored", "--nocapture"])
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .env("HOME", home.path())
                .env("HGS_STATE_DIR", home.path().join("state"))
                .env("HGS_CONFIG_DIR", home.path().join("config"))
                .env("DSH_HOME", home.path().join("dsh-home"))
                .env("ZERUS_PARENT_SCAN_PARSER", parser)
                .env("ZERUS_PARENT_SCAN_OPTION", &option)
                .stdin(std::process::Stdio::null())
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{parser} {option}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

struct FixtureTerminal;
static TERMINAL: FixtureTerminal = FixtureTerminal;
impl TerminalBackend for FixtureTerminal {
    fn panes(&self) -> std::result::Result<Vec<state::backend::PaneRow>, String> {
        Ok(vec![state::backend::PaneRow {
            session: NAME.into(),
            session_id: String::new(),
            pane: "%fixture".into(),
            dead: "0".into(),
            run_marker: RUN.into(),
        }])
    }
}

#[test]
#[ignore = "private parser fixture; invoked once per option by its parent test"]
fn parser_fixture() {
    use sha2::{Digest, Sha256};
    let root = PathBuf::from(env::var_os("HGS_STATE_DIR").unwrap());
    fs::create_dir_all(&root).unwrap();
    let record = json!({"version":1,"name":NAME,"agent":"claude","session_id":SID,
        "run_id":RUN,"conversation_id":"fixture-conversation","pid":std::process::id(),
        "process_start":platform::process_start_time(std::process::id()).unwrap(),"pane":"%fixture"});
    fs::write(
        root.join(format!("{:x}.json", Sha256::digest(NAME.as_bytes()))),
        record.to_string(),
    )
    .unwrap();
    let claim = json!({"session_id":SID,"name":NAME,"run_id":RUN,
        "conversation_id":"fixture-conversation"})
    .to_string();
    let option = env::var("ZERUS_PARENT_SCAN_OPTION").unwrap();
    let parser = env::var("ZERUS_PARENT_SCAN_PARSER").unwrap();
    let mut args = Vec::new();
    if parser == "fork" {
        args.push("claude/fixture/source".to_owned());
    }
    if !option.is_empty() {
        args.push(option);
    }
    args.extend(["--parent".to_owned(), claim]);
    assert!(!state::parent_link::claimed_launch());
    state::backend::with_fake(&TERMINAL, || {
        let reached_parent = match parser.as_str() {
            "launch" => {
                // A later parser error cannot undo a claim it already read.
                let parsed = LaunchOptions::parse(&args, true);
                let claimed = state::parent_link::claimed_launch();
                if names_parent(&args) && !claimed {
                    panic!("parent not claimed: {:?}", parsed.err());
                }
                claimed
            }
            "fork" => {
                // Fork deliberately has no --parent option. Its existing
                // error proves the token was reached as an option rather
                // than consumed as the preceding option's value.
                let error = ForkOptions::parse(&args, true)
                    .err()
                    .expect("fork refuses --parent");
                assert!(!state::parent_link::claimed_launch());
                error.message == "unknown fork option: --parent"
            }
            _ => panic!("unknown parser"),
        };
        assert_eq!(names_parent(&args), reached_parent, "{parser} {args:?}");
    });
}
