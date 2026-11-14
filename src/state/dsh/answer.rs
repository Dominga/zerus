//! One durable person answer. The UUID records an attempt, not authority to
//! answer: the original binding, live question and host generation still do.
use super::*;

const UNKNOWN: &str = "delivery uncertain: this answer request may already be in DeepSeek; do not resend automatically";

pub(super) fn read() -> Result<(Value, String)> {
    let mut bytes = Vec::new();
    io::stdin()
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 128 * 1024 {
        return Err("answer payload exceeds128 KiB".into());
    }
    let request = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    Ok((request, format!("{:x}", Sha256::digest(&bytes))))
}

fn previous(
    path: &Path,
    name: &str,
    request: &Value,
    id: &str,
    digest: &str,
) -> Result<Option<Value>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let receipt: Value = serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    if receipt["digest"] != digest
        || receipt["name"] != name
        || receipt["request_id"] != id
        || receipt["source"] != "dsh_native"
        || receipt["run_id"] != request["expected_run_id"]
        || receipt["conversation_id"] != request["expected_conversation_id"]
        || receipt["question_id"] != request["question_id"]
        || receipt["question_hash"] != request["expected_question_hash"]
    {
        return Err("answer request ID was already used for different content".into());
    }
    if receipt["status"] == "answered" {
        return Ok(Some(receipt));
    }
    // A crashed original leaves in_progress. A failed settlement may leave
    // that same reservation. Neither authorizes a second native delivery.
    Err(receipt["error"]
        .as_str()
        .filter(|error| error.starts_with("delivery uncertain:"))
        .unwrap_or(UNKNOWN)
        .to_owned())
}

fn prepare(record: &Value, request: &Value, inspected: &Value) -> Result<Vec<Value>> {
    let cards = question_cards(record, inspected);
    let card = cards
        .iter()
        .find(|q| {
            q["question_id"] == request["question_id"]
                && q["question_hash"] == request["expected_question_hash"]
        })
        .ok_or("This question changed or is no longer pending")?;
    let mut answers = Vec::new();
    for answer in request["answers"]
        .as_array()
        .ok_or("Answers are required")?
    {
        let item = card["questions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["id"] == answer["question_id"])
            .ok_or("Unknown question")?;
        let mut labels = Vec::new();
        for id in answer["selected_option_ids"]
            .as_array()
            .ok_or("Invalid selected options")?
        {
            let option = item["options"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == *id)
                .ok_or("Unknown answer option")?;
            labels.push(option["label"].clone());
        }
        if item["allow_other"] != true && !string(answer, "text").is_empty() {
            return Err("Choose an approval outcome".into());
        }
        answers.push(
            json!({"id":answer["question_id"],"selected":labels,"custom":string(answer,"text")}),
        );
    }
    Ok(answers)
}

pub(super) fn run(name: &str, record: &Value, request: &Value, digest: &str) -> Result<Value> {
    let id = uuid::Uuid::parse_str(string(request, "request_id"))
        .map_err(|_| "Invalid request ID")?
        .to_string();
    let path = absolute_root()?
        .join("question_receipts")
        .join(format!("{id}.json"));
    let _wait = receipt_index::QuestionAdmission::new();
    {
        let _guard = lock(None)?;
        if let Some(receipt) = previous(&path, name, request, &id, digest)? {
            return Ok(receipt);
        }
    }
    // A duplicate reads its original outcome even after the run or pending
    // card changes. Only a new attempt must prove the current live binding.
    if request["expected_run_id"] != record["run_id"]
        || request["expected_conversation_id"] != record["conversation_id"]
    {
        return Err("Session changed; refresh before trying again".into());
    }
    let inspected = rpc(
        record,
        "inspect",
        json!({"sessionId":record["conversation_id"]}),
    )?;
    let answers = prepare(record, request, &inspected)?;
    let mut receipt = json!({"status":"in_progress","request_id":id,"name":name,
        "run_id":record["run_id"],"conversation_id":record["conversation_id"],
        "question_id":request["question_id"],"question_hash":request["expected_question_hash"],
        "source":"dsh_native","digest":digest,"answers":request["answers"],"submitted_at":now()});
    {
        // Share the short canonical reservation lock with Claude/Kimi. A
        // concurrent different session/provider cannot overwrite this UUID.
        let _guard = lock(None)?;
        if let Some(receipt) = previous(&path, name, request, &id, digest)? {
            return Ok(receipt);
        }
        receipt_index::persist(&path, &receipt)?;
    }
    let delivery = rpc(record, "answer", json!({"sessionId":record["conversation_id"],
        "id":request["question_id"],"generation":inspected["generation"],"answers":answers,"requestId":id}))
        .and_then(|reply| {
            // Older resident bridges return only accepted:true. An optional
            // UUID from a newer bridge must identify this exact attempt.
            if reply["accepted"] != true
                || (reply.get("requestId").is_some() && reply["requestId"] != id)
            {
                return Err("DeepSeek answer acknowledgement did not match".into());
            }
            Ok(())
        });
    if let Err(error) = delivery {
        let error = format!("delivery uncertain: DeepSeek answer outcome is unknown: {error}");
        // Keep the canonical pending status understood by existing desktop
        // and phone readers; the native outcome and error are explicit.
        receipt["outcome"] = json!("unknown");
        receipt["error"] = json!(error);
        // If settlement fails, the original durable reservation still blocks
        // replay. A plain native error is not proof of a pre-delivery refusal.
        let _ = receipt_index::persist(&path, &receipt);
        return Err(error);
    }
    receipt["status"] = json!("answered");
    receipt["answered_at"] = json!(now());
    receipt_index::persist(&path, &receipt).map_err(|error| {
        format!("delivery uncertain: cannot save DeepSeek answer outcome: {error}")
    })?;
    Ok(receipt)
}
