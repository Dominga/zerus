//! Original input exclusion from one completed metadata transaction.
//!
//! Dormant: this is not a native permission or delivery acceptance. A future
//! caller must keep its existing live identity, writer, consent and per-effect
//! checks. Neither acquiring this object nor decoding its journal executes work.
use super::*;

struct Evidence {
    file: fs::File,
    version: Value,
    name: &'static str,
}
impl Evidence {
    fn open(directory: &Directory, name: &'static str) -> Result<Self> {
        directory.verify()?;
        let file = directory.read_private(name)?;
        let version = file_version(&file)?;
        let held = Self {
            file,
            version,
            name,
        };
        held.verify(directory)?;
        Ok(held)
    }

    fn read(&mut self, directory: &Directory, cap: usize, until: Instant) -> Result<Vec<u8>> {
        deadline(until)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut self.file)
            .take(cap as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > cap {
            return Err("The original input evidence exceeds its bound".into());
        }
        self.verify(directory)?;
        deadline(until)?;
        Ok(bytes)
    }

    fn verify(&self, directory: &Directory) -> Result<()> {
        directory.verify()?;
        if !directory.private()?
            || file_version(&self.file)? != self.version
            || file_version(&directory.read_private(self.name)?)? != self.version
        {
            return Err("The retained input evidence changed".into());
        }
        Ok(())
    }
}

fn binding(record: &Value) -> Value {
    json!([
        record["version"],
        record["agent"],
        record["session_id"],
        record["layout"],
        record["run_id"],
        record["conversation_id"],
        record["name"],
        record["terminal_name"],
        record[MARKER]
    ])
}

/// Holds ONLY the original inode pair's exclusion and its read-only evidence.
/// Drop releases exclusion but never removes, recreates or repairs a pathname.
pub(in crate::state) struct Input {
    state: Directory,
    transaction: Directory,
    sessions: Directory,
    before: Evidence,
    complete: Evidence,
    writer: Evidence,
    domain: native_domain::Domain,
    sid: String,
    binding: Value,
    until: Instant,
}
impl Input {
    /// Existing files only, one SID only, one nonblocking acquisition attempt.
    /// Absence, pending completion or changed evidence never permits fallback
    /// to the ordinary create-capable input-lock path.
    pub(in crate::state) fn acquire(current: &Value, until: Instant) -> Result<Self> {
        let marker = current
            .get(MARKER)
            .ok_or("No original input transition evidence")?;
        let sid = string(current, "session_id");
        let hash = string(marker, "before_sha256");
        if !session_id::canonical(sid)
            || uuid::Uuid::parse_str(sid).is_ok_and(|id| id.is_nil())
            || !in_id_layout(current)
            || current["version"] != 1
            || !AGENTS.contains(&string(current, "agent"))
            || string(current, "name").is_empty()
            || string(current, "name").len() > 2048
            || !string(current, "name").starts_with(&format!("{}/", string(current, "agent")))
            || string(current, "run_id").is_empty()
            || hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            || *marker != json!({"version":1,"before_sha256":hash})
        {
            return Err("The moved session input identity is unavailable".into());
        }
        deadline(until)?;
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        let transaction = state.child(DIRECTORY)?.child(sid)?;
        let sessions = state.child("sessions")?;
        if !state.private()? || !transaction.private()? || !sessions.private()? {
            return Err("The original input namespace is not private".into());
        }
        let mut complete = Evidence::open(&transaction, "complete.json")?;
        let proof: Complete = serde_json::from_slice(&complete.read(&transaction, 2048, until)?)
            .map_err(|e| e.to_string())?;
        if proof
            != (Complete {
                version: 1,
                session_id: sid.into(),
                before_sha256: hash.into(),
                state: state.object()?,
                transaction: transaction.object()?,
            })
        {
            return Err("The original input completion is unconfirmed".into());
        }
        let mut before_file = Evidence::open(&transaction, "before.json")?;
        let bytes = before_file.read(&transaction, JOURNAL_BYTES, until)?;
        if digest(&bytes) != hash {
            return Err("The original input before-image changed".into());
        }
        let before: Before = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        valid_record(&before.record)?;
        if before.version != 1
            || string(&before.record, "session_id") != sid
            || before.record["agent"] != current["agent"]
            || before.state != state.object()?
            || before.transaction != transaction.object()?
            || before.sessions != sessions.object()?
        {
            return Err("The original input before-image belongs to another scope".into());
        }
        let writer = Evidence::open(&state, ".lock")?;
        if writer.version != before.writer {
            return Err("The original transition writer changed".into());
        }
        let actual = record(&sessions, &format!("{sid}.json"))?
            .ok_or("The moved session binding is unavailable")?;
        // Compare borrowed fields before copying any caller-supplied value.
        // The captured binding comes only from the bounded native file read.
        if [
            "version",
            "agent",
            "session_id",
            "layout",
            "run_id",
            "conversation_id",
            "name",
            "terminal_name",
            MARKER,
        ]
        .iter()
        .any(|key| actual[*key] != current[*key])
        {
            return Err("The moved session binding changed".into());
        }
        let captured = binding(&actual);
        // Only the exact protected original reconstructs correlation with the
        // immutable reservation. No data here authorizes a native operation.
        let domain = native_domain::Domain::recover(&protected(&before, hash), until)?;
        let held = Self {
            state,
            transaction,
            sessions,
            before: before_file,
            complete,
            writer,
            domain,
            sid: sid.into(),
            binding: captured,
            until,
        };
        held.verify()?;
        Ok(held)
    }

    /// Fresh native/path/evidence checks under the SAME original deadline.
    /// Activity and other mutable projections may change; target identity and
    /// marker presence/content may not. This is not a live-process check.
    pub(in crate::state) fn verify(&self) -> Result<()> {
        deadline(self.until)?;
        for directory in [&self.state, &self.transaction, &self.sessions] {
            directory.verify()?;
            if !directory.private()? {
                return Err("The original input directory changed".into());
            }
        }
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != self.state.path {
            return Err("The original input root changed".into());
        }
        self.before.verify(&self.transaction)?;
        self.complete.verify(&self.transaction)?;
        self.writer.verify(&self.state)?;
        self.domain.verify()?;
        let current = record(&self.sessions, &format!("{}.json", self.sid))?
            .ok_or("The moved session binding is unavailable")?;
        if binding(&current) != self.binding {
            return Err("The moved session identity changed".into());
        }
        deadline(self.until)
    }
}
