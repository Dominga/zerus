//! Kimi's --prompt is a non-interactive mode, not an initial TUI message.
//! Keep a launch message out of the resume recipe and use ordinary guarded
//! input once, in the lifetime of the supervisor that created this run.
use super::*;
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

pub(super) struct Prompt {
    text: String,
    request_id: String,
    conversation: String,
    spent: bool,
}

impl Prompt {
    pub(super) fn new(text: String, conversation: String) -> Self {
        Self {
            text,
            request_id: uuid::Uuid::new_v4().to_string(),
            conversation,
            spent: false,
        }
    }
}

pub(super) fn take_prompt(argv: &mut Vec<String>, help: &str) -> Result<Option<String>> {
    // Capability, not version: the Python CLI and the new native CLI have
    // different flags. Unrecognized/positional-capable CLIs retain native argv.
    if !help.lines().any(|line| {
        line.contains("--prompt")
            && (line.contains("non-interactively")
                || line.contains("doesn't enter interactive mode"))
    }) {
        return Ok(None);
    }
    if help.lines().any(|line| {
        line.starts_with("Usage:") && (line.contains("[PROMPT]") || line.contains("[prompt]"))
    }) {
        return Ok(None);
    }
    let commands = help
        .split_once("Commands:")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    let mut positions = Vec::new();
    let mut delimiter = None;
    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--" {
            delimiter = Some(i);
            positions.extend(i + 1..argv.len());
            break;
        }
        if !arg.starts_with('-') {
            if positions.is_empty()
                && commands.lines().any(|line| {
                    line.starts_with("  ")
                        && !line.starts_with("   ")
                        && line
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .split('|')
                            .any(|name| name == arg)
                })
            {
                return Ok(None);
            }
            positions.push(i);
            i += 1;
            continue;
        }
        let flag = arg.split('=').next().unwrap();
        // Explicit non-interactive modes, help and version are native requests.
        if matches!(
            flag,
            "-p" | "--prompt"
                | "--command"
                | "--print"
                | "--quiet"
                | "--acp"
                | "--wire"
                | "-h"
                | "--help"
                | "-V"
                | "--version"
        ) {
            return Ok(None);
        }
        let line = help.lines().find(|line| {
            line.trim_start().starts_with('-')
                && line
                    .split_whitespace()
                    .any(|word| word.trim_end_matches(',') == flag)
        });
        // Hidden, documented aliases. Unknown flags remain native errors;
        // never guess whether the next argument is their value or a prompt.
        let (required, optional) = if let Some(line) = line {
            let signature = line.trim_start().split("  ").next().unwrap();
            (
                signature.contains('<')
                    || signature.contains(" TEXT")
                    || signature.contains(" PATH"),
                signature.contains('['),
            )
        } else if matches!(flag, "-r" | "--resume") {
            (false, true)
        } else if matches!(flag, "--yes" | "--auto-approve") {
            (false, false)
        } else {
            return Ok(None);
        };
        i += 1;
        if !arg.contains('=')
            && (required || optional && argv.get(i).is_some_and(|a| !a.starts_with('-')))
        {
            if i == argv.len() {
                return Ok(None);
            }
            i += 1;
        }
    }
    if positions.is_empty() {
        return Ok(None);
    }
    if positions.len() != 1 {
        return Err(
            "Kimi's initial message must be one quoted argument alongside its native options"
                .into(),
        );
    }
    let text = argv.remove(positions[0]);
    if let Some(at) = delimiter {
        argv.remove(at);
    }
    Ok(Some(text))
}

fn clipped(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut value: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        value.push('…');
    }
    value
}

fn superseded(record: &Value, conversation: &str) -> bool {
    (!string(record, "conversation_id").is_empty()
        && string(record, "conversation_id") != conversation)
        || record["input_pending_at"].as_f64().unwrap_or(0.0) > 0.0
        || record.get("pausing").is_some()
}

fn pending(record: &Value, request: &str) -> bool {
    record["launch_prompt"]["status"] == "pending"
        && record["launch_prompt"]["request_id"] == request
}

