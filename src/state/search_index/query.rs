//! Cursor pages contain freshly verified public history hits. Live/incomplete
//! histories retain the existing bounded scan, separately from the index cursor.
use super::*;
use regex::RegexBuilder;
use std::collections::BTreeMap;

const QUERY_ABI: &str = "content-search-query-v1";
const CANDIDATE_BUDGET: usize = 512;
const INDEX_REPLY_BYTES: usize = 2 << 20;
const FALLBACK_REPLY_BYTES: usize = 5 << 20;

enum SourceProof {
    Current(Value),
    Appended,
}

pub(in crate::state) fn read(query: &str, limit: usize, cursor: Option<&str>) -> Result<Value> {
    if query.chars().count() < 2 || !(1..=100).contains(&limit) {
        return Err("Indexed search requires 2..256 characters and a limit of 1..100".into());
    }
    lower_own_priority();
    let state = root();
    let mut store = match Store::open(&state.join("content-search-v1")) {
        Ok(store) => store,
        Err(_) if cursor.is_none() => return fallback_only(query, limit),
        Err(_) => return Err("Search index unavailable; start a new search".into()),
    };
    let cancel = AtomicBool::new(false);
    // Work exists only while a first-page caller is asking for search. Cursor
    // continuations preserve the insertion snapshot and schedule no catch-up.
    let mut catchup_failed = false;
    if cursor.is_none() {
        catchup_failed = store
            .discover(&state, &DiscoveryBudget::interactive(), &cancel)
            .is_err();
        if !catchup_failed {
            catchup_failed = store
                .consume(&state, &Budget::interactive(), &cancel)
                .is_err();
        }
    }
    let budget = Budget {
        files: 128,
        directory_entries: 2000,
        records: 512,
        bytes: 8 << 20,
        deadline: Instant::now() + Duration::from_millis(500),
    };
    let mut reply = match store.publish(&state, query, limit, cursor, &budget, &cancel) {
        Ok(reply) => reply,
        Err(_) if cursor.is_none() => return fallback_only(query, limit),
        Err(error) => return Err(error),
    };
    if catchup_failed || reply["index_coverage"]["state"] != "ready" {
        reply["fallback"] = bounded_fallback(query, limit)?;
        reply["truncated"] = json!(true);
    }
    Ok(reply)
}

fn bounded_fallback(query: &str, limit: usize) -> Result<Value> {
    let mut reply = search::legacy(query, limit)?;
    let Some(results) = reply["results"].as_array_mut() else {
        return Err("Invalid legacy search response".into());
    };
    let mut bytes = 2;
    let keep = results
        .iter()
        .take_while(|hit| {
            bytes += serde_json::to_vec(hit).map_or(FALLBACK_REPLY_BYTES + 1, |v| v.len() + 1);
            bytes <= FALLBACK_REPLY_BYTES
        })
        .count();
    if keep < results.len() {
        results.truncate(keep);
        reply["truncated"] = json!(true);
    }
    Ok(reply)
}
fn fallback_only(query: &str, limit: usize) -> Result<Value> {
    Ok(
        json!({"abi":QUERY_ABI,"query":query,"results":[],"next_cursor":null,
        "index_coverage":{"state":"unavailable"},"truncated":true,"unavailable_sessions":0,
        "fallback":bounded_fallback(query,limit)?}),
    )
}

