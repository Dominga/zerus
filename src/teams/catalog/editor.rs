//! Opt-in editor snapshot/import receipts. A reservation precedes publication;
//! an interrupted reservation is unknown forever and is never replayed.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Read;

pub(crate) const EDIT_BYTES: u64 = 240_000;
const RECEIPT_BYTES: u64 = 8192;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Edit {
    pub schema: u32,
    pub request_id: String,
    pub expected_revision: String,
    pub catalog: Catalog,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    request_id: String,
    input_digest: String,
    receipt: Value,
}

fn digest(value: &impl Serialize) -> Result<String, Problem> {
    let bytes = serde_json::to_vec(value).map_err(write_error_serde)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn write_error_serde(error: impl std::fmt::Display) -> Problem {
    Problem::new(code::STORAGE_FAILED, error.to_string())
}
fn revision(person: &Catalog) -> Result<String, Problem> {
    Ok(format!("catalog-v1:{}", digest(&(builtin(), person))?))
}

fn selected(config: &Path) -> Result<Catalog, Problem> {
    let file = match File::open(path(config)) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Catalog::default()),
        Err(e) => return Err(read_error(e)),
    };
    if !file.metadata().map_err(read_error)?.is_file() {
        return Err(Problem::new(
            code::INVALID_STATE,
            "The catalog is not a regular file.",
        ));
    }
    let mut bytes = Vec::new();
    file.take(EDIT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    if bytes.len() as u64 > EDIT_BYTES {
        return Err(Problem::new(
            code::OVER_CAP,
            "The catalog exceeds the editor size bound.",
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| Problem::new(code::INVALID_STATE, e.to_string()))
}

pub(crate) fn snapshot(config: &Path) -> Result<Value, Problem> {
    let person = selected(config)?;
    let catalog = merge(&builtin(), &person);
    first_problem(validate_catalog(&catalog))?;
    Ok(
        json!({"schema":1,"revision":revision(&person)?,"catalog":catalog,"person":person,"builtin":builtin()}),
    )
}

/// Catalog commands share this lock, including legacy read/modify/write saves.
/// A read-only snapshot needs no lock: it reads one atomically published file.
pub(crate) fn writer(config: &Path) -> Result<crate::platform::FileLock, Problem> {
    let dir = config.join(DIR);
    crate::platform::private_dir(&dir).map_err(write_error)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("catalog.lock"))
        .map_err(write_error)?;
    crate::platform::FileLock::exclusive(file).map_err(write_error)
}

fn id(id: &str) -> Result<(), Problem> {
    if uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id) {
        Ok(())
    } else {
        Err(Problem::new(
            code::INVALID_STATE,
            "Not a catalog request UUID.",
        ))
    }
}
fn record_path(config: &Path, request: &str) -> PathBuf {
    config
        .join(DIR)
        .join("catalog-requests")
        .join(format!("{request}.json"))
}
fn record(config: &Path, request: &str) -> Result<Option<Record>, Problem> {
    id(request)?;
    let file = match File::open(record_path(config, request)) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(write_error_serde(e)),
    };
    let mut bytes = Vec::new();
    file.take(RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(write_error_serde)?;
    if bytes.len() as u64 > RECEIPT_BYTES {
        return Err(Problem::new(
            code::STORAGE_FAILED,
            "The catalog receipt exceeds its bound.",
        ));
    }
    let record: Record = serde_json::from_slice(&bytes).map_err(write_error_serde)?;
    if record.schema != 1 || record.request_id != request {
        return Err(Problem::new(
            code::STORAGE_FAILED,
            "Catalog receipt identity differs.",
        ));
    }
    Ok(Some(record))
}
pub(crate) fn receipt(config: &Path, request: &str) -> Result<Value, Problem> {
    Ok(record(config, request)?.map_or_else(
        || json!({"schema":1,"request_id":request,"outcome":"uncertain"}),
        |record| record.receipt,
    ))
}
fn publish(config: &Path, record: &Record) -> Result<(), Problem> {
    let path = record_path(config, &record.request_id);
    let directory = path.parent().expect("receipt parent");
    crate::platform::private_dir(directory).map_err(write_error)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(write_error)?;
    serde_json::to_writer(&mut temporary, record).map_err(write_error_serde)?;
    temporary.as_file().sync_all().map_err(write_error)?;
    crate::platform::persist(temporary, &path).map_err(|e| write_error(e.error))?;
    crate::platform::sync_dir(directory).map_err(write_error_serde)?;
    crate::platform::sync_dir(&config.join(DIR)).map_err(write_error_serde)?;
    Ok(())
}

