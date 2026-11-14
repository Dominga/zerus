//! Team delegations have their own entry point and categories. The ordinary
//! automatic-tool admission never admits either of these kinds.
use super::*;

#[derive(Clone, Copy)]
#[derive(PartialEq)]
pub(crate) enum TeamKind {
    FolderTrust,
    Resume,
}

pub(crate) fn begin_team(db: &mut Connection, payload: &Value, kind: TeamKind) -> Result<Value> {
    let mut payload = payload.clone();
    let answers = match kind {
        TeamKind::FolderTrust => {
            let Some(answers) = folder_answers(&payload) else {
                return Ok(json!({"eligible":false}));
            };
            payload["categories"] = json!(["team_folder_trust"]);
            answers
        }
        TeamKind::Resume => {
            if payload["question"]["source"] != "team_resume" {
                return Ok(json!({"eligible":false}));
            }
            payload["categories"] = json!(["team_resume"]);
            json!([])
        }
    };
    reserve(db, &payload, answers, matches!(kind, TeamKind::FolderTrust))
}

fn folder_answers(payload: &Value) -> Option<Value> {
    let q = &payload["question"];
    let (source, transport, id, label) = match payload["provider"].as_str()? {
        "claude" => (
            "claude_folder_trust",
            "claude_tui",
            "trust_1",
            "Yes, I trust this folder",
        ),
        "codex" => (
            "codex_folder_trust",
            "codex_tui",
            "trust_0",
            "Trust and continue",
        ),
        "kimi" => (
            "kimi_folder_trust",
            "kimi_tui",
            "trust_0",
            "Trust this folder",
        ),
        _ => return None,
    };
    if q["source"] != source
        || q["answer_transport"] != transport
        || q["can_answer"] != true
        // A startup card has no conversation yet: the producers copy the
        // record's null, which is the same identity as "" (zerus-63rru.60).
        || crate::state::native_identity(q, "conversation_id") != Some("")
    {
        return None;
    }
    let items = q["questions"].as_array()?;
    if items.len() != 1
        || items[0]["id"] != "trust"
        || (items[0]["question"] != "Trust this folder?"
            && !(q["source"] == "codex_folder_trust"
                && items[0]["question"] == "Trust this Git repository?"))
        || items[0]["multi_select"] == true
        || items[0]["allow_other"] == true
    {
        return None;
    }
    let options = items[0]["options"].as_array()?;
    if options.iter().filter(|o| o["id"] == id).count() != 1
        || options.len() != 2
        || options
            .iter()
            .filter(|o| {
                o["id"] == id
                    && (o["label"] == label
                        || source == "kimi_folder_trust" && o["label"] == "Trust and continue")
            })
            .count()
            != 1
    {
        return None;
    }
    Some(json!([{"question_id":"trust","selected_option_ids":[id],"text":""}]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn team_kinds_are_distinct_and_ordinary_admission_stays_closed() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(&dir.path().join("audit")).unwrap();
        // The card Claude's producer writes for a new run (zerus-63rru.60).
        let q = crate::state::trust_fixture::startup_card("claude");
        let mut p = json!({"host":"machine-1","session":"claude/example/member","provider":"claude","question":q});
        assert_eq!(begin(&mut db, &p).unwrap()["eligible"], false);
        let trust = begin_team(&mut db, &p, TeamKind::FolderTrust).unwrap();
        assert_eq!(trust["fresh"], true);
        assert_eq!(
            begin_team(&mut db, &p, TeamKind::FolderTrust).unwrap()["fresh"],
            false
        );
        p["question"]["source"] = json!("team_resume");
        p["question"]["question_id"] = json!("resume:run-2");
        p["question"]["conversation_id"] = json!("conversation-1");
        assert_eq!(begin(&mut db, &p).unwrap()["eligible"], false);
        assert_eq!(
            begin_team(&mut db, &p, TeamKind::Resume).unwrap()["fresh"],
            true
        );
        let rows = list(&db, &json!({})).unwrap();
        assert_eq!(rows["entries"][0]["categories"], json!(["team_resume"]));
        assert_eq!(
            rows["entries"][1]["categories"],
            json!(["team_folder_trust"])
        );
        finish(
            &db,
            &json!({"id":trust["id"],"request_id":trust["request_id"],"status":"unknown"}),
        )
        .unwrap();
        drop(db);
        let mut db = open(&dir.path().join("audit")).unwrap();
        p["question"] = q;
        assert_eq!(
            begin_team(&mut db, &p, TeamKind::FolderTrust).unwrap()["fresh"],
            false
        );
    }

    fn payload(agent: &str, q: &Value) -> Value {
        json!({"host":"machine-1","session":format!("{agent}/example/member"),"provider":agent,"question":q})
    }

    /// The startup card each producer writes is the shape this audit reads
    /// (zerus-63rru.60): a changed shape fails here before a real member is
    /// refused. The conversation is null, as every producer copies it.
    #[test]
    fn each_producers_startup_card_is_the_shape_the_audit_reads() {
        for (agent, option) in [
            ("claude", "trust_1"),
            ("codex", "trust_0"),
            ("kimi", "trust_0"),
        ] {
            let q = crate::state::trust_fixture::startup_card(agent);
            assert!(q["conversation_id"].is_null(), "{agent}");
            let mut keys: Vec<&String> = q.as_object().unwrap().keys().collect();
            keys.sort();
            assert_eq!(
                keys,
                [
                    "agent_id",
                    "answer_transport",
                    "answer_unavailable_reason",
                    "can_answer",
                    "conversation_id",
                    "created_at",
                    "question_hash",
                    "question_id",
                    "questions",
                    "run_id",
                    "source",
                    "tool_call_id"
                ],
                "{agent}"
            );
            assert_eq!(
                folder_answers(&payload(agent, &q)),
                Some(json!([{"question_id":"trust","selected_option_ids":[option],"text":""}])),
                "{agent}"
            );
        }
    }

    /// One question, one reservation: a startup card's null and "" are one
    /// identity. The row keeps the card as it was seen.
    #[test]
    fn a_startup_card_is_reserved_once_and_saved_as_it_was_seen() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(&dir.path().join("audit")).unwrap();
        for agent in ["claude", "codex", "kimi"] {
            let q = crate::state::trust_fixture::startup_card(agent);
            assert_eq!(
                begin(&mut db, &payload(agent, &q)).unwrap()["eligible"],
                false
            );
            let first = begin_team(&mut db, &payload(agent, &q), TeamKind::FolderTrust).unwrap();
            assert_eq!(
                (first["eligible"].clone(), first["fresh"].clone()),
                (json!(true), json!(true)),
                "{agent}"
            );
            let mut empty = q.clone();
            empty["conversation_id"] = json!("");
            let second =
                begin_team(&mut db, &payload(agent, &empty), TeamKind::FolderTrust).unwrap();
            assert_eq!(second["fresh"], false, "{agent}");
            assert_eq!(second["id"], first["id"], "{agent}");
            let saved: String = db
                .query_row(
                    "SELECT question FROM attempts WHERE id=?1",
                    [first["id"].as_i64().unwrap()],
                    |r| r.get(0),
                )
                .unwrap();
            let saved: Value = serde_json::from_str(&saved).unwrap();
            assert_eq!(
                saved["question"], q,
                "{agent}: the row keeps the card as seen"
            );
            assert!(saved["question"]["conversation_id"].is_null(), "{agent}");
        }
    }

    /// Another conversation, or one of another type, is no startup card.
    #[test]
    fn a_card_with_a_conversation_or_a_bad_identity_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(&dir.path().join("audit")).unwrap();
        for conversation in [json!("conversation-1"), json!(7), json!({})] {
            let mut q = crate::state::trust_fixture::startup_card("claude");
            q["conversation_id"] = conversation.clone();
            assert_eq!(
                begin_team(&mut db, &payload("claude", &q), TeamKind::FolderTrust).unwrap()
                    ["eligible"],
                false,
                "{conversation}"
            );
        }
        let mut q = crate::state::trust_fixture::startup_card("claude");
        q.as_object_mut().unwrap().remove("conversation_id");
        assert_eq!(
            begin_team(&mut db, &payload("claude", &q), TeamKind::FolderTrust).unwrap()["eligible"],
            false
        );
    }
}
