//! Saved session records can contain large private shell-job histories. Keep
//! their raw I/O charged, but retain only the fields used by history search.
//! Public snapshot text shares that raw bound, not the smaller metadata bound.
use super::*;
use crate::platform::FileIdentity;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use std::{collections::BTreeMap, fmt};

struct Entry {
    #[cfg(not(windows))]
    file: fs::File,
    #[cfg(windows)]
    created: Option<std::time::SystemTime>,
    stamp: FileIdentity,
    len: u64,
    record: Value,
}

#[derive(Debug, PartialEq)]
pub(super) enum Record {
    Ready(Value),
    Deferred,
    OverBudget,
}

impl Entry {
    fn current(&self, stamp: FileIdentity, metadata: &fs::Metadata) -> bool {
        if self.stamp != stamp || self.len != metadata.len() {
            return false;
        }
        #[cfg(not(windows))]
        {
            FileIdentity::with_metadata(&self.file)
                .is_ok_and(|(held, meta)| held == stamp && meta.len() == self.len)
        }
        #[cfg(windows)]
        {
            // Do not hold a Windows rename destination open for a 2 s call.
            // Reopen and verify its file ID, write stamp, length and birth time.
            self.created.is_some() && metadata.created().ok() == self.created
        }
    }
}

/// One bounded call owns this cache. Every reuse reopens and verifies the path;
/// Unix also pins the original inode with a held handle.
#[derive(Default)]
pub(super) struct Reader(BTreeMap<PathBuf, Entry>);

impl Reader {
    pub fn read(
        &mut self,
        path: &Path,
        progress: &mut Progress,
        budget: &Budget,
        reserve: usize,
    ) -> Result<Record> {
        if progress.files >= budget.files || Instant::now() >= budget.deadline {
            return Ok(Record::Deferred);
        }
        progress.files += 1;
        let mut file = crate::platform::no_follow(fs::OpenOptions::new().read(true))
            .open(path)
            .map_err(|_| "Session metadata unavailable")?;
        let (stamp, metadata) = FileIdentity::with_metadata(&file).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > session_graph::MAX_RECORD {
            return Err("Session metadata exceeds the saved-record limit".into());
        }
        if let Some(entry) = self.0.get(path) {
            if entry.current(stamp, &metadata) {
                return Ok(Record::Ready(entry.record.clone()));
            }
        }
        let remaining = budget.bytes.saturating_sub(progress.bytes);
        let required = metadata.len() as usize + 1;
        if required.saturating_add(reserve) > budget.bytes {
            // This caller cannot prove it even on a fresh page. Publication
            // must advance past it with incomplete coverage, not loop forever.
            return Ok(Record::OverBudget);
        }
        if required.saturating_add(reserve) > remaining {
            // A short slice is not proof that this source has disappeared.
            return Ok(Record::Deferred);
        }
        let mut bytes = Vec::new();
        let read = file.by_ref().take(required as u64).read_to_end(&mut bytes);
        progress.bytes += bytes.len();
        read.map_err(|e| e.to_string())?;
        let (after, after_metadata) =
            FileIdentity::with_metadata(&file).map_err(|e| e.to_string())?;
        if after != stamp
            || after_metadata.len() != metadata.len()
            || bytes.len() != metadata.len() as usize
        {
            return Err("Session metadata changed during read".into());
        }
        let Metadata(record) = serde_json::from_slice(&bytes)
            .map_err(|_| "Session metadata is incomplete or invalid")?;
        let bounded_fields: BTreeMap<_, _> = record
            .as_object()
            .expect("Metadata deserializes an object")
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "prompt" | "last_message"))
            .collect();
        if serde_json::to_vec(&bounded_fields)
            .map_err(|e| e.to_string())?
            .len()
            > RECORD_BYTES
        {
            return Err("Session search metadata exceeds its budget".into());
        }
        self.0.insert(
            path.to_owned(),
            Entry {
                #[cfg(not(windows))]
                file,
                #[cfg(windows)]
                created: metadata.created().ok(),
                stamp,
                len: metadata.len(),
                record: record.clone(),
            },
        );
        if Instant::now() >= budget.deadline {
            return Ok(Record::Deferred);
        }
        Ok(Record::Ready(record))
    }
}

