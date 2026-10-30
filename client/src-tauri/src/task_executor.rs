//! The exact fresh task launch follows existing creation/catalog wakeups.
//! No timer or restarted record can authorize task input.
use crate::{
    creations::{Admission, CreationHost},
    task_details::TaskWrites,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, Manager, State};
use zerus_client_core::{
    actions::Target,
    task_executor as core,
    task_start::{Assignment, Selection},
    tasks_write,
};

struct Records {
    all: BTreeMap<String, Value>,
    fresh: BTreeSet<String>,
}
struct Inner {
    root: PathBuf,
    pump: crate::pump::Pump,
    creations: CreationHost,
    writes: TaskWrites,
    records: Mutex<Option<Result<Records, String>>>,
    fresh: AtomicBool,
    running: AtomicBool,
    dirty: AtomicBool,
    app: AppHandle,
}
#[derive(Clone)]
pub struct TaskExecutors(Arc<Inner>);
fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

fn matches_launch(intent: &Value, id: &str, row: &zerus_client_core::ViewRow) -> bool {
    row.tracked
        && row.state != "archived"
        && row.notify.cmd == text(intent, "cmd")
        && row.name.splitn(3, '/').nth(2) == Some(text(intent, "tag"))
        && row.launch_id.as_deref() == Some(id)
}
fn ready_for_task(row: &zerus_client_core::ViewRow) -> bool {
    row.state == "running"
        && row.process_state != "exited"
        && !row.needs_action
        && row.run_id.as_deref().is_some_and(|id| !id.is_empty())
}
fn checked_parent(
    input: &Launch,
    row: &zerus_client_core::ViewRow,
    doc: &Value,
) -> Result<(), String> {
    crate::task_details::assignment_target(row, &input.target, &input.session_id)?;
    if input.assignment.kind != "new_session"
        || input.assignment.machine.is_empty()
        || input.assignment.machine != row.machine
        || input.assignment.session_id != input.session_id
        || input.plan["group"] != input.project
        || input.plan["new_project"] == true
        || doc["writable"] != true
        || !matches!(doc["stage"].as_str(), Some("todo" | "backlog"))
        || doc["stage_at"].as_i64() != Some(input.stage_at)
        || doc["target"]["kind"] != "new_session"
        || doc["target"]["parent"]["machine"] != input.assignment.machine
        || doc["target"]["parent"]["host"] != input.target.computer
        || doc["target"]["parent"]["session_id"] != input.session_id
    {
        return Err("The task or executor changed. Nothing was sent. Refresh the task.".into());
    }
    Ok(())
}
impl TaskExecutors {
    pub fn start(app: &AppHandle) -> Self {
        let host = Self(Arc::new(Inner {
            root: app
                .state::<crate::composer::Composers>()
                .0
                .store
                .root()
                .to_path_buf(),
            pump: app.state::<crate::commands::AppState>().pump(),
            creations: app.state::<CreationHost>().inner().clone(),
            writes: app.state::<TaskWrites>().inner().clone(),
            records: Mutex::new(None),
            fresh: AtomicBool::new(false),
            running: AtomicBool::new(false),
            dirty: AtomicBool::new(false),
            app: app.clone(),
        }));
        let weak = Arc::downgrade(&host.0);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if let Some(inner) = weak.upgrade() {
                TaskExecutors(inner).changed();
            }
        });
        host.0.creations.on_catalog(wake.clone());
        host.0.writes.on_idle(wake);
        host
    }
    fn with<T>(&self, f: impl FnOnce(&mut Records) -> Result<T, String>) -> Result<T, String> {
        let mut guard = crate::composer::lock(&self.0.records);
        if guard.is_none() {
            *guard = Some(core::load(&self.0.root).map(|all| Records {
                all,
                fresh: BTreeSet::new(),
            }));
        }
        let state = guard.as_mut().unwrap().as_mut().map_err(|s| s.clone())?;
        let result = f(state);
        self.0
            .fresh
            .store(!state.fresh.is_empty(), Ordering::Release);
        result
    }
    fn emit(&self) {
        let _ = self.0.app.emit("task-executors", ());
    }
    pub(crate) fn changed(&self) {
        if !self.0.fresh.load(Ordering::Acquire) {
            return;
        }
        self.0.dirty.store(true, Ordering::Release);
        if self.0.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let host = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            while host.0.dirty.swap(false, Ordering::AcqRel) {
                let _ = host.tick();
            }
            host.0.running.store(false, Ordering::Release);
            if host.0.dirty.load(Ordering::Acquire) {
                host.changed();
            }
        });
    }
    fn tick(&self) -> Result<(), String> {
        self.with(|records|{
            for id in records.fresh.clone() {
                let Some(record)=records.all.get(&id).cloned() else{records.fresh.remove(&id);continue;};
                let intent=&record["intent"];let host=text(intent,"host");
                let phase=self.0.creations.phase(&id);
                if phase.as_deref().is_some_and(|s|matches!(s,"failed"|"stopped"|"unknown"|"recover"|"storage_error")) {
                    records.fresh.remove(&id);
                    let next=core::review(&self.0.root,&record,"Session setup needs review. Check its creation card before allowing another launch.")?;
                    records.all.insert(id,next);self.emit();continue;
                }
                let Some(row)=self.0.pump.row_for_launch(host,&id)else{continue;};
                if !self.0.pump.online(host){continue;}
                if !matches_launch(intent,&id,&row) {continue;}
                if !self.0.pump.members_of(text(intent,"project")).iter().any(|(member,_)|member==&format!("{}\n{}",row.machine,row.name)){continue;}
                let Ok(_permit)=self.0.writes.begin()else{return Ok(());};
                records.fresh.remove(&id);
                if !ready_for_task(&row) {
                    let next=core::review(&self.0.root,&record,"Session ended or needs input before task delivery. Check this executor before allowing another launch.")?;records.all.insert(id,next);self.emit();continue;
                }
                let selection=Selection{host:host.into(),name:row.name,session_id:format!("launch_id:{id}"),run_id:row.run_id.unwrap_or_default(),conversation_id:row.conversation_id.unwrap_or_default()};
                let result=core::deliver(&self.0.root,&record,&selection,|call|tasks_write::run(&zerus_client_core::default_hgs(),call));
                match result {
                    Ok(None)=>{records.all.remove(&id);},
                    Ok(Some(next))=>{records.all.insert(id,next);},
                    Err(error)=>{
                        // A failed checkpoint or lost receipt never re-enters
                        // the fresh set. Reload exact evidence for review.
                        match core::load(&self.0.root) {Ok(all)=>records.all=all,Err(e)=>return Err(e)}
                        if let Some(current)=records.all.get(&id).cloned(){if let Ok(next)=core::review(&self.0.root,&current,&error){records.all.insert(id,next);}}
                    }
                }
                self.emit();
            }Ok(())
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    project: String,
    task: String,
    stage_at: i64,
    assignment: Assignment,
    target: Target,
    catalog_key: String,
    session_id: String,
    plan: Value,
}
#[tauri::command]
pub async fn task_executor_submit(
    state: State<'_, TaskExecutors>,
    input: Launch,
) -> Result<Admission, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let host = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||{
        let result=host.with(|records|{
            let _permit=host.0.writes.begin()?;
            let row=host.0.pump.row(&input.target.computer,&input.catalog_key).ok_or("The selected parent session is unavailable.")?;
            crate::task_details::assignment_target(&row,&input.target,&input.session_id)?;
            if input.assignment.kind!="new_session" || input.assignment.machine.is_empty() || input.assignment.session_id!=input.session_id || !host.0.pump.online(&input.target.computer)
                || !host.0.pump.members_of(&input.project).iter().any(|(member,_)|member==&format!("{}\n{}",row.machine,row.name)) || input.plan["group"]!=input.project || input.plan["new_project"]==true
            {return Err("Choose the task's project and a confirmed parent session.".into());}
            let doc=tasks_write::run(&zerus_client_core::default_hgs(),&zerus_client_core::task_details::document_call(&input.project,&input.task)?)?;
            let doc=zerus_client_core::task_details::checked(&doc,&input.project,&input.task)?;
            checked_parent(&input, &row, &doc)?;
            if records.all.len()>=core::LIMIT{return Err(core::FULL.into());}
            let id=uuid::Uuid::new_v4().to_string();
            let intent=json!({"project":input.project,"task":input.task,"stage_at":input.stage_at,"assignment":input.assignment,"host":input.plan["host"],"cmd":input.plan["cmd"],"tag":input.plan["tag"]});
            let record=core::reserve(&host.0.root,&intent,&id)?;records.all.insert(id.clone(),record.clone());records.fresh.insert(id.clone());
            host.0.fresh.store(true,Ordering::Release);
            match host.0.creations.submit_reserved(input.plan,id.clone()) {
                Ok(answer@Admission::Admitted{..})=>Ok(answer),
                Ok(Admission::Refused{refused})|Err(refused)=>{
                    records.fresh.remove(&id);records.all.insert(id,core::review(&host.0.root,&record,&refused)?);Ok(Admission::Refused{refused})
                }
            }
        });host.emit();host.changed();result
    }).await.map_err(|e|e.to_string())?
}
#[tauri::command]
pub async fn task_executor_open(
    state: State<'_, TaskExecutors>,
    project: String,
    task: String,
) -> Result<Option<core::View>, String> {
    zerus_client_core::task_details::document_call(&project, &task)?;
    let host = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        host.with(|records| {
            Ok(records
                .all
                .values()
                .find(|r| r["intent"]["project"] == project && r["intent"]["task"] == task)
                .map(|r| core::view(r, records.fresh.contains(text(r, "id")))))
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn task_executor_release(
    state: State<'_, TaskExecutors>,
    id: String,
    snapshot: String,
) -> Result<(), String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let host = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = host.with(|records| {
            let _permit = host.0.writes.begin()?;
            if records.fresh.contains(&id) {
                return Err("The new executor is starting".into());
            }
            let record = records.all.get(&id).ok_or(core::RELEASE_FAILED)?;
            core::release(&host.0.root, record, &snapshot)?;
            records.all.remove(&id);
            Ok(())
        });
        host.emit();
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::ViewRow;
    fn child() -> ViewRow {
        let mut row = ViewRow {
            name: "codex/example/child".into(),
            machine: "workstation".into(),
            tracked: true,
            state: "running".into(),
            process_state: "running".into(),
            run_id: Some("child-run".into()),
            conversation_id: Some("child-conversation".into()),
            launch_id: Some("launch".into()),
            ..Default::default()
        };
        row.notify.cmd = "codex".into();
        row
    }
    #[test]
    fn dispatch_requires_the_exact_tracked_launch_provider_tag_and_live_run() {
        let intent = json!({"cmd":"codex","tag":"child"});
        let row = child();
        assert!(matches_launch(&intent, "launch", &row));
        assert!(ready_for_task(&row));
        for changed in [
            ViewRow {
                launch_id: Some("other".into()),
                ..row.clone()
            },
            ViewRow {
                name: "codex/example/renamed".into(),
                ..row.clone()
            },
            ViewRow {
                tracked: false,
                ..row.clone()
            },
            ViewRow {
                state: "archived".into(),
                ..row.clone()
            },
        ] {
            assert!(!matches_launch(&intent, "launch", &changed));
        }
        let mut wrong_provider = row.clone();
        wrong_provider.notify.cmd = "claude".into();
        assert!(!matches_launch(&intent, "launch", &wrong_provider));
        for changed in [
            ViewRow {
                run_id: None,
                ..row.clone()
            },
            ViewRow {
                needs_action: true,
                ..row.clone()
            },
            ViewRow {
                state: "stopped".into(),
                ..row.clone()
            },
            ViewRow {
                process_state: "exited".into(),
                ..row.clone()
            },
        ] {
            assert!(!ready_for_task(&changed));
        }
    }
    #[test]
    fn launch_admission_binds_the_task_epoch_parent_machine_run_and_conversation() {
        let row = child();
        let input = Launch {
            project: "project".into(),
            task: "task".into(),
            stage_at: 12,
            assignment: Assignment {
                kind: "new_session".into(),
                machine: row.machine.clone(),
                session_id: "launch_id:launch".into(),
                name: String::new(),
            },
            target: Target {
                computer: "peer".into(),
                name: row.name.clone(),
                run_id: "child-run".into(),
                conversation_id: "child-conversation".into(),
            },
            catalog_key: "run:child-run".into(),
            session_id: "launch_id:launch".into(),
            plan: json!({"group":"project","new_project":false}),
        };
        let doc = json!({"writable":true,"stage":"todo","stage_at":12,"target":{"kind":"new_session","parent":{"host":"peer","machine":"workstation","session_id":"launch_id:launch"}}});
        assert!(checked_parent(&input, &row, &doc).is_ok());
        for path in [
            vec!["stage_at"],
            vec!["target", "kind"],
            vec!["target", "parent", "machine"],
            vec!["target", "parent", "host"],
            vec!["target", "parent", "session_id"],
        ] {
            let mut changed = doc.clone();
            let mut field = &mut changed;
            for key in path {
                field = &mut field[key];
            }
            *field = json!("changed");
            assert!(checked_parent(&input, &row, &changed).is_err());
        }
        for changed in [
            ViewRow {
                machine: "other".into(),
                ..row.clone()
            },
            ViewRow {
                run_id: Some("new-run".into()),
                ..row.clone()
            },
            ViewRow {
                conversation_id: Some("new-conversation".into()),
                ..row.clone()
            },
        ] {
            assert!(checked_parent(&input, &changed, &doc).is_err());
        }
    }
}
