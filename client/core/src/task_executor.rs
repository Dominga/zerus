//! Task new-session launches use the existing private outbox writer. Only
//! launches admitted in this process may advance from waiting; loaded records
//! are review evidence. Every original delivery plan is durable before input.
use crate::{
    creation::canonical_uuid,
    task_details,
    task_start::{self, Assignment, Selection},
    tasks_write::{outbox, TaskCall},
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const LIMIT: usize = 16;
pub const PENDING: &str = "This task already has an executor launch pending. Check its session and task history before starting it again.";
pub const UNREADABLE: &str = "The saved executor launch records could not be read. Review them before creating another task executor.";
pub const FULL: &str = "Finish or review the pending task executor launches first.";
pub const RELEASE_FAILED: &str = "Could not release the executor launch record.";
const MAX_BYTES: u64 = 32 * 1024 * 1024;

pub fn root(store: &Path) -> PathBuf {
    store.join("task-executors")
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
fn checked(record: &Value) -> Result<(), String> {
    let intent = &record["intent"];
    task_details::document_call(text(intent, "project"), text(intent, "task"))?;
    if record["schema"] != 1
        || !canonical_uuid(text(record, "id"))
        || !matches!(text(record, "phase"), "waiting" | "reserved" | "review")
        || intent["assignment"]["kind"] != "new_session"
        || intent["assignment"]["machine"]
            .as_str()
            .is_none_or(str::is_empty)
        || intent["assignment"]["session_id"]
            .as_str()
            .is_none_or(str::is_empty)
        || intent["stage_at"].as_i64().is_none_or(|n| n <= 0)
        || ["host", "cmd", "tag"].iter().any(|k| {
            intent[*k]
                .as_str()
                .is_none_or(|s| s.len() > 512 || s.chars().any(char::is_control))
        })
        || text(intent, "cmd").is_empty()
        || text(intent, "tag").is_empty()
    {
        return Err(UNREADABLE.into());
    }
    if let Some(plan) = intent.get("plan") {
        if plan["schema"] != 1
            || plan["id"] != record["id"]
            || plan["request"]["request_id"] != record["id"]
            || plan["project"] != intent["project"]
            || plan["task"] != intent["task"]
        {
            return Err(UNREADABLE.into());
        }
    }
    Ok(())
}

/// At most 16 records and their markers, with a total byte bound. Unknown,
/// damaged or oversized evidence refuses admission without modifying a file.
pub fn load(store: &Path) -> Result<BTreeMap<String, Value>, String> {
    let scope = root(store);
    let folder = scope.join("task-delivery");
    let entries = match fs::read_dir(&folder) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(_) => return Err(UNREADABLE.into()),
    };
    let mut records = BTreeMap::new();
    let mut bytes = 0;
    let mut count = 0;
    for entry in entries.take(LIMIT * 2 + 1) {
        count += 1;
        if count > LIMIT * 2 {
            return Err(UNREADABLE.into());
        }
        let path = entry.map_err(|_| UNREADABLE)?.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| UNREADABLE)?;
        if !meta.is_file()
            || !matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("json" | "marker")
            )
        {
            return Err(UNREADABLE.into());
        }
        bytes += meta.len();
        if bytes > MAX_BYTES {
            return Err(UNREADABLE.into());
        }
        if path.extension().and_then(|s| s.to_str()) == Some("marker") {
            if !path.with_extension("json").is_file() {
                return Err(UNREADABLE.into());
            }
            continue;
        }
        let record = outbox::read(&path)?;
        checked(&record)?;
        let intent = &record["intent"];
        if path != outbox::path(&scope, text(intent, "project"), text(intent, "task"))
            || outbox::pending(&scope, text(intent, "project"), text(intent, "task"))?.as_ref()
                != Some(&record)
        {
            return Err(UNREADABLE.into());
        }
        if records.insert(text(&record, "id").into(), record).is_some() {
            return Err(UNREADABLE.into());
        }
    }
    Ok(records)
}

