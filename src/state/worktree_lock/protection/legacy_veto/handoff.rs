//! One original advisory veto and one census-bound remaining holder. An
//! interrupted handoff is review-only; no recovery or release is inferred.
use super::*;
use sha2::{Digest, Sha256};

// A Ready rename may be visible even if its publication reports Unknown.
// Keep this separately durable negative fence until every final check passes.
// Drop never removes it, and no reader repairs or resumes an incomplete handoff.
struct Fence<'a> {
    directory: &'a Directory,
    name: String,
    file: fs::File,
    version: Value,
}
impl<'a> Fence<'a> {
    fn create(directory: &'a Directory, name: String, holder: &str) -> Result<Self> {
        storage::cut_write()?;
        let file = directory.write_new_file(
            &name,
            format!("Pending shared handoff to {holder}\n").as_bytes(),
        )?;
        let version = version(&file)?;
        let held = Self {
            directory,
            name,
            file,
            version,
        };
        held.verify()?;
        Ok(held)
    }
    fn verify(&self) -> Result<()> {
        self.directory.verify()?;
        let current = self.directory.read_private(&self.name)?;
        if self.version != version(&self.file)? || self.version != version(&current)? {
            return Err("The original shared-handoff fence changed; retain the Git veto".into());
        }
        Ok(())
    }
    fn retire(&self) -> Result<()> {
        self.verify()?;
        storage::cut_write()?;
        let name = std::ffi::CString::new(self.name.as_str()).map_err(|e| e.to_string())?;
        // SAFETY: one internally derived component under the original held
        // receipt directory, freshly checked against this exact retained file.
        if unsafe { libc::unlinkat(self.directory.descriptor(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        // Ready and its namespace are already durable. Persisting this
        // negative fence's absence is unnecessary for safety: a fence that
        // reappears after a crash only keeps the original Git veto for review.
        self.directory.verify()
    }
}

struct Holder {
    directories: Vec<Directory>,
    provider: Option<crate::platform::FileLock>,
    name: String,
    file: fs::File,
    version: Value,
    marked: Option<Vec<u8>>,
}

impl Holder {
    fn directory(&self) -> &Directory {
        self.directories.last().unwrap()
    }

    fn retain(protection: &Protection, holder: &worktree_cleanup::Reference) -> Result<Self> {
        let binding = holder
            .binding
            .as_ref()
            .ok_or("The shared holder has no saved binding")?;
        let (provider, id_layout) = match binding.directory.as_str() {
            "" | "bindings" => (false, false),
            "sessions" => (false, true),
            "dsh/bindings" | "opencode/bindings" => (true, false),
            "dsh/sessions" | "opencode/sessions" => (true, true),
            _ => return Err("The shared holder has an unknown binding namespace".into()),
        };
        if !session_id::canonical(&holder.session_id)
            || uuid::Uuid::parse_str(&holder.session_id).is_ok_and(|id| id.is_nil())
            || binding.file
                != format!(
                    "{}.json",
                    if id_layout {
                        holder.session_id.clone()
                    } else {
                        storage::label_key(&holder.name)
                    }
                )
        {
            return Err("The shared holder's original binding name changed".into());
        }
        let state = Directory::open(&protection.state.path)?;
        if state.object()? != protection.state.object()? {
            return Err("The original state root changed".into());
        }
        let mut directories = vec![state];
        for component in binding.directory.split('/').filter(|s| !s.is_empty()) {
            directories.push(directories.last().unwrap().child(component)?);
        }
        let provider = if provider {
            let file = directories[1].open_private_lock("bindings.lock")?;
            file.try_lock_exclusive()
                .map_err(|_| "The shared holder is busy; the original veto stays")?;
            let file = crate::platform::FileLock::held(file);
            #[cfg(test)]
            crate::platform::lock_tests::reached("v2-provider-acquired");
            Some(file)
        } else {
            None
        };
        let directory = directories.last().unwrap();
        let mut file = directory.read_private(&binding.file)?;
        let version = version(&file)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(storage::ID_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > storage::ID_RECORD_BYTES
            || format!("{:x}", Sha256::digest(&bytes)) != binding.digest
        {
            return Err("The shared holder changed after the reference census".into());
        }
        let mut record: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if storage::in_id_layout(&record) != id_layout
            || record["worktree_owner"] != protection.owner
            || string(&record, "session_id") != holder.session_id
            || string(&record, "name") != holder.name
        {
            return Err("The shared holder's original identity changed".into());
        }
        let marked = if let Some(marker) = record.get("worktree_protection") {
            if marker != &protection.owner["id"] {
                return Err("The shared holder's protection marker changed".into());
            }
            None
        } else {
            if id_layout {
                return Err("The shared ID holder's protection marker is missing".into());
            }
            record["worktree_protection"] = protection.owner["id"].clone();
            let bytes = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > storage::ID_RECORD_BYTES {
                return Err("The shared binding exceeds its bound".into());
            }
            Some(bytes)
        };
        let held = Self {
            directories,
            provider,
            name: binding.file.clone(),
            file,
            version,
            marked,
        };
        held.verify()?;
        Ok(held)
    }

    fn verify(&self) -> Result<()> {
        for directory in &self.directories {
            directory.verify()?;
        }
        if let Some(provider) = &self.provider {
            if object(&self.directories[1].read_private("bindings.lock")?)?
                != exclusion_object(provider)?
            {
                return Err("The shared holder's original exclusion changed".into());
            }
        }
        let current = self.directory().read_private(&self.name)?;
        if self.version != version(&self.file)? || self.version != version(&current)? {
            return Err("The shared holder's original record changed".into());
        }
        Ok(())
    }

    fn mark(&mut self) -> Result<()> {
        self.verify()?;
        if let Some(bytes) = self.marked.take() {
            storage::cut_write()?;
            let file = self.directory().replace_file_retained(&self.name, &bytes)?;
            self.version = version(&file)?;
            self.file = file;
        }
        self.verify()
    }
}

pub(super) fn transfer(
    mut protection: Protection,
    holder: &worktree_cleanup::Reference,
) -> Result<()> {
    protection.verify()?;
    let mut remaining = Holder::retain(&protection, holder)?;
    protection.verify()?;
    let original_version = protection.before["lock_version"].clone();
    let mut intent = protection.before.clone();
    intent["status"] = json!("handoff");
    intent["pending_holder"] = json!(holder.session_id);
    if intent.to_string().len() > 16384 {
        return Err("The handoff proof exceeds its bound".into());
    }
    remaining.verify()?;
    let fence = Fence::create(
        &protection.receipts,
        protection.handoff_name(),
        &holder.session_id,
    )?;
    protection.verify_legacy_version(&original_version, "ready")?;
    remaining.verify()?;
    fence.verify()?;
    write_receipt(&protection.receipts, &protection.receipt, &intent, true)?;
    protection.before = intent;
    let check = |protection: &Protection, remaining: &Holder, version: &Value| -> Result<()> {
        protection.verify_legacy_version(version, "handoff")?;
        remaining.verify()?;
        fence.verify()
    };
    check(&protection, &remaining, &original_version)?;
    remaining.mark()?;
    #[cfg(test)]
    crate::platform::lock_tests::reached("v2-holder-marked");
    check(&protection, &remaining, &original_version)?;
    storage::cut_write()?;
    // Canonical UUID reasons have the same fixed length. Never truncate,
    // rename, unlink, recreate or change the mode of this Git-created veto.
    protection
        .lock
        .seek(SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    protection
        .lock
        .write_all(format!("zerus:{}\n", holder.session_id).as_bytes())
        .map_err(|e| e.to_string())?;
    protection.lock.sync_all().map_err(|e| e.to_string())?;
    protection.reason = holder.session_id.clone();
    let advanced = version(&protection.lock)?;
    if advanced.as_array().map(|v| &v[..6]) != original_version.as_array().map(|v| &v[..6]) {
        return Err("The original Git veto's object or mode changed".into());
    }
    #[cfg(test)]
    crate::platform::lock_tests::reached("v2-reason-written");
    check(&protection, &remaining, &advanced)?;
    let mut ready = protection.before.clone();
    ready["status"] = json!("ready");
    ready["lock_version"] = advanced;
    ready.as_object_mut().unwrap().remove("pending_holder");
    write_receipt(&protection.receipts, &protection.receipt, &ready, true)?;
    protection.before = ready;
    #[cfg(test)]
    crate::platform::lock_tests::reached("v2-ready-written");
    protection.verify_legacy_version(&protection.before["lock_version"], "ready")?;
    remaining.verify()?;
    fence.retire()
}
