//! Public form metadata is only a lookup hint. Native message/tool state and
//! the complete generated question fields must independently agree.
use super::*;
use crate::state::opencode::{session_path, Native};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const LOOKUPS: usize = 8;

fn reference(form: &Value) -> Option<(&str, &str)> {
    let message = form["metadata"]["tool"]["messageID"].as_str()?;
    let call = form["metadata"]["tool"]["id"].as_str()?;
    (form["metadata"]["kind"] == "question"
        && message.len() > 4
        && super::super::native_id(message, "msg_")
        && call.len() > 5
        && super::super::native_id(call, "call_"))
    .then_some((message, call))
}

/// The exact form projection emitted by the native `question` tool. Unknown
/// input or field semantics stay manual, rather than being silently dropped.
fn fields(input: &Value) -> Option<Value> {
    if input.as_object()?.len() != 1 {
        return None;
    }
    let questions = input["questions"].as_array()?;
    if questions.is_empty() || questions.len() > MAX_FIELDS {
        return None;
    }
    let mut fields = Vec::new();
    for (i, q) in questions.iter().enumerate() {
        if q.as_object()?
            .keys()
            .any(|k| !["question", "header", "options", "multiple", "custom"].contains(&k.as_str()))
        {
            return None;
        }
        let prompt = q["question"].as_str()?;
        let header = q["header"].as_str()?;
        let multiple = match q.get("multiple") {
            None => false,
            Some(v) => v.as_bool()?,
        };
        let custom = match q.get("custom") {
            None => true,
            Some(v) => v.as_bool()?,
        };
        let options = q["options"].as_array()?;
        if options.len() > MAX_OPTIONS {
            return None;
        }
        let mut choices = Vec::new();
        for option in options {
            if option
                .as_object()?
                .keys()
                .any(|k| !["label", "description"].contains(&k.as_str()))
            {
                return None;
            }
            let label = option["label"].as_str()?;
            let mut choice = json!({"label":label,"value":label});
            if let Some(description) = option.get("description") {
                choice["description"] = json!(description.as_str()?);
            }
            choices.push(choice);
        }
        fields.push(
            json!({"key":format!("q{i}"),"title":header,"description":prompt,
            "type":if multiple {"multiselect"} else {"string"},"custom":custom,"options":choices}),
        );
    }
    let fields = json!(fields);
    (fields.to_string().len() <= MAX_TEXT).then_some(fields)
}

fn matches(form: &Value, message: &Value) -> bool {
    let Some((id, call)) = reference(form) else {
        return false;
    };
    if message["id"] != id
        || message["type"] != "assistant"
        || message
            .get("sessionID")
            .is_some_and(|cid| cid != &form["sessionID"])
        || message["time"]
            .get("completed")
            .is_some_and(|v| !v.is_null())
        || form["title"] != "Questions"
    {
        return false;
    }
    let Some(parts) = message["content"].as_array().filter(|p| p.len() <= 64) else {
        return false;
    };
    let mut matching = parts.iter().filter(|p| p["id"] == call);
    let Some(tool) = matching.next() else {
        return false;
    };
    matching.next().is_none()
        && tool["type"] == "tool"
        && tool["name"] == "question"
        && tool["state"]["status"] == "running"
        && tool["time"]["ran"]
            .as_f64()
            .is_some_and(|at| at.is_finite() && at > 0.0)
        && tool["time"].get("completed").is_none_or(Value::is_null)
        && fields(&tool["state"]["input"]).is_some_and(|fields| fields == form["fields"])
}

