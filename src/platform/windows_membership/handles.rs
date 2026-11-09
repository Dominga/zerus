//! Checked ProcessHandleInformation DATA. Kernel object addresses are neither
//! requested nor retained; file identities come from metadata-only copies.
use super::*;

pub(super) const MAX_HANDLES: usize = 16_384;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Handle {
    pub(super) value: u64,
    pub(super) kind: u32,
}
#[derive(Debug)]
pub(super) struct Snapshot(BTreeMap<u64, Handle>);
impl Snapshot {
    pub(super) fn decode(bytes: &[u8], until: Instant) -> Result<Self> {
        live(until)?;
        let count = usize::try_from(word(bytes, 0, 8)?).map_err(|_| Error::Incomplete)?;
        let size = count
            .checked_mul(40)
            .and_then(|n| n.checked_add(16))
            .ok_or(Error::Incomplete)?;
        if count > MAX_HANDLES || size != bytes.len() || word(bytes, 8, 8)? != 0 {
            return Err(Error::Incomplete);
        }
        let mut rows = BTreeMap::new();
        for row in bytes[16..].as_chunks::<40>().0 {
            live(until)?;
            let value = word(row, 0, 8)?;
            let kind = u32::try_from(word(row, 28, 4)?).map_err(|_| Error::Incomplete)?;
            if value == 0
                || value == u64::MAX
                || kind == 0
                || kind > u16::MAX.into()
                || word(row, 36, 4)? != 0
                || rows.insert(value, Handle { value, kind }).is_some()
            {
                return Err(Error::Incomplete);
            }
        }
        Ok(Self(rows))
    }
    /// Type calibration from ONE held handle already proven to be a disk file.
    /// No hardcoded object-type index or program name is an exclusion.
    pub(super) fn file_kind(&self, original_disk_handle: u64) -> Result<u32> {
        self.0
            .get(&original_disk_handle)
            .map(|h| h.kind)
            .ok_or(Error::Incomplete)
    }
    pub(super) fn files(&self, kind: u32) -> impl Iterator<Item = Handle> + '_ {
        self.0.values().filter(move |h| h.kind == kind).copied()
    }
}

