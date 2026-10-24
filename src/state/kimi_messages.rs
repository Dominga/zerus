//! Recover public assistant text, including commentary emitted between tools. The wire
//! file belongs to the exact recorded conversation; reading it never sends input.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Seek, SeekFrom};
use sha2::{Digest, Sha256};

const MAX_BYTES: u64 = 8 * 1024 * 1024;

fn assistant_message(event: &Value) -> Option<Value> {
    if string(event, "type") != "agent.message.appended"
        || !["", "main"].contains(&string(event, "agentId"))
    {
        return None;
    }
    let envelope = &event["message"];
    let message = &envelope["message"];
    if string(message, "role") != "assistant"
        || !["completed", "stop", "end_turn", "tool_use", "tool_calls"]
            .contains(&string(&envelope["meta"]["finish"], "finishReason"))
    {
        return None;
    }
    let parts = message["content"].as_array()?;
    let text = parts
        .iter()
        .filter(|part| string(part, "type") == "text")
        .map(|part| string(part, "text"))
        .collect::<Vec<_>>()
        .join("\n");
    let at = event["time"].as_f64()? / 1000.0;
    if text.trim().is_empty() || at <= 0.0 {
        return None;
    }
    Some(json!({"type":"AgentMessage", "source":"kimi_wire", "at":at,
        "message_id":envelope["meta"]["messageId"], "agent_id":"",
        "message_phase":if ["completed", "stop", "end_turn"].contains(&string(&envelope["meta"]["finish"], "finishReason")) {"final"} else {"commentary"},
        "detail":journal::clipped(&json!(text), 32000)}))
}

// Kimi's live loop records text as it becomes visible in the TUI. The older
// agent.message.appended envelopes can arrive together only at the end of the
// turn. step.end links those copies by messageId, without guessing from text or
// the (late) envelope timestamp. Retain the original text position in the turn.
fn wire_messages(bytes: &[u8], clipped_start: bool) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    let mut envelopes = Vec::new();
    let mut steps = BTreeMap::<String, usize>::new();
    let mut finishes = BTreeMap::<String, (String, String)>::new();
    let mut parts = BTreeSet::new();
    for (index, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if (clipped_start && index == 0) || line.len() > 1024 * 1024 || !line.ends_with(b"\n") {
            continue;
        }
        if ![
            b"agent.message.appended".as_slice(),
            b"context.append_loop_event".as_slice(),
        ]
        .iter()
        .any(|kind| line.windows(kind.len()).any(|part| part == *kind))
        {
            continue;
        }
        let Ok(event) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if !["", "main"].contains(&string(&event, "agentId")) {
            continue;
        }
        if let Some(message) = assistant_message(&event) {
            envelopes.push(message);
            continue;
        }
        if string(&event, "type") != "context.append_loop_event" {
            continue;
        }
        let item = &event["event"];
        if string(item, "type") == "step.end" && !string(item, "uuid").is_empty() {
            finishes.insert(
                string(item, "uuid").into(),
                (
                    string(item, "messageId").into(),
                    string(item, "finishReason").into(),
                ),
            );
            continue;
        }
        if string(item, "type") != "content.part" || string(&item["part"], "type") != "text" {
            continue;
        }
        let step = string(item, "stepUuid");
        let part = string(item, "uuid");
        let text = string(&item["part"], "text");
        let at = event["time"].as_f64().unwrap_or(0.0) / 1000.0;
        if step.is_empty()
            || part.is_empty()
            || text.trim().is_empty()
            || at <= 0.0
            || !parts.insert(part.to_owned())
        {
            continue;
        }
        if let Some(index) = steps.get(step) {
            let combined = format!("{}\n{}", string(&messages[*index], "detail"), text);
            messages[*index]["detail"] = json!(journal::clipped(&json!(combined), 32000));
        } else {
            steps.insert(step.into(), messages.len());
            messages.push(json!({"type":"AgentMessage", "source":"kimi_wire", "at":at,
                "message_id":format!("kimi-step:{step}"), "agent_id":"", "message_phase":"commentary",
                "detail":journal::clipped(&json!(text),32000)}));
        }
    }
    let mut mirrored = BTreeSet::new();
    for (step, index) in steps {
        if let Some((id, finish)) = finishes.get(&step) {
            if !id.is_empty() {
                mirrored.insert(id.clone());
            }
            if ["completed", "stop", "end_turn"].contains(&finish.as_str()) {
                messages[index]["message_phase"] = json!("final");
            }
        }
    }
    for message in envelopes {
        let id = string(&message, "message_id");
        if !id.is_empty() && !mirrored.insert(id.to_owned()) {
            continue;
        }
        messages.push(message);
    }
    messages.sort_by(|a, b| {
        a["at"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&b["at"].as_f64().unwrap_or(0.0))
    });
    if messages.len() > 50 {
        messages.drain(..messages.len() - 50);
    }
    messages
}

