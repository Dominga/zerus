//! Kimi's --prompt is a non-interactive mode, not an initial TUI message.
//! Keep a launch message out of the resume recipe and use ordinary guarded
//! input once, in the lifetime of the supervisor that created this run.
use super::*;
use std::process::{Child, ExitStatus};
use std::time::Duration;

pub(super) struct Prompt {
    text: String,
    request_id: String,
    spent: bool,
}

impl Prompt {
    pub(super) fn new(text: String) -> Self {
        Self {
            text,
            request_id: uuid::Uuid::new_v4().to_string(),
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

/// This loop never survives or restarts its supervisor, and it never retries a
/// submit call, including definite refusals. Readiness polling sends no keys.
pub(super) fn wait(
    child: &mut Child,
    name: &str,
    run: &str,
    prompt: Option<&mut Prompt>,
) -> Result<ExitStatus> {
    let Some(prompt) = prompt.filter(|prompt| !prompt.spent) else {
        return supervisor::wait(child);
    };
    let text = &prompt.text;
    let request_id = &prompt.request_id;
    let conversation = {
        let _guard = lock(None)?;
        let mut record = read_run(name, run)?.ok_or("Kimi launch identity changed")?;
        let conversation = ["conversation_id", "expected_id", "requested_id"]
            .iter()
            .map(|key| string(&record, key))
            .find(|value| !value.is_empty())
            .unwrap_or("")
            .to_owned();
        record["launch_prompt"] = json!({"request_id":request_id,"status":"pending","text":text});
        write(&mut record)?;
        conversation
    };
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            // wait() also clears the platform's shutdown child registration.
            let _ = supervisor::wait(child)?;
            return Ok(status);
        }
        if supervisor::shutdown_requested() {
            return supervisor::wait(child);
        }
        let ready = {
            let _guard = lock(None)?;
            let Some(mut record) = read_run(name, run)? else {
                prompt.spent = true;
                drop(_guard);
                return supervisor::wait(child);
            };
            if record["launch_prompt"]["status"] != "pending" {
                prompt.spent = true;
                drop(_guard);
                return supervisor::wait(child);
            }
            if string(&record, "conversation_id") != conversation
                && !string(&record, "conversation_id").is_empty()
                || record["input_pending_at"].as_f64().unwrap_or(0.0) > 0.0
                || record.get("pausing").is_some()
            {
                prompt.spent = true;
                record["launch_prompt"]["status"] = json!("cancelled");
                write(&mut record)?;
                false
            } else if input::launch_ready(name, run, &conversation).is_ok() {
                // Durable before submission. No resume, restart or failed
                // delivery may replay this attempt. input::submit additionally
                // reserves its ordinary receipt before any terminal mutation.
                record["launch_prompt"]["status"] = json!("attempted");
                prompt.spent = true;
                write(&mut record)?;
                true
            } else {
                false
            }
        };
        if ready {
            let payload = json!({"request_id":request_id,"text":text,
                "expected_run_id":run,"expected_conversation_id":conversation});
            let result = input::submit(name, payload.to_string().as_bytes(), None);
            let _guard = lock(None)?;
            if let Some(mut record) = read_run(name, run)? {
                record["launch_prompt"]["status"] = json!(if result.is_ok() {
                    "submitted"
                } else {
                    "stopped"
                });
                if let Err(error) = result {
                    eprintln!("hgs: Kimi initial message was not confirmed: {error}. Review before sending again.");
                    record["launch_prompt"]["error"] = json!(error);
                }
                write(&mut record)?;
            }
            drop(_guard);
            return supervisor::wait(child);
        }
        std::thread::sleep(Duration::from_millis(250));
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
