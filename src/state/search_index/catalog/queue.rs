//! Bounded keyset consumption of staged source paths. Queue rows never carry
//! full bindings; authoritative metadata is read again for every source step.
use super::*;

#[derive(Default, Serialize)]
pub(crate) struct CatchupProgress {
    pub visited_paths: usize,
    pub pruned_paths: usize,
    pub ingestion: Progress,
}

impl Store {
    pub(in crate::state::search_index) fn mark_retired_references(&self, key: &str) -> Result<()> {
        let current: bool = self
            .index
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='catalog_source_fallback')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if !current {
            return Ok(());
        }
        self.index
            .connection()
            .execute(
                "UPDATE catalog_queue SET fallback=1 WHERE id IN
            (SELECT id FROM catalog_queue WHERE source=? AND fallback=0 ORDER BY id LIMIT 16)",
                [key],
            )
            .map_err(|e| e.to_string())?;
        let overflow: bool = self
            .index
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM catalog_queue WHERE source=? AND fallback=0 LIMIT 1)",
                [key],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if overflow {
            self.index
                .connection()
                .execute(
                    "INSERT OR REPLACE INTO meta VALUES('invalidation_overflow','1')",
                    [],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn consume(
        &mut self,
        state: &Path,
        budget: &Budget,
        cancel: &AtomicBool,
    ) -> Result<CatchupProgress> {
        if budget.files > 256
            || budget.directory_entries > 2000
            || budget.records > 512
            || budget.bytes > 8 << 20
            || budget.deadline > Instant::now() + Duration::from_secs(2)
        {
            return Err("Invalid queue catch-up budget".into());
        }
        let mut progress = CatchupProgress::default();
        if !progress.ingestion.available(budget, cancel) {
            progress.ingestion.stopped = true;
            return Ok(progress);
        }
        self.catalog_schema()?;
        progress.pruned_paths = self.prune_catalog(budget, cancel)?;
        self.cleanup()?;
        self.sweep(state, budget, &mut progress.ingestion, cancel)?;
        let last:i64=self.index.connection().query_row("SELECT COALESCE((SELECT CAST(value AS INTEGER) FROM meta WHERE key='queue_cursor'),0)",[],|r|r.get(0)).map_err(|e|e.to_string())?;
        let mut rows = self.queue_page(last)?;
        if rows.is_empty() && last > 0 {
            rows = self.queue_page(0)?;
        }
        for (id, path, kind) in rows {
            if !progress.ingestion.available(budget, cancel)
                || budget.bytes - progress.ingestion.bytes < RECORD_BYTES + 1
            {
                progress.ingestion.stopped = true;
                break;
            }
            let record = queued_record(state, &path, &kind, &mut progress.ingestion, budget);
            let mut record = match record {
                Ok(record) => record,
                Err(_) => {
                    // An invalid/missing staged file is not a verified source.
                    // Keep a visible unavailable marker, not a fake empty match.
                    self.skip_queued(id, false)?;
                    self.queue_cursor(id)?;
                    progress.visited_paths += 1;
                    continue;
                }
            };
            if kind == "archive" {
                rename::normalize_archive(&mut record)?;
            }
            if record["rename_shadow"] == true {
                self.skip_queued(id, true)?;
                self.queue_cursor(id)?;
                progress.visited_paths += 1;
                continue;
            }
            if kind == "native" {
                record["agent"] = json!("dsh");
            }
            let target = record_target(&record);
            let source = match target.key() {
                Ok(key) => key,
                Err(_) => {
                    self.skip_queued(id, false)?;
                    self.queue_cursor(id)?;
                    progress.visited_paths += 1;
                    continue;
                }
            };
            let identity = Identity {
                name: target.name.clone(),
                run_id: target.run_id.clone(),
                conversation_id: target.conversation_id.clone(),
                archive_id: target.archive_id.clone(),
            };
            let updated = record["updated"]
                .as_f64()
                .or_else(|| record["created"].as_f64())
                .filter(|n| n.is_finite())
                .unwrap_or(0.0);
            self.index
                .connection()
                .execute(
                    "UPDATE catalog_queue SET identity=?,source=?,updated=?,skipped=0 WHERE id=?",
                    params![
                        serde_json::to_string(&identity).map_err(|e| e.to_string())?,
                        source,
                        updated,
                        id
                    ],
                )
                .map_err(|e| e.to_string())?;
            if kind == "native" {
                // The native engine remains authoritative; the existing bounded
                // query stays its fallback until an indexed adapter is admitted.
                if self.index.register(&target).is_ok() {
                    let _ = self.index.source_progress(&source, false, true);
                }
            } else {
                let remaining = Budget {
                    files: budget.files - progress.ingestion.files,
                    directory_entries: budget.directory_entries.saturating_sub(
                        progress
                            .ingestion
                            .directory_entries
                            .max(progress.ingestion.locator_probes),
                    ),
                    records: budget.records - progress.ingestion.records,
                    bytes: budget.bytes - progress.ingestion.bytes,
                    deadline: budget.deadline,
                };
                let read = self.step(state, &[identity], &remaining, cancel)?;
                progress.ingestion.files += read.files;
                progress.ingestion.directory_entries += read.directory_entries;
                progress.ingestion.locator_probes += read.locator_probes;
                progress.ingestion.records += read.records;
                progress.ingestion.bytes += read.bytes;
                progress.ingestion.targets += read.targets;
                progress.ingestion.stopped |= read.stopped;
                progress.ingestion.errors.extend(read.errors);
                progress.ingestion.errors.truncate(16);
            }
            let complete: Option<(bool, bool, bool)> = self
                .index
                .connection()
                .query_row(
                    "SELECT active,complete,unavailable FROM sources WHERE id=?",
                    [&source],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            // Known unavailable histories are served by fallback. Unconsumed
            // healthy cursors remain in the round-robin pending queue.
            let done = complete
                .is_some_and(|(active, complete, unavailable)| !active || complete || unavailable);
            let fallback = kind != "archive"
                || !complete.is_some_and(|(active, complete, unavailable)| {
                    active && complete && !unavailable
                });
            self.index
                .connection()
                .execute(
                    "UPDATE catalog_queue SET done=?,fallback=? WHERE id=?",
                    params![done, fallback, id],
                )
                .map_err(|e| e.to_string())?;
            self.queue_cursor(id)?;
            progress.visited_paths += 1;
        }
        let catalog = self.catalog_status()?;
        let coverage = self.index.status().map_err(|e| e.to_string())?;
        let ready = catalog["discovery"] == "complete"
            && catalog["pending_paths"] == 0
            && catalog["fallback_sources"] == 0
            && catalog["invalidation_overflow"] == false
            && catalog["unknown_metadata"] == false
            && catalog["invalid_metadata"] == false
            && coverage.pending_sources == 0
            && coverage.unavailable_sources == 0
            && !coverage.evicted;
        self.index.catalog_ready(ready).map_err(|e| e.to_string())?;
        Ok(progress)
    }

    fn skip_queued(&self, id: i64, shadow: bool) -> Result<()> {
        self.index.connection().execute("UPDATE catalog_queue SET done=1,identity=?,source=NULL,skipped=?,fallback=? WHERE id=?",params![if shadow{"null"}else{""},if shadow{2}else{1},!shadow,id]).map_err(|e|e.to_string())?;
        Ok(())
    }

    fn queue_cursor(&self, id: i64) -> Result<()> {
        self.index
            .connection()
            .execute(
                "INSERT OR REPLACE INTO meta VALUES('queue_cursor',?)",
                [id.to_string()],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn prune_catalog(&self, budget: &Budget, cancel: &AtomicBool) -> Result<usize> {
        if self.catalog_status()?["discovery"] != "complete" {
            return Ok(0);
        }
        let generation: Option<String> = self
            .index
            .connection()
            .query_row(
                "SELECT value FROM meta WHERE key='catalog_generation'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(generation) = generation else {
            return Ok(0);
        };
        let mut old = Vec::new();
        for op in ["<", ">"] {
            let sql = format!(
                "SELECT id,source FROM catalog_queue WHERE seen{op}? ORDER BY seen,id LIMIT 16"
            );
            let batch = self
                .index
                .connection()
                .prepare(&sql)
                .map_err(|e| e.to_string())?
                .query_map([&generation], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
                })
                .map_err(|e| e.to_string())?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| e.to_string())?;
            old.extend(batch);
        }
        let mut pruned = 0;
        for (id, source) in old.into_iter().take(16) {
            if cancel.load(Ordering::Relaxed) || Instant::now() >= budget.deadline {
                break;
            }
            if let Some(source) = source {
                self.retire(&source)?;
            }
            self.index
                .connection()
                .execute("DELETE FROM catalog_queue WHERE id=?", [id])
                .map_err(|e| e.to_string())?;
            pruned += 1;
        }
        Ok(pruned)
    }
    fn queue_page(&self, after: i64) -> Result<Vec<(i64, PathBuf, String)>> {
        self.index
            .connection()
            .prepare(
                "SELECT id,path,kind FROM catalog_queue WHERE done=0 AND id>? ORDER BY id LIMIT 16",
            )
            .map_err(|e| e.to_string())?
            .query_map([after], |r| {
                Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?), r.get(2)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())
    }
}

fn queued_record(
    state: &Path,
    path: &Path,
    kind: &str,
    progress: &mut Progress,
    budget: &Budget,
) -> Result<Value> {
    let parents = match kind {
        "binding" => vec![state.to_owned(), state.join("bindings")],
        "archive" => vec![state.join("archives")],
        "native" => vec![state.join("dsh/bindings")],
        _ => return Err("Unknown queued source kind".into()),
    };
    if !parents.iter().any(|parent| path.parent() == Some(parent)) || !candidate(path, kind) {
        return Err("Queued source is outside this state configuration".into());
    }
    progress.files += 1;
    let file = crate::platform::no_follow(fs::OpenOptions::new().read(true))
        .open(path)
        .map_err(|_| "Queued source unavailable")?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Queued source is not a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((RECORD_BYTES + 1).min(budget.bytes.saturating_sub(progress.bytes)) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    progress.bytes += bytes.len();
    if bytes.len() > RECORD_BYTES {
        return Err("Queued source exceeds its metadata budget".into());
    }
    let record: Value = serde_json::from_slice(&bytes).map_err(|_| "Queued source is invalid")?;
    if !record.is_object() || (kind != "native" && record["version"] != 1) {
        return Err("Queued source is not a binding".into());
    }
    if kind == "native" && record["backend"] != "dsh" {
        return Err("Native source identity changed".into());
    }
    Ok(record)
}

#[cfg(test)]
mod tests;