impl Store {
    /// Bounded query/proof entry point for a future owned worker. It enumerates
    /// no catalog, starts no task, and retains only this page's verified targets.
    pub fn publish(
        &mut self,
        state: &Path,
        query: &str,
        limit: usize,
        cursor: Option<&str>,
        budget: &Budget,
        cancel: &AtomicBool,
    ) -> Result<Value> {
        if !(1..=100).contains(&limit)
            || budget.files > 128
            || budget.directory_entries > 2000
            || budget.bytes > 8 << 20
            || budget.deadline > Instant::now() + Duration::from_secs(2)
        {
            return Err("Invalid indexed publication budget".into());
        }
        if crate::platform::canonical(state).map_err(|e| e.to_string())?
            != crate::platform::canonical(self.directory.parent().ok_or("Invalid index path")?)
                .map_err(|e| e.to_string())?
        {
            return Err("Search-index query configuration changed".into());
        }
        self.wire_roots.clear();
        let pattern = RegexBuilder::new(&regex::escape(query))
            .case_insensitive(true)
            .build()
            .map_err(|e| e.to_string())?;
        let mut next = Some(match cursor {
            Some(cursor) => cursor.to_owned(),
            None => self
                .index
                .start_cursor(query, limit)
                .map_err(|e| e.to_string())?,
        });
        let mut results = Vec::new();
        let mut scopes = search::ArchiveScopes::new();
        let mut progress = Progress::default();
        let mut verified = BTreeMap::<String, Option<Value>>::new();
        let mut candidates = 0;
        let mut bytes = 2;
        let mut rejected = 0;
        while results.len() < limit
            && candidates <= CANDIDATE_BUDGET - zerus_content_search::CANDIDATES
            && next.is_some()
            && progress.available(budget, cancel)
        {
            let before = next.clone();
            let page = self
                .index
                .query_part(query, limit, 1, next.as_deref())
                .map_err(|e| e.to_string())?;
            candidates += page.candidates;
            next = page.next_cursor;
            let Some(hit) = page.hits.into_iter().next() else {
                continue;
            };
            // Unsupported adapters retain native/legacy reads. They cannot
            // acquire a retirement tombstone merely for lacking a fingerprint.
            if !["codex", "claude", "kimi"].contains(&hit.target.agent.as_str()) {
                continue;
            }
            let key = hit.target.key().map_err(|e| e.to_string())?;
            if !verified.contains_key(&key) {
                if budget.bytes.saturating_sub(progress.bytes) < RECORD_BYTES + 1 + 512
                    || budget.files.saturating_sub(progress.files) < 2
                {
                    next = before;
                    break;
                }
                let identity = Identity {
                    name: hit.target.name.clone(),
                    run_id: hit.target.run_id.clone(),
                    conversation_id: hit.target.conversation_id.clone(),
                    archive_id: hit.target.archive_id.clone(),
                };
                let proof =
                    self.verify_source(state, &identity, &key, &mut progress, budget, cancel);
                let target = match proof {
                    Ok(SourceProof::Current(target)) => Some(target),
                    Ok(SourceProof::Appended) => {
                        self.revisit(&key)?;
                        rejected += 1;
                        None
                    }
                    Err(_) => {
                        self.retire(&key)?;
                        rejected += 1;
                        None
                    }
                };
                verified.insert(key.clone(), target);
            }
            let Some(record) = verified.get(&key).and_then(Clone::clone) else {
                continue;
            };
            let Some(mut hit) = search::indexed_hit(pattern.clone(), hit, &record) else {
                continue;
            };
            scopes.enrich(&mut hit);
            let size = serde_json::to_vec(&hit).map_err(|e| e.to_string())?.len() + 1;
            if bytes + size > INDEX_REPLY_BYTES {
                next = before;
                break;
            }
            bytes += size;
            results.push(hit);
        }
        let status = self.status()?;
        let ready = status["coverage"]["state"] == "ready"
            && !scopes.incomplete
            && status["catalog"]["discovery"] == "complete"
            && status["catalog"]["fallback_sources"] == 0
            && status["catalog"]["invalidation_overflow"] == false
            && rejected == 0;
        let mut coverage = status["coverage"].clone();
        if !ready && coverage["state"] == "ready" {
            coverage["state"] = json!("indexing");
        }
        Ok(
            json!({"abi":QUERY_ABI,"query":query,"results":results,"next_cursor":next,
            "index_coverage":coverage,"catalog":status["catalog"],"truncated":next.is_some()||!ready,
            "unavailable_sessions":rejected,"candidates":candidates,"proof":progress,"fallback":null}),
        )
    }

    fn revisit(&self, key: &str) -> Result<()> {
        self.index
            .source_progress(key, false, false)
            .map_err(|e| e.to_string())?;
        let catalog: bool = self
            .index
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='catalog_source')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if catalog {
            self.index
                .connection()
                .execute(
                    "UPDATE catalog_queue SET done=0,fallback=1 WHERE id IN
                (SELECT id FROM catalog_queue WHERE source=? ORDER BY id LIMIT 16)",
                    [key],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn verify_source(
        &mut self,
        state: &Path,
        identity: &Identity,
        key: &str,
        progress: &mut Progress,
        budget: &Budget,
        cancel: &AtomicBool,
    ) -> Result<SourceProof> {
        let record = binding(state, identity, progress, budget)?;
        let encoded: String = self
            .index
            .connection()
            .query_row(
                "SELECT cursor FROM input_cursors WHERE source=? AND length(cursor)<=16384",
                [key],
                |r| r.get(0),
            )
            .map_err(|_| "Source cursor unavailable")?;
        let cursor: Cursor = serde_json::from_str(&encoded).map_err(|_| "Invalid source cursor")?;
        let stamp = match cursor.stamp.as_ref() {
            Some(stamp) => stamp,
            None if record["agent"] == "kimi" => return Ok(SourceProof::Appended),
            None => return Err("Source fingerprint unavailable".into()),
        };
        if record["agent"] == "kimi" {
            if !self
                .verify_wire(&record, &cursor, budget, progress, cancel)
                .unwrap_or(false)
            {
                return Ok(SourceProof::Appended);
            }
        } else if stamp.path != Path::new(string(&record, "transcript")) {
            return Err("Source path changed".into());
        }
        progress.files += 1;
        let mut file = crate::platform::no_follow(fs::OpenOptions::new().read(true))
            .open(&stamp.path)
            .map_err(|_| "Source unavailable")?;
        let (identity, metadata) =
            crate::platform::FileIdentity::with_metadata(&file).map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.len() < stamp.len
            || identity.device != stamp.device
            || identity.inode != stamp.inode
            || (metadata.len() == stamp.len
                && (identity.modified != stamp.modified || identity.modified_nanos != stamp.nanos))
        {
            return Err("Source fingerprint changed".into());
        }
        let head = sample(&mut file, 0, stamp.head_bytes, progress, budget)?;
        let tail = sample(
            &mut file,
            cursor.offset.saturating_sub(256),
            cursor.offset.min(256) as usize,
            progress,
            budget,
        )?;
        if stamp.head != head || stamp.tail != tail {
            return Err("Source text changed".into());
        }
        if metadata.len() > stamp.len {
            // The indexed prefix still has the same identity/fingerprints.
            // Do not fence a healthy append forever: make its saved input
            // cursor pending, and let the next bounded demand ingest it.
            return Ok(SourceProof::Appended);
        }
        Ok(SourceProof::Current(record))
    }
}

#[cfg(test)]
mod tests;
