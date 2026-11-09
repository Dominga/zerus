//! Fresh ID input exclusion. No legacy alias, inferred origin or repair path.
use super::*;

fn valid(record: &Value) -> Result<&str> {
    let id = string(record, "session_id");
    if !is_id_layout_record(record, id)
        || uuid::Uuid::parse_str(id).is_ok_and(|id| id.is_nil())
        || record["version"] != 1
        || !AGENTS.contains(&string(record, "agent"))
        || string(record, "name").len() > 2048
        || !string(record, "name").starts_with(&format!("{}/", string(record, "agent")))
        || record.get("layout_transition").is_some()
    {
        return Err("This session requires its original input kind evidence".into());
    }
    Ok(id)
}

fn binding(record: &Value) -> Value {
    json!([
        record["version"],
        record["agent"],
        record["session_id"],
        record["layout"],
        record["name"],
        record["run_id"],
        record["conversation_id"],
        record["terminal_name"],
        record.get("layout_transition")
    ])
}

/// Exclusion only. Blank conversation IDs are allowed; this is neither native
/// readiness nor permission to deliver input. Caller keeps all such guards.
pub(in crate::state) struct Fresh {
    state: Directory,
    inputs: Directory,
    file: File,
    proof: reservation::SidHeld,
    writer: Option<(File, Value)>,
    id: String,
    binding: Option<Value>,
    until: Instant,
}

impl Fresh {
    /// Dormant publisher only. The actual Root writer is checked BEFORE any
    /// input namespace write. One new SID lock and proof are retained forever,
    /// even if publication fails. A retry can never reuse this pair.
    pub(in crate::state) fn create_under(
        writer: &Guard,
        record: &Value,
        until: Instant,
    ) -> Result<Self> {
        let id = valid(record)?;
        deadline(until)?;
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let actual = state.read_private(".lock")?;
        if crate::platform::FileIdentity::of(&actual).map_err(|e| e.to_string())?
            != writer.identity().map_err(|e| e.to_string())?
        {
            return Err("The original writer changed before fresh input preparation".into());
        }
        let writer_version = file_version(&actual)?;
        let sessions = state.child("sessions")?;
        if !sessions.private()?
            || sessions
                .read_private_optional(&format!("{id}.json"))?
                .is_some()
        {
            return Err("This SID is already published or its namespace is unavailable".into());
        }
        let inputs = reservation::inputs_for_fresh(&state, id)?;
        deadline(until)?;
        cut_write()?;
        let file = inputs.write_new_file(&format!("{id}.lock"), &[])?;
        file.try_lock_exclusive()
            .map_err(|e| format!("The new SID input is unavailable: {e}"))?;
        let proof = reservation::SidHeld::create_fresh(
            &state,
            &inputs,
            id,
            identity(&file.metadata().map_err(|e| e.to_string())?),
        )?;
        let held = Self {
            state,
            inputs,
            file,
            proof,
            writer: Some((actual, writer_version)),
            id: id.into(),
            binding: None,
            until,
        };
        held.verify()?;
        Ok(held)
    }

    /// Opens existing objects only. A missing marker or one remaining link
    /// never selects Fresh: the exact SID proof must explicitly say Fresh.
    pub(in crate::state) fn acquire(record: &Value, until: Instant) -> Result<Self> {
        let id = valid(record)?;
        deadline(until)?;
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let inputs = state.child("native-input")?;
        let proof = reservation::SidHeld::load(&state, &inputs, id)?;
        proof.fresh()?;
        let file = inputs.existing_private_lock(&format!("{id}.lock"))?;
        file.try_lock_exclusive()
            .map_err(|e| format!("The original SID input is busy: {e}"))?;
        let sessions = state.child("sessions")?;
        let actual = read_original(&sessions, &format!("{id}.json"))?;
        valid(&actual)?;
        // Compare borrowed caller fields before copying the bounded native
        // record's binding; no unbounded caller value enters the held guard.
        if [
            "version",
            "agent",
            "session_id",
            "layout",
            "name",
            "run_id",
            "conversation_id",
            "terminal_name",
        ]
        .iter()
        .any(|key| actual[*key] != record[*key])
        {
            return Err("The fresh session identity changed".into());
        }
        let held = Self {
            state,
            inputs,
            file,
            proof,
            writer: None,
            id: id.into(),
            binding: Some(binding(&actual)),
            until,
        };
        held.verify()?;
        Ok(held)
    }

    pub(in crate::state) fn verify(&self) -> Result<()> {
        deadline(self.until)?;
        self.state.verify()?;
        self.inputs.verify()?;
        self.proof.verify(&self.state, &self.inputs)?;
        self.proof.fresh()?;
        if let Some((file, version)) = &self.writer {
            if file_version(file)? != *version
                || file_version(&self.state.read_private(".lock")?)? != *version
            {
                return Err("The original publication writer changed".into());
            }
        }
        let name = format!("{}.lock", self.id);
        let current = self.inputs.existing_private_lock(&name)?;
        if !self.state.private()?
            || !self.inputs.private()?
            || crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path
            || !crate::platform::verify_private_file(&self.file).map_err(|e| e.to_string())?
            || self.file.metadata().map_err(|e| e.to_string())?.len() != 0
            || identity(&self.file.metadata().map_err(|e| e.to_string())?) != self.proof.original()
            || identity(&current.metadata().map_err(|e| e.to_string())?) != self.proof.original()
        {
            return Err("The original fresh SID input object changed".into());
        }
        if let Some(expected) = &self.binding {
            let sessions = self.state.child("sessions")?;
            let current = read_original(&sessions, &format!("{}.json", self.id))?;
            valid(&current)?;
            if !sessions.private()? || binding(&current) != *expected {
                return Err("The original fresh session binding changed".into());
            }
        }
        deadline(self.until)
    }
}

impl Drop for Fresh {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
