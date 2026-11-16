use super::*;
use anyhow::Context;
use std::{fs, io::Read};

const STEP_BYTES: usize = 16 << 20;
const STEP_ENTRIES: usize = 256;
const STEP_ROWS: usize = 128;
const CONTROL_DIRS: [&str; 3] = ["renames", "name_routes", "labels"];
pub(in crate::state) struct Background {
    root: PathBuf,
    epoch: Option<String>,
    iterator: Option<(i64, i64, fs::ReadDir)>,
    changes: Option<changes::Changes>,
    covered: bool,
}
#[derive(Default, Debug)]
pub(in crate::state) struct Stats {
    pub entries: usize,
    pub bytes: usize,
    pub rows: usize,
    pub seed_bytes: usize,
    pub done: bool,
    pub complete: bool,
}
struct Scan {
    generation: i64,
    phase: i64,
    folder: i64,
    after: String,
    dirs: String,
    writes: i64,
    gap: String,
    problems: i64,
    base: i64,
}
fn scan(db: &Connection) -> anyhow::Result<Option<Scan>> {
    Ok(db.query_row("SELECT generation,phase,folder,after_key,dirs,writes,gap,problems,base FROM scan WHERE id=1",[],|r|Ok(Scan{
        generation:r.get(0)?,phase:r.get(1)?,folder:r.get(2)?,after:r.get(3)?,dirs:r.get(4)?,writes:r.get(5)?,gap:r.get(6)?,problems:r.get(7)?,base:r.get(8)?,
    })).optional()?)
}
// Fixed seven directory witnesses, never a full directory inventory here.
pub(super) fn directories(store: &Store) -> anyhow::Result<String> {
    let stamps = directories_at(&store.root)?;
    store.verify()?;
    Ok(stamps)
}
pub(super) fn directories_at(root: &Directory) -> anyhow::Result<String> {
    let mut stamps = Vec::new();
    for folder in storage::BINDING_DIRS.into_iter().chain(CONTROL_DIRS) {
        let mut parent = checked(Directory::open(&root.path))?;
        let mut missing = false;
        for part in Path::new(folder).components() {
            let name = part
                .as_os_str()
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid binding namespace"))?;
            match fs::symlink_metadata(parent.path.join(name)) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    missing = true;
                    break;
                }
                Err(e) => return Err(e.into()),
                Ok(m) => anyhow::ensure!(
                    m.is_dir() && !m.file_type().is_symlink(),
                    "Binding namespace is not a directory"
                ),
            }
            parent = checked(parent.child(name))?;
        }
        if missing {
            stamps.push(Value::Null)
        } else {
            let (id, m) = crate::platform::FileIdentity::at(&parent.path)?;
            checked(parent.verify())?;
            stamps.push(json!([
                id.device,
                id.inode,
                id.modified,
                id.modified_nanos,
                m.len()
            ]));
        }
    }
    checked(root.verify())?;
    Ok(serde_json::to_string(&stamps)?)
}
pub(super) fn gap_stamp(store: &Store) -> anyhow::Result<String> {
    let parent = checked(store.root.child("listing-metadata"))?;
    Ok(checked(parent.read_private_optional("coverage-lost.json"))?
        .as_ref()
        .map(store::version)
        .transpose()?
        .unwrap_or_default())
}
fn begin(store: &Store, base: i64) -> anyhow::Result<Scan> {
    let dirs = directories(store)?;
    let gap = gap_stamp(store)?;
    store
        .db
        .execute("UPDATE meta SET ready=0,serial=serial+1 WHERE id=1", [])?;
    store.db.execute(
        "INSERT INTO generations(generation,complete,dirs,gap) SELECT serial,0,?1,?2 FROM meta WHERE id=1",
        params![dirs, gap],
    )?;
    store.db.execute(
        "INSERT INTO scan SELECT 1,serial,?3,0,'',?1,writes,?2,0,?4 FROM meta WHERE id=1",
        params![dirs, gap, if base == 0 { 0 } else { 6 }, base],
    )?;
    scan(&store.db)?.ok_or_else(|| anyhow::anyhow!("Missing display scan"))
}
fn save(store: &Store, s: &Scan) -> anyhow::Result<()> {
    store.db.execute(
        "UPDATE scan SET phase=?1,folder=?2,after_key=?3,problems=?4 WHERE id=1 AND generation=?5",
        params![s.phase, s.folder, s.after, s.problems, s.generation],
    )?;
    Ok(())
}
enum Observation {
    Known {
        seed: Option<String>,
        stamp: Option<String>,
    },
    Later,
}
// A legacy alias needing canonical repair is unsupported display input, not a
// second current session. This point read is charged to the same raw-I/O budget.
fn current_route(
    store: &Store,
    key: &Key,
    record: &Value,
    stats: &mut Stats,
) -> anyhow::Result<bool> {
    if key.0.contains("/") && !key.0.starts_with("bindings/") || record["rename_shadow"] == true {
        return Ok(true);
    }
    let path = store.root.path.join("name_routes").join(format!(
        "{}.json",
        storage::label_key(string(record, "name"))
    ));
    let mut file = match crate::platform::no_follow(fs::OpenOptions::new().read(true)).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error.into()),
    };
    let length = file.metadata()?.len();
    anyhow::ensure!(
        file.metadata()?.is_file() && length <= 32 << 10,
        "Legacy display route is unavailable"
    );
    if length as usize > STEP_BYTES - stats.bytes {
        return Ok(false);
    }
    let mut raw = Vec::new();
    let result = Read::by_ref(&mut file).take(length).read_to_end(&mut raw);
    stats.bytes += raw.len();
    result?;
    let route: Value = serde_json::from_slice(&raw)?;
    anyhow::ensure!(
        route["version"] == 1 && route["name"] == record["name"],
        "Legacy display route identity changed"
    );
    anyhow::bail!("Legacy alias requires canonical reconciliation before display");
}
fn observe(store: &Store, key: &Key, stats: &mut Stats) -> anyhow::Result<Observation> {
    key.validate()?;
    let mut parent = checked(Directory::open(&store.root.path))?;
    let relative = Path::new(&key.0);
    for part in relative.parent().unwrap().components() {
        let name = part.as_os_str().to_str().unwrap();
        match fs::symlink_metadata(parent.path.join(name)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                store.verify()?;
                return Ok(Observation::Known {
                    seed: None,
                    stamp: None,
                });
            }
            Err(e) => return Err(e.into()),
            Ok(m) => anyhow::ensure!(
                m.is_dir() && !m.file_type().is_symlink(),
                "Unverified display source parent"
            ),
        }
        parent = checked(parent.child(name))?;
    }
    let path = parent.path.join(relative.file_name().unwrap());
    let mut file = match crate::platform::no_follow(fs::OpenOptions::new().read(true)).open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            checked(parent.verify())?;
            return Ok(Observation::Known {
                seed: None,
                stamp: None,
            });
        }
        Err(e) => return Err(e.into()),
    };
    let stamp = store::version(&file)?;
    let cached:Option<String>=store.db.query_row("SELECT CASE WHEN length(CAST(seed AS BLOB))<=262144 THEN seed ELSE NULL END FROM sources WHERE source=?1 AND stamp=?2 AND tombstone=0 AND issue IS NULL",params![key.0,stamp],|r|r.get(0)).optional()?.flatten();
    if let Some(seed) = cached.filter(|seed| Seed::decode(key, seed).is_ok()) {
        let projected = Seed::decode(key, &seed)?;
        if !current_route(store, key, &projected["record"], stats)? {
            return Ok(Observation::Later);
        }
        checked(parent.verify())?;
        let current = crate::platform::no_follow(fs::OpenOptions::new().read(true)).open(&path)?;
        anyhow::ensure!(
            store::version(&current)? == stamp,
            "Display source changed during cache check"
        );
        return Ok(Observation::Known {
            seed: Some(seed),
            stamp: Some(stamp),
        });
    }
    let length = file.metadata()?.len();
    anyhow::ensure!(
        length <= storage::ID_RECORD_BYTES,
        "Display source exceeds the saved-record bound"
    );
    if length as usize > STEP_BYTES - stats.bytes {
        return Ok(Observation::Later);
    }
    let mut bytes = Vec::new();
    let read = Read::by_ref(&mut file).take(length).read_to_end(&mut bytes);
    stats.bytes += bytes.len();
    read?;
    anyhow::ensure!(
        bytes.len() as u64 == length && store::version(&file)? == stamp,
        "Display source changed during bounded read"
    );
    checked(parent.verify())?;
    let current = crate::platform::no_follow(fs::OpenOptions::new().read(true)).open(&path)?;
    anyhow::ensure!(
        store::version(&current)? == stamp,
        "Display source route changed during bounded read"
    );
    let value: Value = serde_json::from_slice(&bytes)?;
    if !current_route(store, key, &value, stats)? {
        return Ok(Observation::Later);
    }
    let seed = Seed::encode(key, &value)?;
    Ok(Observation::Known {
        seed: Some(seed),
        stamp: Some(stamp),
    })
}
fn project(store: &Store, s: &mut Scan, key: &Key, stats: &mut Stats) -> anyhow::Result<bool> {
    match observe(store, key, stats) {
        Ok(Observation::Later) => return Ok(false),
        Ok(Observation::Known { seed, stamp }) => {
            let size = seed.as_ref().map_or(0, String::len);
            if size > (512 << 10) - stats.seed_bytes {
                return Ok(false);
            }
            stats.seed_bytes += size;
            store
                .db
                .execute("UPDATE meta SET revision=revision+1 WHERE id=1", [])?;
            store.db.execute("INSERT INTO sources(source,seed,stamp,revision,tombstone,issue) VALUES(?1,?2,?3,(SELECT revision FROM meta WHERE id=1),?4,NULL)
                ON CONFLICT(source) DO UPDATE SET seed=excluded.seed,stamp=excluded.stamp,revision=excluded.revision,tombstone=excluded.tombstone,issue=NULL",params![key.0,seed,stamp,i64::from(stamp.is_none())])?;
            store
                .db
                .execute("DELETE FROM intents WHERE source=?1", [&key.0])?;
            if let Some(seed) = seed {
                let value: Value = serde_json::from_str(&seed)?;
                if value["record"]["rename_shadow"] != true {
                    store.db.execute(
                        "INSERT OR REPLACE INTO snapshots(generation,source,seed,stamp) VALUES(?1,?2,?3,?4)",
                        params![s.generation, key.0, seed, stamp],
                    )?;
                } else {
                    store.db.execute(
                        "DELETE FROM snapshots WHERE generation=?1 AND source=?2",
                        params![s.generation, key.0],
                    )?;
                }
            } else {
                store.db.execute(
                    "DELETE FROM snapshots WHERE generation=?1 AND source=?2",
                    params![s.generation, key.0],
                )?;
            }
        }
        Err(error) => {
            // Unknown retains its previous seed. It is NOT a tombstone and
            // this scan cannot replace a complete published generation.
            let reason = error.to_string().chars().take(256).collect::<String>();
            store.db.execute("INSERT INTO sources(source,seed,stamp,revision,tombstone,issue) VALUES(?1,NULL,NULL,0,0,?2) ON CONFLICT(source) DO UPDATE SET issue=excluded.issue",params![key.0,reason])?;
        }
    }
    stats.rows += 1;
    Ok(true)
}
fn keys(store: &Store, sql: &str, generation: i64, after: &str) -> anyhow::Result<Vec<String>> {
    let mut statement = store.db.prepare_cached(sql)?;
    let rows = statement
        .query_map(params![generation, after], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// The previous two complete snapshots are immutable cursor targets. Remove
// abandoned scans and older generations in small indexed batches, not one
// unbounded DELETE at startup or on a writer's hot path.
fn collect(store: &Store, until: Instant) -> anyhow::Result<()> {
    let scanning: Option<i64> = store
        .db
        .query_row("SELECT generation FROM scan WHERE id=1", [], |r| r.get(0))
        .optional()?;
    let previous:Option<i64>=store.db.query_row("SELECT generation FROM generations WHERE complete=1 ORDER BY generation DESC LIMIT 1 OFFSET 1",[],|r|r.get(0)).optional()?;
    let failed:Option<i64>=store.db.query_row("SELECT generation FROM generations WHERE complete=0 AND generation!=?1 ORDER BY generation LIMIT 1",[scanning.unwrap_or(-1)],|r|r.get(0)).optional()?;
    let old:Option<i64>=store.db.query_row("SELECT generation FROM generations WHERE complete=1 AND generation<?1 ORDER BY generation LIMIT 1",[previous.unwrap_or(-1)],|r|r.get(0)).optional()?;
    if let Some(generation) = failed.or(old) {
        // A retired cursor must fail, not return a truncated old generation
        // while its rows are being collected in bounded batches.
        store.db.execute(
            "UPDATE generations SET complete=0 WHERE generation=?1",
            [generation],
        )?;
        let names = keys(
            store,
            "SELECT source FROM snapshots WHERE generation=?1 AND source>?2 ORDER BY source LIMIT 128",
            generation,
            "",
        )?;
        for name in names {
            if Instant::now() >= until {
                break;
            }
            store.db.execute(
                "DELETE FROM snapshots WHERE generation=?1 AND source=?2",
                params![generation, name],
            )?;
        }
        if Instant::now() < until {
            store.db.execute("DELETE FROM queue WHERE generation=?1 AND source IN(SELECT source FROM queue WHERE generation=?1 ORDER BY source LIMIT 128)",[generation])?;
        }
        store.db.execute("DELETE FROM dirty WHERE generation=?1 AND source IN(SELECT source FROM dirty WHERE generation=?1 ORDER BY source LIMIT 128)",[generation])?;
        let remains:bool=store.db.query_row("SELECT EXISTS(SELECT 1 FROM snapshots WHERE generation=?1) OR EXISTS(SELECT 1 FROM queue WHERE generation=?1) OR EXISTS(SELECT 1 FROM dirty WHERE generation=?1)",[generation],|r|r.get(0))?;
        if !remains {
            store
                .db
                .execute("DELETE FROM generations WHERE generation=?1", [generation])?;
        }
    }
    store.db.execute("DELETE FROM sources WHERE source IN(SELECT source FROM sources WHERE tombstone=1 AND NOT EXISTS(SELECT 1 FROM intents WHERE intents.source=sources.source) ORDER BY source LIMIT 8)",[])?;
    Ok(())
}
impl Background {
    fn reset_feed(&mut self) {
        self.iterator = None;
        self.changes = None;
        self.covered = false;
        self.epoch = None;
    }
    fn hints(
        &mut self,
        store: &Store,
        s: &Scan,
        stats: &mut Stats,
    ) -> anyhow::Result<(bool, bool)> {
        let Some(changes) = self.changes.as_mut() else {
            return Ok((true, false));
        };
        let batch = changes.drain(STEP_ENTRIES - stats.entries)?;
        stats.entries += batch.entries;
        if batch.restart {
            store.db.execute("UPDATE meta SET ready=0 WHERE id=1", [])?;
            store.db.execute("DELETE FROM scan WHERE id=1", [])?;
            self.reset_feed();
            return Ok((false, true));
        }
        for key in batch.keys {
            store.db.execute(
                "INSERT OR IGNORE INTO dirty VALUES(?1,?2)",
                params![s.generation, key.0],
            )?;
        }
        Ok((batch.caught_up, false))
    }
    fn step(&mut self) -> anyhow::Result<Stats> {
        let store = match Store::open(&self.root) {
            Ok(store) => store,
            // No event has been consumed. Dropping this stream on ordinary
            // publisher contention would restart discovery at the prefix.
            Err(error) if error.is::<store::PublisherBusy>() => return Ok(Stats::default()),
            Err(error) => return Err(error),
        };
        store.verify()?;
        let epoch: String = store
            .db
            .query_row("SELECT epoch FROM meta WHERE id=1", [], |r| r.get(0))?;
        if self.epoch.as_deref() != Some(epoch.as_str()) {
            self.iterator = None;
            self.covered = false;
            self.changes = match changes::Changes::new(&store.root.path) {
                Ok(changes) => changes,
                Err(error) => {
                    store.db.execute("UPDATE meta SET ready=0 WHERE id=1", [])?;
                    gap(&store.root.path);
                    return Err(error);
                }
            };
            if self.changes.is_some() {
                // A new native stream cannot certify changes from its gap.
                // Restart discovery, retaining only version-checked sources.
                store.db.execute_batch("BEGIN IMMEDIATE; UPDATE meta SET ready=0 WHERE id=1; DELETE FROM scan; COMMIT;")?;
            }
            self.epoch = Some(epoch);
        }
        // A resident feed can remain idle without copying every snapshot on
        // each pulse. No directory enumeration or canonical body read here.
        let pending_scan = scan(&store.db)?.is_some();
        let idle = if !pending_scan && self.covered {
            let batch = self.changes.as_mut().unwrap().drain(STEP_ENTRIES)?;
            if batch.restart {
                store.db.execute("UPDATE meta SET ready=0 WHERE id=1", [])?;
                self.reset_feed();
                return Ok(Stats {
                    entries: batch.entries,
                    ..Stats::default()
                });
            }
            // `complete` is our generation column, never a label manifest.
            let current:Option<(bool,String,String)>=store.db.query_row("SELECT m.ready,g.dirs,g.gap FROM meta m JOIN generations g ON g.generation=m.generation AND complete=1 WHERE m.id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if batch.caught_up
                && batch.keys.is_empty()
                && current.as_ref().is_some_and(|(ready, dirs, gap)| {
                    *ready
                        && directories(&store).ok().as_ref() == Some(dirs)
                        && gap_stamp(&store).ok().as_ref() == Some(gap)
                })
            {
                return Ok(Stats {
                    entries: batch.entries,
                    done: true,
                    complete: true,
                    ..Stats::default()
                });
            }
            Some(batch)
        } else {
            None
        };
        store.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> anyhow::Result<Stats> {
            let until = Instant::now() + Duration::from_millis(50);
            collect(&store, until)?;
            let mut s = match scan(&store.db)? {
                Some(s) => s,
                None => {
                    let base = if self.covered {
                        store
                            .db
                            .query_row("SELECT generation FROM meta WHERE id=1", [], |r| r.get(0))?
                    } else {
                        0
                    };
                    begin(&store, base)?
                }
            };
            let mut stats = Stats::default();
            if let Some(batch) = idle {
                stats.entries += batch.entries;
                for key in batch.keys {
                    store.db.execute(
                        "INSERT OR IGNORE INTO dirty VALUES(?1,?2)",
                        params![s.generation, key.0],
                    )?;
                }
            }
            let (_, restart) = self.hints(&store, &s, &mut stats)?;
            if restart {
                return Ok(stats);
            }
            while stats.entries < STEP_ENTRIES && stats.rows < STEP_ROWS && Instant::now() < until {
                match s.phase {
                    0 => {
                        if s.folder as usize >= storage::BINDING_DIRS.len() + CONTROL_DIRS.len() {
                            s.phase = 1;
                            s.after.clear();
                            self.iterator = None;
                            continue;
                        }
                        let folder = storage::BINDING_DIRS
                            .into_iter()
                            .chain(CONTROL_DIRS)
                            .nth(s.folder as usize)
                            .unwrap();
                        if self.iterator.as_ref().is_none_or(|(generation, index, _)| {
                            *generation != s.generation || *index != s.folder
                        }) {
                            let path = store.root.path.join(folder);
                            match fs::read_dir(&path) {
                                Ok(iter) => self.iterator = Some((s.generation, s.folder, iter)),
                                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                                    s.folder += 1;
                                    continue;
                                }
                                Err(e) => return Err(e.into()),
                            }
                        }
                        let entry = self.iterator.as_mut().unwrap().2.next();
                        stats.entries += 1;
                        match entry {
                            Some(Ok(entry)) => {
                                if folder == "renames"
                                    && entry.path().extension().is_some_and(|v| v == "json")
                                {
                                    s.problems += 1;
                                }
                                if let Some(key) = Key::at(&store.root.path, &entry.path()) {
                                    store.db.execute("INSERT OR IGNORE INTO queue(generation,source) VALUES(?1,?2)",params![s.generation,key.0])?;
                                }
                            }
                            Some(Err(e)) => return Err(e.into()),
                            None => {
                                s.folder += 1;
                                self.iterator = None;
                            }
                        }
                    }
                    1 => {
                        let names = keys(
                            &store,
                            "SELECT DISTINCT source FROM intents WHERE ?1>=0 AND source>?2 ORDER BY source LIMIT 128",
                            s.generation,
                            &s.after,
                        )?;
                        if names.is_empty() {
                            s.phase = 2;
                            s.after.clear();
                            continue;
                        }
                        for name in names {
                            if stats.entries == STEP_ENTRIES || Instant::now() >= until {
                                break;
                            }
                            store.db.execute(
                                "INSERT OR IGNORE INTO queue VALUES(?1,?2)",
                                params![s.generation, name],
                            )?;
                            s.after = name;
                            stats.entries += 1;
                        }
                    }
                    2 | 3 => {
                        let sql = if s.phase == 2 {
                            "SELECT source FROM queue WHERE generation=?1 AND source>?2 ORDER BY source LIMIT 128"
                        } else {
                            "SELECT source FROM sources WHERE ?1>=0 AND source>?2 ORDER BY source LIMIT 128"
                        };
                        let names = keys(&store, sql, s.generation, &s.after)?;
                        if names.is_empty() {
                            s.phase += 1;
                            s.after.clear();
                            continue;
                        }
                        for name in names {
                            if stats.entries == STEP_ENTRIES
                                || stats.rows == STEP_ROWS
                                || Instant::now() >= until
                            {
                                break;
                            }
                            if s.phase == 3 {
                                let visited:bool=store.db.query_row("SELECT EXISTS(SELECT 1 FROM queue WHERE generation=?1 AND source=?2)",params![s.generation,name],|r|r.get(0))?;
                                if visited {
                                    s.after = name;
                                    stats.entries += 1;
                                    continue;
                                }
                            }
                            let key = Key(name.clone());
                            if !project(&store, &mut s, &key, &mut stats)? {
                                save(&store, &s)?;
                                return Ok(stats);
                            }
                            s.after = name;
                            stats.entries += 1;
                        }
                    }
                    5 => {
                        let names = keys(
                            &store,
                            "SELECT source FROM dirty WHERE generation=?1 AND source>?2 ORDER BY source LIMIT 128",
                            s.generation,
                            "",
                        )?;
                        if names.is_empty() {
                            s.phase = 4;
                            continue;
                        }
                        for name in names {
                            if stats.entries == STEP_ENTRIES
                                || stats.rows == STEP_ROWS
                                || Instant::now() >= until
                            {
                                break;
                            }
                            let key = Key(name.clone());
                            if !project(&store, &mut s, &key, &mut stats)? {
                                save(&store, &s)?;
                                return Ok(stats);
                            }
                            store.db.execute(
                                "DELETE FROM dirty WHERE generation=?1 AND source=?2",
                                params![s.generation, name],
                            )?;
                            stats.entries += 1;
                        }
                    }
                    6 => {
                        let mut statement=store.db.prepare_cached("SELECT substr(source,1,257),CASE WHEN length(CAST(seed AS BLOB))<=262144 THEN seed ELSE NULL END,substr(stamp,1,1025) FROM snapshots WHERE generation=?1 AND source>?2 ORDER BY source LIMIT 128")?;
                        let mut query = statement.query(params![s.base, s.after])?;
                        let mut found = false;
                        let mut bytes = 0;
                        while let Some(row) = query.next()? {
                            if stats.entries == STEP_ENTRIES
                                || stats.rows == STEP_ROWS
                                || Instant::now() >= until
                            {
                                break;
                            }
                            let (source, seed, stamp): (String, String, String) =
                                (row.get(0)?, row.get(1)?, row.get(2)?);
                            anyhow::ensure!(
                                source.len() <= 256
                                    && seed.len() <= model::SEED_BYTES
                                    && stamp.len() <= 1024,
                                "Invalid display snapshot clone"
                            );
                            if bytes + source.len() + seed.len() + stamp.len() > 512 << 10 {
                                break;
                            }
                            found = true;
                            bytes += source.len() + seed.len() + stamp.len();
                            stats.seed_bytes += seed.len();
                            Seed::decode(&Key(source.clone()), &seed)?;
                            store.db.execute("INSERT INTO snapshots(generation,source,seed,stamp) VALUES(?1,?2,?3,?4)",params![s.generation,source,seed,stamp])?;
                            s.after = source;
                            stats.entries += 1;
                            stats.rows += 1;
                        }
                        if !found && stats.rows < STEP_ROWS && Instant::now() < until {
                            s.phase = 5;
                            s.after.clear();
                        }
                    }
                    4 => {
                        let native = self.changes.is_some();
                        let (caught_up, restart) = self.hints(&store, &s, &mut stats)?;
                        if restart {
                            return Ok(stats);
                        }
                        let dirty: bool = store.db.query_row(
                            "SELECT EXISTS(SELECT 1 FROM dirty WHERE generation=?1)",
                            [s.generation],
                            |r| r.get(0),
                        )?;
                        if dirty {
                            s.phase = 5;
                            continue;
                        }
                        if !caught_up {
                            break;
                        }
                        let dirs = directories(&store)?;
                        let gap = gap_stamp(&store)?;
                        let (caught_up, restart) = self.hints(&store, &s, &mut stats)?;
                        if restart {
                            return Ok(stats);
                        }
                        let dirty: bool = store.db.query_row(
                            "SELECT EXISTS(SELECT 1 FROM dirty WHERE generation=?1)",
                            [s.generation],
                            |r| r.get(0),
                        )?;
                        if dirty {
                            s.phase = 5;
                            continue;
                        }
                        if !caught_up || directories(&store)? != dirs || gap_stamp(&store)? != gap {
                            break;
                        }
                        let writes: i64 =
                            store
                                .db
                                .query_row("SELECT writes FROM meta WHERE id=1", [], |r| {
                                    r.get(0)
                                })?;
                        let pending: bool = store.db.query_row(
                            "SELECT EXISTS(SELECT 1 FROM intents)",
                            [],
                            |r| r.get(0),
                        )?;
                        let issues: bool = store.db.query_row(
                            "SELECT EXISTS(SELECT 1 FROM sources WHERE issue IS NOT NULL)",
                            [],
                            |r| r.get(0),
                        )?;
                        stats.complete = s.problems == 0
                            && !issues
                            && !pending
                            && (native || (writes == s.writes && dirs == s.dirs && gap == s.gap));
                        store.db.execute(
                            "UPDATE generations SET complete=?2,dirs=?3,gap=?4,high_water=(SELECT revision FROM meta WHERE id=1) WHERE generation=?1",
                            params![s.generation, i64::from(stats.complete), dirs, gap],
                        )?;
                        if stats.complete {
                            store.db.execute(
                                "UPDATE meta SET generation=?1,ready=1 WHERE id=1",
                                [s.generation],
                            )?;
                        }
                        store.db.execute("DELETE FROM scan WHERE id=1", [])?;
                        stats.done = true;
                        self.covered = stats.complete && native;
                        break;
                    }
                    _ => anyhow::bail!("Unknown display repair phase"),
                }
            }
            if !stats.done {
                save(&store, &s)?;
            }
            store.verify()?;
            Ok(stats)
        })();
        match result {
            Ok(stats) => {
                store
                    .db
                    .execute_batch("COMMIT")
                    .context("Display repair COMMIT failed")?;
                Ok(stats)
            }
            Err(error) => {
                let _ = store.db.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }
}
pub(in crate::state) fn background_at(
    root: &Path,
    state: &mut Option<Background>,
) -> anyhow::Result<Stats> {
    if !enabled(root) {
        *state = None;
        return Ok(Stats::default());
    }
    let canonical = crate::platform::canonical(root)?;
    if state.as_ref().is_none_or(|work| work.root != canonical) {
        *state = Some(Background {
            root: canonical,
            epoch: None,
            iterator: None,
            changes: None,
            covered: false,
        });
    }
    let mut result = state.as_mut().unwrap().step();
    if let Err(error) = &result {
        if Store::recover(&state.as_ref().unwrap().root, error)? {
            result = Ok(Stats::default());
            *state = None;
        }
    }
    if result.is_err() || (result.as_ref().is_ok_and(|s| s.done) && !changes::qualified()) {
        *state = None;
    }
    result
}
