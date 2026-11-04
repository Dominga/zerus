//! The drafts capture's committed originals as a Staged source of the
//! historical report (zerus-5wmh.66.5.2.1.2): evidence only, never a draft
//! send, a blob copy or download, an import, a deletion or effect authority.
//!
//! The source is only the capture's receipt and the exact original manifests
//! it binds (`draft_preload`, container `draft-preload-container-v1`). Each
//! store is its own family, its entries in the container's order:
//!
//! - `next-drafts-preload`: Next's own store before its first job
//!   (`next-drafts-before-first-job-v1`). Absent when an earlier run wrote
//!   that store without capturing first: then the report stays incomplete.
//! - `next-dev-drafts-preload`: the dev app's stores as observed then
//!   (`next-dev-drafts-observed-v1`).
//! - `qt-drafts-preload`: Qt's store as observed then
//!   (`qt-drafts-observed-v1`).
//!
//! A supported manifest carries the unchanged `sources::draft_checkpoint`
//! body. Unsupported, unreadable and manifest-less entries keep their class
//! and hashes; their raw bytes stay in the private capture. Attachment
//! references are the capture's stat-only classes, never content. The
//! `next-drafts-preload-provenance` family binds the receipt and records each
//! root, its boundary and whether it is complete. A not-captured outcome
//! publishes nothing here: it stays unavailable.
use super::{
    draft_preload::{
        manifest_key, Capture, Class, Committed, Entry, Store, BOUNDARY, DEV_BOUNDARY, FORMAT,
        MAX_BYTES, QT_BOUNDARY,
    },
    sources::{self, Origin},
    SourceError, SourceResult, SourceStream,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use zerus_desktop_migration::{canonical_uuid, sha256, SourceRecord};

/// The provenance family: the receipt and every root's outcome.
pub const PROVENANCE: &str = "next-drafts-preload-provenance";
const ENTRIES_FORMAT: &str = "draft-preload-entries-v1";
const PROVENANCE_FORMAT: &str = "draft-preload-provenance-v1";

/// What a report record binds: the exact receipt and container it publishes.
/// Another capture or receipt is another source, never adopted under this
/// record's ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// The container format, `draft-preload-container-v1`.
    pub format: String,
    pub capture_id: String,
    /// `index.json`'s exact bytes, as the receipt binds them.
    pub index_sha256: String,
    pub container_bytes: u64,
    pub container_sha256: String,
    /// `receipt.json`'s exact bytes.
    pub receipt_sha256: String,
    /// Next's store showed an earlier run that wrote it without capturing
    /// first: its family is not recorded and the report stays incomplete.
    pub earlier_run: bool,
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Binding {
    pub fn capture_id(&self) -> &str {
        &self.capture_id
    }
    pub fn valid(&self) -> bool {
        self.format == FORMAT
            && canonical_uuid(&self.capture_id)
            && digest(&self.index_sha256)
            && self.container_bytes <= MAX_BYTES as u64
            && digest(&self.container_sha256)
            && digest(&self.receipt_sha256)
    }
}

fn family(store: Store) -> &'static str {
    match store {
        Store::Next => "next-drafts-preload",
        Store::Dev => "next-dev-drafts-preload",
        Store::Qt => "qt-drafts-preload",
    }
}

fn boundary(store: Store) -> &'static str {
    match store {
        Store::Next => BOUNDARY,
        Store::Dev => DEV_BOUNDARY,
        Store::Qt => QT_BOUNDARY,
    }
}

/// The capture's committed version, held for one report attempt.
pub struct Snapshot {
    committed: Committed,
    binding: Binding,
    /// Each published store and its family's source SHA: the SHA-256 of its
    /// ordered index entries, bound to the capture and container.
    families: Vec<(Store, String)>,
}

impl Snapshot {
    /// The binding, after the slot is checked again by bounded stats: the
    /// same receipt, and so the same stamped index and container parts.
    pub fn reference(&self) -> SourceResult<Binding> {
        if self.committed.unchanged() {
            Ok(self.binding.clone())
        } else {
            Err(SourceError::SourceChanged)
        }
    }

    fn entries(&self) -> &[Entry] {
        &self.committed.snapshot.index.entries
    }

    /// One entry as published: its class, hashes and attachment classes,
    /// and a supported manifest's unchanged checkpoint body.
    fn item(&self, n: usize) -> SourceResult<Value> {
        let entry = &self.entries()[n];
        let mut value = json!({"kind":"draft-preload-entry","root":entry.root,
            "folder":entry.folder,"class":entry.class,"size":entry.size,
            "sha256":entry.sha256,"blobs":entry.blobs,"blob_bytes_included":false});
        if entry.class == Class::Supported {
            let bytes = self.committed.snapshot.bytes(entry);
            let key = manifest_key(&entry.folder, bytes).ok_or(SourceError::InvalidMetadata)?;
            let origin = match entry.store {
                Store::Qt => Origin::Qt,
                Store::Next | Store::Dev => Origin::Next,
            };
            let mut body = sources::draft_checkpoint(origin, &key, bytes)?.into_chunks();
            let checkpoint = body.next().ok_or(SourceError::InvalidMetadata)??;
            if body.next().is_some() {
                return Err(SourceError::InvalidMetadata);
            }
            value["checkpoint"] = checkpoint;
        }
        Ok(value)
    }

