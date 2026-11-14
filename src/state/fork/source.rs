//! Logical source equality only; no physical/native custody or admission.
use super::*;
use crate::state::{record_source::Digest, storage::selected_source};

pub(super) struct Selected {
    source: selected_source::Snapshot,
    slot: PathBuf,
    identity: Digest,
}

pub(super) struct Fence {
    source: selected_source::Fence,
    slot: PathBuf,
    identity: Digest,
}

pub(super) fn changed(name: &str) -> String {
    format!(
        "{name}: fork source changed or is unavailable; select the session again before forking"
    )
}

fn namespace(name: &str, slot: &Path, identity: &Digest) -> Result<()> {
    let current =
        storage::read_bounded(name, storage::ID_RECORD_BYTES).map_err(|_| changed(name))?;
    let current_slot =
        std::path::absolute(storage::record_path(name)).map_err(|_| changed(name))?;
    if current_slot != slot || !identity.matches(&lifecycle::saved_binding(&current)) {
        return Err(changed(name));
    }
    Ok(())
}

impl Selected {
    pub(super) fn capture(name: &str) -> Result<Self> {
        let slot = std::path::absolute(storage::record_path(name)).map_err(|_| changed(name))?;
        let source = selected_source::Snapshot::capture(&slot, name).map_err(|_| changed(name))?;
        let identity = Digest::capture(&lifecycle::saved_binding(source.record()));
        Ok(Self {
            source,
            slot,
            identity,
        })
    }

    pub(super) fn record(&self) -> &Value {
        self.source.record()
    }

    pub(super) fn check(&self, name: &str) -> Result<()> {
        namespace(name, &self.slot, &self.identity)?;
        self.source.recheck().map_err(|_| changed(name))
    }

    pub(super) fn into_fence(self) -> Fence {
        Fence {
            source: self.source.into_fence(),
            slot: self.slot,
            identity: self.identity,
        }
    }
}

impl Fence {
    pub(super) fn check(&self, name: &str) -> Result<()> {
        namespace(name, &self.slot, &self.identity)?;
        self.source.recheck().map_err(|_| changed(name))
    }
}

pub(super) fn kept(error: String, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("{error}; Kimi fork {id} is saved and can be opened explicitly"),
        None => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_source_has_only_fixed_proofs_and_locators() {
        let _parts: fn(Fence) -> (selected_source::Fence, PathBuf, Digest) =
            |Fence {
                 source,
                 slot,
                 identity,
             }| (source, slot, identity);
        assert_eq!(
            std::mem::size_of::<Fence>(),
            std::mem::size_of::<selected_source::Fence>()
                + std::mem::size_of::<PathBuf>()
                + std::mem::size_of::<Digest>()
        );
    }
}
