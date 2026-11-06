// The shared G3 lineage leaves, on synthetic private folders under the
// system temporary folder.
use super::*;
use std::{fs, path::PathBuf};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("zerus-lineage-{}", uuid::Uuid::new_v4()));
        drop(disk::Parents::open(&root, true).unwrap());
        Self(root.canonicalize().unwrap())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const ID: &str = "8b0c0bd4-6b1f-4bb1-9d8e-0d3a3f2b9a51";

/// The seed's bytes are exactly what both captures wrote before they shared
/// it: an existing `identity.json` reads back unchanged, and a new one is
/// written the same way.
#[test]
fn a_seed_keeps_its_exact_bytes() {
    let earlier = format!(
        r#"{{"version":1,"boundary":"next-drafts-before-first-job-v1","capture_id":"{ID}"}}"#
    );
    let seed: Seed = serde_json::from_str(&earlier).unwrap();
    assert_eq!(serde_json::to_string(&seed).unwrap(), earlier);
    assert!(seed.valid("next-drafts-before-first-job-v1"));
    // Unknown fields are refused, as before.
    let extra = earlier.replace(r#""version":1"#, r#""version":1,"more":true"#);
    assert!(serde_json::from_str::<Seed>(&extra).is_err());
}

/// Validity is for the native family the caller names, never the seed's own
/// boundary: version 1 and a canonical capture ID.
#[test]
fn a_seed_is_valid_only_for_the_named_family() {
    let seed = Seed::new("next-creations-before-first-load-v1");
    assert!(canonical_uuid(&seed.capture_id));
    assert!(seed.valid("next-creations-before-first-load-v1"));
    assert!(!seed.valid("next-drafts-before-first-job-v1"));
    let mut other = seed.clone();
    other.version = 2;
    assert!(!other.valid("next-creations-before-first-load-v1"));
    let mut other = seed.clone();
    other.capture_id = "not-a-uuid".into();
    assert!(!other.valid("next-creations-before-first-load-v1"));
}

#[test]
fn a_leaf_is_created_once_and_read_back_with_its_stamp() {
    let s = Scratch::new();
    let path = s.0.join("identity.json");
    assert!(read_json::<Seed>(&path).unwrap().is_none());
    let seed = Seed::new("next-drafts-before-first-job-v1");
    let stamp = create_json(&path, &seed).unwrap();
    let (again, read) = read_json::<Seed>(&path).unwrap().unwrap();
    assert_eq!((again, read), (seed.clone(), stamp));
    // Never over an existing leaf.
    assert!(create_json(&path, &Seed::new("other")).is_err());
    assert_eq!(read_json::<Seed>(&path).unwrap().unwrap().0, seed);
}

/// Over the bound is too large and refused before anything is created; a
/// leaf that does not parse is invalid, never absent.
#[test]
fn bounds_and_errors_keep_their_outcomes() {
    let s = Scratch::new();
    let path = s.0.join("receipt.json");
    let large = "x".repeat(META);
    assert_eq!(create_json(&path, &large), Err(SourceError::TooLarge));
    assert!(!path.exists());
    disk::create(&path, b"{not json").unwrap();
    assert_eq!(
        read_json::<Seed>(&path).map(|leaf| leaf.map(|(seed, _)| seed)),
        Err(SourceError::InvalidMetadata)
    );
    assert_eq!(
        unreadable(std::io::ErrorKind::FileTooLarge.into()),
        SourceError::TooLarge
    );
    assert_eq!(
        unreadable(std::io::ErrorKind::PermissionDenied.into()),
        SourceError::SourceUnreadable
    );
}
