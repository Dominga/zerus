use super::*;
use serde::{Deserialize, Serialize};
use std::path::Component;
pub(super) const SEED_BYTES: usize = 256 << 10;
// V1 could pair a writer's projection with a different writer's file stamp.
// Never reuse those rows: bounded repair must observe the canonical bytes.
const SEED_VERSION: u64 = 2;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Key(pub String);
impl Key {
    pub fn at(root: &Path, path: &Path) -> Option<Self> {
        let relative = path.strip_prefix(root).ok()?;
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return None;
        }
        let folder = relative.parent()?.to_str()?.replace('\\', "/");
        if !storage::BINDING_DIRS.contains(&folder.as_str()) {
            return None;
        }
        let stem = relative.file_stem()?.to_str()?;
        if relative.extension()? != "json" {
            return None;
        }
        let valid = if folder.ends_with("sessions") {
            session_id::canonical(stem)
        } else {
            stem.len() == 64
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        valid.then(|| Self(relative.to_str().unwrap().replace('\\', "/")))
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            Self::at(Path::new(""), Path::new(&self.0)).as_ref() == Some(self),
            "Invalid display source"
        );
        Ok(())
    }
    pub fn matches(&self, record: &Value) -> bool {
        let path = Path::new(&self.0);
        let id = path.file_stem().and_then(|v| v.to_str()).unwrap_or("");
        record["version"] == 1
            && !string(record, "name").is_empty()
            && if self.0.contains("sessions/") {
                storage::is_id_layout_record(record, id)
            } else {
                !storage::in_id_layout(record) && storage::label_key(string(record, "name")) == id
            }
    }
}
/// Pure, bounded public inputs. journal::summary does native/journal I/O and
/// must not be called here. No shell output, transcript, consent or tool args.
pub(super) struct Seed;
impl Seed {
    pub fn encode(key: &Key, record: &Value) -> anyhow::Result<String> {
        anyhow::ensure!(
            key.matches(record),
            "Display record identity/layout mismatch"
        );
        anyhow::ensure!(
            record.get("layout_transition").is_none(),
            "Legacy identity transition requires canonical reconciliation"
        );
        let mut out = serde_json::Map::new();
        for key in [
            "version",
            "layout",
            "name",
            "journal_name",
            "session_id",
            "run_id",
            "conversation_id",
            "launch_id",
            "launch_label",
            "terminal_name",
            "agent",
            "backend",
            "activity",
            "phase",
            "state",
            "conversation_state",
            "created",
            "updated",
            "pid",
            "process_start",
            "supervisor_pid",
            "supervisor_start",
            "cwd",
            "launch_dir",
            "model",
            "effort",
            "last_event_at",
            "turn_started",
            "last_main_progress_at",
            "compaction_started",
            "attention_started",
            "main_done",
            "paused",
            "starting",
            "clearing",
            "resuming",
            "rename_shadow",
            "rename_target",
            "kimi_native_roster",
            "subagent_groups_complete",
            "subagents_completed_pruned",
            "subagents_ended_pruned",
            "attention_id",
            "reply_id",
            "last_reply_at",
            "last_error",
            "completion",
            "startup_kind",
            "cache_hint",
            "context_window",
            "context_tokens",
            "context_percent",
            "goal",
            "goal_progress",
            "account_id",
            "agent_home",
            "supervisor",
        ] {
            if let Some(value) = record.get(key) {
                bounded(value)?;
                out.insert(key.into(), value.clone());
            }
        }
        for key in ["prompt", "last_message"] {
            if record.get(key).is_some() {
                out.insert(key.into(), json!(journal::clipped(&record[key], 240)));
            }
        }
        if record["keep_warm_quiet"].is_object() {
            out.insert(
                "keep_warm_quiet".into(),
                json!({
            "prompt":journal::clipped(&record["keep_warm_quiet"]["prompt"],240),
            "last_message":journal::clipped(&record["keep_warm_quiet"]["last_message"],240)}),
            );
        }
        for (key, fields) in [
            ("worktree_owner", &["id"][..]),
            ("account_binding", &["id", "home", "agent"][..]),
            (
                "parent",
                &[
                    "schema",
                    "machine",
                    "host",
                    "name",
                    "session_id",
                    "run_id",
                    "conversation_id",
                    "source",
                    "at",
                ][..],
            ),
        ] {
            if record[key].is_object() {
                out.insert(key.into(), selected(&record[key], fields)?);
            }
        }
        for (key, fields) in [
            ("active_tools", &["name", "detail", "started"][..]),
            (
                "subagents",
                &[
                    "id",
                    "name",
                    "state",
                    "display_state",
                    "provider",
                    "external",
                    "model",
                    "reply_id",
                    "native_roster",
                    "revived_at",
                    "last_event_at",
                    "started",
                    "ended",
                    "current_tool",
                    "tool_detail",
                    "prompt",
                    "description",
                    "attention_id",
                    "detail",
                    "updated",
                    "completion_source",
                    "tracking_evidence",
                    "process_state",
                ][..],
            ),
            (
                "subagent_groups",
                &[
                    "active_count",
                    "total_count",
                    "completed_count",
                    "unknown_count",
                    "name",
                    "profile",
                    "started",
                    "label",
                    "detail",
                    "updated",
                ][..],
            ),
        ] {
            if let Some(values) = record[key].as_object() {
                anyhow::ensure!(
                    values.len() <= 128,
                    "Display child/tool set exceeds its bound"
                );
                let mut result = serde_json::Map::new();
                for (id, value) in values {
                    anyhow::ensure!(id.len() <= 2048, "Display identity exceeds its bound");
                    result.insert(id.clone(), selected(value, fields)?);
                }
                out.insert(key.into(), Value::Object(result));
            }
        }
        let digest = storage::label_key(&serde_json::to_string(&out)?);
        let encoded =
            serde_json::to_string(&json!({"version":SEED_VERSION,"record":out,"digest":digest}))?;
        anyhow::ensure!(
            encoded.len() <= SEED_BYTES,
            "Display seed exceeds its bound"
        );
        Ok(encoded)
    }
    pub fn decode(key: &Key, encoded: &str) -> anyhow::Result<Value> {
        anyhow::ensure!(
            encoded.len() <= SEED_BYTES,
            "Display seed exceeds its bound"
        );
        key.validate()?;
        let value: Value = serde_json::from_str(encoded)?;
        anyhow::ensure!(
            value["version"] == SEED_VERSION && key.matches(&value["record"]),
            "Display seed identity changed"
        );
        let digest = storage::label_key(&serde_json::to_string(&value["record"])?);
        anyhow::ensure!(
            value["digest"].as_str() == Some(digest.as_str()),
            "Display seed checksum mismatch"
        );
        Ok(value)
    }
}
fn selected(value: &Value, fields: &[&str]) -> anyhow::Result<Value> {
    let mut result = serde_json::Map::new();
    for field in fields {
        if let Some(value) = value.get(*field) {
            bounded(value)?;
            result.insert((*field).into(), value.clone());
        }
    }
    Ok(Value::Object(result))
}
fn bounded(value: &Value) -> anyhow::Result<()> {
    fn visit(v: &Value, bytes: &mut usize, nodes: &mut usize, depth: usize) -> anyhow::Result<()> {
        anyhow::ensure!(
            depth <= 8 && *nodes > 0,
            "Display field exceeds its structural bound"
        );
        *nodes -= 1;
        let length = match v {
            Value::String(s) => s.len(),
            Value::Array(a) => {
                for v in a {
                    visit(v, bytes, nodes, depth + 1)?;
                }
                0
            }
            Value::Object(o) => {
                for (k, v) in o {
                    anyhow::ensure!(k.len() <= *bytes, "Display field exceeds its byte bound");
                    *bytes -= k.len();
                    visit(v, bytes, nodes, depth + 1)?;
                }
                0
            }
            _ => 8,
        };
        anyhow::ensure!(length <= *bytes, "Display field exceeds its byte bound");
        *bytes -= length;
        Ok(())
    }
    visit(value, &mut (16 << 10), &mut 256, 0)
}
