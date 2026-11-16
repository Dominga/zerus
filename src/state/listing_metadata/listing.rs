//! Ordinary ls only. Watch and canonical/action readers retain their own paths.
use super::*;

const COMPAT_ENTRIES: usize = 256;
const COMPAT_BYTES: u64 = 16 << 20;
const BODY_BYTES: usize = 64 << 20;
const PAGE_BYTES: usize = 512 << 10;

#[derive(Default)]
pub(crate) struct Query {
    pub paged: bool,
    pub cursor: Option<String>,
    pub page_size: usize,
}

struct Canonical {
    root: Directory,
    dirs: String,
    witnesses: Vec<(Key, String)>,
    rows: Vec<(String, Value)>,
}
impl Canonical {
    fn capture(path: &Path) -> anyhow::Result<Self> {
        let root = checked(Directory::open(&crate::platform::canonical(path)?))?;
        let dirs = repair::directories_at(&root)?;
        let until = Instant::now() + Duration::from_millis(250);
        let mut entries = 0;
        let mut bytes = 0u64;
        let mut witnesses = Vec::new();
        // Enumerate and prove the COMPLETE small roster before reading a body.
        for namespace in storage::BINDING_DIRS {
            let paths = match std::fs::read_dir(root.path.join(namespace)) {
                Ok(paths) => paths,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for entry in paths {
                entries += 1;
                anyhow::ensure!(entries<=COMPAT_ENTRIES && Instant::now()<until,
                    "Session listing exceeds the bounded default-off inventory; enable listing-index on this computer");
                let path = entry?.path();
                let Some(key) = Key::at(&root.path, &path) else {
                    continue;
                };
                let metadata = std::fs::symlink_metadata(&path)?;
                anyhow::ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "Session listing source is unavailable"
                );
                bytes = bytes.saturating_add(metadata.len());
                anyhow::ensure!(bytes<=COMPAT_BYTES,"Session listing exceeds the bounded default-off byte budget; enable listing-index on this computer");
                let file = crate::platform::no_follow(std::fs::OpenOptions::new().read(true))
                    .open(&path)?;
                witnesses.push((key, store::version(&file)?));
            }
        }
        anyhow::ensure!(
            repair::directories_at(&root)? == dirs,
            "Session listing inventory changed during preflight"
        );
        let mut rows = Vec::new();
        let mut read = 0;
        let mut files = 0;
        for (key, stamp) in &witnesses {
            let record = selected_record(
                &root.path,
                &key.0,
                &json!({"native_stamp":stamp}),
                &mut read,
                &mut files,
                COMPAT_ENTRIES,
                until,
            )?;
            // Unsupported private/public shapes refuse before native overlays.
            let _projection = Seed::encode(key, &record)?;
            if record["rename_shadow"] != true {
                rows.push((key.0.clone(), json!({"record":record,"native_stamp":stamp})));
            }
        }
        let result = Self {
            root,
            dirs,
            witnesses,
            rows,
        };
        result.verify()?;
        Ok(result)
    }
    fn verify(&self) -> anyhow::Result<()> {
        let until = Instant::now() + Duration::from_millis(250);
        for (key, stamp) in &self.witnesses {
            anyhow::ensure!(
                Instant::now() < until,
                "Session listing verification timed out"
            );
            let file = crate::platform::no_follow(std::fs::OpenOptions::new().read(true))
                .open(self.root.path.join(&key.0))?;
            anyhow::ensure!(
                store::version(&file)? == *stamp,
                "Session listing changed before publication"
            );
        }
        anyhow::ensure!(
            repair::directories_at(&self.root)? == self.dirs,
            "Session listing membership changed before publication"
        );
        checked(self.root.verify())
    }
}

