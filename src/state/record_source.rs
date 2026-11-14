//! Complete logical-source equality, never a writer or native-admission lease.
use serde_json::Value;
use sha2::{Digest as _, Sha256};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Digest([u8; 32]);

impl Digest {
    pub(super) fn capture(value: &Value) -> Self {
        let mut hash = Sha256::new();
        // Preserve the archive fence's existing canonical encoding exactly.
        hash.update(b"zerus-archive-source-v1\0");
        canonical(&mut hash, value);
        Self(hash.finalize().into())
    }

    pub(super) fn matches(&self, value: &Value) -> bool {
        Self::capture(value).0 == self.0
    }
}

fn bytes(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

/// A typed, length-delimited canonical JSON value stream. Object ordering and
/// formatting do not matter; array ordering, types, and absent/null do. There
/// is no field whitelist and no second serialized history-sized allocation.
fn canonical(hash: &mut Sha256, value: &Value) {
    match value {
        Value::Null => hash.update(b"n"),
        Value::Bool(value) => hash.update(if *value { b"t" } else { b"f" }),
        Value::Number(value) => {
            hash.update(b"d");
            bytes(hash, value.to_string().as_bytes());
        }
        Value::String(value) => {
            hash.update(b"s");
            bytes(hash, value.as_bytes());
        }
        Value::Array(values) => {
            hash.update(b"a");
            hash.update((values.len() as u64).to_le_bytes());
            for value in values {
                canonical(hash, value);
            }
        }
        Value::Object(values) => {
            hash.update(b"o");
            hash.update((values.len() as u64).to_le_bytes());
            // Be canonical even if a dependency enables preserve_order later.
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by_key(|(key, _)| *key);
            for (key, value) in entries {
                bytes(hash, key.as_bytes());
                canonical(hash, value);
            }
        }
    }
}
