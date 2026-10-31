//! U62, #717: the team's test queue and its local results database. A request
//! names a provider's suite (command, folder, targets, recorded time and
//! resource class); the scheduler runs what the machine has room for, forced
//! and short suites first, never two heavy ones at once, and long suites only
//! at night. Only `run_due` writes results, each with its commit, input hash,
//! status, duration, a bounded excerpt and who asked.
use crate::test_discipline::config::{self, Resources};
use super::templates::{last_chars, run_tree};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub(crate) const RESULT_LIMIT: i64 = 5000;
pub(crate) const EXCERPT_BYTES: usize = 4000;
pub(crate) const HEAVY_CPU: u32 = 4;
pub(crate) const HEAVY_RAM_MB: u32 = 4096;

/// A suite's declared class: the config's `[resources.NAME]`.
pub(crate) type Classes = BTreeMap<String, Resources>;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Load {
    pub cpu_free: u32,
    pub ram_free_mb: u32,
    pub gpu_free: bool,
    pub at_night: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Request {
    pub id: String,
    pub team: String,
    pub suite: String,
    pub task: Option<u32>,
    pub targets: Vec<String>,
    pub commit: String,
    pub input_hash: String,
    pub asked_by: String,
    pub forced: bool,
    pub at: f64,
    pub seconds: f64,
    pub budget_minutes: f64,
    pub resources: String,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Record {
    pub id: String,
    pub team: String,
    pub suite: String,
    pub commit: String,
    pub input_hash: String,
    pub status: String,
    pub duration_ms: u64,
    pub excerpt: String,
    pub asked_by: String,
    /// When the run started.
    pub at: f64,
    pub affected: Vec<String>,
    /// #719: the task that asked for it, so its drawer lists its runs.
    pub task: Option<u32>,
}

/// #719: a request still in the queue: waiting, or running since `started`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Pending {
    #[serde(flatten)]
    pub request: Request,
    pub state: String,
    pub started: Option<f64>,
}

/// #719: at most this many queued and running requests in one read.
pub(crate) const PENDING_LIMIT: i64 = 100;

const REQUEST_COLUMNS: &str = "id,team,suite,task,targets,commit_id,input_hash,asked_by,forced,at,seconds,budget_minutes,resources,command";
const RECORD_COLUMNS: &str = "id,team,suite,commit_id,input_hash,status,duration_ms,excerpt,asked_by,at,affected,task";

fn add_column(db: &Connection, table: &str, column: &str, kind: &str) -> rusqlite::Result<()> {
    let present = db.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1"))?.exists([column])?;
    if !present {
        db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?;
    }
    Ok(())
}

pub(crate) fn database_path() -> PathBuf {
    super::store::path().with_file_name("tests.sqlite3")
}

#[derive(Debug)]
pub(crate) struct TestStore {
    db: Connection,
}

impl TestStore {
    pub(crate) fn open(read_only: bool) -> anyhow::Result<Self> {
        Self::open_at(&database_path(), read_only)
    }

    pub(crate) fn open_at(path: &Path, read_only: bool) -> anyhow::Result<Self> {
        if !read_only {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let flags = if read_only { rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY } else { rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_CREATE };
        let db = Connection::open_with_flags(path, flags)?;
        db.busy_timeout(Duration::from_secs(5))?;
        if !read_only {
            db.execute_batch(
                "PRAGMA journal_mode=DELETE; PRAGMA max_page_count=262144;
                 CREATE TABLE IF NOT EXISTS queue(id TEXT PRIMARY KEY,team TEXT NOT NULL,suite TEXT NOT NULL,task INTEGER,
                    targets TEXT NOT NULL,commit_id TEXT NOT NULL,input_hash TEXT NOT NULL,asked_by TEXT NOT NULL,
                    forced INTEGER NOT NULL,at REAL NOT NULL,seconds REAL NOT NULL,budget_minutes REAL NOT NULL,
                    resources TEXT NOT NULL,command TEXT NOT NULL,state TEXT NOT NULL);
                 CREATE INDEX IF NOT EXISTS queue_team ON queue(team,state,at);
                 CREATE TABLE IF NOT EXISTS results(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,team TEXT NOT NULL,
                    suite TEXT NOT NULL,commit_id TEXT NOT NULL,input_hash TEXT NOT NULL,status TEXT NOT NULL,
                    duration_ms INTEGER NOT NULL,excerpt TEXT NOT NULL,asked_by TEXT NOT NULL,at REAL NOT NULL,
                    affected TEXT NOT NULL);
                 CREATE INDEX IF NOT EXISTS results_input ON results(suite,commit_id,input_hash);
                 CREATE INDEX IF NOT EXISTS results_team ON results(team,at);
                 CREATE INDEX IF NOT EXISTS results_at ON results(at);",
            )?;
            // #719: a running request's start, and a result's task, in a
            // database #717 made without them.
            add_column(&db, "queue", "started", "REAL")?;
            add_column(&db, "results", "task", "INTEGER")?;
            db.execute_batch("CREATE INDEX IF NOT EXISTS results_task ON results(team,task,at);")?;
        }
        Ok(Self { db })
    }

    pub(crate) fn enqueue(&self, request: &Request) -> anyhow::Result<()> {
        self.db.execute(
            &format!("INSERT OR REPLACE INTO queue({REQUEST_COLUMNS},state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'queued')"),
            params![
                request.id,
                request.team,
                request.suite,
                request.task,
                serde_json::to_string(&request.targets)?,
                request.commit,
                request.input_hash,
                request.asked_by,
                request.forced as i64,
                request.at,
                request.seconds,
                request.budget_minutes,
                request.resources,
                request.command
            ],
        )?;
        Ok(())
    }

    pub(crate) fn queued(&self, team: &str) -> anyhow::Result<Vec<Request>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {REQUEST_COLUMNS} FROM queue WHERE team=?1 AND state='queued' ORDER BY forced DESC, at ASC"
        ))?;
        let rows = statement.query_map([team], Self::request)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// #719: what the Tests view shows as the queue: the running requests
    /// first, then the waiting ones in the order the scheduler weighs them.
    pub(crate) fn pending(&self, team: &str) -> anyhow::Result<Vec<Pending>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {REQUEST_COLUMNS},state,started FROM queue WHERE team=?1 AND state IN ('queued','running')
             ORDER BY state='running' DESC, forced DESC, at ASC LIMIT ?2"
        ))?;
        let rows = statement.query_map(params![team, PENDING_LIMIT], |row| {
            Ok(Pending { request: Self::request(row)?, state: row.get(14)?, started: row.get(15)? })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn request(row: &rusqlite::Row<'_>) -> rusqlite::Result<Request> {
        Ok(Request {
            id: row.get(0)?,
            team: row.get(1)?,
            suite: row.get(2)?,
            task: row.get(3)?,
            targets: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
            commit: row.get(5)?,
            input_hash: row.get(6)?,
            asked_by: row.get(7)?,
            forced: row.get::<_, i64>(8)? != 0,
            at: row.get(9)?,
            seconds: row.get(10)?,
            budget_minutes: row.get(11)?,
            resources: row.get(12)?,
            command: row.get(13)?,
        })
    }

    /// #719: the request runs now; the Tests view shows it as running.
    fn start(&self, id: &str, at: f64) -> anyhow::Result<()> {
        self.db.execute("UPDATE queue SET state='running', started=?2 WHERE id=?1", params![id, at])?;
        Ok(())
    }

    fn finish(&self, id: &str) -> anyhow::Result<()> {
        self.db.execute("UPDATE queue SET state='done' WHERE id=?1", [id])?;
        Ok(())
    }

    /// Only the runner calls this: a result names its suite, commit, input
    /// hash, status, duration, a bounded excerpt and who asked.
    fn write(&self, record: &Record) -> anyhow::Result<()> {
        self.db.execute(
            &format!("INSERT OR REPLACE INTO results({RECORD_COLUMNS}) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"),
            params![
                record.id,
                record.team,
                record.suite,
                record.commit,
                record.input_hash,
                record.status,
                record.duration_ms as i64,
                record.excerpt,
                record.asked_by,
                record.at,
                serde_json::to_string(&record.affected)?,
                record.task
            ],
        )?;
        // Bounded: only the oldest records past the limit go.
        let count: i64 = self.db.query_row("SELECT COUNT(*) FROM results", [], |row| row.get(0))?;
        if count > RESULT_LIMIT {
            self.db.execute(
                "DELETE FROM results WHERE seq IN (SELECT seq FROM results ORDER BY seq ASC LIMIT ?1)",
                [count - RESULT_LIMIT],
            )?;
        }
        Ok(())
    }

    /// The last result for the same inputs, if the runner already measured it.
    pub(crate) fn reused(&self, suite: &str, commit: &str, input_hash: &str) -> anyhow::Result<Option<Record>> {
        let mut statement = self.db.prepare(&format!(
            "SELECT {RECORD_COLUMNS} FROM results WHERE suite=?1 AND commit_id=?2 AND input_hash=?3 ORDER BY seq DESC LIMIT 1"
        ))?;
        Ok(statement.query_row(params![suite, commit, input_hash], Self::record).optional()?)
    }

    /// Newest first, a page before `before`; #719: only one task's runs, or
    /// only runs that started at `from` or later (the profiler's window).
    pub(crate) fn results(&self, team: Option<&str>, before: Option<f64>, limit: usize, task: Option<u32>, from: Option<f64>) -> anyhow::Result<Vec<Record>> {
        let limit = limit.clamp(1, 100) as i64;
        let mut statement = self.db.prepare(&format!(
            "SELECT {RECORD_COLUMNS} FROM results WHERE (?1 IS NULL OR team=?1) AND (?2 IS NULL OR at<?2)
             AND (?4 IS NULL OR task=?4) AND (?5 IS NULL OR at>=?5) ORDER BY at DESC, seq DESC LIMIT ?3"
        ))?;
        let rows = statement.query_map(params![team, before, limit, task, from], Self::record)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Record> {
        Ok(Record {
            id: row.get(0)?,
            team: row.get(1)?,
            suite: row.get(2)?,
            commit: row.get(3)?,
            input_hash: row.get(4)?,
            status: row.get(5)?,
            duration_ms: row.get::<_, i64>(6)? as u64,
            excerpt: row.get(7)?,
            asked_by: row.get(8)?,
            at: row.get(9)?,
            affected: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_default(),
            task: row.get(11)?,
        })
    }
}

fn is_heavy(class: Option<&Resources>) -> bool {
    class.is_some_and(|class| class.gpu || class.exclusive || class.cpu >= HEAVY_CPU || class.ram_mb >= HEAVY_RAM_MB)
}

fn long(request: &Request) -> bool {
    request.seconds > request.budget_minutes.max(0.0) * 60.0
}

/// U62 (#717): records the tick's load time and one notice per result (newest
/// last, at most 50); the results themselves stay in the database.
pub(crate) fn record_notices(team: &mut super::model::Team, records: &[Record], now: f64) {
    team.test_load_at = now;
    for record in records {
        team.test_notices.push(super::model::TestNotice {
            record: record.id.clone(),
            suite: record.suite.clone(),
            status: record.status.clone(),
            duration_ms: record.duration_ms,
            asked_by: record.asked_by.clone(),
            at: record.at,
        });
    }
    let extra = team.test_notices.len().saturating_sub(50);
    team.test_notices.drain(0..extra);
}

/// U62 (#717): the machine's real free resources. The scheduler reads them at
/// most once per 30 s (see `tick`) and never when the queue is empty.
pub(crate) fn load_now() -> Load {
    let cpu_free = std::thread::available_parallelism().map(|count| count.get() as u32).unwrap_or(1);
    Load { cpu_free, ram_free_mb: free_memory_mb().unwrap_or(0), gpu_free: true, at_night: false }
}

#[cfg(windows)]
fn free_memory_mb() -> Option<u32> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    (unsafe { GlobalMemoryStatusEx(&mut status) } != 0).then_some((status.ullAvailPhys / (1024 * 1024)) as u32)
}

#[cfg(unix)]
fn free_memory_mb() -> Option<u32> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kb: u64 = text
        .lines()
        .find(|line| line.starts_with("MemAvailable:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((kb / 1024) as u32)
}

/// U62 (#717): one worker tick. An empty queue costs no load read; after a read
/// the next is 30 s away (the returned bool says a read happened); the night
/// window comes from the project's `[schedule]`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn tick(
    store: &TestStore,
    project: &Path,
    team: &str,
    classes: &Classes,
    night: &config::Schedule,
    local_hour: u32,
    last_load_at: f64,
    now: f64,
    read_load: impl FnOnce() -> Load,
) -> anyhow::Result<(Vec<Record>, bool)> {
    let queued = store.queued(team)?;
    if queued.is_empty() || now - last_load_at < 30.0 {
        return Ok((Vec::new(), false));
    }
    let mut load = read_load();
    load.at_night = config::is_night(local_hour, night);
    let chosen = schedule(&queued, classes, &load);
    let records = run_due(store, project, &chosen, now)?;
    Ok((records, true))
}

/// The requests a tick runs, in order: forced first, then short before long,
/// then by age; a long suite waits for the night (or the CI) unless forced, and
/// never two heavy suites at once. The machine's free CPU, RAM and GPU must fit.
pub(crate) fn schedule(requests: &[Request], classes: &Classes, load: &Load) -> Vec<Request> {
    let mut ordered: Vec<&Request> = requests.iter().collect();
    ordered.sort_by(|a, b| b.forced.cmp(&a.forced).then(a.seconds.total_cmp(&b.seconds)).then(a.at.total_cmp(&b.at)).then(a.id.cmp(&b.id)));
    let (mut cpu, mut ram, mut gpu, mut heavy) = (load.cpu_free, load.ram_free_mb, load.gpu_free, false);
    let mut chosen = Vec::new();
    for request in ordered {
        if !request.forced && long(request) && !load.at_night {
            continue;
        }
        let class = classes.get(&request.resources);
        let (need_cpu, need_ram, need_gpu) = class.map_or((1, 0, false), |class| (class.cpu.max(1), class.ram_mb, class.gpu));
        if need_cpu > cpu || need_ram > ram || (need_gpu && !gpu) {
            continue;
        }
        let this_heavy = is_heavy(class);
        if this_heavy && heavy {
            continue;
        }
        cpu -= need_cpu;
        ram = ram.saturating_sub(need_ram);
        if need_gpu {
            gpu = false;
        }
        heavy |= this_heavy;
        chosen.push(request.clone());
    }
    chosen
}

fn shell(command: &str, cwd: &Path) -> Command {
    let mut process = if cfg!(windows) {
        let mut process = Command::new("cmd");
        process.args(["/D", "/S", "/C", command]);
        process
    } else {
        let mut process = Command::new("sh");
        process.args(["-c", command]);
        process
    };
    process.current_dir(cwd);
    process
}

/// Runs the scheduled requests in order and writes one result each. The command
/// comes from the project's configuration, never from a report; a timeout or a
/// nonzero exit is the result's status.
pub(crate) fn run_due(store: &TestStore, project: &Path, requests: &[Request], now: f64) -> anyhow::Result<Vec<Record>> {
    let mut written = Vec::new();
    let began = std::time::Instant::now();
    for request in requests {
        let started = std::time::Instant::now();
        let at = now + began.elapsed().as_secs_f64();
        store.start(&request.id, at)?;
        let mut process = shell(&request.command_for(project), project);
        let outcome = run_tree(&mut process, Duration::from_secs((request.budget_minutes.max(1.0) * 60.0) as u64), None);
        let status = if outcome.timed_out {
            "timeout".to_owned()
        } else if outcome.exit == Some(0) {
            "passed".to_owned()
        } else {
            "failed".to_owned()
        };
        let excerpt = if outcome.excerpt.is_empty() { last_chars(&outcome.tail, EXCERPT_BYTES) } else { outcome.excerpt };
        let record = Record {
            id: format!("{}-{}", request.id, now as i64),
            team: request.team.clone(),
            suite: request.suite.clone(),
            commit: request.commit.clone(),
            input_hash: request.input_hash.clone(),
            status,
            duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            excerpt,
            asked_by: request.asked_by.clone(),
            at,
            affected: request.targets.clone(),
            task: request.task,
        };
        store.write(&record)?;
        store.finish(&request.id)?;
        written.push(record);
    }
    Ok(written)
}

impl Request {
    /// The command the runner executes: the project file's provider command.
    fn command_for(&self, _project: &Path) -> String {
        self.command.clone()
    }
}

/// A suite the CLI or a test builds from the configuration: its command, folder,
/// targets, recorded seconds, budget and resource class.
pub(crate) fn suite_from_config(project: &Path, team: Option<&str>, name: &str, targets: &[String]) -> anyhow::Result<Request> {
    let loaded = crate::test_discipline::load(project, team).map_err(|error| anyhow::anyhow!("{error}"))?;
    let config = &loaded.config;
    let provider = config.providers.iter().find(|provider| provider.name == name).ok_or_else(|| anyhow::anyhow!("No provider {name}"))?;
    let command = if provider.command.is_empty() {
        // The kind's own command is the config's job; refuse rather than guess.
        anyhow::bail!("Provider {name} has no command in {}", config::FILE)
    } else {
        provider.command.replace("{targets}", &targets.join(" ")).replace("{manifest}", &provider.manifest).replace("{build_dir}", &provider.build_dir)
    };
    let budget = if provider.budget_minutes > 0.0 { provider.budget_minutes } else { config.budgets.suite_minutes };
    Ok(Request {
        id: uuid::Uuid::new_v4().to_string(),
        team: String::new(),
        suite: name.to_owned(),
        task: None,
        targets: targets.to_vec(),
        commit: String::new(),
        input_hash: String::new(),
        asked_by: String::new(),
        forced: false,
        at: super::super::now(),
        seconds: 0.0,
        budget_minutes: budget,
        resources: provider.resources.clone(),
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, suite: &str, seconds: f64, forced: bool, resources: &str) -> Request {
        Request {
            id: id.into(), team: "t".into(), suite: suite.into(), task: None, targets: Vec::new(),
            commit: "c".into(), input_hash: "h".into(), asked_by: "dev".into(), forced, at: 1.0,
            seconds, budget_minutes: 10.0, resources: resources.into(), command: String::new(),
        }
    }

    #[test]
    fn the_worker_tick_reads_the_load_only_for_a_queue_and_once_in_thirty_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let store = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
        let classes = Classes::new();
        let night = config::Schedule::default();
        let reads = std::cell::Cell::new(0u32);
        let load = || {
            reads.set(reads.get() + 1);
            Load { cpu_free: 8, ram_free_mb: 8000, gpu_free: true, at_night: false }
        };
        // An empty queue: no read at all.
        let (records, read) = tick(&store, dir.path(), "t", &classes, &night, 12, 0.0, 100.0, load).unwrap();
        assert!(records.is_empty() && !read && reads.get() == 0, "an empty queue reads no load");
        let mut queued = request("r1", "unit", 10.0, false, "small");
        queued.command = if cfg!(windows) { "cmd /d /s /c exit 0".into() } else { "true".into() };
        store.enqueue(&queued).unwrap();
        // The last read was a moment ago: no second read.
        let (records, read) = tick(&store, dir.path(), "t", &classes, &night, 12, 90.0, 100.0, load).unwrap();
        assert!(records.is_empty() && !read && reads.get() == 0, "a read waits 30 s");
        // Enough time passed and the queue has work: one read, one run.
        let (records, read) = tick(&store, dir.path(), "t", &classes, &night, 12, 0.0, 100.0, load).unwrap();
        assert!(read && reads.get() == 1, "the tick read the load once");
        assert_eq!(records.len(), 1, "{records:?}");
        assert_eq!(records[0].status, "passed");
        assert!(load_now().cpu_free >= 1, "the real load reader sees the CPUs");
    }

    #[test]
    fn the_night_window_wraps_midnight_and_excludes_the_day() {
        let night = config::Schedule { night_start: 22, night_end: 6 };
        assert!(config::is_night(23, &night) && config::is_night(0, &night) && config::is_night(5, &night));
        assert!(!config::is_night(6, &night) && !config::is_night(12, &night) && !config::is_night(21, &night));
        let day = config::Schedule { night_start: 9, night_end: 17 };
        assert!(config::is_night(10, &day) && !config::is_night(18, &day));
        assert!(!config::is_night(3, &config::Schedule { night_start: 5, night_end: 5 }));
    }

    #[test]
    fn a_result_leaves_one_notice_and_the_inbox_card_links_it() {
        use crate::state::teams::model::{Team, TeamStatus};
        let mut team = Team { status: TeamStatus::Running, ..Default::default() };
        let record = Record {
            id: "r1".into(),
            team: "t".into(),
            suite: "unit".into(),
            commit: "c".into(),
            input_hash: "h".into(),
            status: "passed".into(),
            duration_ms: 1500,
            excerpt: "the log".into(),
            asked_by: "dev".into(),
            at: 10.0,
            affected: Vec::new(),
            task: None,
        };
        record_notices(&mut team, std::slice::from_ref(&record), 100.0);
        assert_eq!(team.test_load_at, 100.0);
        let items = super::super::inbox::items(&team);
        let item = items.iter().find(|item| item.kind == "test_result").expect("a test result card");
        assert!(
            item.summary.contains("unit") && item.summary.contains("passed") && item.summary.contains("r1"),
            "{}",
            item.summary
        );
        assert!(item.command.contains("test results"), "{}", item.command);
        assert!(item.detail.contains("r1") && item.detail.contains("dev"), "{}", item.detail);
        assert!(!item.detail.contains("the log"), "the card links the record, never the log");
        // The notice list is bounded.
        for n in 0..60 {
            record_notices(&mut team, &[Record { id: format!("x{n}"), ..record.clone() }], 100.0);
        }
        assert_eq!(team.test_notices.len(), 50);
    }

    #[test]
    fn the_scheduler_runs_forced_and_short_suites_first_and_one_heavy_at_a_time() {
        let classes: Classes = [
            ("gpu".to_owned(), Resources { cpu: 8, ram_mb: 16000, gpu: true, exclusive: false }),
            ("small".to_owned(), Resources { cpu: 1, ram_mb: 512, ..Default::default() }),
        ]
        .into_iter()
        .collect();
        let requests = vec![
            request("long", "long", 1000.0, false, "small"),
            request("heavy-a", "a", 10.0, false, "gpu"),
            request("heavy-b", "b", 10.0, false, "gpu"),
            request("forced", "f", 20.0, true, "small"),
            request("short", "s", 5.0, false, "small"),
        ];
        let load = Load { cpu_free: 16, ram_free_mb: 32000, gpu_free: true, at_night: false };
        let picked = schedule(&requests, &classes, &load);
        let chosen: Vec<&str> = picked.iter().map(|request| request.id.as_str()).collect();
        assert_eq!(chosen, ["forced", "short", "heavy-a"], "forced first, short before long, one heavy; a long suite waits for the night");
        let night = Load { at_night: true, ..load };
        let picked = schedule(&requests, &classes, &night);
        let chosen: Vec<&str> = picked.iter().map(|request| request.id.as_str()).collect();
        assert!(chosen.contains(&"long"), "{chosen:?}");
    }

    #[test]
    fn results_page_and_resolve_by_their_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let store = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
        let record = |id: &str, suite: &str, at: f64| Record {
            id: id.into(), team: "t".into(), suite: suite.into(), commit: "c".into(), input_hash: "h".into(),
            status: "passed".into(), duration_ms: 10, excerpt: "ok".into(), asked_by: "dev".into(), at,
            affected: vec!["a".into()],
            task: None,
        };
        store.write(&record("1", "cargo", 1.0)).unwrap();
        store.write(&record("2", "cargo", 2.0)).unwrap();
        let found = store.reused("cargo", "c", "h").unwrap().unwrap();
        assert_eq!(found.id, "2");
        let page = store.results(Some("t"), None, 1, None, None).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].id, "2");
        let older = store.results(Some("t"), Some(2.0), 10, None, None).unwrap();
        assert_eq!(older.len(), 1);
        assert_eq!(older[0].id, "1");
    }

    #[test]
    fn the_runner_writes_the_result_with_its_excerpt_and_empties_the_queue() {
        let dir = tempfile::tempdir().unwrap();
        let store = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
        let mut request = request("run", "echo", 5.0, false, "small");
        request.command = if cfg!(windows) { "echo hello & exit /b 0".into() } else { "echo hello".into() };
        store.enqueue(&request).unwrap();
        assert_eq!(store.queued("t").unwrap().len(), 1);
        let written = run_due(&store, dir.path(), &store.queued("t").unwrap(), 100.0).unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].status, "passed");
        assert!(written[0].excerpt.contains("hello"), "{}", written[0].excerpt);
        assert!(store.queued("t").unwrap().is_empty());
        let found = store.reused("echo", "c", "h").unwrap().unwrap();
        assert_eq!(found.id, written[0].id);
    }

    /// #719: the Tests view reads the queue with what runs now first; a result
    /// names its task, and a task or the profiler's window reads only its own.
    #[test]
    fn the_queue_shows_what_runs_and_a_result_names_its_task() {
        let dir = tempfile::tempdir().unwrap();
        let store = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
        let mut waiting = request("waiting", "qt", 5.0, false, "small");
        waiting.task = Some(7);
        store.enqueue(&waiting).unwrap();
        let mut forced = request("forced", "cargo", 5.0, true, "small");
        forced.at = 2.0;
        store.enqueue(&forced).unwrap();
        let mut running = request("running", "echo", 5.0, false, "small");
        running.task = Some(7);
        running.command = if cfg!(windows) { "exit /b 0".into() } else { "true".into() };
        store.enqueue(&running).unwrap();
        store.start("running", 50.0).unwrap();
        let pending = store.pending("t").unwrap();
        let order: Vec<(&str, &str)> = pending.iter().map(|item| (item.request.id.as_str(), item.state.as_str())).collect();
        assert_eq!(order, [("running", "running"), ("forced", "queued"), ("waiting", "queued")]);
        assert_eq!(pending[0].started, Some(50.0));
        assert_eq!(serde_json::to_value(&pending[0]).unwrap()["suite"], "echo", "the request's fields sit beside its state");
        // The scheduler never picks a running request again.
        assert!(store.queued("t").unwrap().iter().all(|item| item.id != "running"));
        let written = run_due(&store, dir.path(), &[running], 100.0).unwrap();
        assert_eq!(written[0].task, Some(7));
        assert!(store.pending("t").unwrap().iter().all(|item| item.request.id != "running"));
        let other = Record { id: "other".into(), task: Some(8), at: 200.0, ..written[0].clone() };
        store.write(&other).unwrap();
        let mine: Vec<String> = store.results(Some("t"), None, 20, Some(7), None).unwrap().into_iter().map(|record| record.id).collect();
        assert_eq!(mine, [written[0].id.clone()]);
        let window: Vec<String> = store.results(Some("t"), None, 20, None, Some(150.0)).unwrap().into_iter().map(|record| record.id).collect();
        assert_eq!(window, ["other"]);
    }

    /// #719: a database #717 made without the new columns gains them on open
    /// and keeps its rows.
    #[test]
    fn an_older_database_gains_the_start_and_task_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tests.sqlite3");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE queue(id TEXT PRIMARY KEY,team TEXT NOT NULL,suite TEXT NOT NULL,task INTEGER,
                    targets TEXT NOT NULL,commit_id TEXT NOT NULL,input_hash TEXT NOT NULL,asked_by TEXT NOT NULL,
                    forced INTEGER NOT NULL,at REAL NOT NULL,seconds REAL NOT NULL,budget_minutes REAL NOT NULL,
                    resources TEXT NOT NULL,command TEXT NOT NULL,state TEXT NOT NULL);
                 CREATE TABLE results(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,team TEXT NOT NULL,
                    suite TEXT NOT NULL,commit_id TEXT NOT NULL,input_hash TEXT NOT NULL,status TEXT NOT NULL,
                    duration_ms INTEGER NOT NULL,excerpt TEXT NOT NULL,asked_by TEXT NOT NULL,at REAL NOT NULL,
                    affected TEXT NOT NULL);
                 INSERT INTO results(id,team,suite,commit_id,input_hash,status,duration_ms,excerpt,asked_by,at,affected)
                    VALUES('old','t','cargo','c','h','passed',5,'ok','dev',1.0,'[]');",
            )
            .unwrap();
        }
        let store = TestStore::open_at(&path, false).unwrap();
        let old = store.results(Some("t"), None, 10, None, None).unwrap();
        assert_eq!((old[0].id.as_str(), old[0].task), ("old", None));
        store.enqueue(&request("new", "cargo", 1.0, false, "small")).unwrap();
        assert_eq!(store.pending("t").unwrap()[0].started, None);
        // Opening again changes nothing.
        drop(store);
        assert_eq!(TestStore::open_at(&path, false).unwrap().pending("t").unwrap().len(), 1);
    }

    #[test]
    fn a_suite_reads_its_command_and_class_from_the_project_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".zerus")).unwrap();
        std::fs::write(
            dir.path().join(".zerus/tests.toml"),
            "version = 1\n[budgets]\nsuite_minutes = 10\n[resources.small]\ncpu = 2\n[[provider]]\nname = \"echo\"\nkind = \"command\"\ncommand = \"echo {targets}\"\nresources = \"small\"\n",
        )
        .unwrap();
        let request = suite_from_config(dir.path(), None, "echo", &["a".into(), "b".into()]).unwrap();
        assert_eq!(request.command, "echo a b");
        assert_eq!(request.suite, "echo");
        assert!(request.budget_minutes > 0.0);
    }

    #[test]
    fn the_results_database_is_bounded_and_indexed() {
        let dir = tempfile::tempdir().unwrap();
        let store = TestStore::open_at(&dir.path().join("tests.sqlite3"), false).unwrap();
        for index in 0..(RESULT_LIMIT + 2) {
            let record = Record {
                id: format!("r{index}"), team: "t".into(), suite: "s".into(), commit: "c".into(), input_hash: "h".into(),
                status: "passed".into(), duration_ms: 1, excerpt: String::new(), asked_by: "dev".into(), at: index as f64,
                affected: Vec::new(),
                task: None,
            };
            store.write(&record).unwrap();
        }
        let count: i64 = store.db.query_row("SELECT COUNT(*) FROM results", [], |row| row.get(0)).unwrap();
        assert_eq!(count, RESULT_LIMIT);
        let indexed: i64 = store.db.query_row("SELECT COUNT(*) FROM pragma_index_list('results') WHERE name='results_input'", [], |row| row.get(0)).unwrap();
        assert_eq!(indexed, 1);
        let newest = store.results(Some("t"), None, 1, None, None).unwrap();
        assert_eq!(newest[0].id, format!("r{}", RESULT_LIMIT + 1));
    }
}
