//! Kimi's exact public-wire locator. Only bounded workspace names/stamps live
//! in memory; the cursor persists one root fingerprint and one verified path.
use super::*;
use std::sync::Arc;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Locator {
    root: PathBuf,
    stamp: String,
    path: PathBuf,
}
pub(super) struct Snapshot {
    root: PathBuf,
    stamp: String,
    workspaces: Vec<PathBuf>,
}

fn directory_id(path: &Path) -> Result<String> {
    let (id, metadata) =
        crate::platform::FileIdentity::at(path).map_err(|_| "Kimi source root unavailable")?;
    if !metadata.is_dir() {
        return Err("Kimi source root changed kind".into());
    }
    Ok(format!(
        "{}:{}:{}:{}",
        id.device, id.inode, id.modified, id.modified_nanos
    ))
}
fn path_budget(path: &Path, progress: &mut Progress, budget: &Budget) -> bool {
    let Some(path) = path.to_str() else {
        return false;
    };
    if path.len() + 512 > budget.bytes.saturating_sub(progress.bytes) {
        return false;
    }
    progress.bytes += path.len();
    true
}

impl Store {
    fn wire_snapshot(
        &mut self,
        record: &Value,
        budget: &Budget,
        progress: &mut Progress,
        cancel: &AtomicBool,
    ) -> Result<Option<Arc<Snapshot>>> {
        let root = crate::platform::canonical(questions::kimi_home(record).join("sessions"))
            .map_err(|_| "Kimi session root unavailable")?;
        if let Some(snapshot) = self.wire_roots.get(&root) {
            return Ok(Some(snapshot.clone()));
        }
        let before = directory_id(&root)?;
        let mut workspaces = Vec::new();
        let mut stamps = Vec::new();
        let entries = fs::read_dir(&root).map_err(|_| "Kimi workspace discovery unavailable")?;
        for entry in entries {
            if !progress.can_read(budget, cancel)
                || progress.directory_entries >= budget.directory_entries
            {
                progress.stopped = true;
                return Ok(None);
            }
            progress.directory_entries += 1;
            let entry = entry.map_err(|_| "Kimi workspace discovery incomplete")?;
            let path = entry.path();
            if !path_budget(&path, progress, budget) {
                progress.stopped = true;
                return Ok(None);
            }
            let kind = entry
                .file_type()
                .map_err(|_| "Kimi workspace identity unavailable")?;
            if kind.is_symlink() {
                return Err("Linked Kimi workspace requires native fallback".into());
            }
            if !kind.is_dir() {
                continue;
            }
            if crate::platform::canonical(&path).map_err(|_| "Kimi workspace unavailable")? != path
            {
                return Err("Kimi workspace identity changed".into());
            }
            stamps.push((path.clone(), directory_id(&path)?));
            workspaces.push(path);
        }
        if !progress.can_read(budget, cancel) || before != directory_id(&root)? {
            progress.stopped = true;
            return Ok(None);
        }
        workspaces.sort();
        stamps.sort();
        // Child workspace stamps matter: adding a second copy of a conversation
        // under an existing workspace does not change the sessions root mtime.
        let stamp = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(&root, before, stamps)).map_err(|e| e.to_string())?
            )
        );
        let snapshot = Arc::new(Snapshot {
            root: root.clone(),
            stamp,
            workspaces,
        });
        self.wire_roots.insert(root, snapshot.clone());
        Ok(Some(snapshot))
    }

    pub(super) fn locate_wire(
        &mut self,
        record: &Value,
        budget: &Budget,
        progress: &mut Progress,
        cancel: &AtomicBool,
    ) -> Result<Option<(PathBuf, Locator)>> {
        let session = string(record, "conversation_id");
        if !questions::token(session) {
            return Err("Invalid Kimi conversation identity".into());
        }
        let Some(snapshot) = self.wire_snapshot(record, budget, progress, cancel)? else {
            return Ok(None);
        };
        let mut found = None;
        for workspace in &snapshot.workspaces {
            if !progress.can_read(budget, cancel)
                || progress.locator_probes >= budget.directory_entries
            {
                progress.stopped = true;
                return Ok(None);
            }
            progress.locator_probes += 1;
            let path = workspace.join(session).join("agents/main/wire.jsonl");
            if !path_budget(&path, progress, budget) {
                progress.stopped = true;
                return Ok(None);
            }
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Ok(metadata) if metadata.is_file() => (),
                _ => return Err("Kimi conversation path unavailable or linked".into()),
            }
            if crate::platform::canonical(&path)
                .map_err(|_| "Kimi conversation path unavailable")?
                != path
            {
                return Err("Kimi conversation path contains an alias".into());
            }
            if found.is_some() {
                return Err("Kimi conversation exists in multiple workspace histories".into());
            }
            found = Some(path);
        }
        let Some(path) = found else {
            return Err("Exact Kimi wire history unavailable".into());
        };
        let locator = Locator {
            root: snapshot.root.clone(),
            stamp: snapshot.stamp.clone(),
            path: path.clone(),
        };
        Ok(Some((path, locator)))
    }

    pub(super) fn verify_wire(
        &mut self,
        record: &Value,
        cursor: &Cursor,
        budget: &Budget,
        progress: &mut Progress,
        cancel: &AtomicBool,
    ) -> Result<bool> {
        let Some(locator) = &cursor.wire else {
            return Ok(false);
        };
        // Probe every exact conversation path again: a wire can be created
        // beneath a pre-existing conversation directory without changing either
        // the workspace or sessions-root stamp.
        let Some((path, current)) = self.locate_wire(record, budget, progress, cancel)? else {
            return Ok(false);
        };
        Ok(locator.root == current.root
            && locator.stamp == current.stamp
            && locator.path == path
            && cursor.stamp.as_ref().is_some_and(|s| s.path == path))
    }
}

#[cfg(test)]
mod tests;