struct Metadata(Value);
impl<'de> Deserialize<'de> for Metadata {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Fields;
        impl<'de> Visitor<'de> for Fields {
            type Value = Metadata;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a saved session object")
            }
            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Metadata, M::Error> {
                let mut fields = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if needed(&key) {
                        let mut value = map.next_value::<Value>()?;
                        match key.as_str() {
                            "legacy_shadow" => retain(&mut value, &["name", "run_id"]),
                            "session_clear" => {
                                retain(&mut value, &["conversation_id", "previous_conversation_id"])
                            }
                            "subagents" => {
                                if let Some(children) = value.as_object_mut() {
                                    for child in children.values_mut() {
                                        retain(child, &["description"]);
                                    }
                                }
                            }
                            _ => {}
                        }
                        fields.insert(key, value);
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(Metadata(Value::Object(fields)))
            }
        }
        deserializer.deserialize_map(Fields)
    }
}

fn needed(key: &str) -> bool {
    matches!(
        key,
        "version"
            | "name"
            | "run_id"
            | "conversation_id"
            | "archive_id"
            | "agent"
            | "backend"
            | "cwd"
            | "archived_at"
            | "transcript"
            | "journal_name"
            | "prompt"
            | "last_message"
            | "turn_started"
            | "last_event_at"
            | "updated"
            | "created"
            | "agent_home"
            | "launch_dir"
            | "account_id"
            | "cleared_conversations"
            | "session_clear"
            | "rename_shadow"
            | "rename_target"
            | "legacy_shadow"
            | "subagents"
    )
}