#[derive(Clone, Serialize)]
pub struct View {
    pub project: String,
    pub task: String,
    pub id: String,
    pub session: String,
    pub phase: String,
    pub snapshot: String,
    pub error: String,
}
pub fn view(record: &Value, fresh: bool) -> View {
    View {
        project: text(&record["intent"], "project").into(),
        task: text(&record["intent"], "task").into(),
        id: text(record, "id").into(),
        session: record["session"]
            .as_str()
            .unwrap_or(text(&record["intent"], "tag"))
            .into(),
        phase: if fresh && text(record, "phase") == "waiting" {
            "waiting"
        } else {
            "review"
        }
        .into(),
        snapshot: outbox::snapshot(record),
        error: text(record, "error").into(),
    }
}
pub fn reserve(store: &Path, intent: &Value, id: &str) -> Result<Value, String> {
    let records = load(store)?;
    if records.values().any(|r| {
        r["intent"]["project"] == intent["project"] && r["intent"]["task"] == intent["task"]
    }) {
        return Err(PENDING.into());
    }
    if records.len() >= LIMIT {
        return Err(FULL.into());
    }
    let mut record = json!({"schema":1,"id":id,"intent":intent,"phase":"waiting","error":""});
    checked(&record)?;
    let previous = outbox::reserve(
        &root(store),
        text(intent, "project"),
        text(intent, "task"),
        intent,
        id,
    )?;
    if previous.recovering {
        return Err(PENDING.into());
    }
    record["id"] = previous.record["id"].clone();
    replace(store, &previous.record, &record)?;
    Ok(record)
}
fn replace(store: &Path, old: &Value, new: &Value) -> Result<(), String> {
    let intent = &old["intent"];
    if !outbox::replace_if_unchanged(
        &root(store),
        text(intent, "project"),
        text(intent, "task"),
        old,
        new,
    )? {
        return Err(RELEASE_FAILED.into());
    }
    Ok(())
}
pub fn review(store: &Path, record: &Value, error: &str) -> Result<Value, String> {
    let mut next = record.clone();
    next["phase"] = json!("review");
    next["error"] = json!(error.chars().take(2048).collect::<String>());
    replace(store, record, &next)?;
    Ok(next)
}
pub fn release(store: &Path, record: &Value, snapshot: &str) -> Result<(), String> {
    checked(record)?;
    if outbox::snapshot(record) != snapshot
        || !outbox::release_if_unchanged(
            &root(store),
            text(&record["intent"], "project"),
            text(&record["intent"], "task"),
            record,
        )?
    {
        return Err(RELEASE_FAILED.into());
    }
    Ok(())
}

