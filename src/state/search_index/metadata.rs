//! Saved session records can contain large private shell-job histories. Keep
//! raw I/O separately bounded, but retain only fields used by history search.
//! A native-version-checked compact cache avoids rereading private job payloads.
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
    version: String,
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
pub(super) struct Reader {
    entries: BTreeMap<PathBuf, Entry>,
    cache: Option<Connection>,
    cache_root: Option<PathBuf>,
    cache_attempts: u8,
    raw: usize,
    foreground: bool,
}
const RAW_BYTES: usize = 64 << 20;
const BATCH_BYTES: usize = 256 << 20;

struct TimedRead<'a> {
    file: &'a mut fs::File,
    read: &'a mut usize,
    until: Instant,
}
impl Read for TimedRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if Instant::now() >= self.until {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Search metadata slice expired",
            ));
        }
        let length = buf.len().min(64 << 10);
        let n = self.file.read(&mut buf[..length])?;
        *self.read += n;
        Ok(n)
    }
}

impl Reader {
    pub fn cached(state: &Path) -> Self {
        Self {
            cache: metadata_cache::open(state),
            cache_root: Some(state.to_owned()),
            ..Self::default()
        }
    }
    pub fn foreground(state: &Path) -> Self {
        Self {
            foreground: true,
            ..Self::cached(state)
        }
    }
    fn ensure_cache(&mut self) {
        if self.cache.is_none() && self.cache_attempts < 3 {
            if let Some(state) = &self.cache_root {
                self.cache_attempts += 1;
                self.cache = metadata_cache::open(state);
            }
        }
    }
    pub fn id_path(&mut self, state: &Path, identity: &Identity) -> Option<PathBuf> {
        self.ensure_cache();
        let namespace = crate::platform::canonical(state).ok()?.join("sessions");
        let valid = |path: &Path, record: &Value| {
            path.parent() == Some(namespace.as_path())
                && path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .is_some_and(|id| storage::is_id_layout_record(record, id))
                && record["name"] == identity.name
                && record["run_id"] == identity.run_id
                && record["conversation_id"] == identity.conversation_id
        };
        for (path, entry) in &self.entries {
            if valid(path, &entry.record) {
                return Some(path.clone());
            }
        }
        let db = self.cache.as_ref()?;
        let source = identity.target().key().ok()?;
        let paths:Vec<String>=db.prepare("SELECT path FROM catalog_queue WHERE source=? AND kind='binding' ORDER BY id LIMIT 2").ok()?
            .query_map([source],|r|r.get(0)).ok()?.collect::<rusqlite::Result<_>>().ok()?;
        paths.into_iter().map(PathBuf::from).find(|path| {
            path.parent() == Some(namespace.as_path())
                && path
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .is_some_and(session_id::canonical)
        })
    }
    pub fn cache_into(&self, db: &Connection, path: &Path) -> Result<()> {
        let Some(path) = metadata_cache::path(path) else {
            return Ok(());
        };
        if let Some(entry) = self.entries.get(&path) {
            #[cfg(not(windows))]
            if metadata_cache::stamp(&entry.file)? != entry.version {
                return Ok(());
            }
            metadata_cache::put(db, &path, &entry.version, &entry.record)?;
        }
        Ok(())
    }
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
        if !metadata.is_file() || metadata.len() > RAW_BYTES as u64 {
            return Err("Session metadata exceeds the saved-record limit".into());
        }
        let path = metadata_cache::path(path).ok_or("Search metadata parent is unavailable")?;
        let path = path.as_path();
        if let Some(entry) = self.entries.get(path) {
            if entry.current(stamp, &metadata) && metadata_cache::stamp(&file)? == entry.version {
                let charge = serde_json::to_vec(&entry.record)
                    .map_err(|e| e.to_string())?
                    .len()
                    + 1;
                if charge.saturating_add(reserve) > budget.bytes {
                    return Ok(Record::OverBudget);
                }
                if reserve > budget.bytes.saturating_sub(progress.bytes) {
                    return Ok(Record::Deferred);
                }
                return Ok(Record::Ready(entry.record.clone()));
            }
        }
        let version = metadata_cache::stamp(&file)?;
        // A helper can briefly own SQLite's commit/schema lock when this
        // reader is created. Retry after that cut instead of remaining blind
        // for every subsequent source in the request. Attempts and the whole
        // call's existing deadline bound these short waits.
        self.ensure_cache();
        let cached = self
            .cache
            .as_ref()
            .and_then(|db| metadata_cache::get(db, path, &version));
        let record = if let Some(record) = cached {
            record
        } else {
            let raw_limit = if self.foreground {
                4 << 20
            } else {
                BATCH_BYTES
            };
            if metadata.len() as usize > raw_limit {
                return Ok(Record::OverBudget);
            }
            if metadata.len() as usize > raw_limit.saturating_sub(self.raw) {
                return Ok(Record::Deferred);
            }
            let before = self.raw;
            // Cold preparation reads one bounded canonical body transiently.
            // Only its small projection survives; foreground reads have their
            // original 4 MiB raw limit and use prepared metadata thereafter.
            let input = TimedRead {
                file: &mut file,
                read: &mut self.raw,
                until: budget.deadline,
            };
            let mut bytes = Vec::with_capacity(metadata.len() as usize);
            let read = input.take(metadata.len() + 1).read_to_end(&mut bytes);
            progress.metadata_io += self.raw - before;
            match read {
                Ok(_) if bytes.len() == metadata.len() as usize => (),
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                    return Ok(Record::Deferred)
                }
                _ => return Err("Session metadata changed during read".into()),
            }
            let Metadata(record) = serde_json::from_slice(&bytes)
                .map_err(|_| "Session metadata is incomplete or invalid")?;
            record
        };
        if metadata_cache::stamp(&file)? != version {
            return Err("Session metadata changed during read".into());
        }
        let required = serde_json::to_vec(&record)
            .map_err(|e| e.to_string())?
            .len()
            + 1;
        if required.saturating_add(reserve) > budget.bytes {
            return Ok(Record::OverBudget);
        }
        if required.saturating_add(reserve) > budget.bytes.saturating_sub(progress.bytes) {
            return Ok(Record::Deferred);
        }
        progress.bytes += required;
        let bounded_fields: BTreeMap<_, _> = record
            .as_object()
            .expect("Metadata deserializes an object")
            .iter()
            .filter(|(key, value)| match key.as_str() {
                "prompt" | "last_message" => !value.is_string(),
                "keep_warm_quiet" => false, // Only public snapshot strings survive decoding.
                _ => true,
            })
            .collect();
        if serde_json::to_vec(&bounded_fields)
            .map_err(|e| e.to_string())?
            .len()
            > RECORD_BYTES
        {
            return Err("Session search metadata exceeds its budget".into());
        }
        if self.entries.len() >= 32 {
            self.entries.pop_first();
        }
        self.entries.insert(
            path.to_owned(),
            Entry {
                #[cfg(not(windows))]
                file,
                #[cfg(windows)]
                created: metadata.created().ok(),
                stamp,
                len: metadata.len(),
                record: record.clone(),
                version,
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
                            "keep_warm_quiet" => {
                                let Some(fields) = value.as_object_mut() else {
                                    continue;
                                };
                                fields.retain(|key, value| {
                                    matches!(key.as_str(), "prompt" | "last_message")
                                        && value.is_string()
                                });
                            }
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
            | "session_id"
            | "layout"
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
            | "keep_warm_quiet"
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
    fn quiet_public_snapshots_keep_their_projection_and_raw_byte_charge() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("binding.json");
        let prompt = "p".repeat(RECORD_BYTES + 1);
        let record = json!({"prompt":"internal prompt","last_message":"internal reply",
            "keep_warm_quiet":{"prompt":prompt,"last_message":"Earlier public answer",
                "private_debug":"not public"},"shell_jobs":{"private":"ignored"}})
        .to_string();
        fs::write(&path, &record).unwrap();
        let mut progress = Progress::default();
        let value = Reader::default()
            .read(&path, &mut progress, &budget(), 0)
            .unwrap()
            .expect_ready();
        assert_eq!(journal::user_prompt(&value).as_str(), Some(prompt.as_str()));
        assert_eq!(journal::last_message(&value), "Earlier public answer");
        assert_eq!(progress.metadata_io, record.len());
        assert!(value["keep_warm_quiet"].get("private_debug").is_none());
        assert!(value.get("shell_jobs").is_none());
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
        assert_eq!(progress.metadata_io, record.len());
        assert_eq!(
            reader.read(&path, &mut progress, &budget(), 0).unwrap(),
            Record::Ready(first)
        );
        assert_eq!(progress.metadata_io, record.len());
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
        assert_eq!(progress.metadata_io, record.len() * 2);

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
            .set_len(RAW_BYTES as u64 + 1)
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
            assert_eq!(progress.metadata_io, bytes.len());
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
        assert_eq!(progress.metadata_io, bytes.len());
    }

    #[test]
    fn non_string_public_snapshot_fields_still_count_toward_the_metadata_cap() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("binding.json");
        for key in ["prompt", "last_message"] {
            for value in [
                json!({"not_public_text":"x".repeat(RECORD_BYTES + 1)}),
                json!(["x".repeat(RECORD_BYTES + 1)]),
            ] {
                let mut record = json!({"version":1});
                record[key] = value;
                let bytes = serde_json::to_vec(&record).unwrap();
                assert!(bytes.len() > RECORD_BYTES && bytes.len() < budget().bytes);
                fs::write(&path, &bytes).unwrap();
                let mut progress = Progress::default();
                let error = Reader::default()
                    .read(&path, &mut progress, &budget(), 0)
                    .unwrap_err();
                assert_eq!(error, "Session search metadata exceeds its budget", "{key}");
                assert_eq!(progress.metadata_io, bytes.len());
            }
        }
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
    #[test]
    fn one_reader_does_not_retain_a_file_handle_for_every_session() {
        let temp = tempfile::tempdir().unwrap();
        let mut reader = Reader::default();
        let mut progress = Progress::default();
        let budget = Budget {
            files: 256,
            directory_entries: 2000,
            records: 512,
            bytes: 4 << 20,
            deadline: Instant::now() + Duration::from_secs(2),
        };
        for n in 0..200 {
            let path = temp.path().join(format!("{n}.json"));
            fs::write(
                &path,
                json!({"version":1,"name":format!("session-{n}")}).to_string(),
            )
            .unwrap();
            let record = reader
                .read(&path, &mut progress, &budget, 0)
                .unwrap()
                .expect_ready();
            assert_eq!(record["name"], format!("session-{n}"));
        }
        assert!(
            reader.entries.len() <= 32,
            "a foreground scan exhausts small descriptor limits"
        );
    }
    #[test]
    fn bounded_preparation_can_use_more_than_one_record_window() {
        let temp = tempfile::tempdir().unwrap();
        let mut reader = Reader::default();
        let mut progress = Progress::default();
        let body =
            json!({"version":1,"shell_jobs":{"job":{"output":"x".repeat(9<<20)}}}).to_string();
        for n in 0..8 {
            let path = temp.path().join(format!("large-{n}.json"));
            fs::write(&path, &body).unwrap();
            // Isolate the aggregate raw ceiling from scheduler delays.
            let budget = Budget {
                files: 32,
                directory_entries: 2000,
                records: 512,
                bytes: 4 << 20,
                deadline: Instant::now() + Duration::from_secs(2),
            };
            assert_eq!(
                reader
                    .read(&path, &mut progress, &budget, 0)
                    .unwrap()
                    .expect_ready()["version"],
                1
            );
        }
        assert_eq!(progress.metadata_io, body.len() * 8);
    }
}