fn retain(value: &mut Value, fields: &[&str]) {
    if let Some(object) = value.as_object_mut() {
        object.retain(|key, _| fields.contains(&key.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Record {
        fn expect_ready(self) -> Value {
            match self {
                Self::Ready(record) => record,
                other => panic!("Expected ready metadata, got {other:?}"),
            }
        }
    }

    fn budget() -> Budget {
        Budget {
            files: 32,
            directory_entries: 2000,
            records: 128,
            bytes: 4 << 20,
            deadline: Instant::now() + Duration::from_secs(1),
        }
    }

    #[test]
    fn shared_reads_skip_jobs_but_recheck_rewrites_and_replacements() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("binding.json");
        let record = json!({"version":1,"run_id":"first","shell_jobs":{"command":"private command ".repeat(80_000)}}).to_string();
        fs::write(&path, &record).unwrap();
        let mut reader = Reader::default();
        let mut progress = Progress::default();
        let first = reader
            .read(&path, &mut progress, &budget(), 0)
            .unwrap()
            .expect_ready();
        assert_eq!(first, json!({"version":1,"run_id":"first"}));
        assert_eq!(progress.bytes, record.len());
        assert_eq!(
            reader.read(&path, &mut progress, &budget(), 0).unwrap(),
            Record::Ready(first)
        );
        assert_eq!(progress.bytes, record.len());
        assert_eq!(progress.files, 2);

        // Same inode and length, with a distinct modification stamp.
        let rewritten = record.replace("first", "other");
        fs::write(&path, &rewritten).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
            .unwrap();
        let second = reader
            .read(&path, &mut progress, &budget(), 0)
            .unwrap()
            .expect_ready();
        assert_eq!(second["run_id"], "other");
        assert_eq!(progress.bytes, record.len() * 2);

        // Atomic replacement is distinct even with the same length and stamp.
        let replacement = temp.path().join("replacement.json");
        fs::write(&replacement, record.replace("first", "third")).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000))
            .unwrap();
        // Windows does not replace an open destination with std::fs::rename.
        fs::remove_file(&path).unwrap();
        fs::rename(&replacement, &path).unwrap();
        assert_eq!(
            reader
                .read(&path, &mut progress, &budget(), 0)
                .unwrap()
                .expect_ready()["run_id"],
            "third"
        );
        fs::remove_file(&path).unwrap();
        assert!(reader.read(&path, &mut progress, &budget(), 0).is_err());
    }

    #[test]
    fn raw_and_projected_limits_and_json_validation_still_apply() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("binding.json");
        let mut reader = Reader::default();
        fs::File::create(&path)
            .unwrap()
            .set_len(session_graph::MAX_RECORD + 1)
            .unwrap();
        let mut progress = Progress::default();
        assert!(reader.read(&path, &mut progress, &budget(), 0).is_err());
        assert_eq!(progress.bytes, 0);
        fs::write(
            &path,
            json!({"name":"x".repeat(RECORD_BYTES + 1),"shell_jobs":{}}).to_string(),
        )
        .unwrap();
        assert!(reader
            .read(&path, &mut Progress::default(), &budget(), 0)
            .is_err());
        for invalid in ["[]", "{\"shell_jobs\":[}", "{\"shell_jobs\":{}} trailing"] {
            fs::write(&path, invalid).unwrap();
            assert!(reader
                .read(&path, &mut Progress::default(), &budget(), 0)
                .is_err());
        }
    }

    #[test]
    fn large_public_snapshots_are_preserved_with_the_original_raw_budgets() {
        let temp = tempfile::tempdir().unwrap();
        for (index, (prompt, reply)) in [
            (RECORD_BYTES + 1, 0),
            (0, RECORD_BYTES + 1),
            (600 << 10, 600 << 10),
        ]
        .into_iter()
        .enumerate()
        {
            let path = temp.path().join(format!("{index}.json"));
            let record = json!({"version":1,"run_id":"first",
                "prompt":"x".repeat(prompt),"last_message":"y".repeat(reply)});
            let bytes = serde_json::to_vec(&record).unwrap();
            assert!(bytes.len() > RECORD_BYTES);
            fs::write(&path, &bytes).unwrap();
            let mut reader = Reader::default();
            let mut progress = Progress::default();
            assert_eq!(
                reader.read(&path, &mut progress, &budget(), 512).unwrap(),
                Record::Ready(record.clone())
            );
            assert_eq!(progress.bytes, bytes.len());
            assert_eq!(progress.files, 1);

            let mut short = budget();
            short.bytes = bytes.len();
            assert_eq!(
                Reader::default()
                    .read(&path, &mut Progress::default(), &short, 0)
                    .unwrap(),
                Record::OverBudget
            );
            let mut progress = Progress {
                bytes: budget().bytes - bytes.len(),
                ..Default::default()
            };
            let charged = progress.bytes;
            assert_eq!(
                Reader::default()
                    .read(&path, &mut progress, &budget(), 0)
                    .unwrap(),
                Record::Deferred
            );
            assert_eq!(progress.bytes, charged);
        }
    }

    #[test]
    fn projection_retains_search_identity_and_filters_without_unrelated_payloads() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("binding.json");
        let expected = json!({"version":1,"name":"codex/example/search", "run_id":"run",
            "conversation_id":"conversation", "agent":"kimi", "agent_home":"/fixture/kimi",
            "launch_dir":"/fixture", "account_id":"fixture-account", "journal_name":"earlier-name",
            "cleared_conversations":["earlier"],
            "session_clear":{"conversation_id":"conversation","previous_conversation_id":"earlier"},
            "rename_shadow":true,"rename_target":"codex/example/renamed",
            "legacy_shadow":{"name":"codex/example/search","run_id":"run"},
            "subagents":{"child":{"description":"Public child label"}}});
        let mut record = expected.clone();
        record["future_private"] = json!("unrelated private payload ".repeat(50_000));
        record["legacy_shadow"]["private_payload"] = json!("private legacy payload");
        record["session_clear"]["private_payload"] = json!("private control payload");
        record["subagents"]["child"]["private_payload"] = json!("private child payload");
        let bytes = serde_json::to_vec(&record).unwrap();
        assert!(bytes.len() > RECORD_BYTES);
        fs::write(&path, &bytes).unwrap();
        let mut progress = Progress::default();
        assert_eq!(
            Reader::default()
                .read(&path, &mut progress, &budget(), 0)
                .unwrap(),
            Record::Ready(expected)
        );
        assert_eq!(progress.bytes, bytes.len());
    }

    #[cfg(windows)]
    #[test]
    fn windows_cached_metadata_releases_the_rename_destination() {
        use std::io::Write;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("binding.json");
        fs::write(&path, r#"{"version":1}"#).unwrap();
        let mut reader = Reader::default();
        reader
            .read(&path, &mut Progress::default(), &budget(), 0)
            .unwrap()
            .expect_ready();
        let mut replacement = tempfile::NamedTempFile::new_in(temp.path()).unwrap();
        replacement.write_all(br#"{"version":2}"#).unwrap();
        crate::platform::persist(replacement, &path).unwrap();
        assert_eq!(
            reader
                .read(&path, &mut Progress::default(), &budget(), 0)
                .unwrap()
                .expect_ready()["version"],
            2
        );
    }
}