fn cancel(name: &str, run: &str, request: &str) -> Result<()> {
    let _guard = lock(None)?;
    if let Some(mut record) = read_run(name, run)?.filter(|record| pending(record, request)) {
        record["launch_prompt"]["status"] = json!("cancelled");
        write(&mut record)?;
    }
    Ok(())
}

/// Compare-and-set only. Terminal capture/readiness must happen outside this
/// global state lock; input::submit repeats all guards for the one submission.
fn reserve(name: &str, run: &str, request: &str, conversation: &str) -> Result<bool> {
    let _guard = lock(None)?;
    let Some(mut record) = read_run(name, run)?.filter(|record| pending(record, request)) else {
        return Ok(false);
    };
    let valid =
        !superseded(&record, conversation) && string(&record, "conversation_id") == conversation;
    record["launch_prompt"]["status"] = json!(if valid { "attempted" } else { "cancelled" });
    write(&mut record)?;
    Ok(valid)
}

fn poll_interval(elapsed: Duration) -> Duration {
    Duration::from_millis(if elapsed < Duration::from_secs(10) {
        250
    } else {
        1500
    })
}

/// This loop never survives or restarts its supervisor, and it never retries a
/// submit call, including definite refusals. Readiness polling sends no keys.
/// Long manual trust/setup waits neither hold the state lock nor poll rapidly.
pub(super) fn wait(
    child: &mut Child,
    name: &str,
    run: &str,
    prompt: Option<&mut Prompt>,
) -> Result<ExitStatus> {
    let Some(prompt) = prompt.filter(|prompt| !prompt.spent) else {
        return supervisor::wait(child);
    };
    let mut name = name.to_owned();
    let text = &prompt.text;
    let request_id = &prompt.request_id;
    // Pin the requested identity before native spawn. A fast external first
    // message must not become this launch prompt's target conversation.
    let conversation = &prompt.conversation;
    {
        let _guard = lock(None)?;
        let mut record = read_run(&name, run)?.ok_or("Kimi launch identity changed")?;
        name = string(&record, "name").to_owned();
        record["launch_prompt"] =
            json!({"request_id":request_id,"status":"pending","preview":clipped(text, 200)});
        write(&mut record)?;
    }
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            cancel(&name, run, request_id)?;
            // wait() also clears the platform's shutdown child registration.
            let _ = supervisor::wait(child)?;
            return Ok(status);
        }
        if supervisor::shutdown_requested() {
            cancel(&name, run, request_id)?;
            return supervisor::wait(child);
        }
        // Atomic record reads and terminal observation do not take lock(None).
        let Some(record) = read_run(&name, run)?.filter(|record| pending(record, request_id))
        else {
            prompt.spent = true;
            return supervisor::wait(child);
        };
        // Follow a verified rename once, rather than repeatedly scanning by
        // the old name or submitting input to a replacement with that name.
        name = string(&record, "name").to_owned();
        if superseded(&record, conversation) {
            prompt.spent = true;
            cancel(&name, run, request_id)?;
            return supervisor::wait(child);
        }
        if input::launch_ready(&name, run, conversation).is_ok() {
            // Spend in memory and reserve durably before the sole submit call.
            // A race/refusal/unknown outcome can never authorize another call.
            prompt.spent = true;
            if reserve(&name, run, request_id, conversation)? {
                let payload = json!({"request_id":request_id,"text":text,
                    "expected_run_id":run,"expected_conversation_id":conversation});
                let result = input::submit(&name, payload.to_string().as_bytes(), None);
                let _guard = lock(None)?;
                if let Some(mut record) = read_run(&name, run)? {
                    record["launch_prompt"]["status"] = json!(if result.is_ok() {
                        "submitted"
                    } else {
                        "stopped"
                    });
                    if let Err(error) = result {
                        eprintln!("hgs: Kimi initial message was not confirmed: {error}. Review before sending again.");
                        record["launch_prompt"]["error"] = json!(clipped(&error, 512));
                    }
                    write(&mut record)?;
                }
            }
            return supervisor::wait(child);
        }
        std::thread::sleep(poll_interval(started.elapsed()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HELP: &str = include_str!("../../tests/fixtures/kimi-help-2.3.3.txt");
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn waiting_backoff_and_inspection_preview_are_bounded() {
        assert_eq!(
            poll_interval(Duration::from_secs(9)),
            Duration::from_millis(250)
        );
        assert_eq!(
            poll_interval(Duration::from_secs(10)),
            Duration::from_millis(1500)
        );
        assert_eq!(clipped(&"Я".repeat(64000), 200).chars().count(), 201);
        assert_eq!(clipped("short", 200), "short");
    }
    #[test]
    fn submission_reservation_refuses_replacement_conversation_or_manual_input() {
        let record = json!({"conversation_id":"", "launch_prompt":{"status":"pending","request_id":"ticket"}});
        assert!(pending(&record, "ticket"));
        assert!(!pending(&record, "other"));
        assert!(!superseded(&record, ""));
        for (key, value) in [
            ("conversation_id", json!("foreign")),
            ("input_pending_at", json!(1.0)),
            ("pausing", json!({})),
        ] {
            let mut changed = record.clone();
            changed[key] = value;
            assert!(superseded(&changed, ""));
        }
    }
    #[test]
    fn interactive_message_is_removed_but_native_flags_and_values_stay() {
        for flags in [
            vec![],
            vec!["--model", "fixture", "--plan"],
            vec!["--skills-dir", "fixture", "--add-dir=other", "--yolo"],
            vec!["--session", "fixture-id"],
            vec!["--continue"],
        ] {
            let mut argv = args(&["kimi"]);
            argv.extend(args(&flags));
            argv.push("First message\\".into());
            let mut expected = argv.clone();
            expected.pop();
            assert_eq!(
                take_prompt(&mut argv, HELP).unwrap(),
                Some("First message\\".into())
            );
            assert_eq!(argv, expected);
        }
    }
    #[test]
    fn account_model_defaults_after_the_message_are_preserved() {
        let mut argv = args(&["kimi", "hello", "--model", "fixture"]);
        assert_eq!(take_prompt(&mut argv, HELP).unwrap(), Some("hello".into()));
        assert_eq!(argv, args(&["kimi", "--model", "fixture"]));
        let mut argv = args(&["kimi", "The"]);
        assert_eq!(take_prompt(&mut argv, HELP).unwrap(), Some("The".into()));
    }
    #[test]
    fn native_commands_and_noninteractive_requests_stay_native() {
        for tail in [
            vec!["session", "list"],
            vec!["--prompt", "hello"],
            vec!["-p", "hello"],
            vec!["--session", "id"],
            vec!["--model", "fixture"],
            vec!["--unknown", "hello"],
            vec!["--help"],
            vec!["--continue"],
        ] {
            let mut argv = args(&["kimi"]);
            argv.extend(args(&tail));
            let saved = argv.clone();
            assert_eq!(take_prompt(&mut argv, HELP).unwrap(), None, "{tail:?}");
            assert_eq!(argv, saved);
        }
    }
    #[test]
    fn capability_not_version_controls_prompt_translation() {
        for help in [
            HELP,
            include_str!("../../tests/fixtures/kimi-help-2.1.1.txt"),
            include_str!("../../tests/fixtures/kimi-help-2.2.0.txt"),
        ] {
            let mut argv = args(&["kimi", "hello"]);
            assert_eq!(take_prompt(&mut argv, help).unwrap(), Some("hello".into()));
        }
        for help in [
            "unrecognized help",
            "Usage: kimi [options] [PROMPT]\n  --prompt <prompt>  Run non-interactively",
        ] {
            let mut argv = args(&["kimi", "hello"]);
            let saved = argv.clone();
            assert_eq!(take_prompt(&mut argv, help).unwrap(), None);
            assert_eq!(argv, saved);
        }
    }
    #[test]
    fn literal_command_name_and_flag_text_require_delimiter() {
        for text in ["session", "--plan", "The"] {
            let mut argv = args(&["kimi", "--", text]);
            assert_eq!(take_prompt(&mut argv, HELP).unwrap(), Some(text.into()));
            assert_eq!(argv, args(&["kimi"]));
        }
        assert!(take_prompt(&mut args(&["kimi", "one", "two"]), HELP).is_err());
    }
}