/// Correlation DATA only, never a permission or pathname-containment predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Object {
    pub(super) volume: u64,
    pub(super) file: [u8; 16],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Metadata {
    pub(super) object: Object,
    pub(super) details: Details,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Details {
    Known { links: u32, directory: bool },
    Denied,
}
/// An attribute denial never erases the already readable native object ID.
/// Other errors stay incomplete and cannot be waived as permission denials.
pub(super) fn with_details(object: Object, details: Result<Details>) -> Result<Metadata> {
    let details = match details {
        Ok(details) => details,
        Err(Error::Denied) => Details::Denied,
        Err(other) => return Err(other),
    };
    Ok(Metadata { object, details })
}

/// The stronger retry can add evidence, but cannot replace a known object or
/// turn a non-permission failure into a permission waiver.
pub(super) fn after_metadata_retry(
    first: Result<Option<Metadata>>,
    second: Result<Option<Metadata>>,
) -> Result<Option<Metadata>> {
    match (&first, &second) {
        (Err(Error::Incomplete) | Ok(None), _) => Err(Error::Incomplete),
        (_, Err(Error::Denied)) => first,
        (Ok(Some(a)), Ok(Some(b))) if a.object != b.object => Err(Error::Incomplete),
        (Ok(Some(_)), Ok(None)) => Err(Error::Incomplete),
        _ => second,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn until() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }
    fn number(bytes: &mut [u8], at: usize, width: usize, value: u64) {
        bytes[at..at + width].copy_from_slice(&value.to_le_bytes()[..width]);
    }
    fn bytes(handles: &[(u64, u32)]) -> Vec<u8> {
        let mut bytes = vec![0; 16 + 40 * handles.len()];
        number(&mut bytes, 0, 8, handles.len() as u64);
        for (i, (handle, kind)) in handles.iter().enumerate() {
            number(&mut bytes, 16 + i * 40, 8, *handle);
            number(&mut bytes, 16 + i * 40 + 28, 4, (*kind).into());
        }
        bytes
    }
    #[test]
    fn file_selection_uses_the_exact_held_object_type_and_preserves_other_file_handles() {
        let snapshot =
            Snapshot::decode(&bytes(&[(0x20, 7), (0x40, 12), (0x60, 7)]), until()).unwrap();
        let kind = snapshot.file_kind(0x20).unwrap();
        assert_eq!(
            snapshot.files(kind).map(|h| h.value).collect::<Vec<_>>(),
            [0x20, 0x60]
        );
        assert!(snapshot.file_kind(0x24).is_err());
    }
    #[test]
    fn truncated_duplicate_or_oversized_handle_tables_are_not_empty_coverage() {
        let valid = bytes(&[(0x20, 7), (0x40, 12)]);
        for len in 0..valid.len() {
            assert!(Snapshot::decode(&valid[..len], until()).is_err(), "{len}");
        }
        for (at, width, value) in [
            (0, 8, u64::MAX),
            (8, 8, 1),
            (16, 8, 0),
            (16, 8, u64::MAX),
            (44, 4, 0),
            (44, 4, u32::MAX.into()),
            (52, 4, 1),
            (56, 8, 0x20),
        ] {
            let mut bad = valid.clone();
            number(&mut bad, at, width, value);
            assert!(Snapshot::decode(&bad, until()).is_err(), "{at}");
        }
        assert!(Snapshot::decode(&valid, Instant::now()).is_err());
        let max: Vec<_> = (1..=MAX_HANDLES as u64).map(|n| (4 * n, 7)).collect();
        assert_eq!(
            Snapshot::decode(&bytes(&max), until()).unwrap().0.len(),
            MAX_HANDLES
        );
        let mut over = max;
        over.push((4 * (MAX_HANDLES as u64 + 1), 7));
        assert!(Snapshot::decode(&bytes(&over), until()).is_err());
    }
    #[test]
    fn only_a_process_bound_native_denial_may_be_reported_as_denied() {
        assert_eq!(
            bounded_table(until(), TableKind::Census, |_| (ACCESS_DENIED, 0)),
            Err(Error::Incomplete)
        );
        assert_eq!(
            bounded_table(until(), TableKind::ProcessHandles, |_| (ACCESS_DENIED, 0)),
            Err(Error::Denied)
        );
        for status in [LENGTH_MISMATCH, BUFFER_TOO_SMALL, -1, 0x8000000du32 as i32] {
            assert_eq!(
                bounded_table(until(), TableKind::ProcessHandles, |_| (status, 0)),
                Err(Error::Incomplete)
            );
        }
        assert_eq!(
            bounded_table(Instant::now(), TableKind::ProcessHandles, |_| (
                ACCESS_DENIED,
                0
            )),
            Err(Error::Incomplete)
        );
        let until = Instant::now() + Duration::from_millis(2);
        assert_eq!(
            bounded_table(until, TableKind::ProcessHandles, |_| {
                std::thread::sleep(Duration::from_millis(10));
                (ACCESS_DENIED, 0)
            }),
            Err(Error::Incomplete),
            "a late native denial cannot waive an expired query"
        );
    }
    #[test]
    fn metadata_retry_cannot_erase_known_use_or_adopt_a_replacement_object() {
        let original = Metadata {
            object: Object {
                volume: 7,
                file: [9; 16],
            },
            details: Details::Denied,
        };
        let mut complete = original;
        complete.details = Details::Known {
            links: 2,
            directory: false,
        };
        let mut replacement = complete;
        replacement.object.file = [8; 16];
        assert_eq!(
            after_metadata_retry(Ok(Some(original)), Err(Error::Denied)),
            Ok(Some(original))
        );
        assert_eq!(
            after_metadata_retry(Ok(Some(original)), Ok(Some(complete))),
            Ok(Some(complete))
        );
        assert_eq!(
            after_metadata_retry(Err(Error::Denied), Ok(Some(complete))),
            Ok(Some(complete))
        );
        for first in [Err(Error::Incomplete), Ok(None)] {
            assert_eq!(
                after_metadata_retry(first, Ok(Some(complete))),
                Err(Error::Incomplete)
            );
        }
        for second in [Err(Error::Incomplete), Ok(None), Ok(Some(replacement))] {
            assert_eq!(
                after_metadata_retry(Ok(Some(original)), second),
                Err(Error::Incomplete)
            );
        }
    }
    #[test]
    fn readable_object_identity_survives_denied_attributes() {
        let object = Object {
            volume: 7,
            file: [9; 16],
        };
        assert_eq!(
            with_details(object, Err(Error::Denied)),
            Ok(Metadata {
                object,
                details: Details::Denied
            })
        );
        let known = Details::Known {
            links: 2,
            directory: false,
        };
        assert_eq!(
            with_details(object, Ok(known)),
            Ok(Metadata {
                object,
                details: known
            })
        );
        assert_eq!(
            with_details(object, Err(Error::Incomplete)),
            Err(Error::Incomplete)
        );
    }
}