fn selected_record(
    root: &Path,
    source: &str,
    seed: &Value,
    bytes: &mut usize,
    files: &mut usize,
    max_files: usize,
    until: Instant,
) -> anyhow::Result<Value> {
    anyhow::ensure!(
        *files < max_files && Instant::now() < until,
        "Selected listing overlays exceeded their bound"
    );
    let key = Key(source.into());
    key.validate()?;
    let parent = checked(Directory::open(root))?;
    let mut directory = parent;
    for part in Path::new(source).parent().unwrap().components() {
        directory = checked(
            directory.child(
                part.as_os_str()
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid display source"))?,
            ),
        )?;
    }
    let path = directory.path.join(Path::new(source).file_name().unwrap());
    let mut file =
        crate::platform::no_follow(std::fs::OpenOptions::new().read(true)).open(&path)?;
    anyhow::ensure!(
        file.metadata()?.is_file(),
        "Selected listing source is not regular"
    );
    let stamp = store::version(&file)?;
    anyhow::ensure!(
        seed["native_stamp"].as_str() == Some(stamp.as_str()),
        "Selected listing source changed"
    );
    let length = file.metadata()?.len() as usize;
    anyhow::ensure!(
        length <= COMPAT_BYTES as usize - *bytes,
        "Selected listing overlay exceeds its byte budget"
    );
    use std::io::Read;
    let mut raw = Vec::new();
    for _ in 0..length.div_ceil(64 << 10) {
        anyhow::ensure!(Instant::now() < until, "Selected listing overlay timed out");
        let mut chunk = [0; 64 << 10];
        let remaining = (length - raw.len()).min(chunk.len());
        let n = file.read(&mut chunk[..remaining])?;
        raw.extend_from_slice(&chunk[..n]);
        if n == 0 {
            break;
        }
    }
    *bytes += raw.len();
    *files += 1;
    anyhow::ensure!(
        raw.len() == length && store::version(&file)? == stamp,
        "Selected listing overlay changed"
    );
    checked(directory.verify())?;
    let current = crate::platform::no_follow(std::fs::OpenOptions::new().read(true)).open(path)?;
    anyhow::ensure!(
        store::version(&current)? == stamp,
        "Selected listing source route changed"
    );
    let record: Value = serde_json::from_slice(&raw)?;
    anyhow::ensure!(key.matches(&record), "Selected listing identity changed");
    Ok(record)
}