/// Caller and writer lock are established by catalog_cmd before this function.
pub(crate) fn import(config: &Path, edit: Edit, actor: &Value) -> Result<Value, Problem> {
    id(&edit.request_id)?;
    if edit.schema != 1 {
        return Err(Problem::new(
            code::INVALID_STATE,
            "Unknown catalog edit schema.",
        ));
    }
    if serde_json::to_vec(&edit.catalog)
        .map_err(write_error_serde)?
        .len() as u64
        > EDIT_BYTES
    {
        return Err(Problem::new(
            code::OVER_CAP,
            "The catalog exceeds the editor size bound.",
        ));
    }
    if [
        edit.catalog.roles.len(),
        edit.catalog.pipelines.len(),
        edit.catalog.templates.len(),
    ]
    .into_iter()
    .any(|count| count > 200)
    {
        return Err(Problem::new(
            code::OVER_CAP,
            "The catalog has too many editor entries.",
        ));
    }
    let input_digest = digest(&edit)?;
    if let Some(old) = record(config, &edit.request_id)? {
        if old.input_digest != input_digest {
            return Err(Problem::new(
                "request_conflict",
                "This request UUID already names another catalog edit.",
            ));
        }
        return Ok(old.receipt);
    }
    if revision(&selected(config)?)? != edit.expected_revision {
        return Err(Problem::new(
            code::STALE_REVISION,
            "The catalog changed. Revert to read it again before saving.",
        ));
    }
    // Same admission as legacy import, before reserving any publication.
    checked(&edit.catalog)?;
    let mut record = Record {
        schema: 1,
        request_id: edit.request_id.clone(),
        input_digest,
        receipt: json!({"ok":true,"schema":1,"request_id":edit.request_id,"outcome":"uncertain","actor":actor}),
    };
    publish(config, &record)?;
    write(config, &edit.catalog).map_err(write_error_serde)?;
    crate::platform::sync_dir(&config.join(DIR)).map_err(write_error_serde)?;
    record.receipt = json!({"ok":true,"schema":1,"request_id":edit.request_id,"outcome":"done","revision":revision(&edit.catalog)?,"actor":actor});
    publish(config, &record).map_err(write_error_serde)?;
    Ok(record.receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_reservation_never_republishes_even_when_catalog_still_matches() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path();
        let edit = Edit {
            schema: 1,
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: revision(&Catalog::default()).unwrap(),
            catalog: Catalog::default(),
        };
        let record = Record {
            schema: 1,
            request_id: edit.request_id.clone(),
            input_digest: digest(&edit).unwrap(),
            receipt: json!({"ok":true,"schema":1,"request_id":edit.request_id,"outcome":"uncertain"}),
        };
        publish(config, &record).unwrap();
        assert_eq!(
            import(config, edit, &json!({"kind":"person"})).unwrap()["outcome"],
            "uncertain"
        );
        assert!(!path(config).exists(), "no replay");
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    #[test]
    fn interrupted_post_publication_keeps_unknown_without_overwriting_current_file() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path();
        let edit = Edit {
            schema: 1,
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_revision: revision(&Catalog::default()).unwrap(),
            catalog: Catalog::default(),
        };
        let record = Record {
            schema: 1,
            request_id: edit.request_id.clone(),
            input_digest: digest(&edit).unwrap(),
            receipt: json!({"ok":true,"schema":1,"request_id":edit.request_id,"outcome":"uncertain"}),
        };
        publish(config, &record).unwrap();
        // Simulate the publication cut using the production catalog writer.
        let latest = Catalog {
            planning_rules: vec!["Already published".into()],
            ..Catalog::default()
        };
        save(config, &latest).unwrap();
        let before = std::fs::read(path(config)).unwrap();
        assert_eq!(
            import(config, edit, &json!({"kind":"person"})).unwrap()["outcome"],
            "uncertain"
        );
        assert_eq!(std::fs::read(path(config)).unwrap(), before);
    }
}
