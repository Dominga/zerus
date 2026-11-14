//! Advisory publication ordering. Index failures cannot replace source outcomes.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Witness {
    pub epoch: Option<String>,
    pub intent: Option<String>,
    pub seq: Option<u64>,
    pub revision: u64,
}

impl Witness {
    pub(crate) fn lost() -> Self {
        Self {
            epoch: None,
            intent: None,
            seq: None,
            revision: 0,
        }
    }
}

/// Only `source` determines this operation's result. `lost` is a best-effort
/// coverage diagnostic/fence, never a permission or a retry of the source.
pub(crate) fn publish<T>(
    store: Option<&mut Store>,
    change: &Change,
    source: impl FnOnce(Witness) -> Result<T>,
    lost: impl FnMut(&str),
) -> Result<T> {
    publish_checked(store, change, source, || Ok(()), lost)
}

pub(crate) fn publish_checked<T>(
    mut store: Option<&mut Store>,
    change: &Change,
    source: impl FnOnce(Witness) -> Result<T>,
    after_source: impl FnOnce() -> Result<()>,
    mut lost: impl FnMut(&str),
) -> Result<T> {
    let ticket = match store.as_deref_mut() {
        Some(index) => match index.reserve(change) {
            Ok(ticket) => Some(ticket),
            Err(error) => {
                lost(&error.to_string());
                None
            }
        },
        None => {
            lost("The advisory process index is unavailable");
            None
        }
    };
    let witness = ticket.as_ref().map_or_else(Witness::lost, |t| Witness {
        epoch: Some(t.epoch.clone()),
        intent: Some(t.id.clone()),
        seq: Some(t.seq),
        revision: change.before + 1,
    });
    let outcome = source(witness);
    if outcome.is_ok() {
        if let Err(error) = after_source() {
            lost(&error.to_string());
        } else if let (Some(index), Some(ticket)) = (store, ticket) {
            if let Err(error) = index.settle(&ticket) {
                lost(&error.to_string());
            }
        }
    }
    outcome
}

#[cfg(test)]
mod tests;
