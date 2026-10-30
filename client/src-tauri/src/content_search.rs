//! One cancellable search generation, at most four owned CLI reads. Cancelled
//! children keep their lane until reaped; changing a query cannot grow a queue.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::State;
use zerus_client_core::{content_search, ComputerId};

#[derive(Default)]
struct Inner {
    through: u64,
    latest: u64,
    running: HashMap<String, (u64, Arc<AtomicBool>)>,
}
#[derive(Clone, Default)]
pub struct Searches(Arc<Mutex<Inner>>);
struct Permit {
    searches: Searches,
    computer: String,
    generation: u64,
    cancel: Arc<AtomicBool>,
}
impl Drop for Permit {
    fn drop(&mut self) {
        let mut state = self.searches.0.lock().unwrap();
        if state
            .running
            .get(&self.computer)
            .is_some_and(|(generation, _)| *generation == self.generation)
        {
            state.running.remove(&self.computer);
        }
    }
}
impl Searches {
    fn begin(&self, computer: String, generation: u64) -> Result<Permit, String> {
        let mut state = self.0.lock().unwrap();
        if generation == 0 || generation <= state.through || generation < state.latest {
            return Err("superseded".into());
        }
        if generation > state.latest {
            for (_, cancel) in state.running.values() {
                cancel.store(true, Ordering::Relaxed);
            }
            state.latest = generation;
        }
        if state.running.len() >= 4 || state.running.contains_key(&computer) {
            return Err("Search is busy; search again.".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        state
            .running
            .insert(computer.clone(), (generation, cancel.clone()));
        Ok(Permit {
            searches: self.clone(),
            computer,
            generation,
            cancel,
        })
    }
    fn cancel(&self, through: u64) {
        let mut state = self.0.lock().unwrap();
        state.through = state.through.max(through);
        for (generation, cancel) in state.running.values() {
            if *generation <= through {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    }
}
#[tauri::command]
pub fn cancel_content_search(state: State<'_, Searches>, through: u64) {
    state.cancel(through);
}
#[tauri::command]
pub async fn content_search(
    state: State<'_, Searches>,
    app: State<'_, crate::commands::AppState>,
    computer: String,
    query: String,
    generation: u64,
) -> Result<content_search::Reply, String> {
    if crate::actions::blocked() {
        return Err(crate::screens::REPLAY_NEEDS_HGS.into());
    }
    let id = ComputerId::from(computer.as_str());
    content_search::invocation(&id, &query).map_err(|error| error.to_string())?;
    let permit = state.begin(computer, generation)?;
    let pump = app.pump();
    let source = pump.archive_generation(&id);
    tauri::async_runtime::spawn_blocking(move || {
        let mut reply = content_search::read(
            &zerus_client_core::default_hgs(),
            &id,
            &query,
            &permit.cancel,
        )
        .map_err(|error| error.to_string())?;
        if permit.cancel.load(Ordering::Relaxed) || source != pump.archive_generation(&id) {
            return Err("superseded".into());
        }
        pump.place_search_archives(&id, &mut reply.results)?;
        Ok(reply)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_does_not_free_a_lane_until_the_old_child_is_reaped() {
        let searches = Searches::default();
        let mut permits: Vec<_> = (0..4)
            .map(|n| searches.begin(format!("computer-{n}"), 1).unwrap())
            .collect();
        assert!(searches.begin("fifth".into(), 1).is_err());
        searches.cancel(1);
        assert!(permits.iter().all(|p| p.cancel.load(Ordering::Relaxed)));
        assert!(searches.begin("new".into(), 2).is_err());
        permits.pop();
        assert!(searches.begin("new".into(), 2).is_ok());
        assert!(searches.begin("late".into(), 1).is_err());
    }
    #[test]
    fn a_new_generation_cancels_old_reads_without_retaining_their_results() {
        let searches = Searches::default();
        let first = searches.begin("a".into(), 10).unwrap();
        let second = searches.begin("b".into(), 11).unwrap();
        assert!(first.cancel.load(Ordering::Relaxed));
        assert!(!second.cancel.load(Ordering::Relaxed));
        drop(first);
        assert_eq!(searches.0.lock().unwrap().running.len(), 1);
        searches.cancel(12);
        assert!(second.cancel.load(Ordering::Relaxed));
        drop(second);
        assert!(searches.0.lock().unwrap().running.is_empty());
    }
}
