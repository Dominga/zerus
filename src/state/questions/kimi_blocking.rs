//! Positive native tool-call evidence for routing a blocking Kimi question.
//! An active tool or an answerable dialog alone also describes background
//! questions. Missing or mismatched bounded history grants no route authority.
use super::{normalize, string, Value};
use std::collections::BTreeMap;

struct Call {
    turn: u64,
    questions: Value,
    blocking: bool,
}
#[derive(Default)]
pub(super) struct Calls(BTreeMap<String, Call>);

fn turn(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let raw = value.as_str()?;
        let number = raw.parse::<u64>().ok()?;
        (number.to_string() == raw).then_some(number)
    })
}

impl Calls {
    pub(super) fn observe(&mut self, event: &Value) {
        if event["type"] != "context.append_loop_event" || event["agentId"] != "main" {
            return;
        }
        let call = &event["event"];
        if call["type"] != "tool.call" {
            return;
        }
        let id = string(call, "toolCallId");
        // A later reuse, malformed call or different tool retires older evidence.
        self.0.remove(id);
        if id.is_empty() || call["name"] != "AskUserQuestion" {
            return;
        }
        let (Some(turn), Ok(questions)) =
            (turn(&call["turnId"]), normalize(&call["args"]["questions"]))
        else {
            return;
        };
        let background = match call["args"].get("background") {
            None => false, // The native tool schema defaults to foreground.
            Some(Value::Bool(value)) => *value,
            _ => return,
        };
        self.0.insert(
            id.into(),
            Call {
                turn,
                questions,
                blocking: !background,
            },
        );
    }

    pub(super) fn blocking(&self, request: &Value, questions: &Value) -> bool {
        let id = string(request, "toolCallId");
        request["agentId"] == "main"
            && request["type"] == "interaction.request"
            && request["kind"] == "question"
            && request["request"]["toolCallId"] == id
            && self.0.get(id).is_some_and(|call| {
                call.blocking
                    && turn(&request["request"]["turnId"]) == Some(call.turn)
                    && call.questions == *questions
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn items() -> Value {
        json!([{"question":"Which release?","options":[{"label":"Stable"},{"label":"Preview"}]}])
    }
    fn call() -> Value {
        json!({"type":"context.append_loop_event","agentId":"main","event":{
            "type":"tool.call","toolCallId":"call","turnId":"0","name":"AskUserQuestion","args":{"questions":items()}}})
    }
    fn request() -> Value {
        json!({"type":"interaction.request","agentId":"main","kind":"question","toolCallId":"call",
            "request":{"turnId":0,"toolCallId":"call","questions":items()}})
    }
    #[test]
    fn only_the_exact_foreground_native_call_proves_blocking() {
        let questions = normalize(&items()).unwrap();
        let mut calls = Calls::default();
        assert!(!calls.blocking(&request(), &questions));
        calls.observe(&call());
        assert!(calls.blocking(&request(), &questions));
        for (key, value) in [
            ("agentId", json!("other")),
            ("toolCallId", json!("other")),
            ("kind", json!("approval")),
        ] {
            let mut changed = request();
            changed[key] = value;
            assert!(!calls.blocking(&changed, &questions));
        }
        for (key, value) in [("turnId", json!(1)), ("toolCallId", json!("other"))] {
            let mut changed = request();
            changed["request"][key] = value;
            assert!(!calls.blocking(&changed, &questions));
        }
        assert!(!calls.blocking(
            &request(),
            &normalize(
                &json!([{"question":"Changed?","options":[{"label":"Stable"},{"label":"Preview"}]}])
            )
            .unwrap()
        ));
        for background in [json!(true), json!(null), json!("false"), json!({})] {
            let mut changed = call();
            changed["event"]["args"]["background"] = background;
            calls.observe(&changed);
            assert!(!calls.blocking(&request(), &questions));
        }
        let mut foreground = call();
        foreground["event"]["args"]["background"] = json!(false);
        calls.observe(&foreground);
        assert!(calls.blocking(&request(), &questions));
    }
    #[test]
    fn later_calls_cannot_adopt_an_older_blocking_witness() {
        for change in [
            json!({"name":"Shell"}),
            json!({"turnId":"bad"}),
            json!({"args":{"questions":[]}}),
            json!({"args":{"questions":items(),"background":true}}),
        ] {
            let mut calls = Calls::default();
            calls.observe(&call());
            let mut later = call();
            for (key, value) in change.as_object().unwrap() {
                later["event"][key] = value.clone();
            }
            calls.observe(&later);
            assert!(!calls.blocking(&request(), &normalize(&items()).unwrap()));
        }
    }
}
