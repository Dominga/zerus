//! Permanent old-key reservations and exact-SID native-input kind proofs.
//!
//! Old-key v1 entries and alias pairs are bounded by legacy sessions moved;
//! new ID sessions never create a legacy alias. Exact `sid-<UUID>.json` entries
//! have a nonhex prefix, so cannot collide with the old 64-hex-key filenames.
//! One immutable, at most 4096-byte kind entry accompanies each ID session's
//! SID lock, including retained interrupted preparations with no published
//! record. Neither kind grows per launch, rename or retry. There is no listing,
//! deletion, expiry, reconstruction or repair operation. An orphan stays unused.
use super::*;
use serde::{Deserialize, Serialize};

mod sid;
pub(super) use sid::{inputs_for_fresh, refuse_allocated_sid, SidHeld};

const DIRECTORY: &str = "native-input-reservations";
const ANCHOR: &str = "native-input-reservation-scope.json";
const MAX_BYTES: usize = 4096;

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Proof {
    version: u8,
    session_id: String,
    old_key: String,
    before_digest: String,
    state: (u64, u64),
    inputs: (u64, u64),
    reservations: (u64, u64),
    lock: (u64, u64, u32, u32),
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Anchor {
    version: u8,
    state: (u64, u64),
    inputs: (u64, u64),
    reservations: (u64, u64),
}

fn scope(state: &Directory, inputs: &Directory, reservations: &Directory) -> Result<Anchor> {
    Ok(Anchor {
        version: 1,
        state: state.object()?,
        inputs: inputs.object()?,
        reservations: reservations.object()?,
    })
}

fn verify_anchor(state: &Directory, inputs: &Directory, reservations: &Directory) -> Result<()> {
    let bytes = read(&state.read_private(ANCHOR)?)?;
    let anchor: Anchor =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid original input reservation scope")?;
    if anchor != scope(state, inputs, reservations)? {
        return Err("The original input reservation namespace changed".into());
    }
    Ok(())
}

fn legacy(record: &Value) -> Result<()> {
    let id = string(record, "session_id");
    let name = string(record, "name");
    if in_id_layout(record)
        || name.is_empty()
        || name.len() > 2048
        || !session_id::canonical(id)
        || uuid::Uuid::parse_str(id).is_ok_and(|id| id.is_nil())
    {
        return Err("The original legacy input reservation identity is unavailable".into());
    }
    Ok(())
}

fn digest(record: &Value) -> String {
    format!("{:x}", Sha256::digest(record.to_string().as_bytes()))
}

fn filename(record: &Value) -> String {
    format!("{}.json", label_key(string(record, "name")))
}

fn absent(directory: &Directory, name: &str) -> Result<bool> {
    let name = component(name)?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: one fixed component under a pinned parent; no link follow.
    if unsafe {
        libc::fstatat(
            directory.descriptor(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(false);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ENOENT) => Ok(true),
        _ => Err("Cannot establish native input reservation absence".into()),
    }
}

pub(super) fn inputs_for_prepare(state: &Directory, record: &Value) -> Result<Directory> {
    inputs_for_mode(state, record, true)
}
pub(super) fn inputs_for_existing(state: &Directory, record: &Value) -> Result<Directory> {
    inputs_for_mode(state, record, false)
}
fn inputs_for_mode(state: &Directory, record: &Value, create: bool) -> Result<Directory> {
    legacy(record)?;
    state.verify()?;
    let first_scope = absent(state, ANCHOR)? && absent(state, DIRECTORY)?;
    let inputs = if first_scope && create {
        state.ensure_child("native-input")?
    } else {
        state.child("native-input")?
    };
    if !state.private()? || !inputs.private()? {
        return Err("The original native input scope is not private".into());
    }
    if !first_scope {
        let reservations = state.child(DIRECTORY)?;
        verify_anchor(state, &inputs, &reservations)?;
        if !reservations.private()? || !absent(&reservations, &filename(record))? {
            return Err(
                "This legacy input key is already reserved; retain its original objects for review"
                    .into(),
            );
        }
        reservations.verify()?;
    }
    if !absent(&inputs, &format!("{}.lock", string(record, "session_id")))? {
        return Err(
            "The original SID input name already exists; it cannot be created again".into(),
        );
    }
    state.verify()?;
    inputs.verify()?;
    Ok(inputs)
}

fn read(file: &File) -> Result<Vec<u8>> {
    use std::os::unix::fs::FileExt;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !crate::platform::verify_private_file(file).map_err(|e| e.to_string())?
        || metadata.len() > MAX_BYTES as u64
    {
        return Err("The original input reservation is not a bounded private file".into());
    }
    // Positional reads leave the held file's cursor unchanged across verifies.
    let mut bytes = vec![0; metadata.len() as usize];
    file.read_exact_at(&mut bytes, 0)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// Original immutable proof file, retained with the native-input file. This
/// is not a journal of input results, native custody, or a metadata commit.
pub(super) struct Held {
    directory: Directory,
    file: File,
    object: (u64, u64, u32, u32),
    proof: Proof,
    bytes: Vec<u8>,
}

impl Held {
    pub(super) fn create(
        state: &Directory,
        inputs: &Directory,
        record: &Value,
        original: (u64, u64, u32, u32),
    ) -> Result<Self> {
        legacy(record)?;
        // A missing anchor is initialized ONLY along with a genuinely new
        // reservation directory. An existing/rebuilt empty directory never
        // supplies historical absence; a partial first bootstrap stays closed.
        let (directory, fresh) = match std::fs::symlink_metadata(state.path.join(DIRECTORY)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let directory = state.create(
                    DIRECTORY,
                    &format!(".input-reservations-{}", uuid::Uuid::new_v4()),
                )?;
                (directory, true)
            }
            Ok(_) => (state.child(DIRECTORY)?, false),
            Err(e) => return Err(e.to_string()),
        };
        if !state.private()? || !inputs.private()? || !directory.private()? {
            return Err("The native input reservation scope is not private".into());
        }
        if !fresh {
            verify_anchor(state, inputs, &directory)?;
        }
        let proof = Proof {
            version: 1,
            session_id: string(record, "session_id").to_owned(),
            old_key: label_key(string(record, "name")),
            before_digest: digest(record),
            state: state.object()?,
            inputs: inputs.object()?,
            reservations: directory.object()?,
            lock: original,
        };
        let bytes = serde_json::to_vec(&proof).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err("The original input reservation exceeds its bound".into());
        }
        cut_write()?;
        let file = directory.write_new_file(&filename(record), &bytes)?;
        if fresh {
            // Publish the first negative BEFORE the scope can prove absence
            // for any key. A crash at either write never exposes a pristine
            // old-key answer for this pending original.
            let anchor = serde_json::to_vec(&scope(state, inputs, &directory)?)
                .map_err(|e| e.to_string())?;
            cut_write()?;
            state.write_new(ANCHOR, &anchor)?;
        }
        let held = Self {
            object: identity(&file.metadata().map_err(|e| e.to_string())?),
            directory,
            file,
            proof,
            bytes,
        };
        held.verify(state, inputs, record)?;
        Ok(held)
    }

    pub(super) fn load(state: &Directory, inputs: &Directory, record: &Value) -> Result<Self> {
        legacy(record)?;
        // Existing only: missing or damaged proof can keep a key unavailable,
        // but must never be rebuilt to authorize recovery of an original.
        let directory = state.child(DIRECTORY)?;
        let file = directory.read_private(&filename(record))?;
        let bytes = read(&file)?;
        let proof: Proof = serde_json::from_slice(&bytes)
            .map_err(|_| "The original input reservation is malformed")?;
        let held = Self {
            object: identity(&file.metadata().map_err(|e| e.to_string())?),
            directory,
            file,
            proof,
            bytes,
        };
        held.verify(state, inputs, record)?;
        Ok(held)
    }

    pub(super) fn original(&self) -> (u64, u64, u32, u32) {
        self.proof.lock
    }

    pub(super) fn verify(
        &self,
        state: &Directory,
        inputs: &Directory,
        record: &Value,
    ) -> Result<()> {
        legacy(record)?;
        state.verify()?;
        inputs.verify()?;
        self.directory.verify()?;
        verify_anchor(state, inputs, &self.directory)?;
        let current = self.directory.read_private(&filename(record))?;
        if !state.private()?
            || !inputs.private()?
            || !self.directory.private()?
            || self.proof.version != 1
            || self.proof.session_id != string(record, "session_id")
            || self.proof.old_key != label_key(string(record, "name"))
            || self.proof.before_digest != digest(record)
            || self.proof.state != state.object()?
            || self.proof.inputs != inputs.object()?
            || self.proof.reservations != self.directory.object()?
            || identity(&current.metadata().map_err(|e| e.to_string())?) != self.object
            || read(&self.file)? != self.bytes
            || read(&current)? != self.bytes
        {
            return Err("The original native input reservation changed".into());
        }
        Ok(())
    }
}

/// True only for verified absence of a negative entry and a pristine legacy
/// lock, if it exists. Any unknown, alias, unreadable entry or extra link means
/// use a NEW session ID and its own ID-layout key. It never changes an existing
/// first holder. Dormant: no constructor calls this allocation check yet.
pub(in crate::state) fn legacy_key_available(name: &str) -> bool {
    fn available(name: &str) -> Result<bool> {
        if name.is_empty() || name.len() > 2048 {
            return Ok(false);
        }
        let state =
            Directory::open(&crate::platform::canonical(root()).map_err(|e| e.to_string())?)?;
        if !state.private()? {
            return Ok(false);
        }
        let key = label_key(name);
        let reservations = state.child(DIRECTORY)?;
        let inputs = state.child("native-input")?;
        verify_anchor(&state, &inputs, &reservations)?;
        // Presence alone is sufficient for a negative. Parsing it never turns
        // damaged or future evidence into availability.
        if !reservations.private()?
            || !inputs.private()?
            || !absent(&reservations, &format!("{key}.json"))?
        {
            return Ok(false);
        }
        if !absent(&inputs, &format!("{key}.lock"))? {
            let file = inputs.read_private(&format!("{key}.lock"))?;
            if file.metadata().map_err(|e| e.to_string())?.len() != 0 {
                return Ok(false);
            }
        }
        reservations.verify()?;
        inputs.verify()?;
        state.verify()?;
        if crate::platform::canonical(root()).map_err(|e| e.to_string())? != state.path {
            return Ok(false);
        }
        Ok(true)
    }
    available(name).unwrap_or(false)
}
