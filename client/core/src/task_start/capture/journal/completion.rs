//! Exact historical metadata plus a native source CAS. No canonical reply is
//! reconstructed and no task, input, plan, admission or worker can be created.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settlement {
    Applied,
    AlreadyApplied,
    ChangedOriginal,
}
impl Record {
    fn query(&self, lookup: &wire::Lookup) -> wire::Query {
        wire::Query {
            request_id: lookup.request_id.clone(),
            scope: lookup.scope.clone(),
            reconcile_id: self.intent.reconcile_id.clone(),
            binding: self.intent.binding.clone(),
        }
    }
    fn submitted(&self, query: &wire::Query, lookup: &wire::Lookup) -> bool {
        query.binding == self.intent.binding
            && query.reconcile_id == self.intent.reconcile_id
            && lookup.matches(query)
            && lookup
                .attempt
                .as_ref()
                .is_some_and(|a| a.submitted(query).is_some())
    }
    pub(super) fn valid_completion(&self) -> bool {
        match (self.phase, &self.completion) {
            (Phase::Prepared | Phase::OfferedUnknown | Phase::CancelledBeforeOffer, None) => true,
            (Phase::SettlementPrepared | Phase::Completed, Some(lookup)) => {
                self.submitted(&self.query(lookup), lookup)
            }
            _ => false,
        }
    }
}
impl Saved {
    /// Called only after the native host verifies the actual selected Service
    /// exchange. `current` retains/rechecks that same native connection; no
    /// serialized flag or historical lookup can substitute for it.
    pub fn settle_submitted(
        &mut self,
        query: &wire::Query,
        lookup: &wire::Lookup,
        mut current: impl FnMut() -> bool,
    ) -> Result<Settlement, Error> {
        if !matches!(
            self.phase(),
            Phase::OfferedUnknown | Phase::SettlementPrepared | Phase::Completed
        ) || !self.disk.record.submitted(query, lookup)
            || !current()
        {
            return Err(Error::Unavailable);
        }
        let lock = self.slot.lock(false)?;
        self.disk
            .current(&self.slot)
            .map_err(|_| Error::Unavailable)?;
        lock_current(&self.slot, &lock).map_err(|_| Error::Unavailable)?;
        let key = &self.disk.record.intent.binding.key;
        let name = outbox::path(Path::new(""), &key.project, &key.task)
            .file_name()
            .ok_or(Error::Unavailable)?
            .to_owned();
        let path = self
            .slot
            .selected
            .join("task-start/task-delivery")
            .join(&name);
        // Coordinate the existing native Start/release/checkpoint writers in
        // this process; native object/version checks still fence replacements.
        let _turn = outbox::Turn::take(&path).ok_or(Error::Busy)?;
        let mut next_value = wire::original_value(&self.disk.record.intent.original)
            .map_err(|_| Error::Unavailable)?;
        next_value["phase"] = serde_json::json!("started");
        next_value["error"] = serde_json::json!("");
        let next_bytes = serde_json::to_vec(&next_value).map_err(|_| Error::Unavailable)?;
        if next_bytes.len() > wire::SOURCE_BYTES {
            return Err(Error::Unavailable);
        }

        let observed = observe_source(self, &name, &next_bytes)?;
        if observed == Observed::Changed {
            return Err(Error::Unavailable);
        }
        if self.phase() == Phase::Completed {
            return Ok(if observed == Observed::Next {
                Settlement::AlreadyApplied
            } else {
                Settlement::ChangedOriginal
            });
        }
        let mut next = self.disk.record.clone();
        next.phase = Phase::SettlementPrepared;
        next.completion = Some(lookup.clone());
        self.disk = publish(
            &self.slot,
            &self.disk,
            &lock,
            &next,
            || {
                if !current() {
                    return Err(refused());
                }
                lock_current(&self.slot, &lock)
            },
            |file, _| file.sync_all(),
        )?;
        if !current() {
            return Err(Error::Unavailable);
        }
        let outcome = match observe_source(self, &name, &next_bytes)? {
            Observed::Original => {
                replace_source(self, &name, &next_bytes, &mut current)?;
                Settlement::Applied
            }
            Observed::Next => Settlement::AlreadyApplied,
            Observed::NewOriginal => Settlement::ChangedOriginal,
            Observed::Changed => return Err(Error::Unavailable),
        };
        if !current() {
            return Err(Error::Unavailable);
        }
        let mut next = self.disk.record.clone();
        next.phase = Phase::Completed;
        self.disk = publish(
            &self.slot,
            &self.disk,
            &lock,
            &next,
            || {
                if !current() {
                    return Err(refused());
                }
                lock_current(&self.slot, &lock)
            },
            |file, _| file.sync_all(),
        )?;
        Ok(outcome)
    }
    /// One separately explicit new Check, only for a genuinely new native
    /// task-delivery identity after the old exact result was completed.
    pub fn prepare_new_original(
        self,
        source: Captured,
        proposal: &CheckId,
    ) -> Result<Journal, Error> {
        if self.phase() != Phase::Completed
            || self.disk.record.intent.binding.key.request_id == source.binding.key.request_id
        {
            return Err(Error::Unavailable);
        }
        let lock = self.slot.lock(false)?;
        self.disk
            .current(&self.slot)
            .map_err(|_| Error::Unavailable)?;
        source.recheck().map_err(|_| Error::Unavailable)?;
        if source.managed.path() != self.slot.managed.path()
            || source.binding.key.project != self.disk.record.intent.binding.key.project
            || source.binding.key.task != self.disk.record.intent.binding.key.task
        {
            return Err(Error::Unavailable);
        }
        proposal.check_before(Some(&self.disk))?;
        let record = Record {
            schema: 1,
            phase: Phase::Prepared,
            version: source.version.clone(),
            previous_cancelled: self.disk.record.previous_cancelled.clone(),
            completion: None,
            intent: source
                .intent(proposal.as_str())
                .map_err(|_| Error::Unavailable)?,
        };
        let disk = publish(
            &self.slot,
            &self.disk,
            &lock,
            &record,
            || {
                source.recheck()?;
                lock_current(&self.slot, &lock)
            },
            |f, _| f.sync_all(),
        )?;
        let journal = Journal {
            saved: Self {
                slot: self.slot,
                disk,
            },
            source,
            lock,
            poisoned: false,
        };
        journal.check_current()?;
        Ok(journal)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Observed {
    Original,
    Next,
    NewOriginal,
    Changed,
}
fn observe_source(saved: &Saved, name: &OsStr, next: &[u8]) -> Result<Observed, Error> {
    saved.slot.current().map_err(|_| Error::Unavailable)?;
    let record = &saved.disk.record;
    if saved.slot.versions
        != [
            record.version.store.clone(),
            record.version.root.clone(),
            record.version.folder.clone(),
        ]
    {
        return Err(Error::Unavailable);
    }
    let file = open_at(&saved.slot.folder, name, false).map_err(|_| Error::Unavailable)?;
    let marker = open_at(
        &saved.slot.folder,
        &Path::new(name).with_extension("marker").into_os_string(),
        false,
    )
    .map_err(|_| Error::Unavailable)?;
    let (bytes, version) = read(&file, wire::SOURCE_BYTES).map_err(|_| Error::Unavailable)?;
    let (marker_bytes, marker_version) = read(&marker, 128).map_err(|_| Error::Unavailable)?;
    saved.slot.current().map_err(|_| Error::Unavailable)?;
    if bytes == record.intent.original.as_bytes()
        && version == record.version.record
        && marker_bytes == record.intent.binding.key.request_id.as_bytes()
        && marker_version == record.version.marker
    {
        return Ok(Observed::Original);
    }
    if bytes == next
        && marker_bytes == record.intent.binding.key.request_id.as_bytes()
        && marker_version == record.version.marker
    {
        return Ok(Observed::Next);
    }
    let value = wire::original_value(std::str::from_utf8(&bytes).map_err(|_| Error::Unavailable)?)
        .map_err(|_| Error::Unavailable)?;
    let key = &record.intent.binding.key;
    if crate::task_start::checked(&value, &key.project, &key.task).is_ok()
        && value["id"]
            .as_str()
            .is_some_and(|id| id != key.request_id && id.as_bytes() == marker_bytes)
    {
        return Ok(Observed::NewOriginal);
    }
    Ok(Observed::Changed)
}
fn replace_source(
    saved: &Saved,
    name: &OsStr,
    bytes: &[u8],
    mut current: impl FnMut() -> bool,
) -> Result<(), Error> {
    if !current() || observe_source(saved, name, bytes)? != Observed::Original {
        return Err(Error::Unavailable);
    }
    let temp = OsString::from(format!(
        "{}.{}.tmp",
        name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let mut file = saved
        .slot
        .create(&temp, true)
        .map_err(|_| Error::Unavailable)?;
    let result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        if !current() || observe_source(saved, name, bytes).ok() != Some(Observed::Original) {
            return Err(refused());
        }
        let opened = open_at(&saved.slot.folder, &temp, false)?;
        if file_version(&opened, wire::SOURCE_BYTES)? != file_version(&file, wire::SOURCE_BYTES)? {
            return Err(refused());
        }
        let from = CString::new(temp.as_bytes()).map_err(|_| refused())?;
        let to = CString::new(name.as_bytes()).map_err(|_| refused())?;
        // SAFETY: exact verified original plus native private temporary, one held directory.
        if unsafe {
            libc::renameat(
                saved.slot.folder.as_raw_fd(),
                from.as_ptr(),
                saved.slot.folder.as_raw_fd(),
                to.as_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        saved.slot.folder.sync_all()
    })();
    if result.is_err() {
        if let (Ok(opened), Ok(held)) = (
            open_at(&saved.slot.folder, &temp, false),
            file_version(&file, wire::SOURCE_BYTES),
        ) {
            if file_version(&opened, wire::SOURCE_BYTES).ok() == Some(held) {
                if let Ok(name) = CString::new(temp.as_bytes()) {
                    // SAFETY: only the same verified original private temporary.
                    let _ =
                        unsafe { libc::unlinkat(saved.slot.folder.as_raw_fd(), name.as_ptr(), 0) };
                }
            }
        }
        return Err(Error::PublicationUnknown);
    }
    if !current() || observe_source(saved, name, bytes)? != Observed::Next {
        return Err(Error::PublicationUnknown);
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
