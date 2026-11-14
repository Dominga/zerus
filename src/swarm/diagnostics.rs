//! Optional catalog captions for a refusal already decided elsewhere. These
//! values never establish identity, routes, writer membership or admission.
use super::catalog::{Catalog, Key};
use crate::config::Config;
use serde::Deserialize;
use std::path::Path;

pub(crate) fn diagnostic_machine_label(config: &Config, node: &str) -> Option<String> {
    label_at(&config.dir.join("swarm/catalog.json"), node)
}

fn label_at(path: &Path, node: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Caption {
        catalog: Catalog,
    }
    // Match the existing catalog read ceiling, but never follow a link or wait
    // for a FIFO. Missing/malformed data only omits this optional caption.
    let bytes = super::read_regular(path, 128 * 1024 * 1024).ok()?;
    let data: Caption = serde_json::from_slice(&bytes).ok()?;
    data.catalog.validate().ok()?;
    let values = data.catalog.values();
    let label = values.get(&Key::machine(node, "name"))?.as_str()?;
    (!label.is_empty() && label.len() <= 320 && !label.chars().any(char::is_control))
        .then(|| label.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const NODE: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";

    #[test]
    fn duplicate_diagnostic_labels_are_selected_only_by_exact_uuid_without_saving() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalog.json");
        let mut catalog = Catalog::default();
        for node in [NODE, OTHER] {
            catalog
                .set(NODE, Key::machine(node, "name"), json!("example-laptop"))
                .unwrap();
        }
        let original = serde_json::to_vec(&json!({"catalog":catalog})).unwrap();
        std::fs::write(&path, &original).unwrap();
        for node in [NODE, OTHER] {
            assert_eq!(label_at(&path, node).as_deref(), Some("example-laptop"));
        }
        assert!(label_at(&path, "33333333-3333-4333-8333-333333333333").is_none());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn missing_or_malformed_catalog_has_no_caption_and_is_never_repaired() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalog.json");
        assert!(label_at(&path, NODE).is_none());
        assert!(!path.exists());
        std::fs::write(&path, b"malformed").unwrap();
        assert!(label_at(&path, NODE).is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"malformed");
    }

    #[cfg(unix)]
    #[test]
    fn captions_do_not_follow_links_or_wait_on_special_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("catalog.json");
        std::os::unix::fs::symlink("missing", &path).unwrap();
        assert!(label_at(&path, NODE).is_none());
        std::fs::remove_file(&path).unwrap();
        let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        assert!(label_at(&path, NODE).is_none());
    }
}