/// The caller freshly verified this process's admitted launch, its project,
/// provider, tag and current native identities. It calls this at most once.
pub fn deliver(
    store: &Path,
    record: &Value,
    selection: &Selection,
    mut run: impl FnMut(&TaskCall) -> Result<Map<String, Value>, String>,
) -> Result<Option<Value>, String> {
    checked(record)?;
    let intent = &record["intent"];
    let id = text(record, "id");
    if text(record, "phase") != "waiting"
        || selection.session_id != format!("launch_id:{id}")
        || selection.host != text(intent, "host")
    {
        return Err(PENDING.into());
    }
    let mut reserved = record.clone();
    reserved["phase"] = json!("reserved");
    reserved["session"] = json!(selection.name);
    replace(store, record, &reserved)?;
    let project = text(intent, "project");
    let task = text(intent, "task");
    let result: Result<(), String> = (|| {
        let assignment: Assignment =
            serde_json::from_value(intent["assignment"].clone()).map_err(|_| UNREADABLE)?;
        let plan = task_start::prepare(
            project,
            task,
            intent["stage_at"].as_i64().unwrap_or(0),
            &assignment,
            selection,
            id,
            &mut run,
        )?;
        let mut ready = reserved.clone();
        ready["intent"]["plan"] = plan.clone();
        let total: usize = load(store)?.values().map(|r| r.to_string().len()).sum();
        if total.saturating_sub(reserved.to_string().len()) + ready.to_string().len()
            > MAX_BYTES as usize - 65536
        {
            return Err(
                "Could not reserve task delivery within its byte limit. Nothing was sent.".into(),
            );
        }
        replace(store, &reserved, &ready)?;
        reserved = ready;
        let reply = run(&TaskCall {
            args: vec![
                "start-deliver".into(),
                task.into(),
                "--project".into(),
                project.into(),
                "--json".into(),
            ],
            input: plan.to_string(),
        })?;
        if reply.get("schema") != Some(&json!(1))
            || reply.get("id").and_then(Value::as_str) != Some(id)
            || reply.get("project").and_then(Value::as_str) != Some(project)
            || reply.get("uuid").and_then(Value::as_str) != Some(task)
            || reply
                .get("task")
                .is_none_or(|t| t["uuid"] != task || t["project"] != project)
        {
            return Err(
                "Task delivery reply did not confirm this exact executor and receipt".into(),
            );
        }
        if reply.get("state").and_then(Value::as_str) != Some("submitted") {
            return Err(reply
                .get("error")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("Task delivery needs review. Check task history before retrying.")
                .into());
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            release(store, &reserved, &outbox::snapshot(&reserved))?;
            Ok(None)
        }
        Err(error) => Ok(Some(review(store, &reserved, &error)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    const TASK: &str = "11111111-1111-4111-8111-111111111111";
    const ID: &str = "22222222-2222-4222-8222-222222222222";
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "zerus-task-executor-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn intent() -> Value {
        json!({"project":"example","task":TASK,"stage_at":123,"assignment":{"kind":"new_session","machine":"node-parent","session_id":"run_id:parent","name":""},"host":"peer","cmd":"codex","tag":"worker"})
    }
    fn selection() -> Selection {
        Selection {
            host: "peer".into(),
            name: "codex/example/worker".into(),
            session_id: format!("launch_id:{ID}"),
            run_id: "child".into(),
            conversation_id: "conversation".into(),
        }
    }
    fn reply(call: &TaskCall) -> Map<String, Value> {
        let i = intent();
        let target = json!({"kind":"new_session","parent":{"machine":"node-parent","session_id":"run_id:parent","session":"codex/example/parent","host":"peer"}});
        match call.args[0].as_str() {
            "get"=>json!({"project":{"id":"example"},"task":{"project":"example","uuid":TASK,"stage":"todo","stage_at":123,"writable":true,"target":target,"history_schema":1,"log":[],"history_next":null}}),
            "start-plan"=>json!({"plan":{"schema":1,"id":ID,"project":"example","task":TASK,"stage_at":i["stage_at"],"target":target,"revision":"original","selection":selection(),"session":{"session_id":selection().session_id},"request":{"request_id":ID,"expected_run_id":"child","expected_conversation_id":"conversation","text":"Original prompt"},"opaque":{"kept":true}}}),
            _=>panic!("unexpected call")
        }.as_object().unwrap().clone()
    }
    #[test]
    fn exact_launch_reserves_the_original_before_input_and_never_repeats_unknown_delivery() {
        let root = Scratch::new();
        let record = reserve(&root.0, &intent(), ID).unwrap();
        let mut deliveries = 0;
        let review = deliver(&root.0, &record, &selection(), |call| {
            if call.args[0] == "start-deliver" {
                deliveries += 1;
                let pending = outbox::pending(&super::root(&root.0), "example", TASK)
                    .unwrap()
                    .unwrap();
                assert_eq!(pending["phase"], "reserved");
                assert_eq!(
                    pending["intent"]["plan"],
                    serde_json::from_str::<Value>(&call.input).unwrap()
                );
                Err("reply lost".into())
            } else {
                Ok(reply(call))
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(deliveries, 1);
        assert_eq!(review["phase"], "review");
        assert_eq!(review["intent"]["plan"]["opaque"]["kept"], true);
        assert!(deliver(&root.0, &review, &selection(), |_| panic!("no replay")).is_err());
        assert_eq!(
            reserve(&root.0, &intent(), "33333333-3333-4333-8333-333333333333"),
            Err(PENDING.into())
        );
        let loaded = load(&root.0).unwrap();
        assert_eq!(loaded[ID], review);
        assert_eq!(view(&record, false).phase, "review");
        assert!(release(&root.0, &review, "stale").is_err());
        assert!(outbox::pending(&super::root(&root.0), "example", TASK)
            .unwrap()
            .is_some());
        release(&root.0, &review, &outbox::snapshot(&review)).unwrap();
        assert!(load(&root.0).unwrap().is_empty());
    }
    #[test]
    fn a_confirmed_delivery_releases_only_its_exact_launch() {
        let root = Scratch::new();
        let record = reserve(&root.0, &intent(), ID).unwrap();
        assert!(deliver(&root.0,&record,&selection(),|call|if call.args[0]=="start-deliver" {Ok(json!({"schema":1,"id":ID,"project":"example","uuid":TASK,"state":"submitted","task":{"uuid":TASK,"project":"example"}}).as_object().unwrap().clone())}else{Ok(reply(call))}).unwrap().is_none());
        assert!(load(&root.0).unwrap().is_empty());
    }
    #[test]
    fn wrong_launch_or_changed_target_produces_no_input() {
        let root = Scratch::new();
        let record = reserve(&root.0, &intent(), ID).unwrap();
        let mut wrong = selection();
        wrong.session_id = "run_id:child".into();
        assert!(deliver(&root.0, &record, &wrong, |_| panic!("wrong launch")).is_err());
        let review = deliver(&root.0, &record, &selection(), |call| {
            assert_eq!(call.args[0], "get");
            let mut r = reply(call);
            r["task"]["stage_at"] = json!(456);
            Ok(r)
        })
        .unwrap()
        .unwrap();
        assert_eq!(review["phase"], "review");
        assert!(review["intent"].get("plan").is_none());
    }
    #[test]
    fn bounded_admission_and_damaged_markers_keep_existing_records() {
        let root = Scratch::new();
        for n in 0..LIMIT {
            let mut i = intent();
            i["task"] = json!(format!("11111111-1111-4111-8111-{n:012}"));
            reserve(&root.0, &i, &format!("22222222-2222-4222-8222-{n:012}")).unwrap();
        }
        assert_eq!(reserve(&root.0, &intent(), ID), Err(FULL.into()));
        let all = load(&root.0).unwrap();
        let record = all.values().next().unwrap();
        let i = &record["intent"];
        let file = outbox::path(&super::root(&root.0), text(i, "project"), text(i, "task"));
        fs::write(file.with_extension("marker"), "changed").unwrap();
        assert!(load(&root.0).is_err());
        assert!(file.is_file());
    }
}