/// At most eight point reads in one shared 250ms budget; no history scan or
/// cache can grant authority. Duplicated form references are ambiguous.
pub(in crate::state::opencode) fn qualify(
    native: &dyn Native,
    record: &Value,
    forms: &Value,
) -> HashMap<String, Value> {
    let mut proofs = HashMap::new();
    let Some(pending) = forms.as_array().filter(|f| f.len() <= LOOKUPS) else {
        return proofs;
    };
    let deadline = Instant::now() + Duration::from_millis(250);
    for form in pending {
        if Instant::now() >= deadline
            || form["sessionID"] != record["conversation_id"]
            || !super::super::native_id(string(form, "id"), "frm_")
            || !supported(form)
        {
            continue;
        }
        let Some((message, call)) = reference(form) else {
            continue;
        };
        if pending
            .iter()
            .filter(|f| reference(f) == Some((message, call)))
            .count()
            != 1
        {
            continue;
        }
        let path = session_path(record, &format!("/message/{message}"));
        let Ok(response) = native.raw_until("GET", &path, None, deadline) else {
            continue;
        };
        if response.status == 200 && matches(form, &response.body["data"]) {
            proofs.insert(
                string(form, "id").to_owned(),
                json!({"message_id":message,"tool_call_id":call}),
            );
        }
    }
    proofs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::opencode::service::Response;
    use std::cell::RefCell;

    fn fixture() -> (Value, Value) {
        let forms: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/opencode2/forms_question.json"
        ))
        .unwrap();
        let messages: Value = serde_json::from_str(include_str!(
            "../../../../tests/fixtures/opencode2/messages_question.json"
        ))
        .unwrap();
        let form = forms["data"][0].clone();
        let mut message = messages["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["id"] == form["metadata"]["tool"]["messageID"])
            .unwrap()
            .clone();
        message["time"].as_object_mut().unwrap().remove("completed");
        message["content"][0]["time"]
            .as_object_mut()
            .unwrap()
            .remove("completed");
        message["content"][0]["state"]["status"] = json!("running");
        (form, message)
    }
    #[test]
    fn only_exact_running_question_input_matches_the_complete_form() {
        let (form, message) = fixture();
        assert!(matches(&form, &message));
        for (pointer, value) in [
            ("/id", json!("msg_other")),
            ("/type", json!("user")),
            ("/time/completed", json!(1)),
            ("/content/0/name", json!("shell")),
            ("/content/0/id", json!("call_other")),
            ("/content/0/state/status", json!("completed")),
            (
                "/content/0/state/input/questions/0/question",
                json!("Different question"),
            ),
            (
                "/content/0/state/input/questions/0/options/0/label",
                json!("Different choice"),
            ),
        ] {
            let mut changed = message.clone();
            if pointer == "/time/completed" {
                changed["time"]["completed"] = value;
            } else {
                *changed.pointer_mut(pointer).unwrap() = value;
            }
            assert!(!matches(&form, &changed), "{pointer}");
        }
        for (key, value) in [
            ("title", json!("Forged title")),
            ("fields", json!([])),
            (
                "metadata",
                json!({"kind":"question","tool":{"messageID":"msg_absent","id":"call_absent"}}),
            ),
        ] {
            let mut changed = form.clone();
            changed[key] = value;
            assert!(!matches(&changed, &message), "{key}");
        }
        let mut duplicate = message.clone();
        duplicate["content"]
            .as_array_mut()
            .unwrap()
            .push(message["content"][0].clone());
        assert!(!matches(&form, &duplicate));
        let mut extra = form.clone();
        extra["fields"][0]["hidden"] = json!(true);
        assert!(!matches(&extra, &message));
    }

    struct Api {
        response: Response,
        paths: RefCell<Vec<String>>,
    }
    impl Native for Api {
        fn raw(&self, method: &str, path: &str, _: Option<&Value>) -> Result<Response> {
            assert_eq!(method, "GET");
            self.paths.borrow_mut().push(path.into());
            Ok(self.response.clone())
        }
    }
    #[test]
    fn qualification_uses_only_bounded_exact_session_point_reads_and_refuses_ambiguity() {
        let (form, message) = fixture();
        let api = Api {
            response: Response {
                status: 200,
                body: json!({"data":message}),
            },
            paths: Default::default(),
        };
        let record = json!({"conversation_id":form["sessionID"]});
        let proof = qualify(&api, &record, &json!([form]));
        assert_eq!(proof.len(), 1);
        assert_eq!(
            api.paths.borrow().as_slice(),
            &[format!(
                "/api/session/{}/message/{}",
                string(&record, "conversation_id"),
                string(&message, "id")
            )]
        );
        api.paths.borrow_mut().clear();
        let mut clone = form.clone();
        clone["id"] = json!("frm_clone");
        assert!(qualify(&api, &record, &json!([form, clone])).is_empty());
        assert!(qualify(&api, &record, &json!(vec![form.clone(); LOOKUPS + 1])).is_empty());
        assert!(qualify(
            &api,
            &json!({"conversation_id":"ses_foreign"}),
            &json!([form])
        )
        .is_empty());
        assert!(api.paths.borrow().is_empty());
        let missing = Api {
            response: Response {
                status: 404,
                body: json!({"_tag":"MessageNotFoundError"}),
            },
            paths: Default::default(),
        };
        assert!(qualify(&missing, &record, &json!([form])).is_empty());
    }
}
