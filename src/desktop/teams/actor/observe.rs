use super::*;
use serde::{Deserialize, Serialize};
#[derive(Default, Serialize, Deserialize)]
struct Position {
    generation: String,
    offset: u64,
    epoch: String,
    #[serde(default)]
    full_at: i64,
}
pub(super) struct Hint {
    loaded: bool,
    generation: String,
    offset: u64,
    start_generation: String,
    start_offset: u64,
    documents: Option<std::collections::BTreeSet<String>>,
}
impl Runtime {
    /// Cache at most this worker page's projects, and read each journal once.
    pub fn prepare(&mut self, teams: &[TeamRow]) -> Result<()> {
        self.begin_pass();
        // Choose the earliest cursor before any team advances it. Teams with
        // another generation force the journal's conservative full fallback.
        for team in teams {
            let p = self
                .load::<Position>(&team.id, "journal")?
                .unwrap_or_default();
            let hint = self.hints.entry(team.project.clone()).or_insert(Hint {
                loaded: false,
                generation: String::new(),
                offset: 0,
                start_generation: p.generation.clone(),
                start_offset: p.offset,
                documents: None,
            });
            if hint.start_generation != p.generation {
                hint.start_generation.clear();
                hint.start_offset = 0
            } else {
                hint.start_offset = hint.start_offset.min(p.offset)
            }
        }
        Ok(())
    }
    pub(super) fn observe(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
        safety: bool,
    ) -> Result<()> {
        let old = self
            .load::<Position>(&team.id, "journal")?
            .unwrap_or_default();
        if !self.hints.contains_key(&team.project) {
            self.hints.insert(
                team.project.clone(),
                Hint {
                    loaded: false,
                    generation: String::new(),
                    offset: 0,
                    start_generation: old.generation.clone(),
                    start_offset: old.offset,
                    documents: None,
                },
            );
        }
        let hint = self.hints.get_mut(&team.project).expect("inserted");
        if !hint.loaded {
            let changes =
                ports.changes(&team.project, &hint.start_generation, hint.start_offset)?;
            hint.loaded = true;
            hint.generation = changes.generation;
            hint.offset = changes.offset;
            hint.documents = changes.documents;
        }
        let full = safety
            || now >= old.full_at.saturating_add(60_000)
            || old.epoch != self.epoch
            || old.generation != hint.start_generation
            || old.offset < hint.start_offset;
        let documents = if full { None } else { hint.documents.clone() };
        let next = Position {
            generation: hint.generation.clone(),
            offset: hint.offset,
            epoch: self.epoch.clone(),
            full_at: if full { now } else { old.full_at },
        };
        if self.observe_nodes(store, ports, team, documents.as_ref(), now)? {
            self.put(&team.id, "journal", &next)?;
        }
        Ok(())
    }
    fn observe_nodes(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        documents: Option<&std::collections::BTreeSet<String>>,
        now: i64,
    ) -> Result<bool> {
        let mut cursor = None;
        let mut complete = true;
        for _ in 0..4 {
            let page = store.queue(&team.id, 200, cursor.as_ref())?;
            for n in page.rows {
                if ports.stopped() {
                    return Ok(false);
                }
                let Some(task) = n.task else { continue };
                if documents.is_some_and(|d| !d.contains(&task.uuid)) {
                    continue;
                }
                let key = format!("observe:{}", n.id);
                if !self.due(store, team, &key, "", now)? {
                    complete = false;
                    continue;
                }
                match ports.observe(store, &task) {
                    Ok(()) => self.recovered(store, team, &key, now)?,
                    Err(e) => {
                        complete = false;
                        self.failure(store, team, &key, "", &e, now)?;
                    }
                }
            }
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        if cursor.is_some() {
            return Err(problem("Live task page bound exceeded"));
        }
        Ok(complete)
    }
}
#[cfg(test)]
pub(super) fn position_fixture(generation: &str) -> serde_json::Value {
    serde_json::json!({"generation":generation,"offset":0,"epoch":"old"})
}
