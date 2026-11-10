//! The binary Zerus ships is CodeLore's, at the pinned revision, and its secret
//! vectors are the ones CodeLore tests against.
use sha2::{Digest, Sha256};
use std::process::Command;

/// The SHA-256 of the secret-scan vectors (the file's LF form, so a CRLF checkout
/// agrees). CodeLore's tests/secret_vectors.rs pins the same value: the vectors are
/// changed in both repositories, with this constant, or neither test stays green.
const VECTORS_SHA256: &str = "dc44e984cd0d507751339cec34a1b5eec32a7de0463c5bff75c82f56267d9cf3";

#[test]
fn zerus_secret_vectors_are_the_ones_codelore_pins() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/secret-scan-vectors.json");
    let text = std::fs::read_to_string(&path)
        .expect("Zerus's copy of the secret-scan vectors")
        .replace("\r\n", "\n");
    assert_eq!(
        format!("{:x}", Sha256::digest(text.as_bytes())),
        VECTORS_SHA256,
        "tests/fixtures/secret-scan-vectors.json changed: change CodeLore's copy and both pinned hashes with it"
    );
}

#[test]
fn the_built_lore_reports_the_version_of_the_pinned_codelore() {
    let output = Command::new(env!("CARGO_BIN_EXE_lore"))
        .arg("--version")
        .output()
        .expect("lore starts");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("lore {}", zerus_lore::VERSION)
    );
}
