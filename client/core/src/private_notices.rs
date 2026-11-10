//! Task 102: the private-store notices already raised (task 75), kept on
//! this device across restarts as Qt keeps `privateStores/notified`
//! (TrayAgent.cpp:62, :593). A key is `computer\npath\nreason`, the same
//! in both clients, so Qt's set seeds this one once. One small private file
//! of at most [`LIMIT`] keys; it never leaves the device.
use serde_json::{json, Value};
use std::path::Path;

/// Keys kept.
pub const LIMIT: usize = 512;
/// The longest key kept.
pub const KEY_BYTES: usize = 8192;
/// Qt's key.
pub const QT_KEY: &str = "privateStores/notified";
/// The file under the client's data folder.
pub const FILE: &str = "private-store-notices.json";

/// A key shaped as the notices make it: three lines, a path in the middle.
pub fn valid(key: &str) -> bool {
    let mut parts = key.splitn(3, '\n');
    let (Some(computer), Some(path), Some(reason)) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    key.len() <= KEY_BYTES
        && !path.is_empty()
        && !reason.contains('\n')
        && !computer.contains('\n')
        && !key.chars().any(|c| c.is_control() && c != '\n')
}

/// At most [`LIMIT`] valid keys, sorted and without repeats.
pub fn bounded<I: IntoIterator<Item = String>>(keys: I) -> Vec<String> {
    let mut keys: Vec<String> = keys.into_iter().filter(|key| valid(key)).collect();
    keys.sort();
    keys.dedup();
    keys.truncate(LIMIT);
    keys
}

/// The keys in `path`; none when it is missing, too large or malformed.
pub fn load(path: &Path) -> Vec<String> {
    let Some(value) = std::fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= LIMIT * KEY_BYTES)
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return Vec::new();
    };
    if value["schema"] != 1 {
        return Vec::new();
    }
    bounded(
        value["known"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned),
    )
}

/// Writes the keys privately and atomically.
pub fn save(path: &Path, keys: &[String]) -> Result<(), String> {
    let bytes = serde_json::to_vec(&json!({"schema": 1, "known": bounded(keys.iter().cloned())}))
        .map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    crate::fsx::write_private_atomic(path, &bytes, true).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_bounded_sorted_and_shaped_as_the_notices_make_them() {
        assert!(valid(
            "\nC:\\Users\\example\\read-marks.json\nit is not private"
        ));
        assert!(valid(
            "studio\n/home/example/.local/state/hgs/x.json\nowned by another user"
        ));
        assert!(!valid("just text"));
        assert!(!valid("\n\nno path"));
        assert!(!valid("a\tb\n/path\nreason"));
        let many: Vec<String> = (0..LIMIT + 40)
            .map(|n| format!("\n/p/{n:04}\nreason"))
            .collect();
        let kept = bounded(many.iter().cloned().chain(std::iter::once(many[0].clone())));
        assert_eq!(kept.len(), LIMIT);
        assert!(kept.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn the_file_round_trips_and_a_missing_or_malformed_file_is_empty() {
        let dir =
            std::env::temp_dir().join(format!("zerus-private-notices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(FILE);
        assert!(load(&path).is_empty());
        save(&path, &["\n/a\nreason".to_owned(), "bad".to_owned()]).unwrap();
        assert_eq!(load(&path), ["\n/a\nreason"]);
        std::fs::write(&path, b"{\"schema\":2,\"known\":[\"\\n/a\\nreason\"]}").unwrap();
        assert!(load(&path).is_empty());
        std::fs::write(&path, b"not json").unwrap();
        assert!(load(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
