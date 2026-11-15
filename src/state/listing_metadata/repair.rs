use super::*;
use std::{fs, io::Read};

const STEP_BYTES: usize = 16 << 20;
const STEP_ENTRIES: usize = 256;
const STEP_ROWS: usize = 128;
const CONTROL_DIRS: [&str; 3] = ["renames", "name_routes", "labels"];
pub(in crate::state) struct Background {
    root: PathBuf,
    epoch: Option<String>,
    iterator: Option<(i64, i64, fs::ReadDir)>,
}
#[derive(Default, Debug)]
pub(in crate::state) struct Stats {
    pub entries: usize,
    pub bytes: usize,
    pub rows: usize,
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
}
fn scan(db: &Connection) -> anyhow::Result<Option<Scan>> {
    Ok(db.query_row("SELECT generation,phase,folder,after_key,dirs,writes,gap,problems FROM scan WHERE id=1",[],|r|Ok(Scan{
        generation:r.get(0)?,phase:r.get(1)?,folder:r.get(2)?,after:r.get(3)?,dirs:r.get(4)?,writes:r.get(5)?,gap:r.get(6)?,problems:r.get(7)?,
    })).optional()?)
}
// Fixed seven directory witnesses, never a full directory inventory here.
pub(super) fn directories(store: &Store) -> anyhow::Result<String> {
    let mut stamps = Vec::new();
    for folder in storage::BINDING_DIRS.into_iter().chain(CONTROL_DIRS) {
        let mut parent = checked(Directory::open(&store.root.path))?;
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
    store.verify()?;
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
fn begin(store: &Store) -> anyhow::Result<Scan> {
    let dirs = directories(store)?;
    let gap = gap_stamp(store)?;
    store
        .db
        .execute("UPDATE meta SET ready=0,serial=serial+1 WHERE id=1", [])?;
    store.db.execute(
        "INSERT INTO generations SELECT serial,0,?1,?2 FROM meta WHERE id=1",
        params![dirs, gap],
    )?;
    store.db.execute(
        "INSERT INTO scan SELECT 1,serial,0,0,'',?1,writes,?2,0 FROM meta WHERE id=1",
        params![dirs, gap],
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
                        "INSERT INTO snapshots(generation,source,seed) VALUES(?1,?2,?3)",
                        params![s.generation, key.0, seed],
                    )?;
                }
            }
        }
        Err(error) => {
            // Unknown retains its previous seed. It is NOT a tombstone and
            // this scan cannot replace a complete published generation.
            s.problems += 1;
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
        let names=keys(store,"SELECT source FROM snapshots WHERE generation=?1 AND source>?2 ORDER BY source LIMIT 128",generation,"")?;
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
        let remains:bool=store.db.query_row("SELECT EXISTS(SELECT 1 FROM snapshots WHERE generation=?1) OR EXISTS(SELECT 1 FROM queue WHERE generation=?1)",[generation],|r|r.get(0))?;
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
    fn step(&mut self) -> anyhow::Result<Stats> {
        let store = Store::open(&self.root)?;
        store.verify()?;
        let epoch: String = store
            .db
            .query_row("SELECT epoch FROM meta WHERE id=1", [], |r| r.get(0))?;
        if self.epoch.as_deref() != Some(epoch.as_str()) {
            self.iterator = None;
            self.epoch = Some(epoch);
        }
        store.db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> anyhow::Result<Stats> {
            let until = Instant::now() + Duration::from_millis(50);
            collect(&store, until)?;
            let mut s = match scan(&store.db)? {
                Some(s) => s,
                None => begin(&store)?,
            };
            let mut stats = Stats::default();
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
                        let names=keys(&store,"SELECT DISTINCT source FROM intents WHERE ?1>=0 AND source>?2 ORDER BY source LIMIT 128",s.generation,&s.after)?;
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
                    4 => {
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
                        stats.complete = s.problems == 0
                            && !pending
                            && writes == s.writes
                            && directories(&store)? == s.dirs
                            && gap_stamp(&store)? == s.gap;
                        store.db.execute(
                            "UPDATE generations SET complete=?2 WHERE generation=?1",
                            params![s.generation, i64::from(stats.complete)],
                        )?;
                        if stats.complete {
                            store.db.execute(
                                "UPDATE meta SET generation=?1,ready=1 WHERE id=1",
                                [s.generation],
                            )?;
                        }
                        store.db.execute("DELETE FROM scan WHERE id=1", [])?;
                        stats.done = true;
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
                store.db.execute_batch("COMMIT")?;
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
    let canonical = crate::platform::canonical(root)?;
    if state.as_ref().is_none_or(|work| work.root != canonical) {
        *state = Some(Background {
            root: canonical,
            epoch: None,
            iterator: None,
        });
    }
    let mut result = state.as_mut().unwrap().step();
    if let Err(error) = &result {
        if Store::recover(&state.as_ref().unwrap().root, error)? {
            result = Ok(Stats::default());
            *state = None;
        }
    }
    if result.is_err() || result.as_ref().is_ok_and(|s| s.done) {
        *state = None;
    }
    result
}