    fn provenance(&self) -> SourceResult<Value> {
        let index = &self.committed.snapshot.index;
        let roots: Vec<Value> = index
            .roots
            .iter()
            .map(|root| {
                let entries = index
                    .entries
                    .iter()
                    .filter(|e| e.store == root.store && e.root == root.root)
                    .count();
                json!({"store":root.store,"root":root.root,"present":root.present,
                    "boundary":boundary(root.store),"earlier_run":root.earlier_run,
                    "complete":!root.earlier_run,"entries":entries})
            })
            .collect();
        Ok(
            json!({"version":1,"format":FORMAT,"historical":true,"current_source_complete":false,
            "drained":false,"complete":!self.binding.earlier_run,"reference":self.reference()?,
            "roots":roots,"entries":index.entries.len(),"container_bytes":index.container_bytes,
            "container_sha256":index.container_sha256}),
        )
    }

    /// Each store's entries, then the provenance, each checked again just
    /// before publication: the same receipt is still there, so the same
    /// originals (their hashes) are what is published.
    pub fn report_sources(self: Arc<Self>) -> SourceResult<Vec<SourceStream>> {
        let mut streams = Vec::new();
        for (store, hash) in &self.families {
            let positions: Vec<usize> = self
                .entries()
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.store == *store)
                .map(|(n, _)| n)
                .collect();
            let source = self.clone();
            let items = positions.into_iter().map(move |n| source.item(n));
            let again = self.clone();
            let expected = hash.clone();
            streams.push(
                SourceStream::new(
                    SourceRecord {
                        family: family(*store).into(),
                        schema: 1,
                        format: ENTRIES_FORMAT.into(),
                        sha256: hash.clone(),
                        build: None,
                    },
                    sources::pages(items),
                )
                .recheck(move || again.reference().map(|_| expected)),
            );
        }
        let value = self.provenance()?;
        let hash = sha256(&serde_json::to_vec(&value).map_err(|_| SourceError::InvalidMetadata)?);
        let again = self.clone();
        streams.push(
            SourceStream::new(
                SourceRecord {
                    family: PROVENANCE.into(),
                    schema: 1,
                    format: PROVENANCE_FORMAT.into(),
                    sha256: hash,
                    build: None,
                },
                std::iter::once(Ok(value)),
            )
            .recheck(move || {
                serde_json::to_vec(&again.provenance()?)
                    .map(|bytes| sha256(&bytes))
                    .map_err(|_| SourceError::InvalidMetadata)
            }),
        );
        Ok(streams)
    }
}

/// Every entry lies inside the container with the hash and size its class
/// allows, under a present root of its store that recorded it.
fn consistent(committed: &Committed) -> bool {
    let index = &committed.snapshot.index;
    index.entries.iter().all(|entry| {
        let end = entry.offset.checked_add(entry.size);
        let recorded = index.roots.iter().any(|root| {
            root.store == entry.store
                && root.root == entry.root
                && root.present
                && !root.earlier_run
        });
        let bytes = match entry.class {
            Class::Supported | Class::Unsupported => entry.size > 0,
            Class::NoManifest | Class::Unreadable => entry.size == 0,
        };
        recorded
            && bytes
            && digest(&entry.folder)
            && end.is_some_and(|end| end <= index.container_bytes)
            && (entry.size == 0 || sha256(committed.snapshot.bytes(entry)) == entry.sha256)
    })
}

/// The capture's committed version. `Ok(None)`: nothing to publish (no
/// draft anywhere, and no store left incomplete). A capture without a
/// receipt, a recorded failure (not captured) and an unreadable or damaged
/// slot are errors, never a complete source.
pub fn retained(capture: &Capture) -> SourceResult<Option<Arc<Snapshot>>> {
    let committed = capture.committed()?;
    let index = &committed.snapshot.index;
    let earlier_run = index.roots.iter().any(|root| root.earlier_run);
    if index.entries.is_empty() && !earlier_run {
        return Ok(None);
    }
    let binding = Binding {
        format: index.format.clone(),
        capture_id: index.capture_id.clone(),
        index_sha256: committed.index_sha256().into(),
        container_bytes: index.container_bytes,
        container_sha256: index.container_sha256.clone(),
        receipt_sha256: sha256(committed.receipt()),
        earlier_run,
    };
    if !binding.valid() || !consistent(&committed) {
        return Err(SourceError::InvalidMetadata);
    }
    let mut families = Vec::new();
    for store in [Store::Next, Store::Dev, Store::Qt] {
        let entries: Vec<&Entry> = index.entries.iter().filter(|e| e.store == store).collect();
        if entries.is_empty() {
            continue;
        }
        let bound = json!({"capture_id":binding.capture_id,"container_sha256":binding.container_sha256,
            "boundary":boundary(store),"entries":entries});
        let hash = sha256(&serde_json::to_vec(&bound).map_err(|_| SourceError::InvalidMetadata)?);
        families.push((store, hash));
    }
    Ok(Some(Arc::new(Snapshot {
        committed,
        binding,
        families,
    })))
}

#[cfg(test)]
mod tests;