// Native turn IDs also identify duplicate callbacks from legacy + current
// tracking registrations. Publish only bounded identity/hash metadata, not the
// injected attachment instructions. Legacy association needs a complete file.
fn wire_turns(bytes: &[u8], clipped_start: bool) -> (Vec<Value>, bool) {
    let mut complete = !clipped_start;
    let mut turns = Vec::new();
    for (index, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        if (clipped_start && index == 0) || line.len() > 1024 * 1024 || !line.ends_with(b"\n") {
            complete = false;
            continue;
        }
        let Ok(event) = serde_json::from_slice::<Value>(line) else { complete = false; continue };
        if string(&event, "type") != "turn.prompt" || !["", "main"].contains(&string(&event, "agentId")) {
            continue;
        }
        let Some(id) = event["turnId"].as_u64().filter(|id| *id <= i64::MAX as u64) else { complete = false; continue };
        let prompt = string(&event, "promptId");
        let origin = string(&event["origin"], "kind");
        let Some(parts) = event["input"].as_array() else { complete = false; continue };
        let at = event["time"].as_f64().unwrap_or(0.0) / 1000.0;
        if prompt.is_empty() || prompt.len() > 160 || !prompt.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || !(1..=40).contains(&origin.len()) || !origin.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
            || !at.is_finite() || at <= 0.0 || parts.is_empty() || parts.len() > 64
            || !parts.iter().all(|part| string(part, "type") == "text" && part["text"].is_string()) {
            complete = false;
            continue;
        }
        // UserPromptSubmit hooks may prepend their own text parts. Hash each
        // native part independently rather than treating injected context as
        // text the person submitted.
        let hashes: Vec<_> = parts.iter().map(|part|
            format!("{:x}", Sha256::digest(string(part, "text").trim().as_bytes()))).collect();
        turns.push(json!({"turn_id":id, "prompt_id":prompt, "at":at,
            "origin":origin, "text_sha256":hashes}));
        if turns.len() > 50 { turns.remove(0); complete = false; }
    }
    (turns, complete)
}

pub(super) fn read(record: &Value) -> Result<Vec<Value>> {
    read_details(record, false).map(|value| value.0)
}

pub(super) fn read_with_turns(record: &Value) -> Result<(Vec<Value>, Vec<Value>, bool)> {
    read_details(record, true)
}