pub(in crate::state) fn merge(
    mut value: Value,
    ids: &BTreeMap<String, String>,
    archives: bool,
    query: &Query,
) -> Result<Value> {
    let mut run = || -> anyhow::Result<Value> {
        let root = root();
        let indexed = changes::qualified() && enabled(&root);
        anyhow::ensure!(
            indexed || !query.paged,
            "Paged session listing needs an enabled qualified index on this computer"
        );
        let _capture = checked(storage::writer_until(
            Instant::now() + Duration::from_millis(250),
        ))?
        .ok_or_else(|| anyhow::anyhow!("Session listing is waiting for the canonical writer"))?;
        let mut canonical = if indexed {
            None
        } else {
            Some(Canonical::capture(&root)?)
        };
        drop(_capture);
        if indexed {
            worker::request(&root);
        }
        let reader = if indexed {
            Some(pages::Reader::open(&root)?)
        } else {
            None
        };
        let mut cursor = query
            .cursor
            .as_deref()
            .map(pages::Cursor::decode)
            .transpose()?;
        let until = Instant::now() + Duration::from_secs(10);
        let ((snapshot, mut names), mut live) = {
            let _guard = checked(storage::writer_until(
                Instant::now() + Duration::from_millis(250),
            ))?
            .ok_or_else(|| {
                anyhow::anyhow!("Session listing is waiting for the canonical writer")
            })?;
            let observation = checked(live_sessions())?;
            let rows = value["sessions"]
                .as_array_mut()
                .ok_or_else(|| anyhow::anyhow!("Invalid session listing"))?;
            (observation, std::mem::take(rows))
        };
        let current = |listed: &str| ids.get(listed).and_then(|id| names.get(id)).cloned();
        let held: std::collections::BTreeSet<_> = live
            .iter()
            .filter_map(|row| current(string(row, "name")))
            .collect();
        live.retain_mut(|row| {
            let listed = string(row, "name").to_owned();
            match current(&listed) {
                Some(name) => {
                    if name != listed {
                        let rec = json!({"name":name});
                        label_row(row, &rec);
                    }
                    true
                }
                None => snapshot.contains_key(&listed) && !held.contains(&listed),
            }
        });
        for row in &mut live {
            row.as_object_mut()
                .unwrap()
                .extend(telemetry::unavailable().as_object().unwrap().clone());
        }
        let marks = read_marks::load();
        let preferences = notifications::load();
        let warm = keep_warm::load();
        let routing = question_routing::load();
        let outcomes = question_route_outcomes::Reader::load();
        let shared = Shared {
            marks: &marks,
            preferences: preferences.as_ref(),
            keep_warm: warm.as_ref(),
            question_routing: routing.as_ref(),
            route_outcomes: outcomes.as_ref(),
            budget: read_marks::question_budget(),
        };
        let mut archive_page = if archives {
            Some(checked(archive_catalog::preview(&marks))?)
        } else {
            None
        };
        let archived = archive_page
            .as_ref()
            .and_then(|page| page["sessions"].as_array())
            .cloned()
            .unwrap_or_default();
        let mut rows = Vec::new();
        let mut process_work = processes::WorkSample::default();
        let mut total_bytes = 0;
        let mut canonical_bytes = 0;
        let mut canonical_files = 0;
        loop {
            anyhow::ensure!(
                Instant::now() < until,
                "Session listing deadline exceeded; use bounded pages"
            );
            let (batch, next) = if let Some(reader) = &reader {
                let page = reader.page_limit(
                    cursor.as_ref(),
                    if query.paged { query.page_size } else { 128 },
                )?;
                anyhow::ensure!(
                    page.fresh,
                    "Session listing is building, stale or unavailable; no complete body returned"
                );
                (page.rows, page.next)
            } else {
                (std::mem::take(&mut canonical.as_mut().unwrap().rows), None)
            };
            let mut deepseek = Vec::new();
            let mut opencode = Vec::new();
            let start_count = rows.len();
            for (source, seed) in batch {
                let mut record = seed["record"].clone();
                if source.starts_with("dsh/") {
                    deepseek.push(record);
                    continue;
                }
                if source.starts_with("opencode/") {
                    opencode.push(record);
                    continue;
                }
                if archive::equivalent(&record, &archived) {
                    continue;
                }
                if let Some(index) = live
                    .iter()
                    .position(|row| string(row, "name") == terminal_name(&record))
                {
                    if matches(&record, panes_of(&snapshot, &record)) {
                        if indexed {
                            record = selected_record(
                                &root,
                                &source,
                                &seed,
                                &mut canonical_bytes,
                                &mut canonical_files,
                                16,
                                until,
                            )?;
                        }
                        let mut row = live.remove(index);
                        label_row(&mut row, &record);
                        workspace::relabel(
                            &mut names,
                            terminal_name(&record),
                            string(&record, "name"),
                        );
                        checked(live_row(&mut row, record, &shared, &mut process_work))?;
                        rows.push(row);
                        continue;
                    }
                }
                if string(&record, "conversation_id").is_empty()
                    || terminal_live(&snapshot, &record).unwrap_or(false)
                {
                    continue;
                }
                if indexed && record["_listing"]["requires_canonical"] == true {
                    record = selected_record(
                        &root,
                        &source,
                        &seed,
                        &mut canonical_bytes,
                        &mut canonical_files,
                        16,
                        until,
                    )?;
                }
                let name = string(&record, "name");
                let parts: Vec<_> = name.splitn(3, '/').collect();
                let mut row = json!({"name":name,"cmd":record["agent"],"project":parts.get(1),"tag":parts.get(2),"attached":0,"clients":[],"created":record["created"],"state":if record["paused"]==true {"paused"}else{"stopped"},"resumable":true});
                row.as_object_mut().unwrap().extend(
                    journal::summary(&record, false)
                        .as_object()
                        .unwrap()
                        .clone(),
                );
                read_marks::enrich_listing(
                    &mut row,
                    &marks,
                    journal::journal_name(&record),
                    Ok(Some(Vec::new())),
                );
                notifications::enrich(
                    &mut row,
                    preferences.as_ref(),
                    journal::journal_name(&record),
                );
                keep_warm::enrich(&mut row, warm.as_ref(), &record);
                question_routing::enrich(&mut row, routing.as_ref());
                rows.push(row);
            }
            let native_until = until.min(Instant::now() + Duration::from_secs(1));
            rows.extend(checked(dsh::listing_records(&deepseek, native_until))?);
            rows.extend(checked(opencode::listing_records(&opencode, native_until))?);
            for row in &rows[start_count..] {
                total_bytes += serde_json::to_vec(row)?.len();
            }
            anyhow::ensure!(
                total_bytes <= if query.paged { PAGE_BYTES } else { BODY_BYTES },
                "Session listing response exceeds its bound; choose a smaller --page-size"
            );
            cursor = next;
            if query.paged || cursor.is_none() {
                break;
            }
        }
        // A late publisher/gap must invalidate the retained transaction's body.
        if let Some(reader) = &reader {
            anyhow::ensure!(
                reader.page_limit(cursor.as_ref(), 1)?.fresh,
                "Session listing changed before publication; no complete body returned"
            );
        } else {
            canonical.as_ref().unwrap().verify()?;
        }

        if !query.paged || query.cursor.is_none() {
            rows.extend(live);
        }
        if let Some(page) = archive_page.as_mut() {
            rows.append(
                page["sessions"]
                    .as_array_mut()
                    .ok_or_else(|| anyhow::anyhow!("Invalid archive preview"))?,
            );
            page.as_object_mut().unwrap().remove("sessions");
        }
        finish_rows(&mut rows, &marks, &names);
        if query.paged {
            for row in &mut rows {
                let Some(link) = row
                    .get("parent")
                    .filter(|parent| parent.is_object())
                    .cloned()
                else {
                    continue;
                };
                let parent = reader.as_ref().unwrap().parent(&link)?;
                let mut pair = Vec::new();
                if let Some(parent) = parent {
                    let native_until = until.min(Instant::now() + Duration::from_secs(1));
                    let observed = match string(&parent, "backend") {
                        "dsh" => checked(dsh::listing_records(&[parent], native_until))?.remove(0),
                        "opencode" => {
                            checked(opencode::listing_records(&[parent], native_until))?.remove(0)
                        }
                        _ => {
                            let live = matches(&parent, panes_of(&snapshot, &parent));
                            let mut observed = journal::normalized_activity(&parent, live);
                            observed["session_id"] = parent["session_id"].clone();
                            observed["conversation_id"] = parent["conversation_id"].clone();
                            observed["state"] = json!(if live { "running" } else { "stopped" });
                            observed
                        }
                    };
                    pair.push(observed);
                } else if let Some(archived) = archived.iter().find(|candidate| {
                    !string(&link, "session_id").is_empty()
                        && candidate["session_id"] == link["session_id"]
                        || string(&link, "session_id").is_empty()
                            && string(candidate, "session_id").is_empty()
                            && candidate["conversation_id"] == link["conversation_id"]
                }) {
                    pair.push(archived.clone());
                }
                pair.push(row.clone());
                parent_link::decorate(&mut pair);
                row["parent"] = pair.pop().unwrap()["parent"].clone();
            }
        }
        value["sessions"] = json!(rows);
        if let Some(page) = archive_page {
            value["archive_page"] = page;
        }
        if query.paged {
            value["listing_page"] = json!({"abi":control::MARKER,"complete":cursor.is_none(),"coverage":"ready","generation":reader.as_ref().unwrap().generation()?,"next_cursor":cursor.as_ref().map(pages::Cursor::encode).transpose()?});
        }
        anyhow::ensure!(
            serde_json::to_vec(&value)?.len() <= if query.paged { PAGE_BYTES } else { BODY_BYTES },
            "Complete session listing exceeds its response bound"
        );
        Ok(value.clone())
    };
    run().map_err(|error| error.to_string())
}
