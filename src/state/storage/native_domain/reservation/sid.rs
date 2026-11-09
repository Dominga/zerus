//! Exact-name kind discriminator in the SAME original reservation scope.
use super::*;

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Kind {
    #[serde(rename = "fresh")]
    Fresh {},
    #[serde(rename = "legacy_moved")]
    LegacyMoved {
        old_key: String,
        before_digest: String,
    },
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SidProof {
    version: u8,
    session_id: String,
    state: (u64, u64),
    inputs: (u64, u64),
    reservations: (u64, u64),
    lock: (u64, u64, u32, u32),
    origin: Kind,
}

fn filename(id: &str) -> Result<String> {
    if !session_id::canonical(id) || uuid::Uuid::parse_str(id).is_ok_and(|id| id.is_nil()) {
        return Err("The input kind requires an exact canonical session ID".into());
    }
    Ok(format!("sid-{id}.json"))
}

pub(in crate::state::storage::native_domain) fn refuse_allocated_sid(
    state: &Directory,
    inputs: &Directory,
    id: &str,
) -> Result<()> {
    let name = filename(id)?;
    if absent(state, DIRECTORY)? && absent(state, ANCHOR)? {
        return Ok(());
    }
    let directory = state.child(DIRECTORY)?;
    verify_anchor(state, inputs, &directory)?;
    if !directory.private()? || !absent(&directory, &name)? {
        return Err("This SID already has retained input kind evidence".into());
    }
    directory.verify()
}

/// Pristine bootstrap is allowed only for creation. Any partial scope stays
/// unavailable. In particular, a rebuilt directory cannot supply absence.
pub(in crate::state::storage::native_domain) fn inputs_for_fresh(
    state: &Directory,
    id: &str,
) -> Result<Directory> {
    let name = filename(id)?;
    state.verify()?;
    if !state.private()? {
        return Err("The fresh input root is not private".into());
    }
    let first = absent(state, ANCHOR)? && absent(state, DIRECTORY)?;
    let inputs = if first {
        state.ensure_child("native-input")?
    } else {
        state.child("native-input")?
    };
    if !state.private()? || !inputs.private()? {
        return Err("The fresh input scope is not private".into());
    }
    if !first {
        let directory = state.child(DIRECTORY)?;
        verify_anchor(state, &inputs, &directory)?;
        if !directory.private()? || !absent(&directory, &name)? {
            return Err("This SID input kind already exists; retain it for review".into());
        }
        directory.verify()?;
    }
    if !absent(&inputs, &format!("{id}.lock"))? {
        return Err("This SID input object already exists; it cannot be minted again".into());
    }
    inputs.verify()?;
    state.verify()?;
    Ok(inputs)
}

/// Retains one exact SID proof, never a catalog or a historical input result.
pub(in crate::state::storage::native_domain) struct SidHeld {
    directory: Directory,
    file: File,
    version: Value,
    proof: SidProof,
    bytes: Vec<u8>,
}

impl SidHeld {
    pub(in crate::state::storage::native_domain) fn create_fresh(
        state: &Directory,
        inputs: &Directory,
        id: &str,
        lock: (u64, u64, u32, u32),
    ) -> Result<Self> {
        filename(id)?;
        let (directory, first) = if absent(state, DIRECTORY)? {
            // A lone anchor is a partial/replaced scope, never pristine.
            if !absent(state, ANCHOR)? {
                return Err("The original input kind namespace is incomplete".into());
            }
            (
                state.create(
                    DIRECTORY,
                    &format!(".input-reservations-{}", uuid::Uuid::new_v4()),
                )?,
                true,
            )
        } else {
            let directory = state.child(DIRECTORY)?;
            verify_anchor(state, inputs, &directory)?;
            (directory, false)
        };
        let held = Self::create(state, inputs, directory, id, lock, Kind::Fresh {})?;
        if first {
            // Publish the SID negative before an anchor can prove any absence.
            let anchor = serde_json::to_vec(&scope(state, inputs, &held.directory)?)
                .map_err(|e| e.to_string())?;
            cut_write()?;
            state.write_new(ANCHOR, &anchor)?;
        }
        held.verify(state, inputs)?;
        Ok(held)
    }

    pub(in crate::state::storage::native_domain) fn create_moved(
        state: &Directory,
        inputs: &Directory,
        original: &Held,
        record: &Value,
    ) -> Result<Self> {
        original.verify(state, inputs, record)?;
        let directory = state.child(DIRECTORY)?;
        let held = Self::create(
            state,
            inputs,
            directory,
            &original.proof.session_id,
            original.proof.lock,
            Kind::LegacyMoved {
                old_key: original.proof.old_key.clone(),
                before_digest: original.proof.before_digest.clone(),
            },
        )?;
        held.verify_moved(state, inputs, original)?;
        Ok(held)
    }

    fn create(
        state: &Directory,
        inputs: &Directory,
        directory: Directory,
        id: &str,
        lock: (u64, u64, u32, u32),
        origin: Kind,
    ) -> Result<Self> {
        if !state.private()? || !inputs.private()? || !directory.private()? {
            return Err("The input kind scope is not private".into());
        }
        let proof = SidProof {
            version: 2,
            session_id: id.into(),
            state: state.object()?,
            inputs: inputs.object()?,
            reservations: directory.object()?,
            lock,
            origin,
        };
        let bytes = serde_json::to_vec(&proof).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err("The input kind proof exceeds its bound".into());
        }
        cut_write()?;
        let file = directory.write_new_file(&filename(id)?, &bytes)?;
        Ok(Self {
            version: file_version(&file)?,
            directory,
            file,
            proof,
            bytes,
        })
    }

    pub(in crate::state::storage::native_domain) fn load(
        state: &Directory,
        inputs: &Directory,
        id: &str,
    ) -> Result<Self> {
        let directory = state.child(DIRECTORY)?;
        let file = directory.read_private(&filename(id)?)?;
        let version = file_version(&file)?;
        let bytes = read(&file)?;
        let proof: SidProof =
            serde_json::from_slice(&bytes).map_err(|_| "The SID input kind is malformed")?;
        if proof.session_id != id {
            return Err("The SID input kind belongs to another session".into());
        }
        let held = Self {
            directory,
            file,
            version,
            proof,
            bytes,
        };
        held.verify(state, inputs)?;
        Ok(held)
    }

    pub(in crate::state::storage::native_domain) fn original(&self) -> (u64, u64, u32, u32) {
        self.proof.lock
    }

    pub(in crate::state::storage::native_domain) fn kind(&self) -> super::super::InputKind {
        match self.proof.origin {
            Kind::Fresh {} => super::super::InputKind::Fresh,
            Kind::LegacyMoved { .. } => super::super::InputKind::LegacyMoved,
        }
    }

    pub(in crate::state::storage::native_domain) fn fresh(&self) -> Result<()> {
        if self.proof.origin != (Kind::Fresh {}) {
            return Err("This session requires its original legacy-moved input domain".into());
        }
        Ok(())
    }

    pub(in crate::state::storage::native_domain) fn verify_moved(
        &self,
        state: &Directory,
        inputs: &Directory,
        old: &Held,
    ) -> Result<()> {
        self.verify(state, inputs)?;
        if self.proof.session_id != old.proof.session_id
            || self.proof.lock != old.proof.lock
            || self.proof.origin
                != (Kind::LegacyMoved {
                    old_key: old.proof.old_key.clone(),
                    before_digest: old.proof.before_digest.clone(),
                })
        {
            return Err("The SID input kind disagrees with its original v1 reservation".into());
        }
        Ok(())
    }

    pub(in crate::state::storage::native_domain) fn verify(
        &self,
        state: &Directory,
        inputs: &Directory,
    ) -> Result<()> {
        state.verify()?;
        inputs.verify()?;
        self.directory.verify()?;
        verify_anchor(state, inputs, &self.directory)?;
        let current = self
            .directory
            .read_private(&filename(&self.proof.session_id)?)?;
        if self.proof.version != 2
            || !state.private()?
            || !inputs.private()?
            || !self.directory.private()?
            || self.proof.state != state.object()?
            || self.proof.inputs != inputs.object()?
            || self.proof.reservations != self.directory.object()?
            || file_version(&self.file)? != self.version
            || file_version(&current)? != self.version
            || read(&self.file)? != self.bytes
            || read(&current)? != self.bytes
            || file_version(&self.file)? != self.version
            || file_version(&current)? != self.version
        {
            return Err("The original SID input kind changed".into());
        }
        Ok(())
    }
}