fn read_details(record: &Value, include_turns: bool) -> Result<(Vec<Value>, Vec<Value>, bool)> {
    let mut messages = Vec::new();
    let mut turns = Vec::new();
    let mut complete = false;
    for path in questions::wire_paths(record)? {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let length = file.metadata().map_err(|e| e.to_string())?.len();
        let offset = length.saturating_sub(MAX_BYTES);
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        (&mut file).take(MAX_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        messages.extend(wire_messages(&bytes, offset > 0));
        if include_turns {
            (turns, complete) = wire_turns(&bytes, offset > 0);
            complete &= file.metadata().map_err(|e| e.to_string())?.len() == length;
        }
    }
    Ok((messages, turns, complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_turn(id: u64, prompt: &str) -> Value {
        json!({"type":"turn.prompt", "agentId":"main", "turnId":id,
            "promptId":format!("prompt-{id}"), "origin":{"kind":"user"}, "time":100000,
            "input":[{"type":"text", "text":prompt}]})
    }

    #[test]
    fn attachment_turn_metadata_retains_zero_and_distinct_native_ids_without_prompt_text() {
        let prompt = "[File #1] (Read the attached file at \"/synthetic/request/document.pdf\") Check it";
        let (turns, complete) = wire_turns(&lines(&[user_turn(0, prompt), user_turn(1, prompt)]), false);
        assert!(complete);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0]["turn_id"], 0);
        assert_eq!(turns[1]["turn_id"], 1);
        assert_eq!(turns[0]["text_sha256"], turns[1]["text_sha256"]);
        assert!(!json!(turns).to_string().contains("/synthetic/"));
        let mut injected = user_turn(0, prompt);
        injected["input"].as_array_mut().unwrap().insert(0, json!({"type":"text", "text":"Synthetic memory hook context"}));
        let (turns, complete) = wire_turns(&lines(&[injected]), false);
        assert!(complete);
        assert_eq!(turns[0]["text_sha256"][1], format!("{:x}", Sha256::digest(prompt.as_bytes())));
    }

    #[test]
    fn incomplete_or_unbounded_native_turn_sources_never_admit_legacy_association() {
        let bytes = lines(&[user_turn(0, "Check")]);
        assert!(!wire_turns(&bytes, true).1);
        assert!(!wire_turns(&bytes[..bytes.len() - 1], false).1);
        assert!(!wire_turns(&[bytes.clone(), b"invalid\n".to_vec()].concat(), false).1);
        let many = (0..51).map(|id| user_turn(id, "Check")).collect::<Vec<_>>();
        let (turns, complete) = wire_turns(&lines(&many), false);
        assert_eq!(turns.len(), 50);
        assert!(!complete);
        let mut unsupported = user_turn(0, "Check");
        unsupported["input"] = json!([{"type":"image", "data":"synthetic"}]);
        assert!(!wire_turns(&lines(&[unsupported]), false).1);
        let mut child = user_turn(0, "Check"); child["agentId"] = json!("agent-1");
        assert!(wire_turns(&lines(&[child]), false).0.is_empty());
    }

    #[test]
    fn malformed_native_turn_origins_never_escape_into_metadata() {
        for origin in [json!({"text":"Synthetic private extra prompt"}), json!(["Synthetic private extra prompt"]),
            json!("Synthetic private extra prompt"), json!("x".repeat(41)), json!(true), json!(0), Value::Null] {
            let mut turn = user_turn(0, "Check"); turn["origin"]["kind"] = origin;
            let (turns, complete) = wire_turns(&lines(&[turn]), false);
            assert!(!complete);
            assert!(turns.is_empty());
        }
        let mut turn = user_turn(0, "Check"); turn["promptId"] = json!("Synthetic private extra prompt");
        assert_eq!(wire_turns(&lines(&[turn]), false), (vec![], false));
    }

    fn text_part(step: &str, part: &str, text: &str, time: u64) -> Value {
        json!({"type":"context.append_loop_event", "agentId":"main", "time":time,
            "event":{"type":"content.part", "uuid":part, "stepUuid":step,
                "part":{"type":"text", "text":text}}})
    }

    fn step_end(step: &str, id: &str, finish: &str) -> Value {
        json!({"type":"context.append_loop_event", "agentId":"main", "time":400000,
            "event":{"type":"step.end", "uuid":step, "messageId":id, "finishReason":finish}})
    }

    fn envelope(id: &str, text: &str, finish: &str) -> Value {
        json!({"type":"agent.message.appended", "time":400000,
            "message":{"message":{"role":"assistant", "content":[{"type":"text", "text":text}]},
                "meta":{"messageId":id, "finish":{"finishReason":finish}}}})
    }

    fn lines(events: &[Value]) -> Vec<u8> {
        events
            .iter()
            .map(|event| event.to_string() + "\n")
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn live_text_appears_before_turn_finishes_without_private_parts() {
        let public = text_part(
            "step-17",
            "part-17",
            "Всё собрано. Теперь делаю HTML-файл.",
            100000,
        );
        let mut think = text_part("step-17", "think", "private reasoning", 99000);
        think["event"]["part"]["type"] = json!("think");
        let mut child = text_part("child-step", "child", "child text", 100001);
        child["agentId"] = json!("worker");
        let tool = json!({"type":"context.append_loop_event", "time":100002,
            "event":{"type":"tool.result", "stepUuid":"step-17", "uuid":"result",
                "part":{"type":"text", "text":"tool output"}}});
        let bytes = lines(&[think, child, public.clone(), tool, public]);
        let messages = wire_messages(&bytes, false);
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0]["detail"],
            "Всё собрано. Теперь делаю HTML-файл."
        );
        assert_eq!(messages[0]["at"], 100.0);
        assert_eq!(messages[0]["message_phase"], "commentary");
        assert_eq!(messages[0]["message_id"], "kimi-step:step-17");
    }

    #[test]
    fn delayed_mirrors_keep_original_times_and_distinct_repeated_messages() {
        let bytes = lines(&[
            text_part("first", "p1", "Checking", 100000),
            text_part("first", "p2", "the file", 100100),
            step_end("first", "msg-1", "tool_use"),
            text_part("second", "p3", "Checking\nthe file", 200000),
            step_end("second", "msg-2", "tool_use"),
            text_part("final", "p4", "Done", 300000),
            step_end("final", "msg-3", "stop"),
            envelope("msg-1", "Checking\nthe file", "tool_calls"),
            envelope("msg-2", "Checking\nthe file", "tool_calls"),
            envelope("msg-3", "Done", "completed"),
        ]);
        let messages = wire_messages(&bytes, false);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["detail"], "Checking\nthe file");
        assert_eq!(messages[1]["detail"], messages[0]["detail"]);
        assert_ne!(messages[0]["message_id"], messages[1]["message_id"]);
        assert_eq!(messages[0]["at"], 100.0);
        assert_eq!(messages[1]["at"], 200.0);
        assert_eq!(messages[2]["at"], 300.0);
        assert_eq!(messages[2]["message_phase"], "final");
        assert_eq!(messages[2]["detail"], "Done");
    }

    #[test]
    fn legacy_envelopes_and_partial_tail_are_handled() {
        let legacy = envelope("old-progress", "Legacy progress", "tool_calls");
        let mut bytes = lines(&[
            legacy.clone(),
            legacy,
            envelope("old-final", "Legacy final", "completed"),
        ]);
        // A writer can be partway through the next record during a poll.
        bytes.extend_from_slice(b"{\"type\":\"context.append_loop_event\"");
        let messages = wire_messages(&bytes, false);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["detail"], "Legacy progress");
        assert_eq!(messages[1]["detail"], "Legacy final");
        let mut clipped = b"partial old record\n".to_vec();
        clipped.extend_from_slice(&bytes);
        assert_eq!(wire_messages(&clipped, true), messages);
    }

    #[test]
    fn public_text_in_final_and_tool_messages_only() {
        let mut event = json!({"type":"agent.message.appended","time":1700000000123u64,
            "message":{"message":{"role":"assistant","content":[
                {"type":"think","think":"private reasoning"},
                {"type":"text","text":"All pushed. **343 tests passed.**"}],"toolCalls":[]},
                "meta":{"finish":{"finishReason":"completed"},"messageId":"msg-final"}}});
        let result = assistant_message(&event).unwrap();
        assert_eq!(result["detail"], "All pushed. **343 tests passed.**");
        assert_eq!(result["at"], 1700000000.123);
        event["message"]["message"]["toolCalls"] = json!([{"name":"Bash"}]);
        event["message"]["meta"]["finish"]["finishReason"] = json!("tool_calls");
        let progress = assistant_message(&event).unwrap();
        assert_eq!(progress["detail"], "All pushed. **343 tests passed.**");
        assert_eq!(progress["message_phase"], "commentary");
        event["message"]["message"]["toolCalls"] = json!([]);
        event["agentId"] = json!("child");
        assert!(assistant_message(&event).is_none());
        event["agentId"] = json!("main");
        event["message"]["meta"]["finish"]["finishReason"] = json!("length");
        assert!(assistant_message(&event).is_none());
    }
}
