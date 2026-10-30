//! The selected task's bounded inspector, sharing the existing media cache.
use crate::composer::{
    fetch::{Fetcher, Opener},
    Composers,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::State;
use zerus_client_core::{attachments, reads::CachedAttachment, task_details, tasks_write};

#[derive(Clone, Default)]
pub struct TaskReads(Arc<AtomicBool>);
struct Permit(TaskReads);
impl TaskReads {
    fn begin(&self) -> Result<Permit, String> {
        self.0
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Task details are already loading.".to_owned())?;
        Ok(Permit(self.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0 .0.store(false, Ordering::SeqCst);
    }
}

#[tauri::command]
pub async fn task_details(
    state: State<'_, TaskReads>,
    composers: State<'_, Composers>,
    fetcher: State<'_, Arc<Fetcher>>,
    project: String,
    task: String,
    before: Option<String>,
) -> Result<Value, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let call = task_details::call(&project, &task, before.as_deref())?;
    let permit = state.begin()?;
    let (composers, fetcher) = (composers.inner().clone(), Arc::clone(fetcher.inner()));
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let reply = tasks_write::run(&zerus_client_core::default_hgs(), &call)?;
        let raw = task_details::checked(&reply, &project, &task)?;
        let cache = composers.cache();
        let presented = task_details::present(raw, |value| {
            let decoded = tasks_write::decode_files(value);
            if value
                .as_array()
                .is_some_and(|files| files.len() != decoded.len())
            {
                return Err("Task attachments could not be read.".into());
            }
            attachments::prepare(cache.root(), &decoded)?
                .into_iter()
                .map(|file| {
                    let (view, _) = fetcher.described(CachedAttachment {
                        local_path: file.local_path,
                        metadata: json!({"name":file.name,"mime":file.mime}),
                    })?;
                    serde_json::to_value(view).map_err(|e| e.to_string())
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        })?;
        cache.prune_later(&composers.0.deps.spawn, Arc::new(HashSet::new));
        Ok(presented)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn task_attachment_open(
    fetcher: State<'_, Arc<Fetcher>>,
    opener: State<'_, Opener>,
    path: String,
) -> Result<Option<String>, String> {
    let copy = fetcher
        .cache()
        .shown(&path)
        .ok_or("Could not open the attachment. Refresh this task.")?;
    let opener = opener.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::composer::fetch::opened(
            &copy,
            zerus_client_core::external_file::Origin::OtherMachine,
            &opener,
            crate::composer::fetch::OPEN_FAILED,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_read_holds_its_lane_until_it_finishes() {
        let reads = TaskReads::default();
        let permit = reads.begin().unwrap();
        assert!(reads.begin().is_err());
        drop(permit);
        assert!(reads.begin().is_ok());
    }
}
