//! Saved session records can contain large private shell-job histories. Keep
//! their raw I/O charged, but retain only the metadata used by history search.
use super::*;
use crate::platform::FileIdentity;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use std::{collections::BTreeMap, fmt};

struct Entry {
    file: fs::File,
    stamp: FileIdentity,
    len: u64,
    record: Value,
}

/// One bounded call owns this cache. Repeated sweep/queue/ingestion reads share
/// bytes only while both the held file and the freshly opened path still match.
#[derive(Default)]
pub(super) struct Reader(BTreeMap<PathBuf, Entry>);

impl Reader {
    pub fn read(
        &mut self,
        path: &Path,
        progress: &mut Progress,
        budget: &Budget,
        reserve: usize,
    ) -> Result<Option<Value>> {
        if progress.files >= budget.files || Instant::now() >= budget.deadline {
            return Ok(None);
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
            if entry.stamp == stamp
                && entry.len == metadata.len()
                && FileIdentity::with_metadata(&entry.file)
                    .is_ok_and(|(held, meta)| held == stamp && meta.len() == entry.len)
            {
                return Ok(Some(entry.record.clone()));
            }
        }
        let remaining = budget.bytes.saturating_sub(progress.bytes);
        let required = metadata.len() as usize + 1;
        if required.saturating_add(reserve) > remaining {
            // A short slice is not proof that this source has disappeared.
            return Ok(None);
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
        if serde_json::to_vec(&record)
            .map_err(|e| e.to_string())?
            .len()
            > RECORD_BYTES
        {
            return Err("Session search metadata exceeds its budget".into());
        }
        self.0.insert(
            path.to_owned(),
            Entry {
                file,
                stamp,
                len: metadata.len(),
                record: record.clone(),
            },
        );
        if Instant::now() >= budget.deadline {
            return Ok(None);
        }
        Ok(Some(record))
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
                    if key == "shell_jobs" {
                        map.next_value::<IgnoredAny>()?;
                    } else {
                        fields.insert(key, map.next_value()?);
                    }
                }
                Ok(Metadata(Value::Object(fields)))
            }
        }
        deserializer.deserialize_map(Fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            .unwrap();
        assert_eq!(first, json!({"version":1,"run_id":"first"}));
        assert_eq!(progress.bytes, record.len());
        assert_eq!(
            reader.read(&path, &mut progress, &budget(), 0).unwrap(),
            Some(first)
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
            .unwrap();
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
                .unwrap()["run_id"],
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
            json!({"prompt":"x".repeat(RECORD_BYTES + 1),"shell_jobs":{}}).to_string(),
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
}
